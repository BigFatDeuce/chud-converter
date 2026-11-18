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
        KNOWN.iter().any(|(k, _)| *k == self.0)
    }

    /// Libellé français destiné aux rapports et à la CLI (l'interface graphique traduit
    /// elle-même à partir de l'identifiant).
    pub fn label_fr(&self) -> &str {
        KNOWN.iter().find(|(k, _)| *k == self.0).map(|(_, l)| *l).unwrap_or(&self.0)
    }
}

impl fmt::Display for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Vocabulaire contrôlé : le catalogue est refusé s'il utilise un identifiant inconnu.
pub const KNOWN: &[(&str, &str)] = &[
    // Images
    ("alpha", "transparence (canal alpha)"),
    ("animation", "animation"),
    ("layers", "calques"),
    ("multipage", "pages ou images multiples"),
    ("vector", "graphismes vectoriels"),
    ("raster", "données raster"),
    ("icc", "profil colorimétrique ICC"),
    ("exif", "métadonnées EXIF"),
    ("xmp", "métadonnées XMP"),
    ("iptc", "métadonnées IPTC"),
    ("high_bit_depth", "profondeur de couleur > 8 bits"),
    ("hdr", "plage dynamique étendue (HDR)"),
    ("cmyk", "couleurs CMJN"),
    ("raw", "données de capteur RAW"),
    ("text_layer", "texte sélectionnable"),
    // Audio / vidéo
    ("video", "piste vidéo"),
    ("audio", "piste audio"),
    ("multi_audio", "pistes audio multiples"),
    ("multi_video", "pistes vidéo multiples"),
    ("subtitles", "sous-titres texte"),
    ("image_subtitles", "sous-titres image (PGS/VobSub)"),
    ("chapters", "chapitres"),
    ("tags", "tags et métadonnées"),
    ("cover_art", "pochette intégrée"),
    ("attachments", "pièces jointes (polices…)"),
    ("surround", "son multicanal"),
    ("timecode", "timecode"),
    ("notation", "notation musicale (notes)"),
    ("timed_text", "texte minuté"),
    ("positioning", "positionnement du texte"),
    // Documents
    ("rich_text", "texte enrichi"),
    ("styles", "styles et mise en forme"),
    ("images", "images intégrées"),
    ("tables", "tableaux"),
    ("comments", "commentaires"),
    ("track_changes", "suivi des modifications"),
    ("metadata", "métadonnées du document"),
    ("fonts_embedded", "polices intégrées"),
    ("forms", "formulaires"),
    ("macros", "macros"),
    ("scripts", "scripts"),
    ("formulas", "formules"),
    ("code", "code exécutable (cellules)"),
    ("multi_sheet", "feuilles ou tables multiples"),
    ("charts", "graphiques"),
    ("slides", "diapositives"),
    ("notes", "notes de l'orateur"),
    ("media", "médias intégrés"),
    ("bookmarks", "signets"),
    ("encryption", "chiffrement"),
    ("toc", "table des matières"),
    // Archives
    ("directories", "arborescence de dossiers"),
    ("timestamps", "dates des fichiers"),
    ("permissions", "permissions Unix"),
    ("symlinks", "liens symboliques"),
    ("recovery", "données de récupération"),
    // Données
    ("nesting", "structures imbriquées"),
    ("types", "types de valeurs (nombres, booléens…)"),
    ("null", "valeurs nulles"),
    ("anchors", "ancres et alias YAML"),
    ("datetime", "dates typées"),
    ("attributes", "attributs"),
    ("schema", "schéma"),
    // Polices, 3D, géo
    ("hinting", "hinting"),
    ("mesh", "maillage"),
    ("materials", "matériaux"),
    ("uv", "coordonnées de texture"),
    ("scene", "hiérarchie de scène"),
    ("skinning", "squelette et pondérations"),
    ("vertex_colors", "couleurs de sommets"),
    ("brep", "géométrie exacte (B-Rep)"),
    ("assemblies", "assemblages"),
    ("crs", "système de coordonnées"),
];
