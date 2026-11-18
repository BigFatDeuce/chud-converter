//! Préréglages : un format cible, des options et des choix de sortie, enregistrés sous un nom.
//!
//! Partagés par la CLI (`chud convert --preset`), l'application et le menu contextuel. Ils sont
//! rangés dans `presets.toml` (dossier de configuration) que l'utilisateur peut modifier ; un
//! fichier illisible n'est jamais remplacé en silence.
//!
//! Le menu contextuel doit se construire instantanément : on y choisit les préréglages d'après le
//! **nom** des fichiers sélectionnés ([`kind_of_path`]), sans les lire. Le format réel est détecté
//! ensuite, au moment de convertir, comme pour toute conversion.

use crate::{Conflict, JobSpec};
use chud_core::{FormatCatalog, FormatId, OptionValue, OptionValues};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

/// Sortes de fichiers auxquelles un préréglage peut s'appliquer : les catégories du catalogue
/// (`video`, `audio`, `image`, `archive`, `data`, `subtitle`…) et `folder` pour un dossier.
pub const FOLDER: &str = "folder";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    /// Identifiant stable (lettres minuscules, chiffres, tirets) : utilisé par le menu contextuel.
    pub id: String,
    pub name: String,
    pub target: FormatId,
    /// Sortes de fichiers pour lesquelles le préréglage est proposé.
    #[serde(default)]
    pub applies_to: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, OptionValue>,
    /// Dossier de sortie ; absent = à côté de chaque original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathBuf>,
    /// Modèle de nom (`{stem}` = nom sans extension) ; absent = `{stem}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name_template: Option<String>,
    #[serde(default)]
    pub conflict: Conflict,
    #[serde(default)]
    pub keep_date: bool,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

fn enabled_default() -> bool {
    true
}

/// Noms des préréglages fournis dont le nom se traduit (identifiant, français, anglais).
const BUILTIN_NAMES: &[(&str, &str, &str)] = &[
    ("mp4", "MP4 (compatible)", "MP4 (compatible)"),
    ("mp4-leger", "MP4 léger (720p)", "Light MP4 (720p)"),
    ("mkv", "MKV (sans réencodage)", "MKV (no re-encoding)"),
    ("gif", "GIF animé", "Animated GIF"),
    ("flac", "FLAC (sans perte)", "FLAC (lossless)"),
    ("jpeg-reduit", "JPEG réduit (1920 px)", "Smaller JPEG (1920 px)"),
    ("extraire", "Extraire ici", "Extract here"),
    ("zip", "Compresser en ZIP", "Compress to ZIP"),
    ("7z", "Compresser en 7z", "Compress to 7z"),
];

impl Preset {
    /// Nom affiché : un préréglage fourni dont le nom n'a pas été modifié suit la langue de
    /// l'interface ; un nom choisi par l'utilisateur est affiché tel quel.
    pub fn display_name(&self) -> &str {
        match BUILTIN_NAMES.iter().find(|(id, fr, en)| *id == self.id && (self.name == *fr || self.name == *en)) {
            Some((_, fr, en)) => chud_core::tr!(*fr, *en),
            None => &self.name,
        }
    }

    fn new(id: &str, name: &str, target: &str, applies_to: &[&str]) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            target: FormatId::new(target),
            applies_to: applies_to.iter().map(|s| s.to_string()).collect(),
            options: BTreeMap::new(),
            output_dir: None,
            name_template: None,
            conflict: Conflict::Rename,
            keep_date: false,
            enabled: true,
        }
    }

    fn with(mut self, key: &str, v: OptionValue) -> Self {
        self.options.insert(key.into(), v);
        self
    }

    pub fn applies_to_kind(&self, kind: &str) -> bool {
        self.applies_to.iter().any(|k| k == kind)
    }

    /// Vrai si le préréglage ne ferait rien d'utile sur un fichier déjà dans son format cible
    /// (« PNG » sur un PNG). Un préréglage avec options reste utile (« JPEG réduit » sur un JPEG).
    pub fn is_pointless_for(&self, source: &FormatId) -> bool {
        self.target == *source && self.options.is_empty()
    }

    /// Tâche de conversion d'un fichier selon ce préréglage.
    pub fn job_spec(&self, input: impl Into<PathBuf>) -> JobSpec {
        let mut s = JobSpec::new(input, self.target.clone());
        let mut o = OptionValues::new();
        for (k, v) in &self.options {
            o.set(k, v.clone());
        }
        s.options = o;
        s.output_dir = self.output_dir.clone();
        if let Some(t) = &self.name_template {
            s.name_template = t.clone();
        }
        s.conflict = self.conflict;
        s.preserve_mtime = self.keep_date;
        s
    }
}

/// Sorte d'un chemin d'après son nom seulement (aucune lecture) : `folder` pour un dossier, sinon
/// la catégorie du format deviné par l'extension. `None` si l'extension est inconnue.
pub fn kind_of_path(path: &Path, catalog: &FormatCatalog) -> Option<String> {
    if path.is_dir() {
        return Some(FOLDER.into());
    }
    let name = path.file_name()?.to_string_lossy();
    catalog.guess_from_name(&name).map(|f| f.category.as_str().to_owned())
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("{}", chud_core::trf!("préréglages illisibles ({}) : {}", "unreadable presets ({}): {}", .0.display(), .1))]
    Parse(PathBuf, String),
    #[error("{}", chud_core::trf!("préréglage invalide « {id} » : {message}", "invalid preset “{id}”: {message}"))]
    Invalid { id: String, message: String },
    #[error("{}", chud_core::trf!("préréglage inconnu : {}", "unknown preset: {}", .0))]
    Unknown(String),
    #[error("{}", chud_core::trf!("écriture des préréglages impossible : {}", "cannot write the presets: {}", .0))]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PresetStore {
    #[serde(default = "version_default")]
    pub version: u32,
    #[serde(default, rename = "preset")]
    pub presets: Vec<Preset>,
}

fn version_default() -> u32 {
    1
}

impl Default for PresetStore {
    fn default() -> Self {
        Self::builtin()
    }
}

impl PresetStore {
    /// Préréglages proposés à la première utilisation.
    pub fn builtin() -> Self {
        use OptionValue::{Int, Text};
        let p = Preset::new;
        let presets = vec![
            // Vidéo
            p("mp4", "MP4 (compatible)", "mp4", &["video"]),
            // « Léger » doit vraiment alléger : réencodage imposé (une simple copie ne réduirait rien).
            p("mp4-leger", "MP4 léger (720p)", "mp4", &["video"])
                .with("video_codec", Text("h264".into()))
                .with("reencode", OptionValue::Bool(true))
                .with("video_quality", Text("small".into()))
                .with("max_height", Int(720)),
            p("webm", "WebM", "webm", &["video"]),
            p("mkv", "MKV (sans réencodage)", "mkv", &["video"]),
            p("gif", "GIF animé", "gif", &["video"]),
            // Audio (et extraction depuis une vidéo)
            p("mp3", "MP3", "mp3", &["audio", "video"]).with("audio_bitrate", Int(192)),
            p("flac", "FLAC (sans perte)", "flac", &["audio"]),
            p("m4a", "AAC (M4A)", "m4a", &["audio"]),
            p("wav", "WAV", "wav", &["audio"]),
            // Images
            p("jpeg", "JPEG", "jpeg", &["image"]).with("quality", Int(90)),
            p("jpeg-reduit", "JPEG réduit (1920 px)", "jpeg", &["image"])
                .with("quality", Int(85))
                .with("max_width", Int(1920))
                .with("max_height", Int(1920)),
            p("png", "PNG", "png", &["image"]),
            p("webp", "WebP", "webp", &["image"]),
            // Archives et dossiers
            p("extraire", "Extraire ici", "directory", &["archive"]),
            p("zip", "Compresser en ZIP", "zip", &[FOLDER, "archive"]),
            p("7z", "Compresser en 7z", "7z", &[FOLDER, "archive"]),
            // Données et sous-titres
            p("json", "JSON", "json", &["data"]),
            p("yaml", "YAML", "yaml", &["data"]),
            p("csv", "CSV", "csv", &["data"]),
            p("srt", "SRT", "srt", &["subtitle"]),
            p("vtt", "WebVTT", "vtt", &["subtitle"]),
        ];
        let mut store = Self { version: 1, presets };
        for p in &mut store.presets {
            p.name = p.display_name().to_owned();
        }
        store
    }

    /// Lit `file` ; s'il n'existe pas, renvoie les préréglages par défaut (sans rien écrire).
    pub fn load(file: &Path, catalog: &FormatCatalog) -> Result<Self, PresetError> {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::builtin()),
            Err(e) => return Err(e.into()),
        };
        let store: Self = toml::from_str(&text).map_err(|e| PresetError::Parse(file.to_path_buf(), e.to_string()))?;
        store.validate(catalog)?;
        Ok(store)
    }

    /// Écriture atomique : fichier temporaire voisin puis renommage.
    pub fn save(&self, file: &Path, catalog: &FormatCatalog) -> Result<(), PresetError> {
        self.validate(catalog)?;
        let text = toml::to_string_pretty(self).map_err(|e| PresetError::Parse(file.to_path_buf(), e.to_string()))?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, format!("# Préréglages de Chud Converter (modifiables).\n{text}"))?;
        std::fs::rename(&tmp, file)?;
        Ok(())
    }

    pub fn validate(&self, catalog: &FormatCatalog) -> Result<(), PresetError> {
        let mut seen = HashSet::new();
        for p in &self.presets {
            let bad = |message: &str| Err(PresetError::Invalid { id: p.id.clone(), message: message.into() });
            if p.id.is_empty() || p.id.len() > 40 || !p.id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
                return bad(chud_core::tr!(
                    "identifiant : 1 à 40 lettres minuscules, chiffres ou tirets",
                    "identifier: 1 to 40 lowercase letters, digits or hyphens"
                ));
            }
            if !seen.insert(p.id.as_str()) {
                return bad("identifiant en double");
            }
            if p.name.trim().is_empty() || p.name.chars().count() > 60 {
                return bad(chud_core::tr!(
                    "nom vide ou trop long (60 caractères au plus)",
                    "name empty or too long (60 characters at most)"
                ));
            }
            if catalog.get(p.target.as_str()).is_none() {
                return bad(&format!("format cible inconnu : {}", p.target));
            }
            if p.applies_to.is_empty() {
                return bad(chud_core::tr!("aucune sorte de fichier visée (applies_to)", "no kind of file targeted (applies_to)"));
            }
            if let Some(t) = &p.name_template
                && (t.contains(['/', '\\']) || t.trim().is_empty())
            {
                return bad(chud_core::tr!(
                    "modèle de nom vide ou contenant un séparateur de dossier",
                    "name template empty or containing a folder separator"
                ));
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<&Preset, PresetError> {
        self.presets.iter().find(|p| p.id == id).ok_or_else(|| PresetError::Unknown(id.to_owned()))
    }

    /// Préréglages actifs qui conviennent à **toutes** les sortes données (sélection mixte).
    pub fn for_kinds<'a>(&'a self, kinds: &'a [String]) -> impl Iterator<Item = &'a Preset> + 'a {
        self.presets.iter().filter(move |p| p.enabled && !kinds.is_empty() && kinds.iter().all(|k| p.applies_to_kind(k)))
    }

    /// Préréglages à proposer pour une sélection, d'après les noms seulement (menu contextuel).
    /// Vide si une sorte est inconnue : mieux vaut ne rien proposer que proposer faux.
    pub fn for_paths<'a>(&'a self, paths: &[PathBuf], catalog: &FormatCatalog) -> Vec<&'a Preset> {
        let mut kinds: Vec<String> = Vec::new();
        let mut formats: Vec<Option<FormatId>> = Vec::new();
        for p in paths {
            let Some(k) = kind_of_path(p, catalog) else { return Vec::new() };
            if !kinds.contains(&k) {
                kinds.push(k);
            }
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            formats.push(if p.is_dir() { None } else { catalog.guess_from_name(&name).map(|f| f.id.clone()) });
        }
        self.presets
            .iter()
            .filter(|p| p.enabled && !kinds.is_empty() && kinds.iter().all(|k| p.applies_to_kind(k)))
            .filter(|preset| !formats.iter().all(|f| f.as_ref().is_some_and(|f| preset.is_pointless_for(f))))
            .collect()
    }

    /// Toutes les sortes visées par au moins un préréglage actif.
    pub fn kinds(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in self.presets.iter().filter(|p| p.enabled) {
            for k in &p.applies_to {
                if !out.contains(k) {
                    out.push(k.clone());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> &'static FormatCatalog {
        FormatCatalog::builtin()
    }

    #[test]
    fn builtin_presets_are_valid_and_round_trip() {
        let store = PresetStore::builtin();
        store.validate(cat()).unwrap();
        let dir = std::env::temp_dir().join(format!("chud-presets-{}", std::process::id()));
        let file = dir.join("presets.toml");
        store.save(&file, cat()).unwrap();
        let back = PresetStore::load(&file, cat()).unwrap();
        assert_eq!(back, store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_gives_defaults_and_broken_file_is_an_error() {
        let dir = std::env::temp_dir().join(format!("chud-presets-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("presets.toml");
        assert_eq!(PresetStore::load(&file, cat()).unwrap(), PresetStore::builtin());
        std::fs::write(&file, "[[preset]]\nid = \"Bad Id\"\nname = \"x\"\ntarget = \"mp4\"\napplies_to = [\"video\"]\n").unwrap();
        assert!(matches!(PresetStore::load(&file, cat()), Err(PresetError::Invalid { .. })));
        std::fs::write(&file, "ceci n'est pas du TOML [").unwrap();
        assert!(matches!(PresetStore::load(&file, cat()), Err(PresetError::Parse(..))));
        assert!(file.exists(), "un fichier illisible n'est jamais remplacé");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selection_kinds_filter_presets() {
        let store = PresetStore::builtin();
        let ids = |kinds: &[&str]| {
            let k: Vec<String> = kinds.iter().map(|s| s.to_string()).collect();
            store.for_kinds(&k).map(|p| p.id.clone()).collect::<Vec<_>>()
        };
        assert!(ids(&["video"]).contains(&"mp4".to_string()));
        assert!(ids(&["video"]).contains(&"mp3".to_string()));
        assert!(!ids(&["image"]).contains(&"mp3".to_string()));
        assert_eq!(ids(&["video", "audio"]), vec!["mp3"], "sélection mixte : préréglages communs seulement");
        assert!(ids(&[FOLDER]).contains(&"zip".to_string()));
        assert!(ids(&[]).is_empty());
        assert_eq!(kind_of_path(Path::new("a/b/Film.MKV"), cat()).as_deref(), Some("video"));
        assert_eq!(kind_of_path(Path::new("x.tar.gz"), cat()).as_deref(), Some("archive"));
        assert_eq!(kind_of_path(Path::new("sans-extension"), cat()), None);
        assert_eq!(kind_of_path(&std::env::temp_dir(), cat()).as_deref(), Some(FOLDER));

        let names = |files: &[&str]| {
            let paths: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
            store.for_paths(&paths, cat()).iter().map(|p| p.id.clone()).collect::<Vec<_>>()
        };
        assert!(!names(&["photo.png"]).contains(&"png".to_string()), "pas de « PNG » pour un PNG");
        assert!(names(&["photo.jpg"]).contains(&"jpeg-reduit".to_string()), "réduire un JPEG reste utile");
        assert!(names(&["photo.png", "photo.jpg"]).contains(&"png".to_string()), "utile pour le JPEG de la sélection");
        assert!(names(&["film.mkv", "inconnu.zzz"]).is_empty(), "sorte inconnue : rien plutôt que faux");
    }

    #[test]
    fn job_spec_carries_preset_choices() {
        let store = PresetStore::builtin();
        let p = store.get("mp4-leger").unwrap();
        let s = p.job_spec("film.mov");
        assert_eq!(s.target.as_str(), "mp4");
        assert_eq!(s.options.get("max_height"), Some(&OptionValue::Int(720)));
        assert!(matches!(store.get("nope"), Err(PresetError::Unknown(_))));
    }

    #[test]
    fn builtin_names_follow_the_language_but_custom_names_stay() {
        use chud_core::{Lang, i18n::with_lang};
        let store = with_lang(Lang::En, PresetStore::builtin);
        let zip = store.get("zip").unwrap();
        assert_eq!(zip.name, "Compress to ZIP");
        assert_eq!(with_lang(Lang::Fr, || zip.display_name().to_owned()), "Compresser en ZIP");
        let mut mine = zip.clone();
        mine.name = "Mes archives".into();
        assert_eq!(with_lang(Lang::En, || mine.display_name().to_owned()), "Mes archives");
    }
}
