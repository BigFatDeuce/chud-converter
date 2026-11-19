//! Adaptateur MediaInfo : analyse technique détaillée des fichiers audio et vidéo (HDR, Dolby
//! Vision, débits, profils, langues des pistes). Moteur d'analyse : il complète la sonde du
//! moteur de conversion (FFmpeg) sans rien convertir.

use crate::tool;
use chud_core::{
    Category, ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, Feature, FormatCatalog, FormatId, SourceInfo,
    StepOutput, StepTask,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct MediaInfo {
    info: EngineInfo,
    exe: PathBuf,
}

fn s<'a>(t: &'a Value, k: &str) -> Option<&'a str> {
    t.get(k).and_then(Value::as_str).filter(|v| !v.is_empty())
}

fn num(t: &Value, k: &str) -> Option<f64> {
    s(t, k).and_then(|v| v.parse().ok())
}

fn rate(bps: f64) -> String {
    if bps >= 1_000_000.0 { format!("{:.1} Mb/s", bps / 1_000_000.0) } else { format!("{:.0} kb/s", bps / 1000.0) }
}

/// Résumé lisible et caractéristiques (HDR) tirés du JSON de MediaInfo.
fn summarize(v: &Value) -> SourceInfo {
    let mut summary = Vec::new();
    let mut feats = chud_core::FeatureSet::new();
    let tracks = v["media"]["track"].as_array().cloned().unwrap_or_default();
    let (mut audio, mut text) = (Vec::new(), 0);
    for t in &tracks {
        match s(t, "@type") {
            Some("General") => {
                if let Some(f) = s(t, "Format") {
                    let p = s(t, "Format_Profile").map(|p| format!(" ({p})")).unwrap_or_default();
                    summary.push((tr!("Conteneur (MediaInfo)", "Container (MediaInfo)").into(), format!("{f}{p}")));
                }
                if let Some(b) = num(t, "OverallBitRate") {
                    summary.push((tr!("Débit total", "Overall bitrate").into(), rate(b)));
                }
                if let Some(a) = s(t, "Encoded_Application") {
                    summary.push((tr!("Créé avec", "Created with").into(), a.to_owned()));
                }
            }
            Some("Video") => {
                let mut d = s(t, "Format").unwrap_or("?").to_owned();
                if let Some(p) = s(t, "Format_Profile") {
                    d.push_str(&format!(" {p}"));
                }
                if let Some(b) = s(t, "BitDepth") {
                    d.push_str(&trf!(", {b} bits", ", {b}-bit"));
                }
                if let Some(c) = s(t, "colour_primaries") {
                    d.push_str(&format!(", {c}"));
                }
                summary.push((tr!("Vidéo (MediaInfo)", "Video (MediaInfo)").into(), d));
                if let Some(h) = s(t, "HDR_Format") {
                    let compat = s(t, "HDR_Format_Compatibility").map(|c| format!(" ({c})")).unwrap_or_default();
                    summary.push((tr!("HDR", "HDR").into(), format!("{h}{compat}")));
                    feats.insert(Feature::new("hdr"));
                } else if matches!(s(t, "transfer_characteristics"), Some(tc) if tc.contains("PQ") || tc.contains("HLG")) {
                    summary.push((tr!("HDR", "HDR").into(), s(t, "transfer_characteristics").unwrap_or("").to_owned()));
                    feats.insert(Feature::new("hdr"));
                }
            }
            Some("Audio") => {
                let mut d = s(t, "Format").unwrap_or("?").to_owned();
                match s(t, "Channels") {
                    Some("1") => d.push_str(tr!(", mono", ", mono")),
                    Some(c) => d.push_str(&trf!(", {c} canaux", ", {c} channels")),
                    None => {}
                }
                if let Some(l) = s(t, "Language") {
                    d.push_str(&format!(" [{l}]"));
                }
                audio.push(d);
            }
            Some("Text") => text += 1,
            _ => {}
        }
    }
    if !audio.is_empty() {
        summary.push((tr!("Pistes audio (MediaInfo)", "Audio tracks (MediaInfo)").into(), audio.join(" ; ")));
    }
    if text > 0 {
        summary.push((tr!("Pistes de sous-titres", "Subtitle tracks").into(), text.to_string()));
    }
    SourceInfo { features: (!feats.is_empty()).then_some(feats), summary, ..Default::default() }
}

impl MediaInfo {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        Ok(Self {
            info: tool::engine_info(
                "mediainfo",
                "MediaInfo",
                version,
                origin,
                &exe,
                "BSD-2-Clause",
                "https://mediaarea.net/MediaInfo",
                tr!(
                    "Analyse détaillée des fichiers audio et vidéo (HDR, débits, pistes).",
                    "Detailed analysis of audio and video files (HDR, bitrates, tracks)."
                ),
            ),
            exe,
        })
    }
}

impl Engine for MediaInfo {
    fn info(&self) -> &EngineInfo {
        &self.info
    }

    fn conversions(&self) -> &[ConversionCap] {
        &[]
    }

    fn enriches_probe(&self) -> bool {
        true
    }

    fn can_probe(&self, format: &FormatId) -> bool {
        FormatCatalog::builtin().get(format.as_str()).is_some_and(|d| matches!(d.category, Category::Video | Category::Audio))
    }

    fn probe(&self, path: &Path, _format: &FormatId) -> Option<SourceInfo> {
        let spec = ProcessSpec::new(&self.exe).arg("--Output=JSON").arg(path).timeout(Some(Duration::from_secs(60)));
        let out = chud_process::run_capture(&spec, 16 * 1024 * 1024).ok()?;
        if !out.success() {
            return None;
        }
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        Some(summarize(&v))
    }

    fn execute(&self, _task: &StepTask, _ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        Err(EngineError::Unsupported(
            tr!("MediaInfo analyse les fichiers, il ne les convertit pas", "MediaInfo analyzes files, it does not convert them").into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdr_is_detected() {
        let v: Value = serde_json::json!({"media": {"track": [
            {"@type": "General", "Format": "Matroska", "OverallBitRate": "25000000"},
            {"@type": "Video", "Format": "HEVC", "BitDepth": "10", "HDR_Format": "Dolby Vision", "HDR_Format_Compatibility": "HDR10"},
            {"@type": "Audio", "Format": "E-AC-3", "Channels": "6", "Language": "fr"},
            {"@type": "Text"}
        ]}});
        let i = summarize(&v);
        assert!(i.features.unwrap().contains(&Feature::new("hdr")));
        assert!(i.summary.iter().any(|(_, v)| v.contains("Dolby Vision")));
        assert!(i.summary.iter().any(|(_, v)| v.contains("25.0 Mb/s")));
    }
}
