//! Catalogue des formats, décrit en données (`data/formats.toml`).

use crate::error::CoreError;
use crate::feature::FeatureSet;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::LazyLock;

/// Identifiant stable d'un format (`"png"`, `"mkv"`, `"docx"`…).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FormatId(String);

impl FormatId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for FormatId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for FormatId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl Borrow<str> for FormatId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for FormatId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for FormatId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Image,
    Audio,
    Video,
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Ebook,
    Archive,
    Data,
    Subtitle,
    Font,
    Model3d,
    Geo,
    Other,
}

impl Category {
    pub const ALL: [Category; 15] = [
        Category::Image,
        Category::Audio,
        Category::Video,
        Category::Document,
        Category::Spreadsheet,
        Category::Presentation,
        Category::Pdf,
        Category::Ebook,
        Category::Archive,
        Category::Data,
        Category::Subtitle,
        Category::Font,
        Category::Model3d,
        Category::Geo,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Image => "image",
            Category::Audio => "audio",
            Category::Video => "video",
            Category::Document => "document",
            Category::Spreadsheet => "spreadsheet",
            Category::Presentation => "presentation",
            Category::Pdf => "pdf",
            Category::Ebook => "ebook",
            Category::Archive => "archive",
            Category::Data => "data",
            Category::Subtitle => "subtitle",
            Category::Font => "font",
            Category::Model3d => "model3d",
            Category::Geo => "geo",
            Category::Other => "other",
        }
    }

    pub fn label_fr(self) -> &'static str {
        match self {
            Category::Image => "Images",
            Category::Audio => "Audio",
            Category::Video => "Vidéo",
            Category::Document => "Documents",
            Category::Spreadsheet => "Tableurs",
            Category::Presentation => "Présentations",
            Category::Pdf => "PDF et mise en page",
            Category::Ebook => "Livres électroniques",
            Category::Archive => "Archives",
            Category::Data => "Données",
            Category::Subtitle => "Sous-titres",
            Category::Font => "Polices",
            Category::Model3d => "3D et CAO",
            Category::Geo => "Géographie",
            Category::Other => "Divers",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.as_str() == s)
    }
}

/// Mode de compression natif du format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    Lossless,
    Lossy,
    /// Le format accepte les deux (WebP, AVIF, JPEG XL…).
    Both,
    /// Données non compressées ou format textuel.
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FormatDef {
    pub id: FormatId,
    pub name: String,
    pub category: Category,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub mime: Vec<String>,
    pub compression: Compression,
    #[serde(default)]
    pub features: FeatureSet,
    #[serde(default)]
    pub text: bool,
    #[serde(default)]
    pub based_on: Vec<FormatId>,
    #[serde(default)]
    pub variant_of: Option<FormatId>,
    #[serde(default)]
    pub pivot: bool,
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl FormatDef {
    /// Extension utilisée pour nommer les fichiers produits.
    pub fn output_extension(&self) -> Option<&str> {
        self.extensions.first().map(String::as_str)
    }

    pub fn is_variant(&self) -> bool {
        self.variant_of.is_some()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    format: Vec<FormatDef>,
}

/// Catalogue indexé des formats.
#[derive(Debug)]
pub struct FormatCatalog {
    formats: Vec<FormatDef>,
    by_id: HashMap<FormatId, usize>,
    by_ext: HashMap<String, Vec<usize>>,
    by_mime: HashMap<String, Vec<usize>>,
}

static BUILTIN: LazyLock<FormatCatalog> = LazyLock::new(|| {
    FormatCatalog::from_toml(include_str!("../data/formats.toml")).expect("le catalogue intégré doit être valide (vérifié par les tests)")
});

impl FormatCatalog {
    /// Catalogue intégré à l'application.
    pub fn builtin() -> &'static FormatCatalog {
        &BUILTIN
    }

    pub fn from_toml(src: &str) -> Result<Self, CoreError> {
        let file: CatalogFile = toml::from_str(src).map_err(|e| CoreError::Catalog(e.to_string()))?;
        Self::from_defs(file.format)
    }

    pub fn from_defs(formats: Vec<FormatDef>) -> Result<Self, CoreError> {
        let mut by_id = HashMap::new();
        for (i, f) in formats.iter().enumerate() {
            let valid_id =
                !f.id.as_str().is_empty() && f.id.as_str().chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid_id {
                return Err(CoreError::Catalog(format!("identifiant invalide : {:?}", f.id)));
            }
            if by_id.insert(f.id.clone(), i).is_some() {
                return Err(CoreError::Catalog(format!("identifiant dupliqué : {}", f.id)));
            }
        }
        let mut by_ext: HashMap<String, Vec<usize>> = HashMap::new();
        let mut by_mime: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, f) in formats.iter().enumerate() {
            for ext in &f.extensions {
                if ext.is_empty() || ext.starts_with('.') || ext.to_lowercase() != *ext {
                    return Err(CoreError::Catalog(format!("{} : extension invalide {ext:?} (minuscules, sans point initial)", f.id)));
                }
                by_ext.entry(ext.clone()).or_default().push(i);
            }
            for m in &f.mime {
                by_mime.entry(m.to_ascii_lowercase()).or_default().push(i);
            }
            for r in f.based_on.iter().chain(f.variant_of.iter()) {
                if !by_id.contains_key(r) {
                    return Err(CoreError::Catalog(format!("{} référence un format inconnu : {r}", f.id)));
                }
            }
            if let Some(unknown) = f.features.iter().find(|ft| !ft.is_known()) {
                return Err(CoreError::Catalog(format!("{} : caractéristique inconnue « {unknown} »", f.id)));
            }
        }
        // Les formats « principaux » passent avant les variantes pour une même extension.
        for list in by_ext.values_mut() {
            list.sort_by_key(|&i| (formats[i].is_variant(), i));
        }
        Ok(Self { formats, by_id, by_ext, by_mime })
    }

    pub fn all(&self) -> &[FormatDef] {
        &self.formats
    }

    pub fn get(&self, id: &str) -> Option<&FormatDef> {
        self.by_id.get(id).map(|&i| &self.formats[i])
    }

    pub fn by_extension(&self, ext: &str) -> Vec<&FormatDef> {
        let ext = ext.trim_start_matches('.').to_lowercase();
        self.by_ext.get(&ext).map(|v| v.iter().map(|&i| &self.formats[i]).collect()).unwrap_or_default()
    }

    pub fn by_mime(&self, mime: &str) -> Option<&FormatDef> {
        self.by_mime.get(&mime.to_ascii_lowercase()).and_then(|v| v.iter().map(|&i| &self.formats[i]).find(|f| !f.is_variant()))
    }

    /// Devine le format d'après le nom de fichier, en tenant compte des extensions
    /// composées (`archive.tar.gz`). Les variantes ne sont jamais renvoyées.
    pub fn guess_from_name(&self, file_name: &str) -> Option<&FormatDef> {
        let lower = file_name.to_lowercase();
        let mut start = 0;
        // Du suffixe le plus long au plus court : « a.tar.gz » → « tar.gz » puis « gz ».
        while let Some(pos) = lower[start..].find('.') {
            let suffix = &lower[start + pos + 1..];
            if !suffix.is_empty()
                && let Some(f) = self.by_extension(suffix).into_iter().find(|f| !f.is_variant())
            {
                return Some(f);
            }
            start += pos + 1;
        }
        None
    }

    /// Vrai si `a` et `b` désignent le même format, une variante l'un de l'autre, ou si l'un
    /// est porté par l'autre (CBZ ↔ ZIP, NEF ↔ TIFF…).
    pub fn related(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        let (Some(fa), Some(fb)) = (self.get(a), self.get(b)) else {
            return false;
        };
        fa.variant_of.as_ref().is_some_and(|v| v == b)
            || fb.variant_of.as_ref().is_some_and(|v| v == a)
            || fa.based_on.iter().any(|x| x == b)
            || fb.based_on.iter().any(|x| x == a)
    }

    /// Recherche rapide (identifiant, nom, extension), pour le sélecteur de formats.
    pub fn search(&self, query: &str) -> Vec<&FormatDef> {
        let q = query.trim().trim_start_matches('.').to_lowercase();
        if q.is_empty() {
            return self.formats.iter().collect();
        }
        let mut seen = HashSet::new();
        let mut exact = Vec::new();
        let mut partial = Vec::new();
        for f in &self.formats {
            let is_exact = f.id.as_str() == q || f.extensions.contains(&q);
            let is_partial =
                f.id.as_str().contains(&q) || f.name.to_lowercase().contains(&q) || f.extensions.iter().any(|e| e.contains(&q));
            if is_exact && seen.insert(f.id.clone()) {
                exact.push(f);
            } else if is_partial && seen.insert(f.id.clone()) {
                partial.push(f);
            }
        }
        exact.extend(partial);
        exact
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_valid_and_rich() {
        let cat = FormatCatalog::builtin();
        assert!(cat.all().len() >= 150, "catalogue trop petit : {}", cat.all().len());
        for c in [Category::Image, Category::Audio, Category::Video, Category::Archive, Category::Data] {
            assert!(cat.all().iter().filter(|f| f.category == c).count() >= 8, "{c:?}");
        }
    }

    #[test]
    fn compound_extensions_win() {
        let cat = FormatCatalog::builtin();
        assert_eq!(cat.guess_from_name("backup.tar.gz").unwrap().id, "tgz");
        assert_eq!(cat.guess_from_name("x.gz").unwrap().id, "gz");
        assert_eq!(cat.guess_from_name("Mon.Film.2024.MKV").unwrap().id, "mkv");
        assert!(cat.guess_from_name("sans_extension").is_none());
    }

    #[test]
    fn variants_are_never_guessed() {
        let cat = FormatCatalog::builtin();
        assert_eq!(cat.guess_from_name("a.m4a").unwrap().id, "m4a");
        assert_eq!(cat.guess_from_name("a.pdf").unwrap().id, "pdf");
        assert_eq!(cat.guess_from_name("a.png").unwrap().id, "png");
    }

    #[test]
    fn relations() {
        let cat = FormatCatalog::builtin();
        assert!(cat.related("cbz", "zip"));
        assert!(cat.related("alac", "m4a"));
        assert!(cat.related("nef", "tiff"));
        assert!(!cat.related("png", "jpeg"));
    }

    #[test]
    fn rejects_unknown_feature_and_bad_reference() {
        let bad = r#"[[format]]
id = "x"
name = "X"
category = "image"
compression = "none"
features = ["teleportation"]"#;
        assert!(FormatCatalog::from_toml(bad).is_err());
        let bad_ref = r#"[[format]]
id = "x"
name = "X"
category = "image"
compression = "none"
based_on = ["nope"]"#;
        assert!(FormatCatalog::from_toml(bad_ref).is_err());
    }

    #[test]
    fn search_puts_exact_matches_first() {
        let cat = FormatCatalog::builtin();
        let r = cat.search("jpg");
        assert_eq!(r[0].id, "jpeg");
        assert!(cat.search("matroska").iter().any(|f| f.id == "mkv"));
    }
}
