//! Opérations sur des documents : ce qui n'est pas une conversion d'un fichier vers un autre
//! format (fusionner, diviser ou pivoter des PDF, réunir plusieurs fichiers dans une archive).
//!
//! Le catalogue est fixe ([`operations`]) : l'interface peut montrer une opération dont le moteur
//! manque, et proposer de l'installer. Chaque moteur déclare celles qu'il sait exécuter
//! ([`crate::Engine::operations`]).

use crate::engine::{EngineId, Fidelity};
use crate::format::{Category, FormatId};
use crate::options::OptionValues;
use serde::Serialize;
use std::path::PathBuf;

pub const PDF_MERGE: &str = "pdf-merge";
pub const PDF_SPLIT: &str = "pdf-split";
pub const PDF_ROTATE: &str = "pdf-rotate";
pub const ARCHIVE_CREATE: &str = "archive-create";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    /// Tous les fichiers, dans l'ordre, donnent un seul résultat.
    Together,
    /// Chaque fichier donne son propre résultat.
    Each,
}

#[derive(Clone, Debug, Serialize)]
pub struct Operation {
    pub id: &'static str,
    pub engine: EngineId,
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
    pub grouping: Grouping,
    pub min_inputs: usize,
    pub max_inputs: usize,
    /// Formats acceptés ; vide : tout fichier, et les dossiers.
    pub accepts: Vec<FormatId>,
    /// L'ordre des fichiers compte (fusion).
    pub ordered: bool,
}

/// Opérations connues, dans l'ordre d'affichage.
pub fn operations() -> Vec<Operation> {
    let pdf = || vec![FormatId::new("pdf")];
    vec![
        Operation {
            id: PDF_MERGE,
            engine: EngineId::new("qpdf"),
            name: crate::tr!("Fusionner des PDF", "Merge PDFs"),
            description: crate::tr!(
                "Réunit plusieurs PDF en un seul, dans l'ordre de la liste.",
                "Joins several PDFs into one, in the order of the list."
            ),
            category: Category::Pdf,
            grouping: Grouping::Together,
            min_inputs: 2,
            max_inputs: 1000,
            accepts: pdf(),
            ordered: true,
        },
        Operation {
            id: PDF_SPLIT,
            engine: EngineId::new("qpdf"),
            name: crate::tr!("Diviser un PDF", "Split a PDF"),
            description: crate::tr!(
                "Un PDF par page, toutes les N pages, ou selon vos plages.",
                "One PDF per page, every N pages, or by your own ranges."
            ),
            category: Category::Pdf,
            grouping: Grouping::Each,
            min_inputs: 1,
            max_inputs: 1000,
            accepts: pdf(),
            ordered: false,
        },
        Operation {
            id: PDF_ROTATE,
            engine: EngineId::new("qpdf"),
            name: crate::tr!("Pivoter un PDF", "Rotate a PDF"),
            description: crate::tr!(
                "Tourne toutes les pages, ou certaines, sans rien recompresser.",
                "Turns all pages, or some, without recompressing anything."
            ),
            category: Category::Pdf,
            grouping: Grouping::Each,
            min_inputs: 1,
            max_inputs: 1000,
            accepts: pdf(),
            ordered: false,
        },
        Operation {
            id: ARCHIVE_CREATE,
            engine: EngineId::new("7zip"),
            name: crate::tr!("Créer une archive", "Create an archive"),
            description: crate::tr!(
                "Réunit fichiers et dossiers dans une seule archive (ZIP, 7z, TAR…).",
                "Puts files and folders into a single archive (ZIP, 7z, TAR…)."
            ),
            category: Category::Archive,
            grouping: Grouping::Together,
            min_inputs: 1,
            max_inputs: 10_000,
            accepts: Vec::new(),
            ordered: false,
        },
    ]
}

/// Opération du catalogue.
pub fn operation(id: &str) -> Option<Operation> {
    operations().into_iter().find(|o| o.id == id)
}

/// Une opération à exécuter par un moteur.
pub struct OperationTask {
    pub operation: String,
    /// Chemins d'origine, dans l'ordre choisi. Jamais modifiés.
    pub inputs: Vec<PathBuf>,
    /// Fichier à produire, ou dossier si le résultat est `directory`.
    pub output: PathBuf,
    pub target: FormatId,
    /// Options déjà validées et complétées.
    pub options: OptionValues,
}

/// Ce qu'une opération conserve ou perd pour ces fichiers précis.
#[derive(Clone, Debug, Serialize)]
pub struct OperationReport {
    pub fidelity: Fidelity,
    /// Ce qui ne sera pas conservé.
    pub lost: Vec<String>,
    /// Autres précisions (nombre de pages, nom changé dans l'archive…).
    pub notes: Vec<String>,
}

impl Default for OperationReport {
    fn default() -> Self {
        Self { fidelity: Fidelity::Lossless, lost: Vec::new(), notes: Vec::new() }
    }
}

impl OperationReport {
    pub fn headline(&self) -> &'static str {
        crate::planner::headline(self.fidelity, !self.lost.is_empty())
    }

    pub fn details(&self) -> Vec<String> {
        self.notes.iter().cloned().chain(self.lost.iter().map(|l| crate::trf!("Non conservé : {l}", "Not kept: {l}"))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        let all = operations();
        for (i, o) in all.iter().enumerate() {
            assert!(o.min_inputs >= 1 && o.max_inputs >= o.min_inputs);
            assert!(!all[..i].iter().any(|p| p.id == o.id), "{}", o.id);
            assert!(operation(o.id).is_some());
            assert!(o.grouping == Grouping::Together || o.min_inputs == 1, "{} : un résultat par fichier", o.id);
        }
        assert!(operation("inconnue").is_none());
    }
}
