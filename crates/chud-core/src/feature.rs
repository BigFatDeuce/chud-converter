//! Caractéristiques qu'un fichier peut contenir (transparence, EXIF, chapitres…).
//!
//! Elles servent à calculer, avant conversion, ce qui sera perdu : une caractéristique présente
//! dans la source mais absente du format cible, ou non conservée par le moteur, est signalée.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Feature(String);

pub type FeatureSet = BTreeSet<Feature>;

/// Construit un ensemble de caractéristiques à partir d'identifiants.
pub fn features(list: &[&str]) -> FeatureSet {
    list.iter().map(|s| Feature::new(*s)).collect()
}

impl Feature {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_known(&self) -> bool {
        KNOWN.iter().any(|(k, _, _)| *k == self.0)
    }

    /// Libellé dans la langue courante (l'identifiant brut s'il est inconnu).
    pub fn label(&self) -> &str {
        KNOWN.iter().find(|(k, _, _)| *k == self.0).map(|(_, fr, en)| crate::tr!(*fr, *en)).unwrap_or(&self.0)
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Vocabulaire contrôlé (identifiant, français, anglais) : le catalogue est refusé s'il utilise
/// un identifiant inconnu.
pub const KNOWN: &[(&str, &str, &str)] = &[
    // Images
    ("alpha", "transparence (canal alpha)", "transparency (alpha channel)"),
    ("animation", "animation", "animation"),
    ("layers", "calques", "layers"),
    ("multipage", "pages ou images multiples", "multiple pages or frames"),
    ("vector", "graphismes vectoriels", "vector graphics"),
    ("raster", "données raster", "raster data"),
    ("icc", "profil colorimétrique ICC", "ICC color profile"),
    ("exif", "métadonnées EXIF", "EXIF metadata"),
    ("xmp", "métadonnées XMP", "XMP metadata"),
    ("iptc", "métadonnées IPTC", "IPTC metadata"),
    ("high_bit_depth", "profondeur de couleur > 8 bits", "color depth > 8 bits"),
    ("hdr", "plage dynamique étendue (HDR)", "high dynamic range (HDR)"),
    ("cmyk", "couleurs CMJN", "CMYK colors"),
    ("raw", "données de capteur RAW", "RAW sensor data"),
    ("text_layer", "texte sélectionnable", "selectable text"),
    // Audio / vidéo
    ("video", "piste vidéo", "video track"),
    ("audio", "piste audio", "audio track"),
    ("multi_audio", "pistes audio multiples", "multiple audio tracks"),
    ("multi_video", "pistes vidéo multiples", "multiple video tracks"),
    ("subtitles", "sous-titres texte", "text subtitles"),
    ("image_subtitles", "sous-titres image (PGS/VobSub)", "image subtitles (PGS/VobSub)"),
    ("chapters", "chapitres", "chapters"),
    ("tags", "tags et métadonnées", "tags and metadata"),
    ("cover_art", "pochette intégrée", "embedded cover art"),
    ("attachments", "pièces jointes (polices…)", "attachments (fonts…)"),
    ("surround", "son multicanal", "multichannel sound"),
    ("timecode", "timecode", "timecode"),
    ("notation", "notation musicale (notes)", "musical notation (notes)"),
    ("timed_text", "texte minuté", "timed text"),
    ("positioning", "positionnement du texte", "text positioning"),
    // Documents
    ("rich_text", "texte enrichi", "rich text"),
    ("styles", "styles et mise en forme", "styles and formatting"),
    ("images", "images intégrées", "embedded images"),
    ("tables", "tableaux", "tables"),
    ("comments", "commentaires", "comments"),
    ("track_changes", "suivi des modifications", "tracked changes"),
    ("metadata", "métadonnées du document", "document metadata"),
    ("fonts_embedded", "polices intégrées", "embedded fonts"),
    ("forms", "formulaires", "forms"),
    ("macros", "macros", "macros"),
    ("scripts", "scripts", "scripts"),
    ("formulas", "formules", "formulas"),
    ("code", "code exécutable (cellules)", "executable code (cells)"),
    ("multi_sheet", "feuilles ou tables multiples", "multiple sheets or tables"),
    ("charts", "graphiques", "charts"),
    ("slides", "diapositives", "slides"),
    ("notes", "notes de l'orateur", "speaker notes"),
    ("media", "médias intégrés", "embedded media"),
    ("bookmarks", "signets", "bookmarks"),
    ("encryption", "chiffrement", "encryption"),
    ("toc", "table des matières", "table of contents"),
    // Archives
    ("directories", "arborescence de dossiers", "folder tree"),
    ("timestamps", "dates des fichiers", "file dates"),
    ("permissions", "permissions Unix", "Unix permissions"),
    ("symlinks", "liens symboliques", "symbolic links"),
    ("recovery", "données de récupération", "recovery data"),
    // Données
    ("nesting", "structures imbriquées", "nested structures"),
    ("types", "types de valeurs (nombres, booléens…)", "value types (numbers, booleans…)"),
    ("null", "valeurs nulles", "null values"),
    ("anchors", "ancres et alias YAML", "YAML anchors and aliases"),
    ("datetime", "dates typées", "typed dates"),
    ("attributes", "attributs", "attributes"),
    ("schema", "schéma", "schema"),
    // Polices, 3D, géo
    ("hinting", "hinting", "hinting"),
    ("mesh", "maillage", "mesh"),
    ("materials", "matériaux", "materials"),
    ("uv", "coordonnées de texture", "texture coordinates"),
    ("scene", "hiérarchie de scène", "scene hierarchy"),
    ("skinning", "squelette et pondérations", "skeleton and weights"),
    ("vertex_colors", "couleurs de sommets", "vertex colors"),
    ("brep", "géométrie exacte (B-Rep)", "exact geometry (B-Rep)"),
    ("assemblies", "assemblages", "assemblies"),
    ("crs", "système de coordonnées", "coordinate reference system"),
];
