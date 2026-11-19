//! Logique de l'application de bureau, indépendante de Tauri (testable seule) : moteurs,
//! analyse des fichiers, cibles possibles, plan et options, file de conversions.
//!
//! Tout ce qui part vers l'interface est une « vue » sérialisable en camelCase ; les textes
//! (rapports de fidélité, erreurs) sont produits ici par le cœur, dans la langue choisie.

use chud_core::{
    Category, Fidelity, FidelityReport, FormatCatalog, FormatId, OptionSpec, OptionValue, OptionValues, SourceInfo, StreamInfo, StreamKind,
    Verification, detect_path,
};
use chud_core::{Lang, LangChoice};
use chud_enginemgr::{
    AppPaths, EngineManager, EngineState, EngineStatus, InstallError, InstallOffer, InstallProgress, Settings, UpdateSettings,
};
use chud_jobs::{Conflict, JobEvent, JobId, JobQueue, JobRunner, JobSpec, Preset, PresetStore, QueueEvent, QueueListener};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

fn catalog() -> &'static FormatCatalog {
    FormatCatalog::builtin()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn format_name(id: &FormatId) -> String {
    catalog().get(id.as_str()).map(|d| d.label().to_owned()).unwrap_or_else(|| id.to_string())
}

// ---------------------------------------------------------------------------------------------
// Vues envoyées à l'interface
// ---------------------------------------------------------------------------------------------

/// Fidélité telle que l'interface l'affiche (badge coloré).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FidelityLevel {
    /// Rien n'est perdu.
    Lossless,
    /// Contenu intact, mais certaines informations ne suivent pas (métadonnées, pistes…).
    Partial,
    /// Selon le contenu réel du fichier.
    Depends,
    Lossy,
}

impl FidelityLevel {
    fn of(report: &FidelityReport) -> Self {
        match report.overall {
            Fidelity::Lossless if report.lost_features.is_empty() => Self::Lossless,
            Fidelity::Lossless => Self::Partial,
            Fidelity::Depends => Self::Depends,
            Fidelity::Lossy => Self::Lossy,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub format: Option<String>,
    pub format_name: Option<String>,
    pub category: Option<Category>,
    pub confidence: Option<&'static str>,
    pub method: Option<&'static str>,
    pub extension_mismatch: bool,
    pub note: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetView {
    pub format: String,
    pub name: String,
    pub category: Category,
    pub extension: Option<String>,
    pub fidelity: FidelityLevel,
    pub headline: String,
    pub lost: Vec<String>,
    pub engines: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTargets {
    pub path: String,
    pub source: String,
    pub summary: Vec<(String, String)>,
    pub targets: Vec<TargetView>,
    /// « Qualité d'origine » : ce qui ressort le contenu sans rien perdre (audio d'origine…).
    pub original: Vec<OriginalChoice>,
}

/// Proposition « qualité d'origine » : un format cible déjà présent dans `targets`, sans perte.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OriginalChoice {
    /// `audio`, `subtitles`, `video` ou `image` : une seule proposition par sorte.
    pub kind: &'static str,
    pub format: String,
    pub title: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub engine: String,
    pub from: String,
    pub to: String,
    pub fidelity: Fidelity,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub source: String,
    pub source_name: String,
    pub target: String,
    pub target_name: String,
    pub fidelity: FidelityLevel,
    pub headline: String,
    pub details: Vec<String>,
    pub steps: Vec<StepView>,
    /// Options de toutes les étapes, sans doublon de clé.
    pub options: Vec<OptionSpec>,
    pub output: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineView {
    pub id: String,
    pub name: String,
    /// ready | no_adapter | broken | missing | disabled
    pub state: &'static str,
    pub problem: Option<String>,
    pub version: Option<String>,
    pub path: Option<String>,
    pub origin: Option<String>,
    pub license: String,
    pub homepage: String,
    pub description: String,
    pub hint: Option<String>,
    /// Chud Converter sait se servir de ce moteur (sinon : détecté seulement, intégration à venir).
    pub supported: bool,
    /// Familles de fichiers traitées (video, audio, archive…).
    pub categories: Vec<String>,
    /// Paquet que Chud Converter sait installer lui-même ici (version, taille, éditeur, licence).
    pub installable: Option<InstallOffer>,
    /// Version précédente d'une installation gérée (retour en arrière possible).
    pub previous: Option<String>,
    /// Une installation de ce moteur est en cours (une fenêtre rouverte la retrouve).
    pub installing: bool,
}

impl From<&EngineStatus> for EngineView {
    fn from(s: &EngineStatus) -> Self {
        let (state, problem) = match &s.state {
            EngineState::Ready => ("ready", None),
            EngineState::NoAdapter => ("no_adapter", None),
            EngineState::Broken(e) => ("broken", Some(e.clone())),
            EngineState::Missing => ("missing", None),
            EngineState::Disabled => ("disabled", None),
        };
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            state,
            problem,
            version: s.version.clone(),
            path: s.path.as_ref().map(|p| p.display().to_string()),
            origin: s.origin.and_then(|o| serde_json::to_value(o).ok()).and_then(|v| v.as_str().map(str::to_owned)),
            license: s.license.clone(),
            homepage: s.homepage.clone(),
            description: s.description.clone(),
            hint: s.hint.clone(),
            supported: s.supported,
            categories: s.categories.clone(),
            installable: s.installable.clone(),
            previous: s.previous.clone(),
            installing: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub portable: bool,
    pub formats: usize,
    pub conversions: usize,
    pub engines_ready: usize,
}

/// Réglages de l'application montrés dans l'écran « Réglages ».
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    /// Choix enregistré : system, fr ou en.
    pub language: LangChoice,
    /// Langue réellement utilisée.
    pub effective: Lang,
    /// Langue du système (pour l'étiquette « Système »).
    pub system: Lang,
    pub file: String,
    /// Fichier de réglages illisible (il n'est jamais écrasé).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Après un changement : mise à jour du menu clic droit installé.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_note: Option<String>,
    /// L'accueil du premier démarrage a déjà été montré.
    pub intro_seen: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanRequest {
    pub path: String,
    pub target: String,
    #[serde(default)]
    pub options: BTreeMap<String, OptionValue>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertRequest {
    pub files: Vec<String>,
    pub target: String,
    #[serde(default)]
    pub options: BTreeMap<String, OptionValue>,
    /// `None` : à côté de chaque original.
    pub output_dir: Option<String>,
    #[serde(default)]
    pub conflict: Conflict,
    #[serde(default)]
    pub keep_date: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submitted {
    pub path: String,
    pub id: JobId,
}

/// Préréglage tel que l'interface l'affiche et le renvoie.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    pub id: String,
    pub name: String,
    pub target: String,
    #[serde(default, skip_deserializing)]
    pub target_name: String,
    pub applies_to: Vec<String>,
    #[serde(default)]
    pub options: BTreeMap<String, OptionValue>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub name_template: Option<String>,
    #[serde(default)]
    pub conflict: Conflict,
    #[serde(default)]
    pub keep_date: bool,
    pub enabled: bool,
    /// Fait partie des préréglages fournis (même identifiant).
    #[serde(default, skip_deserializing)]
    pub builtin: bool,
}

impl PresetView {
    fn of(p: &Preset) -> Self {
        Self {
            id: p.id.clone(),
            name: p.display_name().to_owned(),
            target_name: format_name(&p.target),
            target: p.target.to_string(),
            applies_to: p.applies_to.clone(),
            options: p.options.clone(),
            output_dir: p.output_dir.as_ref().map(|d| d.display().to_string()),
            name_template: p.name_template.clone(),
            conflict: p.conflict,
            keep_date: p.keep_date,
            enabled: p.enabled,
            builtin: PresetStore::builtin().presets.iter().any(|b| b.id == p.id),
        }
    }

    fn into_preset(self) -> Preset {
        Preset {
            id: self.id,
            name: self.name.trim().to_owned(),
            target: FormatId::new(&self.target),
            applies_to: self.applies_to,
            options: self.options,
            output_dir: self.output_dir.filter(|d| !d.trim().is_empty()).map(PathBuf::from),
            name_template: self.name_template.filter(|t| !t.trim().is_empty()),
            conflict: self.conflict,
            keep_date: self.keep_date,
            enabled: self.enabled,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ignored {
    pub path: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunPreset {
    pub preset: PresetView,
    pub submitted: Vec<Submitted>,
    pub ignored: Vec<Ignored>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetsView {
    /// Emplacement de presets.toml (modifiable à la main).
    pub file: String,
    pub presets: Vec<PresetView>,
    /// Fichier illisible : rien n'est proposé ni enregistré tant qu'il n'est pas corrigé.
    pub error: Option<String>,
    /// Après un enregistrement : le menu clic droit installé a été mis à jour, ou pourquoi pas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_note: Option<String>,
}

/// État du menu clic droit de l'explorateur de fichiers.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellView {
    /// Pris en charge sur ce système (Windows, Linux).
    pub supported: bool,
    pub installed: bool,
    pub exe: Option<String>,
    pub exe_missing: bool,
    /// Le menu lance bien ce programme-ci (pas une autre copie, ailleurs).
    pub current: bool,
    pub entries: usize,
    pub location: String,
}

/// Lot lancé depuis le menu contextuel (`--preset`), affiché par la fenêtre de conversion rapide.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuickBatch {
    pub files: Vec<String>,
    pub run: Option<RunPreset>,
    pub error: Option<String>,
    /// Événements déjà survenus pour ces tâches : une conversion rapide peut se terminer avant
    /// que la fenêtre ne soit prête à écouter.
    pub events: Vec<UiEvent>,
}

/// Ce que demande la ligne de commande de `chud-converter`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// Ouvrir ces fichiers dans la fenêtre principale (éventuellement aucun).
    Open(Vec<String>),
    /// `--preset <id> [--] fichiers…` : conversion directe, sans passer par la fenêtre principale.
    Preset { id: String, files: Vec<String> },
}

impl Launch {
    /// Arguments sans le nom du programme. Les chemins restent des données, jamais interprétés.
    pub fn parse(args: impl IntoIterator<Item = std::ffi::OsString>) -> Self {
        let mut preset: Option<String> = None;
        let mut files = Vec::new();
        let mut options_done = false;
        let mut it = args.into_iter().map(|a| a.to_string_lossy().into_owned());
        while let Some(a) = it.next() {
            if a.is_empty() {
                continue;
            }
            if !options_done {
                if a == "--" {
                    options_done = true;
                    continue;
                }
                if a == "--preset" {
                    preset = Some(it.next().unwrap_or_default());
                    continue;
                }
                if let Some(id) = a.strip_prefix("--preset=") {
                    preset = Some(id.to_owned());
                    continue;
                }
            }
            files.push(a);
        }
        match preset {
            Some(id) => Self::Preset { id, files },
            None => Self::Open(files),
        }
    }
}

/// Événements de la file, déjà mis en forme pour l'interface.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum UiEvent {
    Started {
        id: JobId,
    },
    Planned {
        id: JobId,
        fidelity: FidelityLevel,
        headline: String,
        details: Vec<String>,
    },
    Step {
        id: JobId,
        index: usize,
        count: usize,
        engine: String,
        from: String,
        to: String,
    },
    Progress {
        id: JobId,
        fraction: Option<f64>,
        speed: Option<String>,
    },
    /// Rien n'a été écrit (fichier de sortie déjà présent et « ignorer » choisi, par exemple).
    Skipped {
        id: JobId,
        reason: String,
    },
    Done {
        id: JobId,
        output: Option<String>,
        skipped: bool,
        fidelity: Option<FidelityLevel>,
        headline: Option<String>,
        details: Vec<String>,
        verifications: Vec<Verification>,
        notes: Vec<String>,
        duration_ms: u64,
    },
    Failed {
        id: JobId,
        code: String,
        message: String,
    },
}

impl UiEvent {
    pub fn id(&self) -> JobId {
        match self {
            Self::Started { id }
            | Self::Planned { id, .. }
            | Self::Step { id, .. }
            | Self::Progress { id, .. }
            | Self::Skipped { id, .. }
            | Self::Done { id, .. }
            | Self::Failed { id, .. } => *id,
        }
    }

    pub fn from_queue(ev: QueueEvent) -> Option<Self> {
        Some(match ev {
            QueueEvent::Started { id } => UiEvent::Started { id },
            QueueEvent::Job { id, event } => match event {
                JobEvent::Planned { plan } => UiEvent::Planned {
                    id,
                    fidelity: FidelityLevel::of(&plan.report),
                    headline: plan.report.headline().to_owned(),
                    details: plan.report.details(catalog()),
                },
                JobEvent::Step { index, count, engine, from, to } => {
                    UiEvent::Step { id, index, count, engine: engine.to_string(), from: from.to_string(), to: to.to_string() }
                }
                JobEvent::Progress { fraction, speed, .. } => UiEvent::Progress { id, fraction, speed },
                JobEvent::Skipped { reason } => UiEvent::Skipped { id, reason },
                _ => return None,
            },
            QueueEvent::Done { id, outcome } => UiEvent::Done {
                id,
                skipped: outcome.output.is_none(),
                output: outcome.output.as_ref().map(|p| p.display().to_string()),
                fidelity: outcome.plan.as_ref().map(|p| FidelityLevel::of(&p.report)),
                headline: outcome.plan.as_ref().map(|p| p.report.headline().to_owned()),
                details: outcome.plan.as_ref().map(|p| p.report.details(catalog())).unwrap_or_default(),
                verifications: outcome.verifications,
                notes: outcome.notes,
                duration_ms: outcome.duration_ms as u64,
            },
            QueueEvent::Failed { id, failure } => UiEvent::Failed { id, code: failure.code, message: failure.message },
        })
    }
}

// ---------------------------------------------------------------------------------------------
// Moteurs chargés
// ---------------------------------------------------------------------------------------------

pub struct Engines {
    manager: EngineManager,
    statuses: Vec<EngineStatus>,
    runner: Arc<JobRunner>,
}

impl Engines {
    pub fn load(paths: AppPaths) -> Result<Self, String> {
        let manager = EngineManager::new(paths)
            .map_err(|e| chud_core::trf!("réglages des moteurs illisibles : {e}", "unreadable engine settings: {e}"))?;
        let mut statuses = manager.discover();
        let registry = Arc::new(manager.build_registry(&mut statuses));
        let runner = Arc::new(JobRunner::new(registry, manager.paths().cache_dir.clone()));
        runner.cleanup_stale(Duration::from_secs(24 * 3600));
        Ok(Self { manager, statuses, runner })
    }

    pub fn views(&self) -> Vec<EngineView> {
        self.statuses.iter().map(EngineView::from).collect()
    }

    pub fn info(&self) -> AppInfo {
        let paths = self.manager.paths();
        AppInfo {
            version: env!("CARGO_PKG_VERSION"),
            data_dir: paths.data_dir.display().to_string(),
            portable: paths.portable,
            formats: catalog().all().len(),
            conversions: self.runner.registry().engines().iter().map(|e| e.conversions().len()).sum(),
            engines_ready: self.statuses.iter().filter(|s| s.state == EngineState::Ready).count(),
        }
    }

    pub fn inspect(&self, paths: &[String]) -> Vec<FileInfo> {
        paths.iter().map(|p| inspect_one(Path::new(p))).collect()
    }

    pub fn targets(&self, path: &str) -> Result<FileTargets, String> {
        let path = absolute(path)?;
        let d = detect_path(&path, catalog()).map_err(|e| format!("{} : {e}", path.display()))?;
        let source = d.format.clone().ok_or(chud_core::tr!("format non reconnu", "unrecognized format"))?;
        let registry = self.runner.registry();
        let info = registry.probe_checked(&path, &source)?;
        let targets = registry
            .targets(&source, info.as_ref(), &OptionValues::new())
            .into_iter()
            .map(|t| TargetView {
                extension: catalog().get(t.format.as_str()).and_then(|d| d.extensions.first().cloned()),
                format: t.format.to_string(),
                name: t.name,
                category: t.category,
                fidelity: FidelityLevel::of(&t.best.report),
                headline: t.best.report.headline().to_owned(),
                lost: t.best.report.lost_features.iter().map(|l| l.label.clone()).collect(),
                engines: t.best.steps.iter().map(|s| s.engine.to_string()).collect(),
            })
            .collect::<Vec<_>>();
        let original = original_choices(&source, info.as_ref(), &targets);
        Ok(FileTargets {
            path: path.display().to_string(),
            source: source.to_string(),
            summary: info.map(|i| i.summary).unwrap_or_default(),
            targets,
            original,
        })
    }

    pub fn plan(&self, req: &PlanRequest) -> Result<PlanView, String> {
        let mut spec = JobSpec::new(absolute(&req.path)?, FormatId::new(&req.target));
        spec.options = options(&req.options);
        let p = self.runner.prepare(&spec).map_err(|e| e.to_string())?;
        let registry = self.runner.registry();
        let mut seen = HashSet::new();
        let mut opts = Vec::new();
        for s in &p.plan.steps {
            if let Some(e) = registry.engine(&s.engine) {
                opts.extend(e.options(&s.from, &s.to).into_iter().filter(|o| seen.insert(o.key.clone())));
            }
        }
        Ok(PlanView {
            source_name: format_name(&p.plan.source),
            target_name: format_name(&p.plan.target),
            source: p.plan.source.to_string(),
            target: p.plan.target.to_string(),
            fidelity: FidelityLevel::of(&p.plan.report),
            headline: p.plan.report.headline().to_owned(),
            details: p.plan.report.details(catalog()),
            steps: p
                .plan
                .steps
                .iter()
                .map(|s| StepView { engine: s.engine.to_string(), from: s.from.to_string(), to: s.to.to_string(), fidelity: s.fidelity })
                .collect(),
            options: opts,
            output: p.output.display().to_string(),
        })
    }

    pub fn job_specs(&self, req: &ConvertRequest) -> Result<Vec<JobSpec>, String> {
        let output_dir = match &req.output_dir {
            Some(d) => {
                let d = absolute(d)?;
                if !d.is_dir() {
                    return Err(chud_core::trf!(
                        "dossier de destination introuvable : {}",
                        "destination folder not found: {}",
                        d.display()
                    ));
                }
                Some(d)
            }
            None => None,
        };
        let opts = options(&req.options);
        req.files
            .iter()
            .map(|f| {
                let mut s = JobSpec::new(absolute(f)?, FormatId::new(&req.target));
                s.options = opts.clone();
                s.output_dir = output_dir.clone();
                s.conflict = req.conflict;
                s.preserve_mtime = req.keep_date;
                Ok(s)
            })
            .collect()
    }
}

fn options(map: &BTreeMap<String, OptionValue>) -> OptionValues {
    let mut o = OptionValues::new();
    for (k, v) in map {
        o.set(k, v.clone());
    }
    o
}

fn absolute(p: &str) -> Result<PathBuf, String> {
    if p.trim().is_empty() {
        return Err(chud_core::tr!("chemin vide", "empty path").into());
    }
    std::path::absolute(p).map_err(|e| format!("{p} : {e}"))
}

fn inspect_one(path: &Path) -> FileInfo {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string());
    let mut info = FileInfo {
        path: path.display().to_string(),
        name,
        size: 0,
        is_dir: path.is_dir(),
        format: None,
        format_name: None,
        category: None,
        confidence: None,
        method: None,
        extension_mismatch: false,
        note: None,
        error: None,
    };
    match detect_path(&path, catalog()) {
        Ok(d) => {
            info.size = if info.is_dir { chud_dir_size(&path) } else { d.size };
            info.category = d.format.as_ref().and_then(|f| catalog().get(f.as_str())).map(|f| f.category);
            info.format_name = d.format.as_ref().map(format_name);
            info.format = d.format.as_ref().map(|f| f.to_string());
            info.confidence = Some(d.confidence.label());
            info.method = Some(d.method.label());
            info.extension_mismatch = d.extension_mismatch;
            info.note = d.note;
            if info.format.is_none() {
                info.error = Some(chud_core::tr!("format non reconnu", "unrecognized format").into());
            }
        }
        Err(e) => info.error = Some(e.to_string()),
    }
    info
}

/// Format qui contient tel quel un codec audio (copie sans ré-encodage).
fn audio_home(codec: &str) -> Option<&'static str> {
    Some(match codec {
        "mp3" | "mp3float" => "mp3",
        "aac" => "m4a",
        "opus" => "opus",
        "vorbis" => "ogg",
        "flac" => "flac",
        "alac" => "alac",
        "ac3" => "ac3",
        "eac3" => "eac3",
        "dts" => "dts",
        "wmav1" | "wmav2" | "wmapro" => "wma",
        "wavpack" => "wv",
        "tta" => "tta",
        c if c.starts_with("pcm_") => "wav",
        _ => return None,
    })
}

/// Format qui contient tel quel un codec de sous-titres.
fn subtitle_home(codec: &str) -> Option<&'static str> {
    Some(match codec {
        "subrip" | "srt" => "srt",
        "ass" | "ssa" => "ass",
        "webvtt" => "vtt",
        _ => return None,
    })
}

/// Propositions « qualité d'origine » : extraire l'audio (ou les sous-titres) tel quel, garder la
/// vidéo sans ré-encodage, une image sans aucune perte. Chacune n'est proposée que si le format
/// est parmi les cibles et que le plan prévu est réellement sans perte.
fn original_choices(source: &FormatId, info: Option<&SourceInfo>, targets: &[TargetView]) -> Vec<OriginalChoice> {
    use chud_core::{tr, trf};
    let cat = catalog().get(source.as_str()).map(|d| d.category);
    let intact = |f: &str| targets.iter().find(|t| t.format == f && matches!(t.fidelity, FidelityLevel::Lossless | FidelityLevel::Partial));
    let label = |f: &str| catalog().get(f).map(|d| d.label().to_owned()).unwrap_or_else(|| f.to_uppercase());
    let mut out = Vec::new();
    let Some(info) = info else {
        if cat == Some(Category::Image) && source.as_str() != "png" && intact("png").is_some() {
            out.push(OriginalChoice {
                kind: "image",
                format: "png".into(),
                title: tr!("Image sans perte", "Lossless image").into(),
                detail: tr!("PNG : chaque pixel est gardé tel quel", "PNG: every pixel is kept as is").into(),
            });
        }
        return out;
    };
    if matches!(cat, Some(Category::Audio | Category::Video)) {
        let audio: Vec<&StreamInfo> = info.streams_of(StreamKind::Audio).collect();
        if let Some(a) = audio.first() {
            let more =
                if audio.len() > 1 { trf!(" (1re piste audio sur {})", " (first audio track of {})", audio.len()) } else { String::new() };
            let kbps = a.bitrate.filter(|b| *b > 0).map(|b| format!(" {} kbit/s", b.div_ceil(1000))).unwrap_or_default();
            match audio_home(&a.codec).filter(|h| *h != source.as_str()).filter(|h| intact(h).is_some()) {
                Some(h) => out.push(OriginalChoice {
                    kind: "audio",
                    format: h.into(),
                    title: tr!("Audio d'origine", "Original audio").into(),
                    detail: trf!(
                        "{} : la piste {}{kbps} ressort telle quelle, sans ré-encodage{more}",
                        "{}: the {}{kbps} track comes out as is, without re-encoding{more}",
                        label(h),
                        a.codec
                    ),
                }),
                None if audio_home(&a.codec) != Some(source.as_str()) => {
                    if let Some(f) = ["flac", "wav"].into_iter().find(|f| intact(f).is_some()) {
                        out.push(OriginalChoice {
                            kind: "audio",
                            format: f.into(),
                            title: tr!("Audio d'origine", "Original audio").into(),
                            detail: trf!(
                                "{} : le son {} décodé sans aucune perte supplémentaire{more}",
                                "{}: the {} sound decoded without any further loss{more}",
                                label(f),
                                a.codec
                            ),
                        });
                    }
                }
                None => {}
            }
        }
        if let Some(s) = info.streams_of(StreamKind::Subtitle).next()
            && let Some(h) = subtitle_home(&s.codec).filter(|h| intact(h).is_some())
        {
            out.push(OriginalChoice {
                kind: "subtitles",
                format: h.into(),
                title: tr!("Sous-titres d'origine", "Original subtitles").into(),
                detail: trf!("{} : texte et minutage tels quels", "{}: text and timing as they are", label(h)),
            });
        }
        if cat == Some(Category::Video)
            && info.streams_of(StreamKind::Video).next().is_some()
            && source.as_str() != "mkv"
            && intact("mkv").is_some()
        {
            out.push(OriginalChoice {
                kind: "video",
                format: "mkv".into(),
                title: tr!("Vidéo d'origine", "Original video").into(),
                detail: tr!("MKV : toutes les pistes gardées, sans ré-encodage", "MKV: every track kept, without re-encoding").into(),
            });
        }
    }
    if cat == Some(Category::Image) && source.as_str() != "png" && intact("png").is_some() {
        out.push(OriginalChoice {
            kind: "image",
            format: "png".into(),
            title: tr!("Image sans perte", "Lossless image").into(),
            detail: tr!("PNG : chaque pixel est gardé tel quel", "PNG: every pixel is kept as is").into(),
        });
    }
    out
}

/// Dossier remplacé par les fichiers qu'il contient (voir [`expand_paths`]).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedFolder {
    pub folder: String,
    pub files: Vec<String>,
    /// Phrase à afficher : combien de fichiers, lesquels sont laissés de côté.
    pub note: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Expansion {
    /// Chemins à ajouter, dans l'ordre : dossiers remplacés par leurs fichiers, le reste tel quel.
    pub paths: Vec<String>,
    pub folders: Vec<ExpandedFolder>,
}

/// Fichiers qui accompagnent souvent une série (pochettes, listes, sommes de contrôle…) et
/// n'empêchent pas de prendre le dossier comme « des fichiers du même type ».
const COMPANION_EXT: &[&str] =
    &["txt", "nfo", "cue", "log", "m3u", "m3u8", "pls", "sfv", "md5", "sha1", "sha256", "ini", "url", "accurip", "ffp", "db"];
const MAX_FOLDER_FILES: usize = 5000;

/// Un dossier qui ne contient que des fichiers du même type (plus d'éventuels fichiers
/// d'accompagnement : pochettes d'un album, .txt, .cue…) est remplacé par ces fichiers : on veut
/// convertir les musiques d'un dossier d'album, pas l'archiver. Les autres dossiers restent un
/// élément unique (archives ZIP, 7z…). Les sous-dossiers sont parcourus (« CD 1 », « CD 2 »).
pub fn expand_paths(paths: &[String]) -> Expansion {
    let mut out = Expansion { paths: Vec::new(), folders: Vec::new() };
    for p in paths {
        let dir = Path::new(p);
        match dir.is_dir().then(|| same_type_files(dir)).flatten() {
            Some((category, files, aside)) => {
                let files: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
                let n = files.len();
                let mut note = chud_core::trf!(
                    "Dossier « {name} » : {n} fichier(s) ({}) ajouté(s)",
                    "Folder “{name}”: {n} file(s) ({}) added",
                    category.label().to_lowercase()
                );
                if !aside.is_empty() {
                    let names: Vec<String> =
                        aside.iter().take(3).map(|a| a.file_name().unwrap_or_default().to_string_lossy().into_owned()).collect();
                    let more = if aside.len() > 3 { ", …" } else { "" };
                    note.push_str(&chud_core::trf!(" ; laissé(s) de côté : {}{more}", "; left aside: {}{more}", names.join(", ")));
                }
                note.push('.');
                out.paths.extend(files.iter().cloned());
                out.folders.push(ExpandedFolder { folder: p.clone(), files, note });
            }
            None => out.paths.push(p.clone()),
        }
    }
    out
}

/// Catégorie commune, fichiers à convertir (triés) et fichiers d'accompagnement laissés de côté.
fn same_type_files(dir: &Path) -> Option<(Category, Vec<PathBuf>, Vec<PathBuf>)> {
    let mut all = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name == "desktop.ini" || name == "thumbs.db" {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => all.push(e.path()),
                _ => {}
            }
            if all.len() > MAX_FOLDER_FILES {
                return None;
            }
        }
    }
    let mut by_cat: BTreeMap<&'static str, (Category, Vec<PathBuf>)> = BTreeMap::new();
    let mut companions = Vec::new();
    for f in all {
        let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if COMPANION_EXT.contains(&ext.as_str()) {
            companions.push(f);
            continue;
        }
        let cat = detect_path(&f, catalog()).ok()?.format.and_then(|id| catalog().get(id.as_str()).map(|d| d.category))?;
        by_cat.entry(cat.as_str()).or_insert_with(|| (cat, Vec::new())).1.push(f);
    }
    let (main, mut files) = by_cat.values().max_by_key(|(_, v)| v.len()).cloned()?;
    for (cat, v) in by_cat.values() {
        if *cat == main {
            continue;
        }
        // Pochettes et livrets d'un album, sous-titres d'une vidéo : accompagnement. Sinon, types mélangés.
        let companion = (matches!(main, Category::Audio | Category::Video) && matches!(cat, Category::Image | Category::Pdf))
            || (main == Category::Video && *cat == Category::Subtitle);
        if !companion || v.len() >= files.len() {
            return None;
        }
        companions.extend(v.iter().cloned());
    }
    files.sort();
    companions.sort();
    Some((main, files, companions))
}

/// Taille d'un dossier, bornée pour ne pas bloquer l'interface sur un disque entier.
fn chud_dir_size(p: &Path) -> u64 {
    let mut total = 0u64;
    let mut seen = 0usize;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > 200_000 {
                return total;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => {}
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------------------------
// État de l'application
// ---------------------------------------------------------------------------------------------

pub type UiListener = Arc<dyn Fn(UiEvent) + Send + Sync>;

pub struct App {
    paths: AppPaths,
    engines: Mutex<Option<Arc<Engines>>>,
    queue: Mutex<Option<JobQueue>>,
    listener: UiListener,
    /// Fichiers passés au lancement (« Ouvrir avec », futur menu contextuel), remis une fois.
    initial: Mutex<Vec<String>>,
    /// Sérialise les écritures de presets.toml.
    presets_lock: Mutex<()>,
    /// Sérialise les écritures des réglages.
    settings_lock: Mutex<()>,
    /// Lots `--preset` pas encore affichés par la fenêtre rapide.
    quick: Mutex<Vec<QuickBatch>>,
    /// Derniers événements de chaque tâche (sans la progression), pour une fenêtre qui arrive
    /// après coup.
    history: Arc<Mutex<BTreeMap<JobId, Vec<UiEvent>>>>,
    /// Installations de moteurs en cours, avec leur demande d'annulation.
    installs: Mutex<BTreeMap<String, Arc<AtomicBool>>>,
}

/// Retire une installation de la liste des installations en cours, quoi qu'il arrive.
struct InstallSlot<'a>(&'a Mutex<BTreeMap<String, Arc<AtomicBool>>>, String);

impl Drop for InstallSlot<'_> {
    fn drop(&mut self) {
        lock(self.0).remove(&self.1);
    }
}

/// Nombre de tâches dont l'historique est gardé.
const HISTORY_JOBS: usize = 500;

impl App {
    pub fn new(paths: AppPaths, listener: UiListener) -> Self {
        // Langue avant tout texte : celle des réglages, sinon celle du système (réglages
        // illisibles : signalés dans l'écran Réglages, jamais écrasés).
        match Settings::load(&paths.settings_file()) {
            Ok(s) => {
                s.apply_language();
            }
            Err(_) => chud_core::set_lang(chud_core::i18n::system_lang()),
        }
        Self {
            paths,
            engines: Mutex::new(None),
            queue: Mutex::new(None),
            listener,
            initial: Mutex::default(),
            presets_lock: Mutex::default(),
            settings_lock: Mutex::default(),
            quick: Mutex::default(),
            history: Arc::default(),
            installs: Mutex::default(),
        }
    }

    /// Fichiers à ajouter à la liste de la fenêtre principale à son ouverture.
    pub fn add_initial(&self, files: Vec<String>) {
        lock(&self.initial).extend(files);
    }

    pub fn take_initial(&self) -> Vec<String> {
        std::mem::take(&mut *lock(&self.initial))
    }

    /// Moteurs chargés (la première fois : détection, une à deux secondes).
    pub fn engines(&self) -> Result<Arc<Engines>, String> {
        let mut g = lock(&self.engines);
        if let Some(e) = &*g {
            return Ok(e.clone());
        }
        let e = Arc::new(Engines::load(self.paths.clone())?);
        *g = Some(e.clone());
        Ok(e)
    }

    /// Nouvelle détection (après l'installation d'un moteur, par exemple).
    pub fn reload_engines(&self) -> Result<Arc<Engines>, String> {
        let e = Arc::new(Engines::load(self.paths.clone())?);
        *lock(&self.engines) = Some(e.clone());
        Ok(e)
    }

    /// Vues des moteurs, avec les installations en cours.
    pub fn engine_views(&self, refresh: bool) -> Result<Vec<EngineView>, String> {
        let engines = if refresh { self.reload_engines()? } else { self.engines()? };
        let busy = lock(&self.installs);
        Ok(engines
            .views()
            .into_iter()
            .map(|mut v| {
                v.installing = busy.contains_key(&v.id);
                v
            })
            .collect())
    }

    fn manifest(&self, id: &str) -> Result<(Arc<Engines>, chud_enginemgr::Manifest), String> {
        let engines = self.engines()?;
        let m = engines
            .manager
            .manifests()
            .iter()
            .find(|m| m.id == id)
            .cloned()
            .ok_or_else(|| chud_core::trf!("moteur inconnu : {id}", "unknown engine: {id}"))?;
        Ok((engines, m))
    }

    /// Installe un moteur depuis la source figée de son manifeste (téléchargement vérifié,
    /// extraction contrôlée, essai, activation). Une installation du système n'est jamais
    /// modifiée ; une copie gérée n'est posée à côté d'elle que sur demande explicite
    /// (`alongside`, règle `install::check_allowed`).
    pub fn install_engine(&self, id: &str, alongside: bool, progress: &mut dyn FnMut(InstallProgress)) -> Result<Vec<EngineView>, String> {
        let (engines, m) = self.manifest(id)?;
        if let Some(st) = engines.statuses.iter().find(|s| s.id == id) {
            chud_enginemgr::install::check_allowed(st, alongside)?;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = lock(&self.installs);
            if g.contains_key(id) {
                return Err(chud_core::tr!("ce moteur est déjà en cours d'installation", "this engine is already being installed").into());
            }
            g.insert(id.to_owned(), cancel.clone());
        }
        let slot = InstallSlot(&self.installs, id.to_owned());
        let result = chud_enginemgr::install::install(&self.paths, &m, &cancel, progress);
        drop(slot);
        match result {
            Ok(_) => self.engine_views(true),
            Err(InstallError::Cancelled) => {
                Err(chud_core::tr!("installation annulée : rien n'a été installé", "installation cancelled: nothing was installed").into())
            }
            Err(InstallError::Failed(e)) => Err(e),
        }
    }

    /// Demande l'arrêt d'une installation en cours ; `false` s'il n'y en a pas.
    pub fn cancel_engine_install(&self, id: &str) -> bool {
        lock(&self.installs).get(id).map(|c| c.store(true, Ordering::Relaxed)).is_some()
    }

    fn require_idle(&self) -> Result<(), String> {
        if self.is_idle() {
            Ok(())
        } else {
            Err(chud_core::tr!(
                "des conversions sont en cours : attendez qu'elles soient terminées (ou annulez-les) avant de changer de moteur",
                "conversions are running: wait for them to finish (or cancel them) before changing engines"
            )
            .into())
        }
    }

    /// Retire un moteur installé par Chud Converter (jamais une installation du système).
    pub fn uninstall_engine(&self, id: &str) -> Result<Vec<EngineView>, String> {
        self.require_idle()?;
        let (_, m) = self.manifest(id)?;
        if lock(&self.installs).contains_key(id) {
            return Err(chud_core::tr!("ce moteur est en cours d'installation", "this engine is being installed").into());
        }
        // Les moteurs chargés gardent leurs chemins : on les oublie avant de supprimer.
        *lock(&self.engines) = None;
        let r = chud_enginemgr::install::uninstall(&self.paths, &m).map_err(|e| e.to_string());
        let views = self.engine_views(true)?;
        r.map(|_| views)
    }

    /// Revient à la version précédente d'un moteur installé par Chud Converter.
    pub fn rollback_engine(&self, id: &str) -> Result<Vec<EngineView>, String> {
        self.require_idle()?;
        let (_, m) = self.manifest(id)?;
        chud_enginemgr::install::rollback(&self.paths, &m).map_err(|e| e.to_string())?;
        self.engine_views(true)
    }

    pub fn submit(&self, req: &ConvertRequest) -> Result<Vec<Submitted>, String> {
        let engines = self.engines()?;
        let specs = engines.job_specs(req)?;
        self.submit_specs(&engines, specs)
    }

    fn submit_specs(&self, engines: &Arc<Engines>, specs: Vec<JobSpec>) -> Result<Vec<Submitted>, String> {
        let mut q = lock(&self.queue);
        // Après une nouvelle détection des moteurs, la file repart avec le nouveau registre dès
        // qu'elle est inactive ; sinon les tâches s'ajoutent à la file existante.
        let stale = q.as_ref().is_some_and(|q| !Arc::ptr_eq(q.runner(), &engines.runner) && q.is_idle());
        if q.is_none() || stale {
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let listener = self.listener.clone();
            let history = self.history.clone();
            let ql: QueueListener = Arc::new(move |ev| {
                if let Some(ui) = UiEvent::from_queue(ev) {
                    if !matches!(ui, UiEvent::Progress { .. }) {
                        let mut h = lock(&history);
                        h.entry(ui.id()).or_default().push(ui.clone());
                        while h.len() > HISTORY_JOBS {
                            h.pop_first();
                        }
                    }
                    listener(ui);
                }
            });
            *q = Some(JobQueue::new(engines.runner.clone(), cores.clamp(1, 8), cores as u32, ql));
        }
        let queue = q.as_ref().expect("file créée ci-dessus");
        Ok(specs.into_iter().map(|s| Submitted { path: s.input.display().to_string(), id: queue.submit(s) }).collect())
    }

    // ------------------------------------------------------------------ préréglages

    fn load_presets(&self) -> Result<PresetStore, String> {
        PresetStore::load(&self.paths.presets_file(), catalog()).map_err(|e| e.to_string())
    }

    pub fn presets(&self) -> PresetsView {
        let file = self.paths.presets_file().display().to_string();
        match self.load_presets() {
            Ok(s) => PresetsView { file, presets: s.presets.iter().map(PresetView::of).collect(), error: None, menu_note: None },
            Err(e) => PresetsView { file, presets: Vec::new(), error: Some(e), menu_note: None },
        }
    }

    /// Remplace la liste entière (l'interface envoie toujours la liste complète, dans l'ordre).
    /// Refusé si le fichier actuel est illisible : on ne l'écrase jamais sans le dire.
    pub fn save_presets(&self, presets: Vec<PresetView>) -> Result<PresetsView, String> {
        let _g = lock(&self.presets_lock);
        self.load_presets().map_err(|e| {
            chud_core::trf!(
                "{e} — corrigez le fichier ou rétablissez les préréglages par défaut",
                "{e} — fix the file or restore the default presets"
            )
        })?;
        let store = PresetStore { version: 1, presets: presets.into_iter().map(PresetView::into_preset).collect() };
        store.save(&self.paths.presets_file(), catalog()).map_err(|e| e.to_string())?;
        Ok(PresetsView { menu_note: self.refresh_menu(), ..self.presets() })
    }

    /// Rétablit les préréglages fournis. L'ancien fichier, s'il existe, est gardé à côté
    /// (presets.toml.bak-<horodatage>) : rien n'est perdu.
    pub fn reset_presets(&self) -> Result<(PresetsView, Option<String>), String> {
        let _g = lock(&self.presets_lock);
        let file = self.paths.presets_file();
        let mut backup = None;
        if file.exists() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let b = file.with_extension(format!("toml.bak-{stamp}"));
            std::fs::copy(&file, &b)
                .map_err(|e| chud_core::trf!("copie de sauvegarde impossible : {e}", "cannot make a backup copy: {e}"))?;
            backup = Some(b.display().to_string());
        }
        PresetStore::builtin().save(&file, catalog()).map_err(|e| e.to_string())?;
        Ok((PresetsView { menu_note: self.refresh_menu(), ..self.presets() }, backup))
    }

    /// Préréglages proposés pour ces chemins, d'après leurs noms (comme le menu contextuel).
    pub fn presets_for(&self, paths: &[String]) -> Result<Vec<PresetView>, String> {
        let store = self.load_presets()?;
        let paths: Vec<PathBuf> = paths.iter().map(|p| absolute(p)).collect::<Result<_, _>>()?;
        Ok(store.for_paths(&paths, catalog()).into_iter().map(PresetView::of).collect())
    }

    /// Lance un préréglage sur des fichiers. Un fichier déjà au format visé (préréglage sans
    /// option) est ignoré avec sa raison plutôt que copié tel quel.
    pub fn run_preset(&self, id: &str, files: &[String]) -> Result<RunPreset, String> {
        let store = self.load_presets()?;
        let preset = store.get(id).map_err(|e| e.to_string())?.clone();
        if let Some(d) = &preset.output_dir
            && !d.is_dir()
        {
            return Err(chud_core::trf!(
                "« {} » : dossier de destination introuvable : {}",
                "“{}”: destination folder not found: {}",
                preset.display_name(),
                d.display()
            ));
        }
        let engines = self.engines()?;
        let mut specs = Vec::new();
        let mut ignored = Vec::new();
        for f in files {
            let path = absolute(f)?;
            let same = !path.is_dir()
                && detect_path(&path, catalog()).ok().and_then(|d| d.format).is_some_and(|src| preset.is_pointless_for(&src));
            if same {
                ignored.push(Ignored {
                    path: path.display().to_string(),
                    reason: chud_core::trf!("déjà au format {}", "already in {} format", format_name(&preset.target)),
                });
            } else {
                specs.push(preset.job_spec(path));
            }
        }
        let submitted = self.submit_specs(&engines, specs)?;
        Ok(RunPreset { preset: PresetView::of(&preset), submitted, ignored })
    }

    /// Lance un lot `--preset` et le garde pour la fenêtre rapide (même en cas d'erreur, pour
    /// l'afficher au lieu de l'avaler).
    pub fn quick(&self, id: &str, files: Vec<String>) {
        let batch = match self.run_preset(id, &files) {
            Ok(run) => QuickBatch { files, run: Some(run), error: None, events: Vec::new() },
            Err(e) => QuickBatch { files, run: None, error: Some(e), events: Vec::new() },
        };
        lock(&self.quick).push(batch);
    }

    /// Lots en attente, avec ce qui est déjà arrivé à leurs tâches.
    pub fn take_quick(&self) -> Vec<QuickBatch> {
        let mut batches = std::mem::take(&mut *lock(&self.quick));
        let history = lock(&self.history);
        for b in &mut batches {
            if let Some(run) = &b.run {
                b.events = run.submitted.iter().flat_map(|s| history.get(&s.id).cloned().unwrap_or_default()).collect();
            }
        }
        batches
    }

    // ------------------------------------------------------------------ menu clic droit

    /// Menus d'après les préréglages, limités aux conversions possibles avec les moteurs présents.
    fn menus(&self) -> Result<Vec<chud_shell::Menu>, String> {
        let store = self.load_presets()?;
        let engines = self.engines()?;
        let registry = engines.runner.registry();
        let mut cache: std::collections::HashMap<FormatId, Vec<FormatId>> = Default::default();
        Ok(chud_shell::plan_menus_with(&store, catalog(), |src, dst| {
            cache
                .entry(src.clone())
                .or_insert_with(|| registry.targets(src, None, &OptionValues::new()).into_iter().map(|t| t.format).collect())
                .contains(dst)
        }))
    }

    pub fn shell_status(&self) -> ShellView {
        let supported = cfg!(any(windows, target_os = "linux"));
        let me = chud_shell::default_gui_exe().ok();
        match chud_shell::status() {
            Ok(st) => ShellView {
                supported,
                installed: st.installed,
                current: st.installed && st.exe.is_some() && st.exe == me,
                exe: st.exe.map(|e| e.display().to_string()),
                exe_missing: st.exe_missing,
                entries: st.entries,
                location: st.location,
            },
            Err(_) => ShellView {
                supported: false,
                installed: false,
                current: false,
                exe: None,
                exe_missing: false,
                entries: 0,
                location: String::new(),
            },
        }
    }

    /// Installe (ou met à jour) le menu pour lancer ce programme-ci.
    pub fn shell_install(&self) -> Result<ShellView, String> {
        let exe = chud_shell::default_gui_exe().map_err(|e| e.to_string())?;
        let menus = self.menus()?;
        chud_shell::install(&exe, &menus).map_err(|e| e.to_string())?;
        Ok(self.shell_status())
    }

    pub fn shell_uninstall(&self) -> Result<ShellView, String> {
        chud_shell::uninstall().map_err(|e| e.to_string())?;
        Ok(self.shell_status())
    }

    /// Après un changement de préréglages : met à jour le menu s'il est installé pour ce
    /// programme. Ne l'installe jamais de lui-même.
    fn refresh_menu(&self) -> Option<String> {
        let exe = chud_shell::default_gui_exe().ok()?;
        let st = chud_shell::status().ok()?;
        if !st.installed || st.exe.as_deref() != Some(exe.as_path()) {
            return None;
        }
        let r = self.menus().and_then(|m| chud_shell::refresh_if_installed(&exe, &m).map_err(|e| e.to_string()));
        Some(match r {
            Ok(_) => chud_core::tr!("Menu clic droit mis à jour.", "Right-click menu updated.").into(),
            Err(e) => {
                chud_core::trf!("Le menu clic droit n'a pas pu être mis à jour : {e}", "The right-click menu could not be updated: {e}")
            }
        })
    }

    // ------------------------------------------------------------------ réglages

    pub fn settings(&self) -> SettingsView {
        let file = self.paths.settings_file();
        let (language, error) = match Settings::load(&file) {
            Ok(s) => (s.language, None),
            Err(e) => (LangChoice::System, Some(e.to_string())),
        };
        SettingsView {
            language,
            effective: chud_core::lang(),
            system: chud_core::i18n::system_lang(),
            file: file.display().to_string(),
            error,
            menu_note: None,
            intro_seen: self.paths.intro_file().exists(),
        }
    }

    /// L'accueil du premier démarrage ne sera plus montré.
    pub fn intro_done(&self) -> Result<(), String> {
        let file = self.paths.intro_file();
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(
            &file, "1
",
        )
        .map_err(|e| e.to_string())
    }

    /// Change la langue : enregistrée dans les réglages (partagés avec la ligne de commande),
    /// appliquée tout de suite. Les autres réglages du fichier sont conservés ; un fichier
    /// illisible n'est jamais écrasé.
    pub fn set_language(&self, choice: LangChoice) -> Result<SettingsView, String> {
        let file = self.paths.settings_file();
        {
            let _g = lock(&self.settings_lock);
            let mut s = Settings::load(&file).map_err(|e| e.to_string())?;
            s.language = choice;
            s.save(&file).map_err(|e| e.to_string())?;
            s.apply_language();
        }
        // Descriptions des moteurs et libellés des options sont produits dans la langue courante :
        // nouvelle détection au prochain besoin.
        *lock(&self.engines) = None;
        let mut view = self.settings();
        view.menu_note = self.refresh_menu();
        Ok(view)
    }

    // ------------------------------------------------------------------ mises à jour

    pub fn is_portable(&self) -> bool {
        self.paths.portable
    }

    /// Aucune conversion en cours ni en attente.
    pub fn is_idle(&self) -> bool {
        lock(&self.queue).as_ref().is_none_or(|q| q.is_idle())
    }

    pub fn update_settings(&self) -> Result<UpdateSettings, String> {
        Settings::load(&self.paths.settings_file()).map(|s| s.updates).map_err(|e| e.to_string())
    }

    /// Modifie les réglages de mise à jour ; les autres réglages sont conservés et un fichier
    /// illisible n'est jamais écrasé.
    pub fn save_update_settings(&self, f: impl FnOnce(&mut UpdateSettings)) -> Result<UpdateSettings, String> {
        let file = self.paths.settings_file();
        let _g = lock(&self.settings_lock);
        let mut s = Settings::load(&file).map_err(|e| e.to_string())?;
        f(&mut s.updates);
        s.save(&file).map_err(|e| e.to_string())?;
        Ok(s.updates)
    }

    pub fn cancel(&self, id: JobId) -> bool {
        lock(&self.queue).as_ref().is_some_and(|q| q.cancel(id))
    }

    pub fn cancel_all(&self) {
        if let Some(q) = lock(&self.queue).as_ref() {
            q.cancel_all();
        }
    }

    /// Annule tout et attend que chaque tâche ait nettoyé derrière elle.
    pub fn shutdown(&self) {
        let queue = lock(&self.queue).take();
        drop(queue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_1X1: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00,
        0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63,
        0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
        0x42, 0x60, 0x82,
    ];

    /// La langue est un réglage du processus : les tests de ce module passent un par un.
    static SERIAL: Mutex<()> = Mutex::new(());

    type Setup = (PathBuf, App, Arc<Mutex<Vec<UiEvent>>>, MutexGuard<'static, ()>);

    /// Application de test en français (réglage écrit comme le ferait l'écran Réglages).
    fn setup(name: &str) -> Setup {
        let serial = lock(&SERIAL);
        let dir = std::env::temp_dir().join(format!("chud-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = AppPaths::at(&dir.join("app"));
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::write(paths.settings_file(), "language = \"fr\"\n").unwrap();
        let events: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
        let ev = events.clone();
        let app = App::new(paths, Arc::new(move |e| ev.lock().unwrap().push(e)));
        assert_eq!(chud_core::lang(), Lang::Fr);
        (dir, app, events, serial)
    }

    fn wav() -> Vec<u8> {
        let mut b = b"RIFF".to_vec();
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&[1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0]);
        b.extend_from_slice(b"data");
        b.extend_from_slice(&4u32.to_le_bytes());
        b.extend_from_slice(&[0, 0, 0, 0]);
        b
    }

    fn tv(format: &str, fidelity: FidelityLevel) -> TargetView {
        TargetView {
            format: format.into(),
            name: format.into(),
            category: Category::Audio,
            extension: None,
            fidelity,
            headline: String::new(),
            lost: vec![],
            engines: vec![],
        }
    }

    fn stream(kind: StreamKind, codec: &str, bitrate: Option<u64>) -> StreamInfo {
        StreamInfo {
            index: 0,
            kind,
            codec: codec.into(),
            attached_pic: false,
            language: None,
            width: None,
            height: None,
            channels: Some(2),
            sample_rate: Some(32000),
            bit_depth: None,
            sample_format: None,
            frame_rate: None,
            bitrate,
            duration: None,
        }
    }

    #[test]
    fn original_quality_choices() {
        let _serial = lock(&SERIAL);
        chud_core::i18n::with_lang(Lang::Fr, || {
            // MKV d'un jeu, audio MP3 seulement : l'MP3 ressort tel quel.
            let info = SourceInfo { streams: vec![stream(StreamKind::Audio, "mp3", Some(96_000))], ..Default::default() };
            let targets = [tv("mp3", FidelityLevel::Lossless), tv("wav", FidelityLevel::Lossless), tv("aac", FidelityLevel::Lossy)];
            let o = original_choices(&"mka".into(), Some(&info), &targets);
            assert_eq!(o.len(), 1);
            assert_eq!((o[0].kind, o[0].format.as_str()), ("audio", "mp3"));
            assert!(o[0].detail.contains("96 kbit/s") && o[0].detail.contains("sans ré-encodage"), "{}", o[0].detail);
            // Un MP3 est déjà l'audio d'origine : rien à proposer.
            assert!(original_choices(&"mp3".into(), Some(&info), &targets).is_empty());
            // Codec sans format « maison » proposé : décodé sans perte.
            let info = SourceInfo { streams: vec![stream(StreamKind::Audio, "truehd", None)], ..Default::default() };
            let o = original_choices(&"mka".into(), Some(&info), &targets);
            assert_eq!(o[0].format, "wav");
            // Vidéo MP4 avec sous-titres : audio AAC → M4A, sous-titres, vidéo sans ré-encodage.
            let info = SourceInfo {
                streams: vec![
                    stream(StreamKind::Video, "h264", None),
                    stream(StreamKind::Audio, "aac", None),
                    stream(StreamKind::Audio, "aac", None),
                    stream(StreamKind::Subtitle, "subrip", None),
                ],
                ..Default::default()
            };
            let targets = [
                tv("m4a", FidelityLevel::Lossless),
                tv("srt", FidelityLevel::Partial),
                tv("mkv", FidelityLevel::Lossless),
                tv("mp3", FidelityLevel::Lossy),
            ];
            let o = original_choices(&"mp4".into(), Some(&info), &targets);
            let kinds: Vec<_> = o.iter().map(|c| (c.kind, c.format.as_str())).collect();
            assert_eq!(kinds, [("audio", "m4a"), ("subtitles", "srt"), ("video", "mkv")]);
            assert!(o[0].detail.contains("1re piste audio sur 2"));
            // Une cible avec perte n'est jamais proposée comme « d'origine ».
            let o = original_choices(&"mp4".into(), Some(&info), &[tv("m4a", FidelityLevel::Lossy)]);
            assert!(o.is_empty());
            // Image JPEG : PNG sans perte.
            let o = original_choices(&"jpeg".into(), None, &[tv("png", FidelityLevel::Lossless)]);
            assert_eq!(o[0].format, "png");
        });
    }

    #[test]
    fn a_folder_of_same_type_files_is_replaced_by_its_files() {
        let (dir, _app, _events, _serial) = setup("expand");
        let album = dir.join("Album é");
        std::fs::create_dir_all(album.join("CD 2")).unwrap();
        for f in ["Piste 01.wav", "Piste 02.wav", "CD 2/Piste 03.wav"] {
            std::fs::write(album.join(f), wav()).unwrap();
        }
        std::fs::write(album.join("cover.png"), PNG_1X1).unwrap();
        std::fs::write(album.join("infos.txt"), "rip").unwrap();
        std::fs::write(album.join("desktop.ini"), "x").unwrap();
        // Dossier mélangé : reste un seul élément (à archiver).
        let mixed = dir.join("Mélange");
        std::fs::create_dir_all(&mixed).unwrap();
        std::fs::write(mixed.join("son.wav"), wav()).unwrap();
        std::fs::write(mixed.join("photo.png"), PNG_1X1).unwrap();
        std::fs::write(mixed.join("autre.png"), PNG_1X1).unwrap();
        let single = dir.join("seul.png");
        std::fs::write(&single, PNG_1X1).unwrap();

        let e = expand_paths(&[album.display().to_string(), mixed.display().to_string(), single.display().to_string()]);
        assert_eq!(e.folders.len(), 1);
        assert_eq!(e.folders[0].files.len(), 3);
        assert!(e.folders[0].files.iter().all(|f| f.ends_with(".wav")));
        assert!(e.folders[0].note.contains("3 fichier(s)") && e.folders[0].note.contains("cover.png"), "{}", e.folders[0].note);
        assert_eq!(e.paths.len(), 5);
        assert_eq!(e.paths[3], mixed.display().to_string());
        assert_eq!(e.paths[4], single.display().to_string());
        // Un dossier d'images seulement est aussi remplacé par ses images.
        let pics = dir.join("Images");
        std::fs::create_dir_all(&pics).unwrap();
        std::fs::write(pics.join("a.png"), PNG_1X1).unwrap();
        assert_eq!(expand_paths(&[pics.display().to_string()]).folders[0].files.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn inspect_targets_plan_and_convert() {
        let (dir, app, events, _serial) = setup("flow");
        let png = dir.join("-photo é.png");
        std::fs::write(&png, PNG_1X1).unwrap();
        let fake = dir.join("faux.jpg");
        std::fs::write(&fake, PNG_1X1).unwrap();
        let engines = app.engines().unwrap();

        let infos = engines.inspect(&[png.display().to_string(), fake.display().to_string(), dir.join("absent.bin").display().to_string()]);
        assert_eq!(infos[0].format.as_deref(), Some("png"));
        assert_eq!(infos[0].category, Some(Category::Image));
        assert!(infos[1].extension_mismatch);
        assert!(infos[2].error.is_some());

        let t = engines.targets(&png.display().to_string()).unwrap();
        let bmp = t.targets.iter().find(|t| t.format == "bmp").expect("PNG → BMP proposé");
        assert_eq!(bmp.fidelity, FidelityLevel::Lossless);
        let jpg = t.targets.iter().find(|t| t.format == "jpeg").expect("PNG → JPEG proposé");
        assert_eq!(jpg.fidelity, FidelityLevel::Lossy);

        let plan = engines.plan(&PlanRequest { path: png.display().to_string(), target: "jpeg".into(), options: BTreeMap::new() }).unwrap();
        assert_eq!(plan.fidelity, FidelityLevel::Lossy);
        assert!(plan.options.iter().any(|o| o.key == "quality"), "{:?}", plan.options);
        assert!(plan.output.ends_with("-photo é.jpg"));

        let req = ConvertRequest {
            files: vec![png.display().to_string()],
            target: "bmp".into(),
            options: BTreeMap::new(),
            output_dir: None,
            conflict: Conflict::Rename,
            keep_date: false,
        };
        let sub = app.submit(&req).unwrap();
        let id = sub[0].id;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            let done =
                events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::Done { id: i, .. } | UiEvent::Failed { id: i, .. } if *i == id));
            if done {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "conversion bloquée");
            std::thread::sleep(Duration::from_millis(20));
        }
        let evs = events.lock().unwrap().clone();
        let done = evs.iter().find_map(|e| match e {
            UiEvent::Done { output, fidelity, verifications, .. } => Some((output.clone(), *fidelity, verifications.clone())),
            _ => None,
        });
        let (output, fidelity, verifications) = done.expect("conversion réussie");
        assert!(output.unwrap().ends_with("-photo é.bmp"));
        assert_eq!(fidelity, Some(FidelityLevel::Lossless));
        assert!(verifications.iter().all(|v| v.is_ok()));
        assert!(evs.iter().any(|e| matches!(e, UiEvent::Planned { .. })));
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn presets_round_trip_never_overwrite_broken_file_and_run() {
        let (dir, app, events, _serial) = setup("presets");
        let v = app.presets();
        assert!(v.error.is_none());
        assert!(v.presets.iter().all(|p| p.builtin));
        let mut list = v.presets.clone();
        let mut mine = list.iter().find(|p| p.id == "jpeg").unwrap().clone();
        mine.id = "mon-bmp".into();
        mine.name = "  Mon BMP ".into();
        mine.target = "bmp".into();
        mine.options.clear();
        list.insert(0, mine);
        let saved = app.save_presets(list.clone()).unwrap();
        assert_eq!(saved.presets[0].name, "Mon BMP");
        assert_eq!(saved.presets[0].target_name, "BMP");
        assert!(!saved.presets[0].builtin);

        // Un identifiant invalide est refusé, le fichier reste intact.
        let mut bad = list.clone();
        bad[0].id = "Pas Bon".into();
        assert!(app.save_presets(bad).is_err());
        assert_eq!(app.presets().presets[0].id, "mon-bmp");

        // Proposés d'après le nom : un PNG n'a pas le préréglage PNG, mais a « Mon BMP ».
        let png = dir.join("a.png");
        std::fs::write(&png, PNG_1X1).unwrap();
        let ids: Vec<String> = app.presets_for(&[png.display().to_string()]).unwrap().into_iter().map(|p| p.id).collect();
        assert!(ids.contains(&"mon-bmp".to_string()) && !ids.contains(&"png".to_string()), "{ids:?}");

        // Lancer : le PNG est ignoré par « PNG », converti par « Mon BMP ».
        let r = app.run_preset("png", &[png.display().to_string()]).unwrap();
        assert!(r.submitted.is_empty() && r.ignored.len() == 1);
        let r = app.run_preset("mon-bmp", &[png.display().to_string()]).unwrap();
        let id = r.submitted[0].id;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !events.lock().unwrap().iter().any(|e| matches!(e, UiEvent::Done { id: i, .. } if *i == id)) {
            assert!(std::time::Instant::now() < deadline, "conversion bloquée");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(dir.join("a.bmp").is_file());
        assert!(app.run_preset("inconnu", &[png.display().to_string()]).is_err());

        // Fichier abîmé : pas d'écrasement, mais « rétablir » garde une copie.
        let file = AppPaths::at(&dir.join("app")).presets_file();
        std::fs::write(&file, "ceci n'est pas du toml [").unwrap();
        assert!(app.presets().error.is_some());
        assert!(app.save_presets(list).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "ceci n'est pas du toml [");
        let (v, backup) = app.reset_presets().unwrap();
        assert!(v.error.is_none() && !v.presets.is_empty());
        assert_eq!(std::fs::read_to_string(backup.unwrap()).unwrap(), "ceci n'est pas du toml [");
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_launch_arguments() {
        let p = |a: &[&str]| Launch::parse(a.iter().map(std::ffi::OsString::from));
        assert_eq!(p(&[]), Launch::Open(vec![]));
        assert_eq!(p(&["a.mkv", ""]), Launch::Open(vec!["a.mkv".into()]));
        assert_eq!(
            p(&["--preset", "mp4", "--", "--preset", "b c.mkv"]),
            Launch::Preset { id: "mp4".into(), files: vec!["--preset".into(), "b c.mkv".into()] }
        );
        assert_eq!(p(&["--preset=webm", "x"]), Launch::Preset { id: "webm".into(), files: vec!["x".into()] });
        assert_eq!(p(&["--preset"]), Launch::Preset { id: String::new(), files: vec![] });
    }

    #[test]
    fn quick_batches_keep_errors_visible() {
        let (dir, app, _, _serial) = setup("quick");
        app.quick("inconnu", vec!["x.png".into()]);
        let b = app.take_quick();
        assert_eq!(b.len(), 1);
        assert!(b[0].error.as_deref().unwrap().contains("inconnu"), "{:?}", b[0].error);
        assert!(app.take_quick().is_empty());

        // Une conversion terminée avant que la fenêtre n'écoute : son historique est joint.
        let png = dir.join("q.png");
        std::fs::write(&png, PNG_1X1).unwrap();
        app.quick("webp", vec![png.display().to_string()]);
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !dir.join("q.webp").is_file() || lock(&app.history).values().all(|h| !h.iter().any(|e| matches!(e, UiEvent::Done { .. }))) {
            assert!(std::time::Instant::now() < deadline, "conversion bloquée");
            std::thread::sleep(Duration::from_millis(20));
        }
        let b = app.take_quick();
        assert!(b[0].events.iter().any(|e| matches!(e, UiEvent::Started { .. })));
        assert!(b[0].events.iter().any(|e| matches!(e, UiEvent::Done { .. })), "{:?}", b[0].events);
        drop(app);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_missing_destination_and_serializes_camel_case() {
        let (dir, app, _, _serial) = setup("dest");
        let req: ConvertRequest = serde_json::from_value(serde_json::json!({
            "files": ["x.png"], "target": "bmp", "outputDir": dir.join("nope").display().to_string(), "conflict": "skip", "keepDate": true
        }))
        .unwrap();
        assert_eq!(req.conflict, Conflict::Skip);
        assert!(app.submit(&req).unwrap_err().contains("introuvable"));
        let ev = serde_json::to_value(UiEvent::Failed { id: 3, code: "cancelled".into(), message: "x".into() }).unwrap();
        assert_eq!(ev, serde_json::json!({"kind": "failed", "id": 3, "code": "cancelled", "message": "x"}));
        let ev = serde_json::to_value(UiEvent::Progress { id: 1, fraction: Some(0.5), speed: None }).unwrap();
        assert_eq!(ev["kind"], "progress");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn language_switch_is_saved_applied_and_keeps_other_settings() {
        let (dir, app, _, _serial) = setup("lang");
        let file = app.paths.settings_file();
        std::fs::write(&file, "language = \"fr\"\n\n[engines.ffmpeg]\nenabled = false\n").unwrap();
        let png = dir.join("a.png");
        std::fs::write(&png, PNG_1X1).unwrap();
        let req = || PlanRequest { path: png.display().to_string(), target: "bmp".into(), options: Default::default() };
        let fr = app.engines().unwrap().plan(&req()).unwrap();

        let v = app.set_language(LangChoice::En).unwrap();
        assert_eq!((v.language, v.effective), (LangChoice::En, Lang::En));
        assert_eq!(chud_core::lang(), Lang::En);
        let en = app.engines().unwrap().plan(&req()).unwrap();
        assert_ne!(fr.headline, en.headline, "le rapport suit la langue");
        assert_eq!(en.headline, "Lossless");
        let names: Vec<String> = app.presets().presets.into_iter().map(|p| p.name).collect();
        assert!(names.iter().any(|n| n == "Compress to ZIP"), "{names:?}");
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.contains("language = \"en\"") && saved.contains("enabled = false"), "{saved}");

        // Fichier illisible : refus, rien n'est écrasé.
        std::fs::write(&file, "language = [").unwrap();
        assert!(app.set_language(LangChoice::Fr).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "language = [");

        std::fs::write(&file, "").unwrap();
        app.set_language(LangChoice::Fr).unwrap();
        assert_eq!(chud_core::lang(), Lang::Fr);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
