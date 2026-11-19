//! Adaptateur ExifTool : lecture des métadonnées (EXIF, XMP, IPTC, profil ICC, GPS, appareil,
//! logiciel) des images, vidéos, sons et PDF. Moteur d'analyse : il complète la sonde du moteur
//! de conversion et signale les métadonnées présentes, pour que leur perte éventuelle soit
//! annoncée avant la conversion.
//!
//! Le chemin du fichier est transmis par un fichier d'arguments UTF-8 (`-@`) avec
//! `-charset filename=UTF8` : les noms accentués fonctionnent aussi sous Windows.

use chud_core::{
    Category, ConversionCap, Engine, EngineError, EngineInfo, EngineOrigin, ExecContext, Feature, FeatureSet, FormatCatalog, FormatId,
    SourceInfo, StepOutput, StepTask,
};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct ExifTool {
    info: EngineInfo,
    exe: PathBuf,
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(text).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

/// Résumé et métadonnées présentes, tirés du JSON d'ExifTool (`-j -G`).
fn summarize(tags: &Map<String, Value>) -> SourceInfo {
    let has = |prefix: &str| tags.keys().any(|k| k.starts_with(prefix));
    let mut feats = FeatureSet::new();
    for (prefix, f) in [("EXIF:", "exif"), ("XMP", "xmp"), ("IPTC:", "iptc"), ("ICC_Profile:", "icc")] {
        if has(prefix) {
            feats.insert(Feature::new(f));
        }
    }
    let find = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find_map(|n| tags.iter().find(|(k, _)| k.split(':').nth(1) == Some(*n)).map(|(_, v)| text(v)))
            .filter(|s| !s.trim().is_empty())
    };
    let mut summary: Vec<(String, String)> = Vec::new();
    let push = |summary: &mut Vec<(String, String)>, label: &str, names: &[&str]| {
        if let Some(v) = find(names) {
            summary.push((label.to_owned(), v.chars().take(120).collect()));
        }
    };
    let camera = match (find(&["Make"]), find(&["Model"])) {
        (Some(a), Some(b)) if b.starts_with(&a) => Some(b),
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    };
    if let Some(c) = camera {
        summary.push((tr!("Appareil", "Camera").into(), c));
    }
    push(&mut summary, tr!("Objectif", "Lens"), &["LensModel", "LensID"]);
    push(&mut summary, tr!("Prise de vue", "Taken on"), &["DateTimeOriginal", "CreateDate"]);
    push(&mut summary, tr!("Logiciel", "Software"), &["Software", "CreatorTool", "Producer"]);
    push(&mut summary, tr!("Auteur", "Author"), &["Artist", "Creator", "Author", "By-line"]);
    push(&mut summary, tr!("Droits", "Copyright"), &["Copyright", "Rights", "CopyrightNotice"]);
    push(&mut summary, tr!("Titre", "Title"), &["Title", "ObjectName"]);
    if tags.keys().any(|k| k.contains("GPSLatitude")) {
        summary.push((
            tr!("Position GPS", "GPS position").into(),
            tr!("présente (retirée si le format cible n'a pas d'EXIF)", "present (removed if the target format has no EXIF)").into(),
        ));
    }
    let groups: Vec<&str> = [("EXIF:", "EXIF"), ("XMP", "XMP"), ("IPTC:", "IPTC"), ("ICC_Profile:", "ICC")]
        .iter()
        .filter(|(p, _)| has(p))
        .map(|(_, n)| *n)
        .collect();
    if !groups.is_empty() {
        summary.push((tr!("Métadonnées", "Metadata").into(), groups.join(", ")));
    }
    SourceInfo { features: Some(feats), summary, ..Default::default() }
}

impl ExifTool {
    pub fn new(exe: PathBuf, version: Option<String>, origin: EngineOrigin) -> Result<Self, String> {
        if !exe.is_file() {
            return Err(trf!("introuvable : {}", "not found: {}", exe.display()));
        }
        Ok(Self {
            info: crate::tool::engine_info(
                "exiftool",
                "ExifTool",
                version,
                origin,
                &exe,
                tr!("Artistic-1.0-Perl ou GPL-1.0-or-later", "Artistic-1.0-Perl or GPL-1.0-or-later"),
                "https://exiftool.org",
                tr!(
                    "Lecture des métadonnées : EXIF, XMP, IPTC, GPS, appareil, logiciel.",
                    "Metadata reading: EXIF, XMP, IPTC, GPS, camera, software."
                ),
            ),
            exe,
        })
    }
}

impl Engine for ExifTool {
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
        FormatCatalog::builtin()
            .get(format.as_str())
            .is_some_and(|d| matches!(d.category, Category::Image | Category::Video | Category::Audio | Category::Pdf))
    }

    fn probe(&self, path: &Path, _format: &FormatId) -> Option<SourceInfo> {
        // Un dossier par analyse, retiré ensuite (les analyses peuvent être simultanées).
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("chud-exiftool-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).ok()?;
        let r = self.probe_with(path, &dir);
        let _ = std::fs::remove_dir_all(&dir);
        r
    }

    fn execute(&self, _task: &StepTask, _ctx: &ExecContext) -> Result<StepOutput, EngineError> {
        Err(EngineError::Unsupported(
            tr!("ExifTool analyse les fichiers, il ne les convertit pas", "ExifTool analyzes files, it does not convert them").into(),
        ))
    }
}

impl ExifTool {
    fn probe_with(&self, path: &Path, dir: &Path) -> Option<SourceInfo> {
        let args = dir.join("args.txt");
        // Un argument par ligne ; un nom de fichier ne peut pas contenir de saut de ligne ici.
        let p = path.to_str()?;
        if p.contains(['\n', '\r']) {
            return None;
        }
        std::fs::write(&args, format!("{p}\n")).ok()?;
        let spec = ProcessSpec::new(&self.exe)
            .args(["-charset", "filename=UTF8", "-j", "-G", "-n", "-q", "-@"])
            .arg(&args)
            .timeout(Some(Duration::from_secs(60)));
        let out = chud_process::run_capture(&spec, 16 * 1024 * 1024).ok()?;
        let v: Value = serde_json::from_slice(&out.stdout).ok()?;
        let tags = v.as_array()?.first()?.as_object()?;
        Some(summarize(tags))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_groups_become_features() {
        let v = serde_json::json!({
            "EXIF:Make": "Canon", "EXIF:Model": "Canon EOS R5", "EXIF:GPSLatitude": 48.85,
            "XMP-dc:Creator": ["Alice"], "IPTC:ObjectName": "Tour", "File:FileType": "JPEG"
        });
        let i = summarize(v.as_object().unwrap());
        let f = i.features.unwrap();
        assert!(f.contains(&Feature::new("exif")) && f.contains(&Feature::new("xmp")) && f.contains(&Feature::new("iptc")));
        assert!(!f.contains(&Feature::new("icc")));
        assert!(i.summary.iter().any(|(_, v)| v == "Canon EOS R5"));
        assert!(i.summary.iter().any(|(_, v)| v == "Alice"));
    }
}
