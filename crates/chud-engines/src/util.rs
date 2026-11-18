use chud_core::trf;
use chud_core::{EngineError, ExecContext};
use chud_process::{ProcessError, ProcessOutput, ProcessSpec};
use std::path::Path;

/// Exécute un outil externe dans le contexte d'une tâche (annulation, délai, mémoire).
pub fn run_tool(
    spec: ProcessSpec,
    ctx: &ExecContext,
    on_stdout: &mut dyn FnMut(&str),
    on_stderr: &mut dyn FnMut(&str),
) -> Result<ProcessOutput, EngineError> {
    run_tool_with(spec, ctx, &|| false, on_stdout, on_stderr)
}

/// Variante avec une condition d'arrêt supplémentaire (ex. dépassement de taille).
pub fn run_tool_with(
    mut spec: ProcessSpec,
    ctx: &ExecContext,
    extra_stop: &dyn Fn() -> bool,
    on_stdout: &mut dyn FnMut(&str),
    on_stderr: &mut dyn FnMut(&str),
) -> Result<ProcessOutput, EngineError> {
    if spec.timeout.is_none() {
        spec.timeout = ctx.timeout;
    }
    if spec.memory_limit.is_none() {
        spec.memory_limit = ctx.memory_limit;
    }
    let cancel = ctx.cancel.clone();
    let stop = move || cancel.is_cancelled() || extra_stop();
    chud_process::run(&spec, &stop, on_stdout, on_stderr).map_err(map_process_error)
}

pub fn map_process_error(e: ProcessError) -> EngineError {
    match e {
        ProcessError::Cancelled { .. } => EngineError::Cancelled,
        ProcessError::Timeout(t, _) => EngineError::Timeout(t),
        ProcessError::NotAbsolute(p) => {
            EngineError::Other(trf!("chemin de moteur non absolu : {}", "engine path is not absolute: {}", p.display()))
        }
        ProcessError::Spawn { program, source } => {
            EngineError::Other(trf!("impossible de lancer {} : {source}", "cannot start {}: {source}", program.display()))
        }
        ProcessError::Io(e) => EngineError::Io(e),
    }
}

/// Résume la fin de stderr pour un message d'erreur lisible.
pub fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

pub fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// Taille totale d'un répertoire (sans suivre les liens).
pub fn dir_size(p: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if m.is_dir() {
                stack.push(e.path());
            } else {
                total += m.len();
            }
        }
    }
    total
}

/// Analyse une durée « 90 », « 1:30 », « 01:02:03.5 » en secondes. Rien d'autre n'est accepté.
pub fn parse_time(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit() || c == '.')) {
        return None;
    }
    let mut total = 0.0;
    for p in &parts {
        let v: f64 = p.parse().ok()?;
        total = total * 60.0 + v;
    }
    total.is_finite().then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_parsing() {
        assert_eq!(parse_time("90"), Some(90.0));
        assert_eq!(parse_time("1:30"), Some(90.0));
        assert_eq!(parse_time("01:00:01.5"), Some(3601.5));
        assert_eq!(parse_time("1;rm -rf"), None);
        assert_eq!(parse_time("-5"), None);
        assert_eq!(parse_time(""), None);
    }
}
