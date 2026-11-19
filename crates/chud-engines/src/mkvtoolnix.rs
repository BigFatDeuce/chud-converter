//! Adaptateur MKVToolNix (`mkvmerge`) : passage sans perte vers Matroska (MKV). Toutes les pistes
//! (vidéo, audio, sous-titres), chapitres et pièces jointes sont recopiées telles quelles, sans
//! ré-encodage.
//!
//! Vérification : le résultat est relu par `mkvmerge -J` ; le nombre de pistes est comparé à
//! celui de la source, et une piste écartée est signalée.

use crate::tool::{self, Stage};
use chud_core::feature::features;
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, Fidelity, FormatId, StepOutput, StepTask, Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::path::{Path, PathBuf};
use std::time::Duration;

const INPUTS: &[&str] = &["mp4", "mov", "avi", "ts", "m2ts", "webm", "ogv", "flv", "mpeg", "mkv", "3gp", "vob"];

pub struct MkvToolNix {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
}

/// Description d'un fichier par `mkvmerge -J` : (reconnu, nombre de pistes).
fn identify(exe: &Path, file: &Path, ctx: &ExecContext) -> Result<(bool, usize), String> {
    let spec = ProcessSpec::new(exe).arg("-J").arg(file).capture_stdout(32 * 1024 * 1024);
    let out = crate::util::run_tool(spec, ctx, &mut |_| {}, &mut |_| {}).map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let recognized = v["container"]["recognized"].as_bool().unwrap_or(false) && v["container"]["supported"].as_bool().unwrap_or(false);
    Ok((recognized, v["tracks"].as_array().map_or(0, Vec::len)))
}

impl MkvToolNix {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        let keep = features(&[
            "video",
            "audio",
            "multi_audio",
            "multi_video",
            "subtitles",
            "image_subtitles",
            "chapters",
            "tags",
            "attachments",
            "surround",
            "cover_art",
            "hdr",
        ]);
        let caps = INPUTS.iter().map(|f| ConversionCap::new(f, "mkv", Fidelity::Lossless, &keep, 3)).collect();
        Ok(Self {
            info: tool::engine_info(
                "mkvtoolnix",
                "MKVToolNix",
                version,
                origin,
                &exe,
                "GPL-2.0-or-later",
                "https://mkvtoolnix.download",
                tr!(
                    "Matroska (MKV) sans ré-encodage : toutes les pistes, chapitres et pièces jointes.",
                    "Matroska (MKV) without re-encoding: every track, chapter and attachment."
                ),
            ),
            exe,
            caps,
        })
    }
}

impl Engine for MkvToolNix {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn weight(&self, _from: &FormatId, _to: &FormatId) -> u32 {
        2
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        if task.to.as_str() != "mkv" || !INPUTS.contains(&task.from.as_str()) {
            return Err(EngineError::Unsupported(format!("{} → {}", task.from, task.to)));
        }
        let (recognized, tracks_in) = identify(&self.exe, &task.input, ctx).map_err(EngineError::InvalidInput)?;
        if !recognized {
            return Err(EngineError::InvalidInput(
                tr!("mkvmerge ne reconnaît pas ce fichier", "mkvmerge does not recognize this file").into(),
            ));
        }
        let stage = Stage::new(ctx, "mkvmerge")?;
        let out = stage.path("out.mkv");
        let spec =
            ProcessSpec::new(&self.exe).arg("-o").arg(&out).arg(&task.input).timeout(ctx.timeout.or(Some(Duration::from_secs(7200))));
        let mut warnings = Vec::new();
        let o = tool::run_ok(spec, ctx, &[1], &mut |l| {
            if let Some(f) = l.strip_prefix("Progress: ").and_then(tool::percent_in) {
                tool::report_fraction(ctx, f);
            } else if l.starts_with("Warning:") {
                warnings.push(l.trim_start_matches("Warning:").trim().to_owned());
            }
        })?;
        let (_, tracks_out) = identify(&self.exe, &out, ctx).map_err(EngineError::Other)?;
        tool::deliver(&out, &task.output, "mkvmerge")?;
        let mut notes =
            vec![trf!("{tracks_out} piste(s) recopiée(s) sans ré-encodage", "{tracks_out} track(s) copied without re-encoding")];
        if tracks_out < tracks_in {
            notes.push(trf!(
                "{} piste(s) de la source écartée(s) par mkvmerge (format non pris en charge en MKV)",
                "{} source track(s) dropped by mkvmerge (format not supported in MKV)",
                tracks_in - tracks_out
            ));
        }
        if o.code == Some(1) {
            notes.extend(warnings.into_iter().take(3).map(|w| trf!("avertissement de mkvmerge : {w}", "mkvmerge warning: {w}")));
        }
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, _task: &StepTask, ctx: &ExecContext) -> Verification {
        match identify(&self.exe, output, ctx) {
            Ok((true, n)) if n > 0 => {
                Verification::Structural(trf!("Matroska relu par mkvmerge : {n} piste(s)", "Matroska re-read by mkvmerge: {n} track(s)"))
            }
            Ok((true, _)) => Verification::Failed(tr!("aucune piste dans le résultat", "no track in the result").into()),
            Ok((false, _)) => {
                Verification::Failed(tr!("le résultat n'est pas un Matroska valide", "the result is not a valid Matroska file").into())
            }
            Err(e) => Verification::Failed(e),
        }
    }
}
