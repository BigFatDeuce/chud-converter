//! `chud` : interface en ligne de commande de Chud Converter.

use anyhow::{Context, Result, bail};
use chud_core::detect::detect_path;
use chud_core::{CancelToken, Category, EngineId, FormatCatalog, FormatId, OptionValues, Plan, Registry, Verification};
use chud_enginemgr::{AppPaths, EngineManager, EngineState, EngineStatus};
use chud_jobs::{Conflict, JobEvent, JobRunner, JobSpec, run_batch};
use clap::{Parser, Subcommand, ValueEnum};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Parser)]
#[command(name = "chud", version, about = "Chud Converter : conversion de fichiers locale, honnête et vérifiée")]
struct Cli {
    /// Sortie JSON (machine).
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Liste les formats connus.
    Formats {
        /// Filtre par catégorie (image, audio, video, document, archive…).
        #[arg(long)]
        category: Option<String>,
        /// Recherche dans les noms et extensions.
        query: Option<String>,
    },
    /// Détecte les moteurs installés (FFmpeg, 7-Zip, LibreOffice…).
    Engines,
    /// Détecte le format réel de fichiers.
    Detect { files: Vec<PathBuf> },
    /// Formats cibles possibles pour un fichier, avec la fidélité attendue.
    Targets { file: PathBuf },
    /// Montre le plan de conversion et le rapport de fidélité, sans rien écrire.
    Plan {
        file: PathBuf,
        #[arg(long = "to", short = 't')]
        to: String,
        /// Option de conversion « clé=valeur » (répétable).
        #[arg(long = "set", short = 's')]
        set: Vec<String>,
        #[arg(long)]
        engine: Option<String>,
    },
    /// Convertit un ou plusieurs fichiers.
    Convert {
        files: Vec<PathBuf>,
        #[arg(long = "to", short = 't')]
        to: String,
        /// Dossier de sortie (par défaut : celui de chaque source).
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long = "set", short = 's')]
        set: Vec<String>,
        /// Modèle du nom de sortie : {stem}, {target}.
        #[arg(long, default_value = "{stem}")]
        name: String,
        #[arg(long = "on-conflict", value_enum, default_value_t = ConflictArg::Rename)]
        on_conflict: ConflictArg,
        /// Nombre de conversions simultanées.
        #[arg(long, short = 'j')]
        jobs: Option<usize>,
        /// Recopie la date de modification de la source.
        #[arg(long)]
        keep_date: bool,
        /// Moteur à privilégier (ffmpeg, image, 7zip…).
        #[arg(long)]
        engine: Option<String>,
        /// Délai maximal par étape, en secondes.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Matrice des conversions disponibles par moteur.
    Matrix {
        /// Limite à un format source.
        #[arg(long)]
        from: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ConflictArg {
    Rename,
    Overwrite,
    Skip,
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("erreur : {e:#}");
            std::process::exit(2);
        }
    }
}

fn catalog() -> &'static FormatCatalog {
    FormatCatalog::builtin()
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

struct Env {
    manager: EngineManager,
    statuses: Vec<EngineStatus>,
    registry: Arc<Registry>,
}

fn load_engines() -> Result<Env> {
    let manager = EngineManager::new(AppPaths::detect()).context("chargement des réglages des moteurs")?;
    let mut statuses = manager.discover();
    let registry = Arc::new(manager.build_registry(&mut statuses));
    Ok(Env { manager, statuses, registry })
}

fn parse_options(set: &[String]) -> Result<OptionValues> {
    let mut o = OptionValues::new();
    for s in set {
        o.parse_assignment(s).map_err(|e| anyhow::anyhow!("--set {s} : {e}"))?;
    }
    Ok(o)
}

fn resolve_target(s: &str) -> Result<FormatId> {
    let c = catalog();
    let lower = s.trim().trim_start_matches('.').to_lowercase();
    if c.get(&lower).is_some() {
        return Ok(FormatId::from(lower.as_str()));
    }
    if let Some(d) = c.by_extension(&lower).into_iter().find(|d| !d.is_variant()) {
        return Ok(d.id.clone());
    }
    let suggestions: Vec<String> = c.search(&lower).into_iter().take(5).map(|d| d.id.to_string()).collect();
    if suggestions.is_empty() {
        bail!("format cible inconnu : « {s} » (voir `chud formats`)");
    }
    bail!("format cible inconnu : « {s} ». Vouliez-vous dire : {} ?", suggestions.join(", "))
}

fn fmt_name(id: &FormatId) -> String {
    catalog().get(id.as_str()).map_or_else(|| id.to_string(), |d| format!("{} ({})", d.name, id))
}

fn print_plan(plan: &Plan) {
    println!("Plan : {} → {}", fmt_name(&plan.source), fmt_name(&plan.target));
    for (i, s) in plan.steps.iter().enumerate() {
        println!("  {}. {} → {} avec {} — {}", i + 1, s.from, s.to, s.engine, s.fidelity.label_fr());
    }
    println!("Fidélité : {}", plan.report.headline_fr());
    for d in plan.report.details_fr(catalog()) {
        println!("  • {d}");
    }
}

/// Correspondance `*` / `?` sur un nom de fichier (insensible à la casse sous Windows).
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let fold = |s: &str| -> Vec<char> { if cfg!(windows) { s.to_lowercase().chars().collect() } else { s.chars().collect() } };
    let (p, n) = (fold(pattern), fold(name));
    let (mut pi, mut ni, mut star, mut mark) = (0, 0, None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Les invites Windows ne développent pas `*.mov` : on le fait ici, sur le seul nom de fichier
/// (pas de récursion, pas de motif dans les dossiers). Un chemin existant n'est jamais interprété.
fn expand_patterns(args: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for a in args {
        let name = a.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if a.exists() || !(name.contains('*') || name.contains('?')) {
            out.push(a);
            continue;
        }
        let parent = a.parent().filter(|p| !p.as_os_str().is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
        let mut found: Vec<PathBuf> = std::fs::read_dir(&parent)
            .with_context(|| format!("dossier illisible : {}", parent.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| wildcard_match(&name, &e.file_name().to_string_lossy()))
            .map(|e| if parent == *"." { PathBuf::from(e.file_name()) } else { e.path() })
            .collect();
        if found.is_empty() {
            bail!("aucun fichier ne correspond à « {} »", a.display());
        }
        found.sort();
        out.extend(found);
    }
    Ok(out)
}

fn run(cli: Cli) -> Result<i32> {
    match cli.cmd {
        Cmd::Formats { category, query } => {
            let cat = match &category {
                Some(c) => Some(Category::parse(c).with_context(|| format!("catégorie inconnue : {c}"))?),
                None => None,
            };
            let mut list: Vec<_> = match &query {
                Some(q) => catalog().search(q),
                None => catalog().all().iter().collect(),
            };
            list.retain(|d| cat.is_none_or(|c| d.category == c));
            if cli.json {
                return print_json(&list).map(|_| 0);
            }
            let mut current = None;
            for d in list {
                if current != Some(d.category) {
                    current = Some(d.category);
                    println!("\n{}", d.category.label_fr());
                }
                println!("  {:<12} {:<40} .{}", d.id.as_str(), d.name, d.extensions.join(" ."));
            }
            Ok(0)
        }
        Cmd::Engines => {
            let env = load_engines()?;
            if cli.json {
                return print_json(&env.statuses).map(|_| 0);
            }
            println!(
                "Données : {}{}",
                env.manager.paths().data_dir.display(),
                if env.manager.paths().portable { " (portable)" } else { "" }
            );
            println!("Moteurs intégrés : images (natif), données (natif)\n");
            for s in &env.statuses {
                let state = match &s.state {
                    EngineState::Ready => "prêt".to_string(),
                    EngineState::NoAdapter => "détecté (intégration à venir)".to_string(),
                    EngineState::Broken(e) => format!("en panne : {e}"),
                    EngineState::Missing => "absent".to_string(),
                    EngineState::Disabled => "désactivé".to_string(),
                };
                let origin = match s.origin {
                    Some(chud_core::EngineOrigin::Custom) => " [chemin personnalisé]",
                    Some(chud_core::EngineOrigin::Managed) => " [géré]",
                    Some(chud_core::EngineOrigin::System) => " [système]",
                    _ => "",
                };
                println!("{:<26} {}{}", s.name, state, s.version.as_ref().map(|v| format!(" — {v}")).unwrap_or_default());
                if let Some(p) = &s.path {
                    println!("{:<26} {}{origin}", "", p.display());
                }
                if s.state == EngineState::Missing
                    && let Some(h) = &s.hint
                {
                    println!("{:<26} installation : {h}", "");
                }
            }
            let n: usize = env.registry.engines().iter().map(|e| e.conversions().len()).sum();
            println!("\n{} conversions directes disponibles.", n);
            Ok(0)
        }
        Cmd::Detect { files } => {
            let files = expand_patterns(files)?;
            let mut out = Vec::new();
            let mut code = 0;
            for f in &files {
                match detect_path(f, catalog()) {
                    Ok(d) => {
                        if !cli.json {
                            let name = d.format.as_ref().map_or("inconnu".into(), fmt_name);
                            println!(
                                "{} : {} — confiance {}, méthode : {}",
                                f.display(),
                                name,
                                d.confidence.label_fr(),
                                d.method.label_fr()
                            );
                            if d.extension_mismatch {
                                println!("  ⚠ l'extension ne correspond pas au contenu");
                            }
                            if let Some(n) = &d.note {
                                println!("  {n}");
                            }
                        }
                        out.push(serde_json::json!({ "file": f, "detection": d }));
                    }
                    Err(e) => {
                        code = 1;
                        eprintln!("{} : {e}", f.display());
                        out.push(serde_json::json!({ "file": f, "error": e.to_string() }));
                    }
                }
            }
            if cli.json {
                print_json(&out)?;
            }
            Ok(code)
        }
        Cmd::Targets { file } => {
            let env = load_engines()?;
            let d = detect_path(&file, catalog())?;
            let src = d.format.clone().context("format non reconnu")?;
            let info = env.registry.probe(&std::path::absolute(&file)?, &src);
            let targets = env.registry.targets(&src, info.as_ref(), &OptionValues::new());
            if cli.json {
                return print_json(&serde_json::json!({ "source": src, "probe": info, "targets": targets })).map(|_| 0);
            }
            println!("{} : {}", file.display(), fmt_name(&src));
            if let Some(i) = &info {
                for (k, v) in &i.summary {
                    println!("  {k} : {v}");
                }
            }
            let mut current = None;
            for t in &targets {
                if current != Some(t.category) {
                    current = Some(t.category);
                    println!("\n{}", t.category.label_fr());
                }
                let via: Vec<String> = t.best.steps.iter().map(|s| s.engine.to_string()).collect();
                println!("  {:<12} {:<34} {:<18} via {}", t.format.as_str(), t.name, t.best.report.headline_fr(), via.join(" + "));
            }
            Ok(0)
        }
        Cmd::Plan { file, to, set, engine } => {
            let env = load_engines()?;
            let runner = JobRunner::new(env.registry.clone(), env.manager.paths().cache_dir.clone());
            let mut spec = JobSpec::new(file, resolve_target(&to)?);
            spec.options = parse_options(&set)?;
            spec.prefer_engine = engine.map(EngineId::new);
            let p = runner.prepare(&spec)?;
            if cli.json {
                return print_json(&p).map(|_| 0);
            }
            print_plan(&p.plan);
            for s in &p.plan.steps {
                if let Some(e) = env.registry.engine(&s.engine) {
                    let opts = e.options(&s.from, &s.to);
                    if !opts.is_empty() {
                        println!("Options de l'étape {} → {} ({}) :", s.from, s.to, s.engine);
                        for o in opts {
                            println!("  --set {}=…  {}{}", o.key, o.label, if o.advanced { " (avancé)" } else { "" });
                        }
                    }
                }
            }
            println!("Sortie prévue : {}", p.output.display());
            Ok(0)
        }
        Cmd::Convert { files, to, output, set, name, on_conflict, jobs, keep_date, engine, timeout } => {
            let files = expand_patterns(files)?;
            if files.is_empty() {
                bail!("aucun fichier à convertir");
            }
            let env = load_engines()?;
            let runner = JobRunner::new(env.registry.clone(), env.manager.paths().cache_dir.clone());
            runner.cleanup_stale(Duration::from_secs(24 * 3600));
            let target = resolve_target(&to)?;
            let options = parse_options(&set)?;
            let specs: Vec<JobSpec> = files
                .iter()
                .map(|f| {
                    let mut s = JobSpec::new(f.clone(), target.clone());
                    s.output_dir = output.clone();
                    s.options = options.clone();
                    s.name_template = name.clone();
                    s.conflict = match on_conflict {
                        ConflictArg::Rename => Conflict::Rename,
                        ConflictArg::Overwrite => Conflict::Overwrite,
                        ConflictArg::Skip => Conflict::Skip,
                    };
                    s.preserve_mtime = keep_date;
                    s.prefer_engine = engine.clone().map(EngineId::new);
                    s.timeout = timeout.map(Duration::from_secs);
                    s
                })
                .collect();
            let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
            let workers = jobs.unwrap_or(cores.min(4)).max(1);
            let cancel = CancelToken::new();

            let multi = MultiProgress::new();
            let style = ProgressStyle::with_template("{prefix:30!} [{bar:30}] {percent:>3}% {msg}").expect("modèle").progress_chars("=> ");
            let bars: Vec<ProgressBar> = if cli.json {
                Vec::new()
            } else {
                specs
                    .iter()
                    .map(|s| {
                        let b = multi.add(ProgressBar::new(1000));
                        b.set_style(style.clone());
                        b.set_prefix(s.input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
                        b.set_message("en attente");
                        b
                    })
                    .collect()
            };
            let stdout = Mutex::new(());
            let json = cli.json;
            let results = run_batch(&runner, &specs, workers, cores as u32, &cancel, &|idx, ev| {
                if json {
                    let _g = stdout.lock();
                    println!("{}", serde_json::json!({ "job": idx, "data": ev }));
                    return;
                }
                let b = &bars[idx];
                match ev {
                    JobEvent::Step { index, count, engine, from, to, .. } => {
                        b.set_message(format!("étape {}/{count} : {from} → {to} ({engine})", index + 1));
                    }
                    JobEvent::Progress { fraction, speed, .. } => {
                        if let Some(f) = fraction {
                            b.set_position((f * 1000.0) as u64);
                        }
                        if let Some(s) = speed {
                            b.set_message(format!("vitesse {s}"));
                        }
                    }
                    JobEvent::Verified { .. } => b.set_message("vérifié"),
                    _ => {}
                }
            });
            let mut failed = 0;
            for (i, r) in results.iter().enumerate() {
                if let Some(b) = bars.get(i) {
                    b.finish_and_clear();
                }
                let input = specs[i].input.display();
                match r {
                    Ok(o) => {
                        if json {
                            continue;
                        }
                        match &o.output {
                            Some(out) => println!("✔ {input} → {}", out.display()),
                            None => println!("↷ {input} : ignoré"),
                        }
                        if let Some(p) = &o.plan {
                            println!("    Fidélité : {}", p.report.headline_fr());
                            for d in p.report.details_fr(catalog()) {
                                println!("    • {d}");
                            }
                        }
                        for v in &o.verifications {
                            let t = match v {
                                Verification::Decoded(d) => format!("vérifié : {d}"),
                                Verification::Structural(d) => format!("contrôlé : {d}"),
                                Verification::SignatureOnly => "signature contrôlée seulement".into(),
                                Verification::Failed(d) => format!("ÉCHEC : {d}"),
                            };
                            println!("    ✓ {t}");
                        }
                        let shown: Vec<&String> = o.plan.iter().flat_map(|p| p.steps.iter().flat_map(|s| s.notes.iter())).collect();
                        for n in o.notes.iter().filter(|n| !shown.contains(n)) {
                            println!("    · {n}");
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        if json {
                            println!("{}", serde_json::json!({ "job": i, "error": { "code": e.code(), "message": e.to_string() } }));
                        } else {
                            println!("✘ {input} : {e}");
                        }
                    }
                }
            }
            if !json && specs.len() > 1 {
                println!("\n{} réussie(s), {failed} échec(s).", specs.len() - failed);
            }
            Ok(if failed > 0 { 1 } else { 0 })
        }
        Cmd::Matrix { from } => {
            let env = load_engines()?;
            let mut rows = Vec::new();
            for e in env.registry.engines() {
                for c in e.conversions() {
                    if from.as_ref().is_some_and(|f| c.from.as_str() != f) {
                        continue;
                    }
                    rows.push(serde_json::json!({ "engine": e.id(), "from": c.from, "to": c.to, "fidelity": c.fidelity, "cost": c.cost }));
                }
            }
            if cli.json {
                return print_json(&rows).map(|_| 0);
            }
            let mut by_engine: std::collections::BTreeMap<String, std::collections::BTreeMap<String, Vec<String>>> = Default::default();
            for r in &rows {
                by_engine
                    .entry(r["engine"].as_str().unwrap_or("").to_owned())
                    .or_default()
                    .entry(r["from"].as_str().unwrap_or("").to_owned())
                    .or_default()
                    .push(r["to"].as_str().unwrap_or("").to_owned());
            }
            for (engine, m) in by_engine {
                println!("{engine} ({} conversions)", m.values().map(Vec::len).sum::<usize>());
                for (f, tos) in m {
                    println!("  {f:<10} → {}", tos.join(", "));
                }
            }
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*.mov", "Vacances été.MOV") == cfg!(windows));
        assert!(wildcard_match("*.mov", "a.mov"));
        assert!(wildcard_match("img_??.png", "img_01.png"));
        assert!(!wildcard_match("img_??.png", "img_1.png"));
        assert!(wildcard_match("*", "-début.txt"));
        assert!(!wildcard_match("*.mov", "a.mov.txt"));
        assert!(wildcard_match("a*b*c", "aXXbYYc"));
    }

    #[test]
    fn expansion_never_reinterprets_existing_paths() {
        let dir = std::env::temp_dir().join(format!("chud-cli-glob-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["un.mov", "deux.mov", "trois.txt", "-quatre é.mov"] {
            std::fs::write(dir.join(n), b"x").unwrap();
        }
        let got = expand_patterns(vec![dir.join("*.mov")]).unwrap();
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|p| p.is_absolute() && p.extension().unwrap() == "mov"));
        // Linux : un fichier dont le nom contient réellement « * » est pris tel quel.
        #[cfg(unix)]
        {
            std::fs::write(dir.join("étrange*.mov"), b"x").unwrap();
            assert_eq!(expand_patterns(vec![dir.join("étrange*.mov")]).unwrap(), vec![dir.join("étrange*.mov")]);
        }
        assert!(expand_patterns(vec![dir.join("*.mkv")]).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
