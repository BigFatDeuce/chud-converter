//! Exécution des tâches de conversion.
//!
//! Déroulement d'une tâche ([`JobRunner::run`]) :
//! 1. détection du format réel de la source (jamais la seule extension quand le contenu parle) ;
//! 2. sonde (flux, caractéristiques) puis plan le plus fidèle ;
//! 3. exécution de chaque étape dans un **répertoire de travail** propre à la tâche ;
//! 4. vérification de chaque résultat : par le moteur (décodage, intégrité) **et** par une détection
//!    indépendante du format produit — un code de sortie 0 ne suffit jamais ;
//! 5. finalisation atomique : réservation du nom (`create_new`), renommage, repli copie + renommage
//!    entre volumes. La source n'est jamais modifiée ni écrasée.
//!
//! [`run_batch`] exécute plusieurs tâches en parallèle, sous une jauge de ressources qui tient compte
//! du poids des moteurs (une conversion vidéo occupe plusieurs cœurs).

use chud_core::detect::{Confidence, Detection, detect_path};
use chud_core::{
    CancelToken, Engine, EngineError, EngineId, ExecContext, FormatCatalog, FormatId, OptionValues, Plan, PlanError, Progress, Registry,
    StepTask, Verification,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;

pub mod presets;
pub mod queue;
pub use presets::{Preset, PresetError, PresetStore, kind_of_path};
pub use queue::{JobFailure, JobId, JobQueue, QueueEvent, QueueListener};

/// Que faire si le fichier de sortie existe déjà.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// Ajoute « (1) », « (2) »… au nom.
    #[default]
    Rename,
    /// Remplace le fichier existant (jamais la source elle-même, jamais un dossier).
    Overwrite,
    /// Ne fait rien.
    Skip,
}

#[derive(Clone, Debug)]
pub struct JobSpec {
    pub input: PathBuf,
    pub target: FormatId,
    /// Dossier de sortie ; par défaut celui de la source.
    pub output_dir: Option<PathBuf>,
    pub options: OptionValues,
    /// Modèle du nom de sortie (sans extension) : `{stem}`, `{target}`.
    pub name_template: String,
    pub conflict: Conflict,
    /// Recopie la date de modification de la source sur le résultat.
    pub preserve_mtime: bool,
    pub prefer_engine: Option<EngineId>,
    pub timeout: Option<Duration>,
    pub memory_limit: Option<u64>,
}

impl JobSpec {
    pub fn new(input: impl Into<PathBuf>, target: impl Into<FormatId>) -> Self {
        Self {
            input: input.into(),
            target: target.into(),
            output_dir: None,
            options: OptionValues::new(),
            name_template: "{stem}".into(),
            conflict: Conflict::Rename,
            preserve_mtime: false,
            prefer_engine: None,
            timeout: None,
            memory_limit: None,
        }
    }
}

/// Événements émis pendant une tâche (sérialisables pour l'interface ou `--json`).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEvent {
    Detected { format: FormatId, confidence: Confidence, extension_mismatch: bool },
    Planned { plan: Plan },
    Step { index: usize, count: usize, engine: EngineId, from: FormatId, to: FormatId },
    Progress { fraction: Option<f64>, speed: Option<String>, message: Option<String> },
    Verified { index: usize, verification: Verification },
    Finished { output: PathBuf },
    Skipped { reason: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct JobOutcome {
    pub input: PathBuf,
    /// `None` si la tâche a été ignorée (conflit « skip »).
    pub output: Option<PathBuf>,
    pub detected: FormatId,
    pub plan: Option<Plan>,
    pub verifications: Vec<Verification>,
    pub notes: Vec<String>,
    pub duration_ms: u128,
}

#[derive(Debug, Error)]
pub enum JobError {
    #[error("{}", chud_core::trf!("fichier introuvable : {}", "file not found: {}", .0.display()))]
    NotFound(PathBuf),
    #[error("{}", chud_core::trf!("format non reconnu : {}", "unrecognized format: {}", .0))]
    Unrecognized(String),
    /// Format reconnu, mais contenu que les moteurs ne parviennent pas à analyser.
    #[error("{0}")]
    Unreadable(String),
    #[error(transparent)]
    Plan(#[from] PlanError),
    #[error("{}", chud_core::trf!("étape {} ({engine}) : {error}", "step {} ({engine}): {error}", .step + 1))]
    Engine { step: usize, engine: EngineId, error: EngineError },
    #[error("{}", chud_core::trf!("vérification échouée à l'étape {} : {detail}", "verification failed at step {}: {detail}", .step + 1))]
    Verification { step: usize, detail: String },
    #[error("{}", chud_core::trf!("opération annulée", "operation cancelled"))]
    Cancelled,
    #[error("{}", chud_core::trf!("la sortie désignerait le fichier source lui-même", "the output would be the source file itself"))]
    SameFile,
    #[error("{}", chud_core::trf!("erreur d'entrée/sortie : {}", "input/output error: {}", .0))]
    Io(#[from] std::io::Error),
}

impl JobError {
    /// Code stable, traduit par l'interface.
    pub fn code(&self) -> &'static str {
        match self {
            JobError::NotFound(_) => "not_found",
            JobError::Unrecognized(_) => "unrecognized",
            JobError::Unreadable(_) => "invalid_input",
            JobError::Plan(PlanError::NoPath { .. }) => "no_path",
            JobError::Plan(PlanError::Unavailable { .. }) => "unavailable",
            JobError::Plan(PlanError::InvalidOption { .. }) => "invalid_option",
            JobError::Plan(_) => "unknown_format",
            JobError::Engine { error: EngineError::Cancelled, .. } | JobError::Cancelled => "cancelled",
            JobError::Engine { error, .. } => error.code(),
            JobError::Verification { .. } => "verification_failed",
            JobError::SameFile => "same_file",
            JobError::Io(_) => "io",
        }
    }
}

/// Résultat de la préparation : ce qui sera fait, sans rien écrire.
#[derive(Clone, Debug, Serialize)]
pub struct Prepared {
    pub input: PathBuf,
    pub detection: Detection,
    pub source: FormatId,
    pub plan: Plan,
    pub output: PathBuf,
}

pub struct JobRunner {
    registry: Arc<Registry>,
    cache_dir: PathBuf,
}

/// Supprime le répertoire de travail d'une tâche, quoi qu'il arrive.
struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unique_dir(base: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(base)?;
    for i in 0..10_000u32 {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let p = base.join(format!("{prefix}-{}-{nanos:09}-{i}", std::process::id()));
        match std::fs::create_dir(&p) {
            Ok(()) => return Ok(p),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other(chud_core::tr!("impossible de créer un répertoire de travail", "cannot create a working folder")))
}

/// Le format détecté sur le résultat est-il celui attendu (ou un proche parent déclaré) ?
fn compatible(catalog: &FormatCatalog, expected: &FormatId, detected: &FormatId) -> bool {
    if expected == detected {
        return true;
    }
    let rel =
        |a: &FormatId, b: &FormatId| catalog.get(a.as_str()).is_some_and(|d| d.based_on.contains(b) || d.variant_of.as_ref() == Some(b));
    rel(expected, detected) || rel(detected, expected)
}

fn sanitize_component(s: &str) -> String {
    let cleaned: String = s.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_owned();
    if trimmed.is_empty() { "sortie".into() } else { trimmed }
}

/// Nom de la source sans son extension (y compris composée).
fn stem_of(path: &Path, catalog: &FormatCatalog) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // Extensions composées (« .tar.gz ») retirées en entier.
    let lower = name.to_lowercase();
    let mut best = 0;
    for def in catalog.all() {
        for ext in &def.extensions {
            let suffix = format!(".{ext}");
            if lower.ends_with(&suffix) && suffix.len() > best && suffix.len() < name.len() {
                best = suffix.len();
            }
        }
    }
    if best > 0 {
        return name[..name.len() - best].to_owned();
    }
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

impl JobRunner {
    pub fn new(registry: Arc<Registry>, cache_dir: PathBuf) -> Self {
        Self { registry, cache_dir }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Supprime les répertoires de travail abandonnés (arrêt brutal) plus vieux que `max_age`.
    pub fn cleanup_stale(&self, max_age: Duration) {
        let Ok(rd) = std::fs::read_dir(self.cache_dir.join("jobs")) else { return };
        for e in rd.flatten() {
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > max_age);
            if old && e.file_name().to_string_lossy().starts_with("job-") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }

    /// Détecte, sonde et planifie, sans rien écrire.
    pub fn prepare(&self, spec: &JobSpec) -> Result<Prepared, JobError> {
        let input = std::path::absolute(&spec.input)?;
        if !input.exists() {
            return Err(JobError::NotFound(input));
        }
        let catalog = self.registry.catalog();
        let detection = detect_path(&input, catalog)?;
        let source = detection
            .format
            .clone()
            .ok_or_else(|| JobError::Unrecognized(detection.note.clone().unwrap_or_else(|| input.display().to_string())))?;
        let info = self.registry.probe_checked(&input, &source).map_err(JobError::Unreadable)?;
        let plan = self.registry.plan(&source, info.as_ref(), &spec.target, &spec.options, spec.prefer_engine.as_ref())?;
        let output = self.output_path(&input, spec)?;
        Ok(Prepared { input, detection, source, plan, output })
    }

    /// Chemin de sortie souhaité (avant résolution des conflits).
    fn output_path(&self, input: &Path, spec: &JobSpec) -> Result<PathBuf, JobError> {
        let catalog = self.registry.catalog();
        let dir = match &spec.output_dir {
            Some(d) => std::path::absolute(d)?,
            None => input.parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        let stem = stem_of(input, catalog);
        let name = spec.name_template.replace("{stem}", &stem).replace("{target}", spec.target.as_str());
        let name = sanitize_component(&name);
        let ext = catalog.get(spec.target.as_str()).and_then(|d| d.output_extension()).map(str::to_owned);
        Ok(match ext {
            Some(e) if spec.target.as_str() != "directory" => dir.join(format!("{name}.{e}")),
            _ => dir.join(name),
        })
    }

    /// Exécute une tâche complète.
    pub fn run(&self, spec: &JobSpec, cancel: &CancelToken, on_event: &(dyn Fn(JobEvent) + Sync)) -> Result<JobOutcome, JobError> {
        let started = Instant::now();
        let prepared = self.prepare(spec)?;
        let Prepared { input, detection, source, plan, output: wanted } = prepared;
        on_event(JobEvent::Detected {
            format: source.clone(),
            confidence: detection.confidence,
            extension_mismatch: detection.extension_mismatch,
        });
        let mut notes = Vec::new();
        if detection.extension_mismatch {
            notes.push(chud_core::trf!(
                "l'extension ne correspond pas au contenu : fichier traité comme {}",
                "the extension does not match the content: file handled as {}",
                self.registry.catalog().get(source.as_str()).map_or(source.as_str(), |d| d.name.as_str())
            ));
        }
        if detection.confidence == Confidence::Low {
            notes.push(
                chud_core::tr!(
                    "format déduit de l'extension seulement : le contenu n'a pas pu être confirmé",
                    "format inferred from the extension only: the content could not be confirmed"
                )
                .into(),
            );
        }
        on_event(JobEvent::Planned { plan: plan.clone() });

        if spec.conflict == Conflict::Skip && wanted.exists() {
            let reason = chud_core::trf!("{} existe déjà", "{} already exists", wanted.display());
            on_event(JobEvent::Skipped { reason: reason.clone() });
            notes.push(reason);
            return Ok(JobOutcome {
                input,
                output: None,
                detected: source,
                plan: Some(plan),
                verifications: vec![],
                notes,
                duration_ms: started.elapsed().as_millis(),
            });
        }
        if same_file(&input, &wanted) && spec.conflict == Conflict::Overwrite {
            return Err(JobError::SameFile);
        }

        let staging = Staging(unique_dir(&self.cache_dir.join("jobs"), "job")?);
        let catalog = self.registry.catalog();
        let count = plan.steps.len();
        let mut current = input.clone();
        let mut verifications = Vec::new();
        let throttle = Mutex::new(Instant::now() - Duration::from_secs(1));

        for (i, step) in plan.steps.iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            let engine: &Arc<dyn Engine> = self.registry.engine(&step.engine).ok_or_else(|| JobError::Engine {
                step: i,
                engine: step.engine.clone(),
                error: EngineError::Other(chud_core::tr!("moteur introuvable", "engine not found").into()),
            })?;
            on_event(JobEvent::Step { index: i, count, engine: step.engine.clone(), from: step.from.clone(), to: step.to.clone() });
            let ext = catalog.get(step.to.as_str()).and_then(|d| d.output_extension()).unwrap_or("bin");
            let out = if step.to.as_str() == "directory" { staging.0.join(format!("s{i}")) } else { staging.0.join(format!("s{i}.{ext}")) };
            let work = staging.0.join(format!("w{i}"));
            std::fs::create_dir_all(&work)?;
            let specs = engine.options(&step.from, &step.to);
            let options =
                spec.options.resolve(&specs).map_err(|e| JobError::Plan(PlanError::InvalidOption { key: e.key, message: e.message }))?;
            let source_info = self.registry.probe(&current, &step.from);
            let task = StepTask {
                input: current.clone(),
                output: out.clone(),
                from: step.from.clone(),
                to: step.to.clone(),
                options,
                source: source_info,
            };

            let progress = {
                let throttle = &throttle;
                move |p: Progress| {
                    let now = Instant::now();
                    let mut last = throttle.lock().unwrap_or_else(|e| e.into_inner());
                    let done = p.fraction.is_some_and(|f| f >= 1.0);
                    if !done && now.duration_since(*last) < Duration::from_millis(100) {
                        return;
                    }
                    *last = now;
                    let overall = p.fraction.map(|f| (i as f64 + f.clamp(0.0, 1.0)) / count as f64);
                    on_event(JobEvent::Progress { fraction: overall, speed: p.speed, message: p.message });
                }
            };
            // Le contexte exige une fermeture 'static : on relaie par un canal borné à cette étape.
            let (tx, rx) = crossbeam_channel::unbounded::<Progress>();
            let ctx = ExecContext {
                cancel: cancel.clone(),
                progress: Arc::new(move |p| {
                    let _ = tx.send(p);
                }),
                work_dir: work.clone(),
                timeout: spec.timeout,
                memory_limit: spec.memory_limit,
            };
            let result = std::thread::scope(|s| {
                let handle = s.spawn(|| {
                    let r = engine.execute(&task, &ctx);
                    let v = r.as_ref().ok().map(|_| engine.verify(&out, &task, &ctx));
                    drop(ctx);
                    (r, v)
                });
                for p in rx.iter() {
                    progress(p);
                }
                handle.join().expect("le moteur a paniqué")
            });
            let (exec, verification) = result;
            let output = match exec {
                Ok(o) => o,
                Err(EngineError::Cancelled) => return Err(JobError::Cancelled),
                Err(error) => return Err(JobError::Engine { step: i, engine: step.engine.clone(), error }),
            };
            notes.extend(output.notes);
            let verification = verification.unwrap_or(Verification::SignatureOnly);
            if cancel.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            if let Verification::Failed(detail) = &verification {
                return Err(JobError::Verification { step: i, detail: detail.clone() });
            }
            // Contrôle indépendant : le fichier existe, n'est pas vide et a bien le format attendu.
            if step.to.as_str() != "directory" {
                let meta = std::fs::metadata(&out).map_err(|_| JobError::Verification {
                    step: i,
                    detail: chud_core::tr!("aucun fichier produit", "no file produced").into(),
                })?;
                if meta.len() == 0 {
                    return Err(JobError::Verification {
                        step: i,
                        detail: chud_core::tr!("fichier produit vide", "empty output file").into(),
                    });
                }
                let d = detect_path(&out, catalog)?;
                match &d.format {
                    Some(f) if compatible(catalog, &step.to, f) => {}
                    Some(f) if d.confidence != Confidence::Low => {
                        return Err(JobError::Verification {
                            step: i,
                            detail: chud_core::trf!(
                                "le fichier produit est reconnu comme {f} et non comme {}",
                                "the output file is recognized as {f}, not as {}",
                                step.to
                            ),
                        });
                    }
                    _ => notes.push(chud_core::trf!(
                        "format du résultat non confirmé par la détection indépendante ({})",
                        "result format not confirmed by independent detection ({})",
                        step.to
                    )),
                }
            } else if !out.is_dir() {
                return Err(JobError::Verification {
                    step: i,
                    detail: chud_core::tr!("aucun dossier produit", "no folder produced").into(),
                });
            }
            on_event(JobEvent::Verified { index: i, verification: verification.clone() });
            verifications.push(verification);
            current = out;
        }

        let is_dir = plan.target.as_str() == "directory";
        let mut wanted = wanted;
        if is_dir && spec.name_template == "{stem}" {
            // Extraction « intelligente » : une archive qui ne contient qu'un dossier racine est
            // extraite directement, sans dossier supplémentaire (évite « x/x/fichier »).
            let children: Vec<_> = std::fs::read_dir(&current)?.flatten().collect();
            if let [only] = children.as_slice()
                && only.file_type().map(|t| t.is_dir()).unwrap_or(false)
            {
                let name = sanitize_component(&only.file_name().to_string_lossy());
                wanted = wanted.parent().map(|p| p.join(&name)).unwrap_or_else(|| PathBuf::from(&name));
                current = only.path();
                notes.push(chud_core::trf!(
                    "un seul dossier racine (« {name} ») : extrait directement, sans dossier supplémentaire",
                    "a single root folder (“{name}”): extracted directly, without an extra folder"
                ));
            }
        }
        let final_path = finalize(&current, &wanted, &input, spec.conflict, is_dir)?;
        let Some(final_path) = final_path else {
            let reason = chud_core::trf!("{} existe déjà", "{} already exists", wanted.display());
            on_event(JobEvent::Skipped { reason: reason.clone() });
            notes.push(reason);
            return Ok(JobOutcome {
                input,
                output: None,
                detected: source,
                plan: Some(plan),
                verifications,
                notes,
                duration_ms: started.elapsed().as_millis(),
            });
        };
        if spec.preserve_mtime
            && let (Ok(t), true) = (std::fs::metadata(&input).and_then(|m| m.modified()), final_path.is_file())
            && let Ok(f) = std::fs::File::options().write(true).open(&final_path)
        {
            let _ = f.set_modified(t);
        }
        drop(staging);
        // Un résultat plus gros que l'original surprend (réencodage d'un fichier déjà très
        // compressé, GIF…) : on le dit plutôt que de laisser croire à un gain.
        if let (Ok(a), Ok(b)) = (std::fs::metadata(&input), std::fs::metadata(&final_path))
            && a.is_file()
            && b.is_file()
            && b.len() > a.len() + a.len() / 20
        {
            notes.push(chud_core::trf!(
                "le résultat ({}) est plus gros que l'original ({})",
                "the result ({}) is larger than the original ({})",
                human_size(b.len()),
                human_size(a.len())
            ));
        }
        on_event(JobEvent::Finished { output: final_path.clone() });
        Ok(JobOutcome {
            input,
            output: Some(final_path),
            detected: source,
            plan: Some(plan),
            verifications,
            notes,
            duration_ms: started.elapsed().as_millis(),
        })
    }

    /// Poids d'ordonnancement d'une tâche préparée (le plus lourd de ses moteurs).
    pub fn weight(&self, plan: &Plan) -> u32 {
        plan.steps.iter().filter_map(|s| self.registry.engine(&s.engine).map(|e| e.weight(&s.from, &s.to))).max().unwrap_or(1)
    }
}

fn numbered(wanted: &Path, n: u32) -> PathBuf {
    let parent = wanted.parent().unwrap_or(Path::new(""));
    let name = wanted.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    // « fichier.tar.gz » → « fichier (1).tar.gz » : on numérote avant la première extension connue.
    let split = name.to_lowercase().rfind(".tar.").or_else(|| name.rfind('.'));
    let (stem, ext) = match split {
        Some(0) | None => (name.as_str(), ""),
        Some(i) => (&name[..i], &name[i..]),
    };
    parent.join(format!("{stem} ({n}){ext}"))
}

/// Déplace le résultat vers sa destination. Renvoie `None` si ignoré (conflit « skip »).
fn finalize(staged: &Path, wanted: &Path, source: &Path, conflict: Conflict, is_dir: bool) -> Result<Option<PathBuf>, JobError> {
    if let Some(parent) = wanted.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let overwrite = conflict == Conflict::Overwrite && !is_dir && !same_file(source, wanted);
    if overwrite && wanted.exists() && !wanted.is_file() {
        return Err(JobError::Io(std::io::Error::other(chud_core::trf!(
            "{} n'est pas un fichier : remplacement refusé",
            "{} is not a file: replacing it is refused",
            wanted.display()
        ))));
    }
    // Réservation du nom.
    let mut n = 0u32;
    let target = loop {
        let candidate = if n == 0 { wanted.to_path_buf() } else { numbered(wanted, n) };
        if overwrite && n == 0 {
            break candidate;
        }
        let reserved = if is_dir {
            std::fs::create_dir(&candidate)
        } else {
            std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate).map(|_| ())
        };
        match reserved {
            Ok(()) => break candidate,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if conflict == Conflict::Skip {
                    return Ok(None);
                }
                n += 1;
                if n > 9999 {
                    return Err(JobError::Io(std::io::Error::other(chud_core::tr!("aucun nom de sortie libre", "no free output name"))));
                }
            }
            Err(e) => return Err(e.into()),
        }
    };
    let res = if is_dir { move_dir_contents(staged, &target) } else { move_file(staged, &target) };
    if let Err(e) = res {
        // On libère la réservation (fichier vide ou dossier créé par nous).
        if !overwrite {
            if is_dir {
                let _ = std::fs::remove_dir_all(&target);
            } else if std::fs::metadata(&target).map(|m| m.len() == 0).unwrap_or(false) {
                let _ = std::fs::remove_file(&target);
            }
        }
        return Err(e.into());
    }
    Ok(Some(target))
}

/// Fichier tenu un instant par un autre programme (aperçu de l'Explorateur, indexation,
/// antivirus) : Windows refuse alors l'accès (erreurs 5, 32, 33).
fn is_busy(e: &std::io::Error) -> bool {
    cfg!(windows) && (e.kind() == std::io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33)))
}

/// Réessaie pendant une dizaine de secondes tant que le fichier est occupé.
fn retry_busy<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut tries = 0;
    loop {
        match op() {
            Err(e) if is_busy(&e) && tries < 40 => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(250));
            }
            r => return r,
        }
    }
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match retry_busy(|| std::fs::rename(from, to)) {
        Ok(()) => Ok(()),
        Err(_) => copy_then_rename(from, to),
    }
}

/// Volumes différents : copie vers un fichier temporaire voisin, propre à cette tâche (les
/// conversions simultanées ne doivent jamais partager ce nom), puis renommage.
fn copy_then_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = to.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".~chud-{}-{n}.part", std::process::id()));
    let r = retry_busy(|| std::fs::copy(from, &tmp)).and_then(|_| retry_busy(|| std::fs::rename(&tmp, to)));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    } else {
        let _ = std::fs::remove_file(from);
    }
    r
}

fn move_dir_contents(from: &Path, to: &Path) -> std::io::Result<()> {
    for ent in std::fs::read_dir(from)? {
        let ent = ent?;
        let dest = to.join(ent.file_name());
        let ft = ent.file_type()?;
        if retry_busy(|| std::fs::rename(ent.path(), &dest)).is_err() {
            if ft.is_dir() {
                std::fs::create_dir(&dest)?;
                move_dir_contents(&ent.path(), &dest)?;
            } else if ft.is_file() {
                retry_busy(|| std::fs::copy(ent.path(), &dest))?;
            } else {
                return Err(std::io::Error::other(chud_core::tr!("lien inattendu dans le résultat", "unexpected link in the result")));
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Exécution par lots
// ---------------------------------------------------------------------------------------------

/// Jauge de ressources : la somme des poids des tâches en cours ne dépasse pas la capacité.
pub struct ResourceGate {
    capacity: u32,
    used: Mutex<u32>,
    cv: Condvar,
}

impl ResourceGate {
    pub fn new(capacity: u32) -> Self {
        Self { capacity: capacity.max(1), used: Mutex::new(0), cv: Condvar::new() }
    }

    /// Réserve `weight` (plafonné à la capacité) ; libéré à la destruction du jeton.
    pub fn acquire(&self, weight: u32) -> GateToken<'_> {
        let w = weight.clamp(1, self.capacity);
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used + w > self.capacity {
            used = self.cv.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += w;
        GateToken { gate: self, weight: w }
    }
}

pub struct GateToken<'a> {
    gate: &'a ResourceGate,
    weight: u32,
}

impl Drop for GateToken<'_> {
    fn drop(&mut self) {
        let mut used = self.gate.used.lock().unwrap_or_else(|e| e.into_inner());
        *used -= self.weight;
        self.gate.cv.notify_all();
    }
}

/// Exécute un lot de tâches avec `workers` fils et une jauge de `capacity` unités.
/// `on_event(index_de_la_tâche, événement)` est appelé depuis les fils de travail.
pub fn run_batch(
    runner: &JobRunner,
    jobs: &[JobSpec],
    workers: usize,
    capacity: u32,
    cancel: &CancelToken,
    on_event: &(dyn Fn(usize, JobEvent) + Sync),
) -> Vec<Result<JobOutcome, JobError>> {
    let gate = ResourceGate::new(capacity);
    let next = Mutex::new(0usize);
    let results: Mutex<Vec<Option<Result<JobOutcome, JobError>>>> = Mutex::new((0..jobs.len()).map(|_| None).collect());
    std::thread::scope(|s| {
        for _ in 0..workers.clamp(1, jobs.len().max(1)) {
            s.spawn(|| {
                loop {
                    let idx = {
                        let mut n = next.lock().unwrap_or_else(|e| e.into_inner());
                        if *n >= jobs.len() {
                            break;
                        }
                        *n += 1;
                        *n - 1
                    };
                    let result = if cancel.is_cancelled() {
                        Err(JobError::Cancelled)
                    } else {
                        let weight = runner.prepare(&jobs[idx]).map(|p| runner.weight(&p.plan)).unwrap_or(1);
                        let _token = gate.acquire(weight);
                        runner.run(&jobs[idx], cancel, &|ev| on_event(idx, ev))
                    };
                    results.lock().unwrap_or_else(|e| e.into_inner())[idx] = Some(result);
                }
            });
        }
    });
    results.into_inner().unwrap_or_else(|e| e.into_inner()).into_iter().map(|r| r.unwrap_or(Err(JobError::Cancelled))).collect()
}

/// Taille lisible dans la langue courante (« 13,3 Mo », « 13.3 MB »).
fn human_size(n: u64) -> String {
    let en = chud_core::i18n::lang() == chud_core::i18n::Lang::En;
    let units: [&str; 4] = if en { ["KB", "MB", "GB", "TB"] } else { ["Ko", "Mo", "Go", "To"] };
    if n < 1024 {
        return if en { format!("{n} B") } else { format!("{n} o") };
    }
    let mut v = n as f64 / 1024.0;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    let s = format!("{:.1} {}", v, units[i]);
    if en { s } else { s.replace('.', ",") }
}

#[cfg(test)]
mod tests {
    /// Régression : des conversions simultanées vers un autre disque partageaient le même
    /// fichier temporaire (« os error 32 », voire contenus mélangés).
    #[test]
    fn simultaneous_cross_volume_moves_never_share_a_temp_file() {
        let dir = std::env::temp_dir().join(format!("chud-move-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("out")).unwrap();
        std::thread::scope(|s| {
            for i in 0..16 {
                let dir = &dir;
                s.spawn(move || {
                    let src = dir.join(format!("src-{i}"));
                    let body = vec![i as u8; 4 * 1024 * 1024];
                    std::fs::write(&src, &body).unwrap();
                    let dst = dir.join("out").join(format!("piste {i}.mp3"));
                    super::copy_then_rename(&src, &dst).unwrap();
                    assert_eq!(std::fs::read(&dst).unwrap(), body);
                    assert!(!src.exists());
                });
            }
        });
        let left: Vec<_> = std::fs::read_dir(dir.join("out"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(left.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn human_sizes_in_french() {
        assert_eq!(super::human_size(512), "512 o");
        assert_eq!(super::human_size(13_311_089), "12,7 Mo");
        chud_core::i18n::with_lang(chud_core::i18n::Lang::En, || {
            assert_eq!(super::human_size(512), "512 B");
            assert_eq!(super::human_size(13_311_089), "12.7 MB");
        });
    }

    use super::*;

    #[test]
    fn numbering_keeps_compound_extensions() {
        assert_eq!(numbered(Path::new("/x/a.tar.gz"), 2), PathBuf::from("/x/a (2).tar.gz"));
        assert_eq!(numbered(Path::new("/x/dossier"), 1), PathBuf::from("/x/dossier (1)"));
    }

    #[test]
    fn sanitizes_names() {
        assert_eq!(sanitize_component("a/b:c?"), "a_b_c_");
        assert_eq!(sanitize_component("  ...  "), "sortie");
    }

    #[test]
    fn gate_limits_weight() {
        let g = ResourceGate::new(4);
        let a = g.acquire(3);
        let b = g.acquire(1);
        assert_eq!(*g.used.lock().unwrap(), 4);
        drop(a);
        drop(b);
        let _c = g.acquire(10); // plafonné à la capacité
        assert_eq!(*g.used.lock().unwrap(), 4);
    }
}
