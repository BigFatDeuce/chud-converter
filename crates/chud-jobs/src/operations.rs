//! Opérations (fusionner, diviser, pivoter, archiver) : même déroulement qu'une conversion —
//! dossier de travail, vérification, finalisation atomique, originaux jamais modifiés.

use crate::{
    Conflict, JobError, JobEvent, JobOutcome, JobRunner, Staging, check_produced, finalize, human_size, run_engine, same_file,
    sanitize_component, stem_of, unique_dir,
};
use chud_core::operation::{ARCHIVE_CREATE, PDF_MERGE, PDF_ROTATE, PDF_SPLIT};
use chud_core::{
    CancelToken, Engine, EngineError, EngineId, FormatId, Grouping, Operation, OperationReport, OperationTask, OptionSpec, OptionValues,
    PlanError, Verification, detect_path, operation, tr, trf,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct OperationSpec {
    pub operation: String,
    /// Fichiers, dans l'ordre choisi.
    pub inputs: Vec<PathBuf>,
    pub options: OptionValues,
    /// Dossier de sortie ; par défaut celui du premier fichier.
    pub output_dir: Option<PathBuf>,
    /// Nom du résultat, sans extension ; par défaut, d'après les fichiers.
    pub name: Option<String>,
    pub conflict: Conflict,
    pub timeout: Option<Duration>,
    pub memory_limit: Option<u64>,
}

impl OperationSpec {
    pub fn new(operation: impl Into<String>, inputs: Vec<PathBuf>) -> Self {
        Self {
            operation: operation.into(),
            inputs,
            options: OptionValues::new(),
            output_dir: None,
            name: None,
            conflict: Conflict::Rename,
            timeout: None,
            memory_limit: None,
        }
    }
}

/// Répartit des fichiers en tâches : une seule, ou une par fichier, selon l'opération.
pub fn operation_specs(base: &OperationSpec) -> Result<Vec<OperationSpec>, JobError> {
    let op = find(&base.operation)?;
    Ok(match op.grouping {
        Grouping::Together => vec![base.clone()],
        Grouping::Each => base
            .inputs
            .iter()
            .map(|p| OperationSpec {
                inputs: vec![p.clone()],
                // Un nom choisi ne vaut que pour un résultat unique.
                name: base.name.clone().filter(|_| base.inputs.len() == 1),
                ..base.clone()
            })
            .collect(),
    })
}

/// Ce que l'opération fera, sans rien écrire.
#[derive(Clone, Debug, Serialize)]
pub struct PreparedOperation {
    pub operation: Operation,
    pub engine: EngineId,
    pub inputs: Vec<PathBuf>,
    pub target: FormatId,
    pub output: PathBuf,
    pub report: OperationReport,
    /// Options complétées par leurs valeurs par défaut.
    pub options: OptionValues,
}

fn find(id: &str) -> Result<Operation, JobError> {
    operation(id).ok_or_else(|| JobError::BadInput(trf!("opération inconnue : {id}", "unknown operation: {id}")))
}

/// Nom proposé pour le résultat, sans extension.
fn default_name(op: &str, inputs: &[PathBuf], stem: &dyn Fn(&Path) -> String) -> String {
    let first = inputs.first().map(|p| stem(p)).unwrap_or_default();
    match op {
        PDF_MERGE => trf!("{first} - fusion", "{first} - merged"),
        PDF_SPLIT => trf!("{first} - pages", "{first} - pages"),
        PDF_ROTATE => trf!("{first} - pivoté", "{first} - rotated"),
        // Comme 7-Zip : le nom de l'élément seul, sinon celui du dossier commun.
        ARCHIVE_CREATE if inputs.len() == 1 => match inputs[0].file_name() {
            Some(n) if inputs[0].is_dir() => n.to_string_lossy().into_owned(),
            _ => first,
        },
        ARCHIVE_CREATE => {
            let parent = inputs[0].parent();
            let common = inputs.iter().all(|p| p.parent() == parent);
            parent
                .filter(|_| common)
                .and_then(Path::file_name)
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Archive".into())
        }
        _ => first,
    }
}

impl JobRunner {
    /// Options de l'opération avec le moteur présent.
    pub fn operation_options(&self, id: &str) -> Option<Vec<OptionSpec>> {
        self.registry().operation_engine(id).map(|e| e.operation_options(id))
    }

    /// Vérifie les fichiers, les options et l'issue attendue, sans rien écrire.
    pub fn prepare_operation(&self, spec: &OperationSpec) -> Result<PreparedOperation, JobError> {
        let op = find(&spec.operation)?;
        let engine = self.registry().operation_engine(op.id).ok_or_else(|| JobError::MissingEngine(op.engine.clone()))?.clone();
        let inputs = self.operation_inputs(&op, &spec.inputs)?;
        let options = resolve(&engine, &op, &spec.options)?;
        let fail = |error: EngineError| engine_error(&engine, error);
        let target = engine.operation_target(op.id, &options).map_err(fail)?;
        let report = engine.assess_operation(op.id, &inputs, &options).map_err(fail)?;
        let output = self.operation_output(&op, spec, &inputs, &target)?;
        Ok(PreparedOperation { engine: engine.id().clone(), operation: op, inputs, target, output, report, options })
    }

    fn operation_inputs(&self, op: &Operation, inputs: &[PathBuf]) -> Result<Vec<PathBuf>, JobError> {
        if inputs.len() < op.min_inputs {
            return Err(JobError::BadInput(if inputs.is_empty() {
                tr!("aucun fichier choisi", "no file chosen").into()
            } else {
                trf!("il faut au moins {} fichiers", "at least {} files are needed", op.min_inputs)
            }));
        }
        if inputs.len() > op.max_inputs {
            return Err(JobError::BadInput(trf!("{} fichiers au maximum", "{} files at most", op.max_inputs)));
        }
        let catalog = self.registry().catalog();
        inputs
            .iter()
            .map(|p| {
                let p = std::path::absolute(p)?;
                if !p.exists() {
                    return Err(JobError::NotFound(p));
                }
                if op.accepts.is_empty() {
                    return Ok(p);
                }
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let format = detect_path(&p, catalog)?.format;
                if format.as_ref().is_some_and(|f| op.accepts.contains(f)) {
                    return Ok(p);
                }
                let label = |f: &FormatId| catalog.get(f.as_str()).map_or(f.to_string(), |d| d.label().to_owned());
                let wanted = op.accepts.iter().map(label).collect::<Vec<_>>().join(", ");
                Err(JobError::BadInput(match &format {
                    Some(f) => trf!(
                        "« {name} » est au format {}, pas {wanted} : convertissez-le d'abord",
                        "“{name}” is in the {} format, not {wanted}: convert it first",
                        label(f)
                    ),
                    None => trf!("« {name} » : format non reconnu", "“{name}”: unrecognized format"),
                }))
            })
            .collect()
    }

    fn operation_output(&self, op: &Operation, spec: &OperationSpec, inputs: &[PathBuf], target: &FormatId) -> Result<PathBuf, JobError> {
        let catalog = self.registry().catalog();
        let dir = match &spec.output_dir {
            Some(d) => std::path::absolute(d)?,
            None => inputs[0].parent().map(Path::to_path_buf).unwrap_or_default(),
        };
        let chosen = spec.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
        let mut name = chosen.unwrap_or_else(|| default_name(op.id, inputs, &|p| stem_of(p, catalog)));
        let Some(ext) = catalog.get(target.as_str()).and_then(|d| d.output_extension()).filter(|_| target.as_str() != "directory") else {
            return Ok(dir.join(sanitize_component(&name)));
        };
        // « rapport.pdf » saisi tel quel ne devient pas « rapport.pdf.pdf ».
        let suffix = format!(".{ext}");
        if let Some(cut) = name.len().checked_sub(suffix.len()).filter(|c| *c > 0 && name.is_char_boundary(*c))
            && name[cut..].eq_ignore_ascii_case(&suffix)
        {
            name.truncate(cut);
        }
        Ok(dir.join(format!("{}{suffix}", sanitize_component(&name))))
    }

    /// Exécute une opération complète.
    pub fn run_operation(
        &self,
        spec: &OperationSpec,
        cancel: &CancelToken,
        on_event: &(dyn Fn(JobEvent) + Sync),
    ) -> Result<JobOutcome, JobError> {
        let started = Instant::now();
        let p = self.prepare_operation(spec)?;
        let engine = self.registry().operation_engine(p.operation.id).ok_or_else(|| JobError::MissingEngine(p.engine.clone()))?.clone();
        on_event(JobEvent::OperationPlanned { operation: p.operation.id.into(), report: p.report.clone() });
        let outcome = |output: Option<PathBuf>, verifications: Vec<Verification>, notes: Vec<String>| JobOutcome {
            input: p.inputs[0].clone(),
            output,
            detected: p.target.clone(),
            plan: None,
            report: Some(p.report.clone()),
            verifications,
            notes,
            duration_ms: started.elapsed().as_millis(),
        };
        let skipped = || trf!("{} existe déjà", "{} already exists", p.output.display());
        if spec.conflict == Conflict::Skip && p.output.exists() {
            on_event(JobEvent::Skipped { reason: skipped() });
            return Ok(outcome(None, Vec::new(), vec![skipped()]));
        }
        if spec.conflict == Conflict::Overwrite && p.inputs.iter().any(|i| same_file(i, &p.output)) {
            return Err(JobError::SameFile);
        }
        if cancel.is_cancelled() {
            return Err(JobError::Cancelled);
        }

        let staging = Staging(unique_dir(&self.cache_dir.join("jobs"), "job")?);
        let is_dir = p.target.as_str() == "directory";
        let catalog = self.registry().catalog();
        let ext = catalog.get(p.target.as_str()).and_then(|d| d.output_extension()).unwrap_or("bin");
        let out = staging.0.join(if is_dir { "out".to_owned() } else { format!("out.{ext}") });
        let work = staging.0.join("work");
        std::fs::create_dir_all(&work)?;
        let from = match p.operation.accepts.first() {
            Some(f) => f.clone(),
            None => FormatId::new("directory"),
        };
        on_event(JobEvent::Step { index: 0, count: 1, engine: p.engine.clone(), from, to: p.target.clone() });
        let task = OperationTask {
            operation: p.operation.id.into(),
            inputs: p.inputs.clone(),
            output: out.clone(),
            target: p.target.clone(),
            options: p.options.clone(),
        };
        let (exec, verification) = run_engine(
            cancel,
            &work,
            spec.timeout,
            spec.memory_limit,
            &|pr| on_event(JobEvent::Progress { fraction: pr.fraction, speed: pr.speed, message: pr.message }),
            &|ctx| {
                let r = engine.execute_operation(&task, ctx);
                let v = r.as_ref().ok().map(|_| engine.verify_operation(&task, ctx));
                (r, v)
            },
        );
        let mut notes = match exec {
            // Les remarques de l'aperçu figurent déjà dans le rapport.
            Ok(o) => o.notes.into_iter().filter(|n| !p.report.notes.contains(n)).collect::<Vec<_>>(),
            Err(EngineError::Cancelled) => return Err(JobError::Cancelled),
            Err(error) => return Err(engine_error(&engine, error)),
        };
        if cancel.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let verification = verification.unwrap_or(Verification::SignatureOnly);
        if let Verification::Failed(detail) = verification {
            return Err(JobError::Verification { step: 0, detail });
        }
        check_produced(catalog, &out, &p.target, &mut notes).map_err(|detail| JobError::Verification { step: 0, detail })?;
        on_event(JobEvent::Verified { index: 0, verification: verification.clone() });

        let Some(final_path) = finalize(&out, &p.output, &p.inputs, spec.conflict, is_dir)? else {
            on_event(JobEvent::Skipped { reason: skipped() });
            notes.push(skipped());
            return Ok(outcome(None, vec![verification], notes));
        };
        drop(staging);
        if let Ok(m) = std::fs::metadata(&final_path)
            && m.is_file()
        {
            notes.push(trf!("taille du résultat : {}", "size of the result: {}", human_size(m.len())));
        }
        on_event(JobEvent::Finished { output: final_path.clone() });
        Ok(outcome(Some(final_path), vec![verification], notes))
    }

    /// Poids d'ordonnancement d'une opération.
    pub fn operation_weight(&self, spec: &OperationSpec) -> u32 {
        let any = FormatId::new("pdf");
        self.registry().operation_engine(&spec.operation).map(|e| e.weight(&any, &any)).unwrap_or(1)
    }
}

fn resolve(engine: &Arc<dyn Engine>, op: &Operation, options: &OptionValues) -> Result<OptionValues, JobError> {
    options
        .resolve(&engine.operation_options(op.id))
        .map_err(|e| JobError::Plan(PlanError::InvalidOption { key: e.key, message: e.message }))
}

/// Une option refusée par le moteur garde le même code que si elle l'avait été avant.
fn engine_error(engine: &Arc<dyn Engine>, error: EngineError) -> JobError {
    match error {
        EngineError::Cancelled => JobError::Cancelled,
        EngineError::InvalidOption { key, message } => JobError::Plan(PlanError::InvalidOption { key, message }),
        EngineError::Refused(m) | EngineError::InvalidInput(m) => JobError::BadInput(m),
        error => JobError::Engine { step: 0, engine: engine.id().clone(), error },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_files() {
        let stem = |p: &Path| p.file_stem().unwrap().to_string_lossy().into_owned();
        let name = |op: &str, files: &[&str]| default_name(op, &files.iter().map(PathBuf::from).collect::<Vec<_>>(), &stem);
        assert_eq!(name(PDF_MERGE, &["x/a.pdf", "x/b.pdf"]), "a - fusion");
        assert_eq!(name(PDF_ROTATE, &["x/a.pdf"]), "a - pivoté");
        assert_eq!(name(ARCHIVE_CREATE, &["photos/a.jpg"]), "a");
        assert_eq!(name(ARCHIVE_CREATE, &["photos/a.jpg", "photos/b.jpg"]), "photos");
        assert_eq!(name(ARCHIVE_CREATE, &["photos/a.jpg", "textes/b.txt"]), "Archive");
    }

    #[test]
    fn each_file_gets_its_own_job() {
        let mut base = OperationSpec::new(PDF_ROTATE, vec!["a.pdf".into(), "b.pdf".into()]);
        base.name = Some("nom".into());
        let each = operation_specs(&base).unwrap();
        assert_eq!(each.len(), 2);
        assert!(each.iter().all(|s| s.name.is_none()), "un nom unique ne vaut pas pour plusieurs résultats");
        base.operation = PDF_MERGE.into();
        assert_eq!(operation_specs(&base).unwrap().len(), 1);
        base.operation = "inconnue".into();
        assert!(operation_specs(&base).is_err());
    }
}
