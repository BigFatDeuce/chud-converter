//! Logique de l'application de bureau, indépendante de Tauri (testable seule) : moteurs,
//! analyse des fichiers, cibles possibles, plan et options, file de conversions.
//!
//! Tout ce qui part vers l'interface est une « vue » sérialisable en camelCase ; les textes
//! (rapports de fidélité, erreurs) sont produits ici, en français, par le cœur.

use chud_core::{
    Category, Fidelity, FidelityReport, FormatCatalog, FormatId, OptionSpec, OptionValue, OptionValues, Verification, detect_path,
};
use chud_enginemgr::{AppPaths, EngineManager, EngineState, EngineStatus};
use chud_jobs::{Conflict, JobEvent, JobId, JobQueue, JobRunner, JobSpec, QueueEvent, QueueListener};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

fn catalog() -> &'static FormatCatalog {
    FormatCatalog::builtin()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn format_name(id: &FormatId) -> String {
    catalog().get(id.as_str()).map(|d| d.name.clone()).unwrap_or_else(|| id.to_string())
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
    pub fn from_queue(ev: QueueEvent) -> Option<Self> {
        Some(match ev {
            QueueEvent::Started { id } => UiEvent::Started { id },
            QueueEvent::Job { id, event } => match event {
                JobEvent::Planned { plan } => UiEvent::Planned {
                    id,
                    fidelity: FidelityLevel::of(&plan.report),
                    headline: plan.report.headline_fr().to_owned(),
                    details: plan.report.details_fr(catalog()),
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
                headline: outcome.plan.as_ref().map(|p| p.report.headline_fr().to_owned()),
                details: outcome.plan.as_ref().map(|p| p.report.details_fr(catalog())).unwrap_or_default(),
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
        let manager = EngineManager::new(paths).map_err(|e| format!("réglages des moteurs illisibles : {e}"))?;
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
        let source = d.format.clone().ok_or("format non reconnu")?;
        let registry = self.runner.registry();
        let info = registry.probe(&path, &source);
        let targets = registry
            .targets(&source, info.as_ref(), &OptionValues::new())
            .into_iter()
            .map(|t| TargetView {
                extension: catalog().get(t.format.as_str()).and_then(|d| d.extensions.first().cloned()),
                format: t.format.to_string(),
                name: t.name,
                category: t.category,
                fidelity: FidelityLevel::of(&t.best.report),
                headline: t.best.report.headline_fr().to_owned(),
                lost: t.best.report.lost_features.iter().map(|l| l.label.clone()).collect(),
                engines: t.best.steps.iter().map(|s| s.engine.to_string()).collect(),
            })
            .collect();
        Ok(FileTargets {
            path: path.display().to_string(),
            source: source.to_string(),
            summary: info.map(|i| i.summary).unwrap_or_default(),
            targets,
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
            headline: p.plan.report.headline_fr().to_owned(),
            details: p.plan.report.details_fr(catalog()),
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
                    return Err(format!("dossier de destination introuvable : {}", d.display()));
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
        return Err("chemin vide".into());
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
            info.confidence = Some(d.confidence.label_fr());
            info.method = Some(d.method.label_fr());
            info.extension_mismatch = d.extension_mismatch;
            info.note = d.note;
            if info.format.is_none() {
                info.error = Some("format non reconnu".into());
            }
        }
        Err(e) => info.error = Some(e.to_string()),
    }
    info
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
}

impl App {
    pub fn new(paths: AppPaths, listener: UiListener) -> Self {
        Self { paths, engines: Mutex::new(None), queue: Mutex::new(None), listener, initial: Mutex::default() }
    }

    pub fn with_initial(self, files: Vec<String>) -> Self {
        *lock(&self.initial) = files;
        self
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

    pub fn submit(&self, req: &ConvertRequest) -> Result<Vec<Submitted>, String> {
        let engines = self.engines()?;
        let specs = engines.job_specs(req)?;
        let mut q = lock(&self.queue);
        // Après une nouvelle détection des moteurs, la file repart avec le nouveau registre dès
        // qu'elle est inactive ; sinon les tâches s'ajoutent à la file existante.
        let stale = q.as_ref().is_some_and(|q| !Arc::ptr_eq(q.runner(), &engines.runner) && q.is_idle());
        if q.is_none() || stale {
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let listener = self.listener.clone();
            let ql: QueueListener = Arc::new(move |ev| {
                if let Some(ui) = UiEvent::from_queue(ev) {
                    listener(ui);
                }
            });
            *q = Some(JobQueue::new(engines.runner.clone(), cores.clamp(1, 8), cores as u32, ql));
        }
        let queue = q.as_ref().expect("file créée ci-dessus");
        Ok(specs.into_iter().map(|s| Submitted { path: s.input.display().to_string(), id: queue.submit(s) }).collect())
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

    fn setup(name: &str) -> (PathBuf, App, Arc<Mutex<Vec<UiEvent>>>) {
        let dir = std::env::temp_dir().join(format!("chud-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let events: Arc<Mutex<Vec<UiEvent>>> = Arc::default();
        let ev = events.clone();
        let app = App::new(AppPaths::at(&dir.join("app")), Arc::new(move |e| ev.lock().unwrap().push(e)));
        (dir, app, events)
    }

    #[test]
    fn inspect_targets_plan_and_convert() {
        let (dir, app, events) = setup("flow");
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
    fn rejects_missing_destination_and_serializes_camel_case() {
        let (dir, app, _) = setup("dest");
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
}
