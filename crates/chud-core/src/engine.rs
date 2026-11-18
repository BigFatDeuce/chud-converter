//! Contrat commun à tous les moteurs (natifs, adaptateurs d'outils externes, plugins).

use crate::cancel::CancelToken;
use crate::feature::FeatureSet;
use crate::format::FormatId;
use crate::options::{OptionSpec, OptionValues};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EngineId(String);

impl EngineId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Fidélité d'une étape de conversion. L'ordre (`Lossless < Depends < Lossy`) sert au tri.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// Aucune perte d'information sur le contenu (pixels, échantillons, texte).
    Lossless,
    /// Dépend du contenu réel ou des options (ex. remux possible si les codecs sont compatibles).
    Depends,
    /// Compression ou transformation avec perte.
    Lossy,
}

impl Fidelity {
    pub fn label(self) -> &'static str {
        match self {
            Fidelity::Lossless => crate::tr!("sans perte", "lossless"),
            Fidelity::Depends => crate::tr!("selon le contenu", "depends on content"),
            Fidelity::Lossy => crate::tr!("avec perte", "lossy"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineOrigin {
    /// Compilé dans l'application.
    Builtin,
    /// Installé et géré par l'application.
    Managed,
    /// Installation du système (PATH, gestionnaire de paquets, Program Files) : lecture seule.
    System,
    /// Chemin choisi par l'utilisateur.
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineInfo {
    pub id: EngineId,
    pub name: String,
    pub version: Option<String>,
    pub license: String,
    pub homepage: String,
    pub origin: EngineOrigin,
    pub path: Option<PathBuf>,
    pub description: String,
}

/// Arête déclarée : ce moteur sait convertir `from` en `to`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversionCap {
    pub from: FormatId,
    pub to: FormatId,
    pub fidelity: Fidelity,
    /// Caractéristiques conservées quand le format cible les accepte.
    pub preserves: FeatureSet,
    /// Coût de préférence (plus bas = préféré quand plusieurs moteurs conviennent).
    pub cost: u32,
}

impl ConversionCap {
    pub fn new(from: &str, to: &str, fidelity: Fidelity, preserves: &FeatureSet, cost: u32) -> Self {
        Self { from: from.into(), to: to.into(), fidelity, preserves: preserves.clone(), cost }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Video,
    Audio,
    Subtitle,
    Data,
    Attachment,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamInfo {
    pub index: u32,
    pub kind: StreamKind,
    pub codec: String,
    #[serde(default)]
    pub attached_pic: bool,
    pub language: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub channels: Option<u32>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    /// Format des échantillons ou des pixels (`s16`, `fltp`, `yuv420p10le`…).
    #[serde(default)]
    pub sample_format: Option<String>,
    pub frame_rate: Option<f64>,
    pub bitrate: Option<u64>,
}

/// Résultat d'une sonde rapide du fichier source.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SourceInfo {
    /// Caractéristiques réellement présentes (si la sonde a pu les établir).
    pub features: Option<FeatureSet>,
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub streams: Vec<StreamInfo>,
    /// Résumé lisible (« Codec vidéo » → « h264 »…).
    pub summary: Vec<(String, String)>,
}

impl SourceInfo {
    pub fn streams_of(&self, kind: StreamKind) -> impl Iterator<Item = &StreamInfo> {
        self.streams.iter().filter(move |s| s.kind == kind && !s.attached_pic)
    }
}

pub struct StepQuery<'a> {
    pub from: &'a FormatId,
    pub to: &'a FormatId,
    /// Sonde de l'entrée de cette étape (disponible pour la première étape seulement).
    pub source: Option<&'a SourceInfo>,
    pub options: &'a OptionValues,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepAssessment {
    pub fidelity: Fidelity,
    pub notes: Vec<String>,
    /// Si présent, l'étape est impossible pour ce fichier précis (ex. aucune piste audio).
    pub unavailable: Option<String>,
    /// Caractéristiques réellement conservées pour ce fichier précis (remplace la déclaration
    /// générale : un remux conserve le HDR, un ré-encodage H.264 8 bits non).
    pub preserves: Option<FeatureSet>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Progress {
    /// Avancement de l'étape entre 0 et 1, si connu de façon fiable.
    pub fraction: Option<f64>,
    pub speed: Option<String>,
    pub message: Option<String>,
}

/// Contexte d'exécution fourni par la tâche au moteur.
#[derive(Clone)]
pub struct ExecContext {
    pub cancel: CancelToken,
    pub progress: Arc<dyn Fn(Progress) + Send + Sync>,
    /// Répertoire de travail propre à la tâche (staging).
    pub work_dir: PathBuf,
    pub timeout: Option<Duration>,
    /// Plafond mémoire par processus externe, en octets.
    pub memory_limit: Option<u64>,
}

impl ExecContext {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { cancel: CancelToken::new(), progress: Arc::new(|_| {}), work_dir, timeout: None, memory_limit: None }
    }

    pub fn report(&self, p: Progress) {
        (self.progress)(p)
    }

    pub fn check_cancelled(&self) -> Result<(), EngineError> {
        if self.cancel.is_cancelled() { Err(EngineError::Cancelled) } else { Ok(()) }
    }
}

pub struct StepTask {
    pub input: PathBuf,
    pub output: PathBuf,
    pub from: FormatId,
    pub to: FormatId,
    /// Options déjà validées et complétées par les valeurs par défaut.
    pub options: OptionValues,
    pub source: Option<SourceInfo>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct StepOutput {
    pub notes: Vec<String>,
}

/// Niveau de vérification atteint sur un fichier produit.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "level", content = "detail", rename_all = "snake_case")]
pub enum Verification {
    /// Le résultat a été entièrement décodé ou ré-analysé par un outil indépendant.
    Decoded(String),
    /// La structure a été contrôlée (en-têtes, flux, test d'intégrité).
    Structural(String),
    /// Seule la signature du fichier a été contrôlée.
    SignatureOnly,
    Failed(String),
}

impl Verification {
    pub fn is_ok(&self) -> bool {
        !matches!(self, Verification::Failed(_))
    }
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{}", crate::tr!("opération annulée", "operation cancelled"))]
    Cancelled,
    #[error("{t} ({0:?})", t = crate::tr!("délai maximal dépassé", "time limit exceeded"))]
    Timeout(Duration),
    #[error("{t}{0}", t = crate::tr!("conversion non prise en charge : ", "unsupported conversion: "))]
    Unsupported(String),
    #[error("{}", crate::trf!("option « {key} » : {message}", "option “{key}”: {message}"))]
    InvalidOption { key: String, message: String },
    #[error("{t}{0}", t = crate::tr!("fichier d'entrée invalide ou corrompu : ", "invalid or corrupt input file: "))]
    InvalidInput(String),
    #[error("{t}{0}", t = crate::tr!("refusé pour raison de sécurité : ", "refused for safety reasons: "))]
    Refused(String),
    #[error("{}", crate::trf!("le moteur a échoué{} : {detail}", "the engine failed{}: {detail}", exit_code_suffix(code)))]
    Process { code: Option<i32>, detail: String },
    #[error("{t}{0}", t = crate::tr!("erreur d'entrée/sortie : ", "input/output error: "))]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

fn exit_code_suffix(code: &Option<i32>) -> String {
    code.map(|c| crate::trf!(" (code de sortie {c})", " (exit code {c})")).unwrap_or_default()
}

impl EngineError {
    /// Code stable, traduit par l'interface.
    pub fn code(&self) -> &'static str {
        match self {
            EngineError::Cancelled => "cancelled",
            EngineError::Timeout(_) => "timeout",
            EngineError::Unsupported(_) => "unsupported",
            EngineError::InvalidOption { .. } => "invalid_option",
            EngineError::InvalidInput(_) => "invalid_input",
            EngineError::Refused(_) => "refused",
            EngineError::Process { .. } => "engine_failed",
            EngineError::Io(_) => "io",
            EngineError::Other(_) => "other",
        }
    }
}

pub trait Engine: Send + Sync {
    fn info(&self) -> &EngineInfo;

    fn id(&self) -> &EngineId {
        &self.info().id
    }

    /// Conversions réellement disponibles (après sonde des capacités pour un outil externe).
    fn conversions(&self) -> &[ConversionCap];

    /// Options pertinentes pour une conversion donnée.
    fn options(&self, _from: &FormatId, _to: &FormatId) -> Vec<OptionSpec> {
        Vec::new()
    }

    /// Indique si ce moteur sait sonder (`probe`) les fichiers de ce format.
    fn can_probe(&self, _format: &FormatId) -> bool {
        false
    }

    /// Sonde rapide du fichier : flux, durée, dimensions, caractéristiques présentes.
    fn probe(&self, _path: &Path, _format: &FormatId) -> Option<SourceInfo> {
        None
    }

    /// Fidélité précise d'une étape pour un fichier donné. Par défaut : la fidélité déclarée.
    fn assess(&self, q: &StepQuery<'_>) -> StepAssessment {
        let cap = self.conversions().iter().find(|c| &c.from == q.from && &c.to == q.to);
        match cap {
            Some(c) => StepAssessment { fidelity: c.fidelity, notes: Vec::new(), unavailable: None, preserves: None },
            None => StepAssessment {
                fidelity: Fidelity::Lossy,
                notes: Vec::new(),
                unavailable: Some(crate::tr!("conversion non déclarée par le moteur", "conversion not declared by the engine").into()),
                preserves: None,
            },
        }
    }

    fn execute(&self, task: &StepTask, ctx: &ExecContext) -> Result<StepOutput, EngineError>;

    /// Vérifie un fichier produit par ce moteur.
    fn verify(&self, _output: &Path, _task: &StepTask, _ctx: &ExecContext) -> Verification {
        Verification::SignatureOnly
    }

    /// Poids d'ordonnancement (1 = léger ; une conversion vidéo occupe plusieurs cœurs).
    fn weight(&self, _from: &FormatId, _to: &FormatId) -> u32 {
        1
    }
}
