//! Adaptateur libvips : images très grandes, HEIC/AVIF, JPEG XL, JPEG 2000, RAW d'appareils
//! photo, SVG ; rapide et économe en mémoire (traitement par bandes).
//!
//! - Chargeurs et enregistreurs demandés à l'exécutable (`vips -l foreign`), limités au
//!   catalogue ; le chargeur générique par ImageMagick n'est jamais utilisé.
//! - Fichiers sous des noms simples dans le dossier de travail : les options entre crochets de
//!   libvips (`image.jpg[Q=90]`) ne peuvent pas venir d'un nom de fichier.
//! - Vérification : le résultat est entièrement décodé par libvips (`vips avg`).

use crate::imagemagick::{raster_fidelity, raster_keep};
use crate::tool::{self, Stage};
use chud_core::{
    ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, FormatCatalog, FormatId, OptionSpec, StepOutput, StepTask,
    Verification,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const READ: &[&str] = &[
    "png", "jpeg", "webp", "tiff", "gif", "heic", "avif", "jxl", "jp2", "svg", "exr", "hdr", "pnm", "dng", "cr2", "cr3", "nef", "arw",
    "orf", "rw2", "raf", "pef",
];
// HEIC en lecture seulement : l'encodeur HEVC (x265) manque aux versions officielles de libvips
// pour Windows et à la plupart des distributions (« heifsave: Unsupported compression »).
const WRITE: &[&str] = &["png", "jpeg", "webp", "tiff", "gif", "avif", "jxl", "jp2", "hdr", "pnm"];
const LOSSY: &[&str] = &["jpeg", "webp", "avif", "jxl", "jp2"];
const PAGED: &[&str] = &["gif", "webp", "tiff"];

/// Suffixes des chargeurs (`load`) et enregistreurs (`save`) de fichiers.
fn parse_foreign(text: &str) -> (HashSet<String>, HashSet<String>) {
    let (mut load, mut save) = (HashSet::new(), HashSet::new());
    for line in text.lines() {
        let l = line.trim();
        let Some(class) = l.split_whitespace().next() else { continue };
        let target = if class.starts_with("VipsForeignLoad") && class.ends_with("File") && !class.contains("Magick") {
            &mut load
        } else if class.starts_with("VipsForeignSave") && class.ends_with("File") && !class.contains("Magick") {
            &mut save
        } else {
            continue;
        };
        // Le premier groupe entre parenthèses après la description donne les suffixes.
        for group in l.split('(').skip(2) {
            let inside = group.split(')').next().unwrap_or("");
            if inside.starts_with('.') {
                target.extend(inside.split(',').map(|s| s.trim().trim_start_matches('.').to_lowercase()).filter(|s| !s.is_empty()));
                break;
            }
        }
    }
    (load, save)
}

/// Essai réel d'écriture d'une petite image (dossier temporaire retiré ensuite).
fn can_write(exe: &Path, ext: &str) -> bool {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("chud-vips-check-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        return true; // essai impossible : la conversion dira elle-même si elle échoue
    }
    let out = format!("out.{ext}");
    let spec = ProcessSpec::new(exe).args(["black", out.as_str(), "16", "16"]).cwd(&dir).timeout(Some(std::time::Duration::from_secs(30)));
    let ok = chud_process::run_capture(&spec, 64 * 1024).is_ok_and(|o| o.success())
        && std::fs::metadata(dir.join(&out)).is_ok_and(|m| m.len() > 0);
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

pub struct Libvips {
    info: EngineInfo,
    exe: PathBuf,
    caps: Vec<ConversionCap>,
}

impl Libvips {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        let out = tool::query(&exe, &["-l", "foreign"])?;
        let (load, mut save) = parse_foreign(&out.stdout_text());
        if load.is_empty() || save.is_empty() {
            return Err(tr!("liste des formats libvips illisible", "unreadable libvips format list").into());
        }
        // heifsave est annoncé même quand libheif n'a pas d'encodeur AV1 (paquets Ubuntu) :
        // « Unsupported compression » à chaque conversion. L'AVIF est donc essayé pour de vrai.
        if save.contains("avif") && !can_write(&exe, "avif") {
            save.remove("avif");
        }
        Ok(Self::with_suffixes(exe, version, origin, &load, &save))
    }

    fn with_suffixes(exe: PathBuf, version: Option<String>, origin: EngineOrigin, load: &HashSet<String>, save: &HashSet<String>) -> Self {
        let cat = FormatCatalog::builtin();
        let known =
            |id: &str, set: &HashSet<String>| cat.get(id).is_some_and(|d| d.extensions.iter().any(|e| set.contains(&e.to_lowercase())));
        let readable: Vec<&str> = READ.iter().copied().filter(|id| known(id, load)).collect();
        let writable: Vec<&str> = WRITE.iter().copied().filter(|id| known(id, save)).collect();
        let mut caps = Vec::new();
        for from in &readable {
            for to in &writable {
                if from != to {
                    caps.push(ConversionCap::new(from, to, raster_fidelity(from, to), &raster_keep(from, to), 2));
                }
            }
        }
        Self {
            info: tool::engine_info(
                "libvips",
                "libvips",
                version,
                origin,
                &exe,
                "LGPL-2.1-or-later",
                "https://www.libvips.org",
                tr!(
                    "Images très grandes, HEIC, AVIF, JPEG XL, RAW ; rapide et économe en mémoire.",
                    "Very large images, HEIC, AVIF, JPEG XL, RAW; fast and memory-frugal."
                ),
            ),
            exe,
            caps,
        }
    }

    fn sibling(&self, name: &str) -> PathBuf {
        let file = if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() };
        self.exe.with_file_name(file)
    }
}

impl Engine for Libvips {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &self.caps
    }

    fn options(&self, _from: &FormatId, to: &FormatId) -> Vec<OptionSpec> {
        if LOSSY.contains(&to.as_str()) { vec![OptionSpec::int("quality", tr!("Qualité", "Quality"), 1, 100, 90)] } else { Vec::new() }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        ctx.check_cancelled()?;
        let (from, to) = (task.from.as_str(), task.to.as_str());
        if !self.caps.iter().any(|c| c.from.as_str() == from && c.to.as_str() == to) {
            return Err(EngineError::Unsupported(format!("{from} → {to}")));
        }
        let stage = Stage::new(ctx, "libvips")?;
        let in_name = format!("input.{}", tool::ext(from));
        stage.input(&task.input, &in_name)?;
        let out_name = format!("out.{}", tool::ext(to));
        let mut notes = Vec::new();
        let source = if PAGED.contains(&from) && PAGED.contains(&to) { format!("{in_name}[n=-1]") } else { in_name.clone() };
        if PAGED.contains(&from) && !PAGED.contains(&to) {
            notes.push(tr!("première image seulement", "first frame only").to_owned());
        }
        let target = if LOSSY.contains(&to) {
            format!("{out_name}[Q={}]", task.options.int_or("quality", 90).clamp(1, 100))
        } else {
            out_name.clone()
        };
        let spec = ProcessSpec::new(&self.exe)
            .arg("copy")
            .arg(source)
            .arg(target)
            .cwd(&stage.dir)
            .timeout(ctx.timeout.or(Some(Duration::from_secs(900))));
        tool::run_ok(spec, ctx, &[], &mut |_| {})?;
        tool::deliver(&stage.path(&out_name), &task.output, "libvips")?;
        Ok(StepOutput { notes })
    }

    fn verify(&self, output: &Path, task: &StepTask, ctx: &ExecContext) -> Verification {
        let name = format!("check.{}", tool::ext(task.to.as_str()));
        let stage = match Stage::new(ctx, "libvips-check").and_then(|st| st.input(output, &name).map(|_| st)) {
            Ok(st) => st,
            Err(e) => return Verification::Failed(e.to_string()),
        };
        let header = self.sibling("vipsheader");
        let mut dims = String::new();
        if header.is_file() {
            let spec = ProcessSpec::new(&header).arg(&name).cwd(&stage.dir);
            let _ = crate::util::run_tool(spec, ctx, &mut |l| dims = l.trim().to_owned(), &mut |_| {});
        }
        let spec = ProcessSpec::new(&self.exe).arg("avg").arg(&name).cwd(&stage.dir);
        match crate::util::run_tool(spec, ctx, &mut |_| {}, &mut |_| {}) {
            Ok(o) if o.success() => {
                let d = dims.split(':').nth(1).map(str::trim).unwrap_or("").split(',').next().unwrap_or("").to_owned();
                Verification::Decoded(if d.is_empty() {
                    tr!("entièrement décodée par libvips", "fully decoded by libvips").into()
                } else {
                    trf!("entièrement décodée par libvips ({d})", "fully decoded by libvips ({d})")
                })
            }
            Ok(o) => Verification::Failed(trf!(
                "libvips ne relit pas le résultat (code {:?})",
                "libvips cannot re-read the result (code {:?})",
                o.code
            )),
            Err(e) => Verification::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_list() {
        let text = "  VipsForeignLoadHeifFile (heifload), load a HEIF image (.heic, .heif, .avif), priority=0, is_a\n  VipsForeignSavePngFile (pngsave), save image to file as png, nocache (.png), priority=0\n  VipsForeignLoadMagick7File (magickload), load file with ImageMagick7, nocache, priority=-100\n  VipsForeignSaveJpegFile (jpegsave), save as jpeg, nocache (.jpg, .jpeg, .jpe, .jfif), priority=0,\n";
        let (l, s) = parse_foreign(text);
        assert!(l.contains("heic") && l.contains("avif") && !l.contains("magick"));
        assert!(s.contains("png") && s.contains("jpg"));
        let v = Libvips::with_suffixes(PathBuf::from("/x"), None, EngineOrigin::System, &l, &s);
        let has = |a: &str, b: &str| v.caps.iter().any(|c| c.from.as_str() == a && c.to.as_str() == b);
        assert!(has("heic", "png") && has("avif", "jpeg") && !has("png", "heic"));
    }
}
