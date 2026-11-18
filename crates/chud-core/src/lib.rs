//! Cœur de Chud Converter.
//!
//! Ce crate ne dépend d'aucun moteur ni d'aucune interface. Il fournit :
//! - le catalogue de formats ([`format`]) décrit en données (`data/formats.toml`) ;
//! - la détection de format par signature, conteneur, contenu et extension ([`detect`]) ;
//! - le contrat commun des moteurs ([`engine`]) et le schéma d'options ([`options`]) ;
//! - le registre des moteurs ([`registry`]) et le planificateur de conversions ([`planner`]),
//!   qui choisit le chemin le plus fidèle et produit un rapport de fidélité honnête.

pub mod cancel;
pub mod detect;
pub mod engine;
pub mod error;
pub mod feature;
pub mod format;
pub mod i18n;
pub mod options;
pub mod planner;
pub mod registry;

pub use cancel::CancelToken;
pub use detect::{Confidence, Detection, DetectionMethod, detect_path};
pub use engine::{
    ConversionCap, Engine, EngineError, EngineId, EngineInfo, EngineOrigin, ExecContext, Fidelity, Progress, SourceInfo, StepAssessment,
    StepOutput, StepQuery, StepTask, StreamInfo, StreamKind, Verification,
};
pub use error::CoreError;
pub use feature::{Feature, FeatureSet};
pub use format::{Category, Compression, FormatCatalog, FormatDef, FormatId};
pub use i18n::{Lang, LangChoice, lang, set_lang};
pub use options::{OptionKind, OptionSpec, OptionValue, OptionValues};
pub use planner::{FidelityReport, Plan, PlanCost, PlanStep, Planner, Warning};
pub use registry::{PlanError, Registry, TargetChoice};
