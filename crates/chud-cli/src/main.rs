//! `chud` : interface en ligne de commande de Chud Converter.

use anyhow::{Context, Result, bail};
use chud_core::detect::detect_path;
use chud_core::{CancelToken, Category, EngineId, FormatCatalog, FormatId, OptionValues, Plan, Registry, Verification};
use chud_core::{LangChoice, set_lang};
use chud_core::{tr, trf};
use chud_enginemgr::{AppPaths, EngineManager, EngineState, EngineStatus, Settings};
use chud_jobs::{Conflict, JobEvent, JobRunner, JobSpec, PresetStore, run_batch};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `println!` qui ne panique pas quand le lecteur ferme la sortie (`chud … | head`) : le
/// programme s'arrête alors sans message, comme les outils Unix (code 141, « SIGPIPE »).
macro_rules! out {
    ($($t:tt)*) => {{
        use std::io::Write as _;
        if let Err(e) = writeln!(std::io::stdout().lock(), $($t)*) {
            std::process::exit(if e.kind() == std::io::ErrorKind::BrokenPipe { 141 } else { 1 });
        }
    }};
}

#[derive(Parser)]
#[command(name = "chud", version, about = "Chud Converter : conversion de fichiers locale, honnête et vérifiée")]
struct Cli {
    /// Sortie JSON (machine).
    #[arg(long, global = true)]
    json: bool,
    /// Langue des messages : system, fr ou en (par défaut : celle choisie dans l'application).
    #[arg(long, global = true, value_name = "LANGUE", value_parser = ["system", "fr", "en"])]
    lang: Option<String>,
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
    /// Détecte les moteurs installés (FFmpeg, 7-Zip, LibreOffice…), ou installe ceux qui manquent.
    Engines {
        #[command(subcommand)]
        action: Option<EngineAction>,
    },
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
    /// Convertit un ou plusieurs fichiers (vers un format, ou selon un préréglage).
    Convert {
        files: Vec<PathBuf>,
        #[arg(long = "to", short = 't', required_unless_present = "preset", conflicts_with = "preset")]
        to: Option<String>,
        /// Préréglage (voir `chud presets`) : format, options et choix de sortie enregistrés.
        #[arg(long, short = 'p')]
        preset: Option<String>,
        /// Dossier de sortie (par défaut : celui de chaque source).
        #[arg(long, short = 'o')]
        output: Option<PathBuf>,
        #[arg(long = "set", short = 's')]
        set: Vec<String>,
        /// Modèle du nom de sortie : {stem}, {target} (par défaut {stem}).
        #[arg(long)]
        name: Option<String>,
        /// Fichier de sortie déjà présent : rename (par défaut), skip ou overwrite.
        #[arg(long = "on-conflict", value_enum)]
        on_conflict: Option<ConflictArg>,
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
    /// Préréglages : tous, ou ceux proposés pour des fichiers donnés (comme le menu clic droit).
    Presets {
        files: Vec<PathBuf>,
        /// Écrit les préréglages par défaut dans le fichier de préréglages s'il n'existe pas encore
        /// (pour les modifier à la main). Un fichier existant n'est jamais remplacé.
        #[arg(long)]
        write_defaults: bool,
    },
    /// Menu clic droit de l'explorateur de fichiers (Windows : registre de l'utilisateur ;
    /// Linux : Dolphin, Nemo, Nautilus). Aucun droit d'administrateur n'est nécessaire.
    Shell {
        #[command(subcommand)]
        action: ShellAction,
    },
    /// Affiche ou change la langue, partagée avec l'application (system, fr, en).
    Language {
        #[arg(value_parser = ["system", "fr", "en"])]
        value: Option<String>,
    },
    /// Matrice des conversions disponibles par moteur.
    Matrix {
        /// Limite à un format source.
        #[arg(long)]
        from: Option<String>,
    },
}

#[derive(Subcommand)]
enum EngineAction {
    /// Télécharge et installe un moteur (FFmpeg, 7-Zip) dans le dossier de Chud Converter :
    /// source officielle, taille et empreinte SHA-256 vérifiées, essai avant activation.
    /// Une installation du système n'est jamais modifiée.
    Install { id: String },
    /// Retire un moteur installé par Chud Converter (jamais une installation du système).
    Uninstall {
        id: String,
        /// Confirme la suppression.
        #[arg(long)]
        yes: bool,
    },
    /// Revient à la version précédente d'un moteur installé par Chud Converter.
    Rollback { id: String },
}

#[derive(Subcommand)]
enum ShellAction {
    /// Ajoute (ou met à jour) le menu « Chud Converter » d'après les préréglages actifs.
    Install {
        /// Programme à lancer (par défaut : chud-converter à côté de chud).
        #[arg(long)]
        exe: Option<PathBuf>,
        /// Affiche ce qui serait installé, sans rien modifier.
        #[arg(long)]
        dry_run: bool,
    },
    /// Retire le menu : uniquement les éléments créés par Chud Converter.
    Uninstall,
    /// Indique si le menu est installé et quel programme il lance.
    Status,
}

#[derive(Clone, Copy, ValueEnum)]
enum ConflictArg {
    Rename,
    Overwrite,
    Skip,
}

fn main() {
    init_language();
    let cli = Cli::from_arg_matches(&localized_command().get_matches()).unwrap_or_else(|e| e.exit());
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("{}", trf!("erreur : {e:#}", "error: {e:#}"));
            std::process::exit(2);
        }
    }
}

/// Langue : `--lang` d'abord (lu avant l'analyse, pour que l'aide soit déjà dans la bonne
/// langue), sinon le réglage de l'application, sinon la langue du système.
fn init_language() {
    let mut args = std::env::args_os().skip(1);
    let mut flag = None;
    while let Some(a) = args.next() {
        let a = a.to_string_lossy().into_owned();
        if a == "--" {
            break;
        }
        if let Some(v) = a.strip_prefix("--lang=") {
            flag = Some(v.to_owned());
        } else if a == "--lang" {
            flag = args.next().map(|v| v.to_string_lossy().into_owned());
        }
    }
    let choice = flag.as_deref().and_then(LangChoice::parse).unwrap_or_else(|| {
        // Réglages illisibles : la langue du système ; l'erreur sera signalée par la commande qui
        // en a besoin.
        Settings::load(&AppPaths::detect().settings_file()).map(|s| s.language).unwrap_or_default()
    });
    set_lang(choice.resolve());
}

/// Aide de la ligne de commande : écrite en français dans le code, traduite ici pour l'anglais.
fn localized_command() -> clap::Command {
    let cmd = Cli::command();
    if chud_core::lang() == chud_core::Lang::Fr {
        return cmd;
    }
    let help = |text: &'static str| move |a: clap::Arg| a.help(text);
    cmd.about("Chud Converter: local, honest and verified file conversion")
        .mut_arg("json", help("JSON output (for machines)."))
        .mut_arg("lang", |a| a.help("Message language: system, fr or en (default: the one chosen in the app).").value_name("LANGUAGE"))
        .mut_subcommand("formats", |c| {
            c.about("Lists the known formats.")
                .mut_arg("category", help("Filter by category (image, audio, video, document, archive…)."))
                .mut_arg("query", help("Search in names and extensions."))
        })
        .mut_subcommand("engines", |c| {
            c.about("Detects the installed engines (FFmpeg, 7-Zip, LibreOffice…), or installs the missing ones.")
                .mut_subcommand("install", |c| {
                    c.about("Downloads and installs an engine (FFmpeg, 7-Zip) into Chud Converter's folder: official source, size and SHA-256 checked, tried before being activated. A system install is never changed.")
                })
                .mut_subcommand("uninstall", |c| {
                    c.about("Removes an engine installed by Chud Converter (never a system install).").mut_arg("yes", help("Confirms the removal."))
                })
                .mut_subcommand("rollback", |c| c.about("Goes back to the previous version of an engine installed by Chud Converter."))
        })
        .mut_subcommand("detect", |c| c.about("Detects the real format of files."))
        .mut_subcommand("targets", |c| c.about("Possible target formats for a file, with the expected fidelity."))
        .mut_subcommand("plan", |c| {
            c.about("Shows the conversion plan and the fidelity report, without writing anything.")
                .mut_arg("set", help("Conversion option “key=value” (repeatable)."))
        })
        .mut_subcommand("convert", |c| {
            c.about("Converts one or more files (to a format, or with a preset).")
                .mut_arg("preset", help("Preset (see `chud presets`): saved format, options and output choices."))
                .mut_arg("output", help("Output folder (default: the folder of each source)."))
                .mut_arg("set", help("Conversion option “key=value” (repeatable)."))
                .mut_arg("name", help("Output name template: {stem}, {target} (default {stem})."))
                .mut_arg("on_conflict", help("Output file already there: rename (default), skip or overwrite."))
                .mut_arg("jobs", help("Number of simultaneous conversions."))
                .mut_arg("keep_date", help("Copies the source's modification date."))
                .mut_arg("engine", help("Engine to prefer (ffmpeg, image, 7zip…)."))
                .mut_arg("timeout", help("Time limit per step, in seconds."))
        })
        .mut_subcommand("presets", |c| {
            c.about("Presets: all of them, or those offered for given files (like the right-click menu).").mut_arg(
                "write_defaults",
                help("Writes the default presets to the presets file if it does not exist yet (to edit them by hand). An existing file is never replaced."),
            )
        })
        .mut_subcommand("shell", |c| {
            c.about("File manager right-click menu (Windows: the user's registry; Linux: Dolphin, Nemo, Nautilus). No administrator rights needed.")
                .mut_subcommand("install", |c| {
                    c.about("Adds (or updates) the “Chud Converter” menu from the active presets.")
                        .mut_arg("exe", help("Program to launch (default: chud-converter next to chud)."))
                        .mut_arg("dry_run", help("Shows what would be installed, without changing anything."))
                })
                .mut_subcommand("uninstall", |c| c.about("Removes the menu: only the items created by Chud Converter."))
                .mut_subcommand("status", |c| c.about("Tells whether the menu is installed and which program it launches."))
        })
        .mut_subcommand("language", |c| c.about("Shows or changes the language, shared with the app (system, fr, en)."))
        .mut_subcommand("matrix", |c| c.about("Matrix of the conversions available per engine.").mut_arg("from", help("Limit to one source format.")))
}

/// `chud engines install|uninstall|rollback`.
fn engine_action(action: EngineAction, json: bool) -> Result<i32> {
    use std::sync::atomic::AtomicBool;
    let manager =
        EngineManager::new(AppPaths::detect()).context(tr!("chargement des réglages des moteurs", "loading the engine settings"))?;
    let id = match &action {
        EngineAction::Install { id } | EngineAction::Uninstall { id, .. } | EngineAction::Rollback { id } => id.clone(),
    };
    let Some(m) = manager.manifests().iter().find(|m| m.id == id).cloned() else {
        let known: Vec<&str> = manager.manifests().iter().map(|m| m.id.as_str()).collect();
        bail!("{}", trf!("moteur inconnu « {} » (connus : {})", "unknown engine “{}” (known: {})", id, known.join(", ")));
    };
    let status = manager.discover_one(&m);
    match action {
        EngineAction::Install { .. } => {
            if status.state == EngineState::Ready && status.origin != Some(chud_core::EngineOrigin::Managed) {
                out!(
                    "{}",
                    trf!(
                        "{} est déjà prêt ({}) : rien à installer. Chud Converter ne remplace pas une installation existante.",
                        "{} is already ready ({}): nothing to install. Chud Converter does not replace an existing install.",
                        m.name,
                        status.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default()
                    )
                );
                return Ok(0);
            }
            let Some(src) = m.install_source() else {
                bail!(
                    "{}",
                    trf!(
                        "{} ne peut pas être installé automatiquement sur cette plateforme.{}",
                        "{} cannot be installed automatically on this platform.{}",
                        m.name,
                        status.hint.as_ref().map(|h| trf!(" Commande suggérée : {h}", " Suggested command: {h}")).unwrap_or_default()
                    )
                );
            };
            if status.origin == Some(chud_core::EngineOrigin::Managed) && status.version.as_deref() == Some(src.version.as_str()) {
                out!("{}", trf!("{} {} est déjà installé.", "{} {} is already installed.", m.name, src.version));
                return Ok(0);
            }
            if !json {
                out!(
                    "{}",
                    trf!(
                        "Installation de {} {} — {} ({:.0} Mo depuis {})",
                        "Installing {} {} — {} ({:.0} MB from {})",
                        m.name,
                        src.version,
                        src.publisher_l(),
                        src.total_size() as f64 / 1e6,
                        src.page
                    )
                );
            }
            let cancel = AtomicBool::new(false);
            let mut last = String::new();
            let mut on_progress = |p: chud_enginemgr::InstallProgress| {
                if json {
                    return;
                }
                let line = match p {
                    chud_enginemgr::InstallProgress::Downloading { received, total } => {
                        trf!("téléchargement… {} %", "downloading… {} %", received * 100 / total.max(1))
                    }
                    chud_enginemgr::InstallProgress::Verifying => {
                        tr!("vérification de l'empreinte SHA-256…", "checking the SHA-256…").into()
                    }
                    chud_enginemgr::InstallProgress::Extracting => tr!("extraction…", "extracting…").into(),
                    chud_enginemgr::InstallProgress::Checking => tr!("essai du moteur…", "trying the engine…").into(),
                };
                if line != last {
                    eprintln!("  {line}");
                    last = line;
                }
            };
            let done =
                chud_enginemgr::install::install(manager.paths(), &m, &cancel, &mut on_progress).map_err(|e| anyhow::anyhow!("{e}"))?;
            if json {
                print_json(&done)?;
            } else {
                out!("{}", trf!("{} {} installé : {}", "{} {} installed: {}", m.name, done.version, done.path.display()));
            }
            Ok(0)
        }
        EngineAction::Uninstall { yes, .. } => {
            let dir = manager.paths().engines_dir().join(&m.id);
            if !yes {
                out!(
                    "{}",
                    trf!(
                        "Ceci supprimera {} (installé par Chud Converter) : {}
Relancez avec --yes pour confirmer. Les installations du système ne sont jamais touchées.",
                        "This will delete {} (installed by Chud Converter): {}
Run again with --yes to confirm. System installs are never touched.",
                        m.name,
                        dir.display()
                    )
                );
                return Ok(2);
            }
            chud_enginemgr::install::uninstall(manager.paths(), &m).map_err(|e| anyhow::anyhow!("{e}"))?;
            out!("{}", trf!("{} retiré.", "{} removed.", m.name));
            Ok(0)
        }
        EngineAction::Rollback { .. } => {
            let v = chud_enginemgr::install::rollback(manager.paths(), &m).map_err(|e| anyhow::anyhow!("{e}"))?;
            out!("{}", trf!("{} revenu à la version {}.", "{} is back to version {}.", m.name, v));
            Ok(0)
        }
    }
}

fn catalog() -> &'static FormatCatalog {
    FormatCatalog::builtin()
}

fn print_json<T: serde::Serialize>(v: &T) -> Result<()> {
    out!("{}", serde_json::to_string_pretty(v)?);
    Ok(())
}

struct Env {
    manager: EngineManager,
    statuses: Vec<EngineStatus>,
    registry: Arc<Registry>,
}

fn load_engines() -> Result<Env> {
    let manager =
        EngineManager::new(AppPaths::detect()).context(tr!("chargement des réglages des moteurs", "loading the engine settings"))?;
    let mut statuses = manager.discover();
    let registry = Arc::new(manager.build_registry(&mut statuses));
    Ok(Env { manager, statuses, registry })
}

fn parse_options(set: &[String]) -> Result<OptionValues> {
    let mut o = OptionValues::new();
    for s in set {
        o.parse_assignment(s).map_err(|e| anyhow::anyhow!("{}", trf!("--set {s} : {e}", "--set {s}: {e}")))?;
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
        bail!("{}", trf!("format cible inconnu : « {s} » (voir `chud formats`)", "unknown target format: “{s}” (see `chud formats`)"));
    }
    bail!(
        "{}",
        trf!(
            "format cible inconnu : « {s} ». Vouliez-vous dire : {} ?",
            "unknown target format: “{s}”. Did you mean: {}?",
            suggestions.join(", ")
        )
    )
}

fn fmt_name(id: &FormatId) -> String {
    catalog().get(id.as_str()).map_or_else(|| id.to_string(), |d| format!("{} ({})", d.label(), id))
}

fn print_plan(plan: &Plan) {
    out!("{}", trf!("Plan : {} → {}", "Plan: {} → {}", fmt_name(&plan.source), fmt_name(&plan.target)));
    for (i, s) in plan.steps.iter().enumerate() {
        out!("{}", trf!("  {}. {} → {} avec {} — {}", "  {}. {} → {} with {} — {}", i + 1, s.from, s.to, s.engine, s.fidelity.label()));
    }
    out!("{}", trf!("Fidélité : {}", "Fidelity: {}", plan.report.headline()));
    for d in plan.report.details(catalog()) {
        out!("  • {d}");
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
            .with_context(|| trf!("dossier illisible : {}", "unreadable folder: {}", parent.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| wildcard_match(&name, &e.file_name().to_string_lossy()))
            .map(|e| if parent == *"." { PathBuf::from(e.file_name()) } else { e.path() })
            .collect();
        if found.is_empty() {
            bail!("{}", trf!("aucun fichier ne correspond à « {} »", "no file matches “{}”", a.display()));
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
                Some(c) => Some(Category::parse(c).with_context(|| trf!("catégorie inconnue : {c}", "unknown category: {c}"))?),
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
                    out!("\n{}", d.category.label());
                }
                out!("  {:<12} {:<40} .{}", d.id.as_str(), d.label(), d.extensions.join(" ."));
            }
            Ok(0)
        }
        Cmd::Engines { action: Some(action) } => engine_action(action, cli.json),
        Cmd::Engines { action: None } => {
            let env = load_engines()?;
            if cli.json {
                return print_json(&env.statuses).map(|_| 0);
            }
            out!(
                "{}",
                trf!(
                    "Données : {}{}",
                    "Data: {}{}",
                    env.manager.paths().data_dir.display(),
                    if env.manager.paths().portable { " (portable)" } else { "" }
                )
            );
            out!("{}", trf!("Moteurs intégrés : images (natif), données (natif)\n", "Built-in engines: images (native), data (native)\n"));
            for s in &env.statuses {
                let state = match &s.state {
                    EngineState::Ready => tr!("prêt", "ready").to_string(),
                    EngineState::NoAdapter => tr!("détecté (intégration à venir)", "detected (integration to come)").to_string(),
                    EngineState::Broken(e) => trf!("en panne : {e}", "broken: {e}"),
                    EngineState::Missing => tr!("absent", "missing").to_string(),
                    EngineState::Disabled => tr!("désactivé", "disabled").to_string(),
                };
                let origin = match s.origin {
                    Some(chud_core::EngineOrigin::Custom) => tr!(" [chemin personnalisé]", " [custom path]"),
                    Some(chud_core::EngineOrigin::Managed) => tr!(" [géré]", " [managed]"),
                    Some(chud_core::EngineOrigin::System) => tr!(" [système]", " [system]"),
                    _ => "",
                };
                out!("{:<26} {}{}", s.name, state, s.version.as_ref().map(|v| format!(" — {v}")).unwrap_or_default());
                if let Some(p) = &s.path {
                    out!("{:<26} {}{origin}", "", p.display());
                }
                if matches!(s.state, EngineState::Missing | EngineState::Broken(_)) {
                    if let Some(offer) = &s.installable {
                        out!(
                            "{}",
                            trf!(
                                "{:<26} installation : chud engines install {} ({} {}, {:.0} Mo)",
                                "{:<26} install: chud engines install {} ({} {}, {:.0} MB)",
                                "",
                                s.id,
                                s.name,
                                offer.version,
                                offer.size as f64 / 1e6
                            )
                        );
                    } else if let Some(h) = &s.hint {
                        out!("{}", trf!("{:<26} installation : {h}", "{:<26} install: {h}", ""));
                    }
                }
            }
            let n: usize = env.registry.engines().iter().map(|e| e.conversions().len()).sum();
            out!("{}", trf!("\n{} conversions directes disponibles.", "\n{} direct conversions available.", n));
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
                            let name = d.format.as_ref().map_or(tr!("inconnu", "unknown").into(), fmt_name);
                            out!(
                                "{}",
                                trf!(
                                    "{} : {} — confiance {}, méthode : {}",
                                    "{}: {} — confidence {}, method: {}",
                                    f.display(),
                                    name,
                                    d.confidence.label(),
                                    d.method.label()
                                )
                            );
                            if d.extension_mismatch {
                                out!(
                                    "{}",
                                    trf!("  ⚠ l'extension ne correspond pas au contenu", "  ⚠ the extension does not match the content")
                                );
                            }
                            if let Some(n) = &d.note {
                                out!("  {n}");
                            }
                        }
                        out.push(serde_json::json!({ "file": f, "detection": d }));
                    }
                    Err(e) => {
                        code = 1;
                        eprintln!("{}", trf!("{} : {e}", "{}: {e}", f.display()));
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
            let src = d.format.clone().context(tr!("format non reconnu", "unrecognized format"))?;
            let info = env.registry.probe_checked(&std::path::absolute(&file)?, &src).map_err(anyhow::Error::msg)?;
            let targets = env.registry.targets(&src, info.as_ref(), &OptionValues::new());
            if cli.json {
                return print_json(&serde_json::json!({ "source": src, "probe": info, "targets": targets })).map(|_| 0);
            }
            out!("{}", trf!("{} : {}", "{}: {}", file.display(), fmt_name(&src)));
            if let Some(i) = &info {
                for (k, v) in &i.summary {
                    out!("{}", trf!("  {k} : {v}", "  {k}: {v}"));
                }
            }
            let mut current = None;
            for t in &targets {
                if current != Some(t.category) {
                    current = Some(t.category);
                    out!("\n{}", t.category.label());
                }
                let via: Vec<String> = t.best.steps.iter().map(|s| s.engine.to_string()).collect();
                out!("  {:<12} {:<34} {:<18} via {}", t.format.as_str(), t.name, t.best.report.headline(), via.join(" + "));
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
                        out!("{}", trf!("Options de l'étape {} → {} ({}) :", "Options for step {} → {} ({}):", s.from, s.to, s.engine));
                        for o in opts {
                            out!("  --set {}=…  {}{}", o.key, o.label, if o.advanced { tr!(" (avancé)", " (advanced)") } else { "" });
                        }
                    }
                }
            }
            out!("{}", trf!("Sortie prévue : {}", "Planned output: {}", p.output.display()));
            Ok(0)
        }
        Cmd::Convert { files, to, preset, output, set, name, on_conflict, jobs, keep_date, engine, timeout } => {
            let files = expand_patterns(files)?;
            if files.is_empty() {
                bail!("{}", trf!("aucun fichier à convertir", "no file to convert"));
            }
            let env = load_engines()?;
            let runner = JobRunner::new(env.registry.clone(), env.manager.paths().cache_dir.clone());
            runner.cleanup_stale(Duration::from_secs(24 * 3600));
            let preset = match &preset {
                Some(id) => {
                    let store = PresetStore::load(&env.manager.paths().presets_file(), catalog())?;
                    Some(store.get(id)?.clone())
                }
                None => None,
            };
            let target = match (&to, &preset) {
                (Some(to), _) => resolve_target(to)?,
                (None, Some(p)) => p.target.clone(),
                (None, None) => bail!("{}", trf!("indiquez --to ou --preset", "give --to or --preset")),
            };
            let extra = parse_options(&set)?;
            let specs: Vec<JobSpec> = files
                .iter()
                .map(|f| {
                    // Le préréglage fournit la base ; les options de la ligne de commande priment.
                    let mut s = match &preset {
                        Some(p) => p.job_spec(f.clone()),
                        None => JobSpec::new(f.clone(), target.clone()),
                    };
                    for k in extra.keys() {
                        if let Some(v) = extra.get(k) {
                            s.options.set(k, v.clone());
                        }
                    }
                    if output.is_some() {
                        s.output_dir = output.clone();
                    }
                    if let Some(n) = &name {
                        s.name_template = n.clone();
                    }
                    if let Some(c) = on_conflict {
                        s.conflict = match c {
                            ConflictArg::Rename => Conflict::Rename,
                            ConflictArg::Overwrite => Conflict::Overwrite,
                            ConflictArg::Skip => Conflict::Skip,
                        };
                    }
                    s.preserve_mtime |= keep_date;
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
                        b.set_message(tr!("en attente", "waiting"));
                        b
                    })
                    .collect()
            };
            let stdout = Mutex::new(());
            let json = cli.json;
            let results = run_batch(&runner, &specs, workers, cores as u32, &cancel, &|idx, ev| {
                if json {
                    let _g = stdout.lock();
                    out!("{}", serde_json::json!({ "job": idx, "data": ev }));
                    return;
                }
                let b = &bars[idx];
                match ev {
                    JobEvent::Step { index, count, engine, from, to, .. } => {
                        b.set_message(trf!(
                            "étape {}/{count} : {from} → {to} ({engine})",
                            "step {}/{count}: {from} → {to} ({engine})",
                            index + 1
                        ));
                    }
                    JobEvent::Progress { fraction, speed, .. } => {
                        if let Some(f) = fraction {
                            b.set_position((f * 1000.0) as u64);
                        }
                        if let Some(s) = speed {
                            b.set_message(trf!("vitesse {s}", "speed {s}"));
                        }
                    }
                    JobEvent::Verified { .. } => b.set_message(tr!("vérifié", "verified")),
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
                            Some(out) => out!("✔ {input} → {}", out.display()),
                            None => out!("{}", trf!("↷ {input} : ignoré", "↷ {input}: skipped")),
                        }
                        if let Some(p) = &o.plan {
                            out!("{}", trf!("    Fidélité : {}", "    Fidelity: {}", p.report.headline()));
                            for d in p.report.details(catalog()) {
                                out!("    • {d}");
                            }
                        }
                        for v in &o.verifications {
                            let t = match v {
                                Verification::Decoded(d) => trf!("vérifié : {d}", "verified: {d}"),
                                Verification::Structural(d) => trf!("contrôlé : {d}", "checked: {d}"),
                                Verification::SignatureOnly => tr!("signature contrôlée seulement", "signature checked only").into(),
                                Verification::Failed(d) => trf!("ÉCHEC : {d}", "FAILED: {d}"),
                            };
                            out!("    ✓ {t}");
                        }
                        let shown: Vec<&String> = o.plan.iter().flat_map(|p| p.steps.iter().flat_map(|s| s.notes.iter())).collect();
                        for n in o.notes.iter().filter(|n| !shown.contains(n)) {
                            out!("    · {n}");
                        }
                    }
                    Err(e) => {
                        failed += 1;
                        if json {
                            out!("{}", serde_json::json!({ "job": i, "error": { "code": e.code(), "message": e.to_string() } }));
                        } else {
                            out!("{}", trf!("✘ {input} : {e}", "✘ {input}: {e}"));
                        }
                    }
                }
            }
            if !json && specs.len() > 1 {
                out!("{}", trf!("\n{} réussie(s), {failed} échec(s).", "\n{} succeeded, {failed} failed.", specs.len() - failed));
            }
            Ok(if failed > 0 { 1 } else { 0 })
        }
        Cmd::Presets { files, write_defaults } => {
            let paths = AppPaths::detect();
            let file = paths.presets_file();
            if write_defaults {
                if file.exists() {
                    bail!(
                        "{}",
                        trf!("{} existe déjà : il n'est jamais remplacé", "{} already exists: it is never replaced", file.display())
                    );
                }
                PresetStore::builtin().save(&file, catalog())?;
                eprintln!("{}", trf!("Préréglages par défaut écrits dans {}", "Default presets written to {}", file.display()));
            }
            let store = PresetStore::load(&file, catalog())?;
            let files = expand_patterns(files)?;
            let shown: Vec<&chud_jobs::Preset> =
                if files.is_empty() { store.presets.iter().collect() } else { store.for_paths(&files, catalog()) };
            if cli.json {
                print_json(&serde_json::json!({ "file": file, "exists": file.exists(), "presets": shown }))?;
                return Ok(0);
            }
            let origin = if file.exists() {
                tr!("fichier", "file")
            } else {
                tr!("valeurs par défaut, fichier non créé", "defaults, file not created")
            };
            out!("{}", trf!("Préréglages ({origin}) : {}", "Presets ({origin}): {}", file.display()));
            if !files.is_empty() && shown.is_empty() {
                out!(
                    "{}",
                    trf!(
                        "Aucun préréglage pour cette sélection (type inconnu ou sans préréglage commun).",
                        "No preset for this selection (unknown type or no common preset)."
                    )
                );
            }
            for p in shown {
                let name = catalog().get(p.target.as_str()).map(|f| f.label()).unwrap_or(p.target.as_str());
                let state = if p.enabled { "" } else { tr!("  (désactivé)", "  (disabled)") };
                out!("  {:<14} {:<26} → {:<26} [{}]{state}", p.id, p.display_name(), name, p.applies_to.join(", "));
                if !p.options.is_empty() {
                    let opts: Vec<String> = p.options.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    out!("{}", trf!("  {:<14} options : {}", "  {:<14} options: {}", "", opts.join(", ")));
                }
            }
            Ok(0)
        }
        Cmd::Shell { action } => shell(action, cli.json),
        Cmd::Language { value } => {
            let paths = AppPaths::detect();
            let file = paths.settings_file();
            let mut settings = Settings::load(&file)?;
            if let Some(v) = value {
                settings.language = LangChoice::parse(&v).context("system, fr, en")?;
                settings.save(&file)?;
                // Les menus clic droit installés suivent la langue.
                settings.apply_language();
                if let Ok(env) = load_engines() {
                    let _ = refresh_menu(&env);
                }
            }
            let effective = settings.language.resolve();
            if cli.json {
                return print_json(&serde_json::json!({ "setting": settings.language, "effective": effective, "file": file })).map(|_| 0);
            }
            let setting = match settings.language {
                LangChoice::System => tr!("langue du système", "system language"),
                LangChoice::Fr => "français",
                LangChoice::En => "English",
            };
            out!("{}", trf!("Langue : {setting} ({}) — {}", "Language: {setting} ({}) — {}", effective.code(), file.display()));
            Ok(0)
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
                out!("{engine} ({} conversions)", m.values().map(Vec::len).sum::<usize>());
                for (f, tos) in m {
                    out!("  {f:<10} → {}", tos.join(", "));
                }
            }
            Ok(0)
        }
    }
}

/// Contenu du menu : seules les conversions réalisables avec les moteurs présents y entrent.
fn plan_menus(env: &Env) -> Result<Vec<chud_shell::Menu>> {
    let store = PresetStore::load(&env.manager.paths().presets_file(), catalog())?;
    let mut cache: std::collections::HashMap<FormatId, Vec<FormatId>> = Default::default();
    Ok(chud_shell::plan_menus_with(&store, catalog(), |src, dst| {
        cache
            .entry(src.clone())
            .or_insert_with(|| env.registry.targets(src, None, &OptionValues::new()).into_iter().map(|t| t.format).collect())
            .contains(dst)
    }))
}

/// Réécrit le menu s'il est installé (ses libellés suivent la langue).
fn refresh_menu(env: &Env) -> Result<bool> {
    let st = chud_shell::status()?;
    match (&st.exe, st.installed) {
        (Some(exe), true) => Ok(chud_shell::refresh_if_installed(exe, &plan_menus(env)?)?),
        _ => Ok(false),
    }
}

fn shell(action: ShellAction, json: bool) -> Result<i32> {
    match action {
        ShellAction::Install { exe, dry_run } => {
            let exe = match exe {
                Some(e) => std::path::absolute(e)?,
                None => chud_shell::default_gui_exe()?,
            };
            let env = load_engines()?;
            let menus = plan_menus(&env)?;
            if dry_run {
                if json {
                    let v: Vec<_> = menus
                        .iter()
                        .map(|m| serde_json::json!({ "key": m.key, "folder": m.folder, "extensions": m.extensions,
                            "items": m.items.iter().map(|i| serde_json::json!({ "preset": i.preset, "name": i.name })).collect::<Vec<_>>() }))
                        .collect();
                    return print_json(&serde_json::json!({ "exe": exe, "menus": v })).map(|_| 0);
                }
                out!("{}", trf!("Programme lancé : {}", "Program launched: {}", exe.display()));
                for m in &menus {
                    let names: Vec<&str> = m.items.iter().map(|i| i.name.as_str()).collect();
                    let target = if m.folder {
                        tr!("dossiers", "folders").to_owned()
                    } else {
                        m.extensions.iter().map(|e| format!(".{e}")).collect::<Vec<_>>().join(" ")
                    };
                    out!("  {target}\n    → {}", names.join(", "));
                }
                out!("{}", trf!("(simulation : rien n'a été modifié)", "(dry run: nothing was changed)"));
                return Ok(0);
            }
            let st = chud_shell::install(&exe, &menus)?;
            if json {
                return print_status(&st, true);
            }
            out!(
                "{}",
                trf!(
                    "Menu clic droit installé : {} types de fichiers, programme {}",
                    "Right-click menu installed: {} file types, program {}",
                    st.entries,
                    exe.display()
                )
            );
            out!("{}", trf!("Emplacement : {}", "Location: {}", st.location));
            if cfg!(windows) {
                out!(
                    "{}",
                    trf!(
                        "Sous Windows 11, il apparaît dans « Afficher plus d'options » (ou Maj+F10).",
                        "On Windows 11 it is under “Show more options” (or Shift+F10)."
                    )
                );
            }
            out!("{}", trf!("Pour le retirer : chud shell uninstall", "To remove it: chud shell uninstall"));
            Ok(0)
        }
        ShellAction::Uninstall => {
            let n = chud_shell::uninstall()?;
            if json {
                return print_json(&serde_json::json!({ "removed": n })).map(|_| 0);
            }
            out!(
                "{}",
                if n == 0 {
                    tr!("Le menu n'était pas installé.", "The menu was not installed.").to_owned()
                } else {
                    trf!("Menu retiré ({n} éléments supprimés).", "Menu removed ({n} items deleted).")
                }
            );
            Ok(0)
        }
        ShellAction::Status => print_status(&chud_shell::status()?, json),
    }
}

fn print_status(st: &chud_shell::Status, json: bool) -> Result<i32> {
    if json {
        print_json(&serde_json::json!({ "installed": st.installed, "exe": st.exe, "exeMissing": st.exe_missing,
            "entries": st.entries, "location": st.location }))?;
        return Ok(0);
    }
    if !st.installed {
        out!("{}", trf!("Menu clic droit : non installé (chud shell install).", "Right-click menu: not installed (chud shell install)."));
        return Ok(0);
    }
    out!("{}", trf!("Menu clic droit : installé, {} types de fichiers.", "Right-click menu: installed, {} file types.", st.entries));
    if let Some(e) = &st.exe {
        out!(
            "{}",
            trf!(
                "Programme : {}{}",
                "Program: {}{}",
                e.display(),
                if st.exe_missing { tr!("  (INTROUVABLE : réinstallez le menu)", "  (NOT FOUND: reinstall the menu)") } else { "" }
            )
        );
    }
    out!("{}", trf!("Emplacement : {}", "Location: {}", st.location));
    Ok(if st.exe_missing { 1 } else { 0 })
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
