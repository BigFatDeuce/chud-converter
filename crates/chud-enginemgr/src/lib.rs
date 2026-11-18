//! Gestionnaire de moteurs : manifestes, détection, sonde de version, réglages et construction du
//! registre des moteurs actifs.
//!
//! Ordre de détection d'un moteur (le premier fonctionnel gagne) :
//! 1. chemin personnalisé (`engines.toml`) — origine `Custom` ;
//! 2. installation gérée par l'application (`<données>/engines/<id>/current`) — `Managed` ;
//! 3. emplacements connus (`%ProgramFiles%\7-Zip\7z.exe`…) — `System` ;
//! 4. `PATH` (chemins absolus seulement, alias d'applications du Store ignorés) — `System`.
//!
//! Le chemin absolu trouvé est conservé : l'exécution ne refait jamais de recherche.
//! Les installations système ne sont jamais modifiées. Le téléchargement et l'installation
//! gérés (index signé, ADR-007) arrivent dans l'itération suivante : voir `docs/ENGINE-MANAGER.md`.

use chud_core::{Engine, EngineId, EngineOrigin, FormatCatalog, Registry};
use chud_engines::ffmpeg::Ffmpeg;
use chud_engines::sevenzip::SevenZip;
use chud_process::ProcessSpec;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const BUILTIN_MANIFESTS: &[&str] = &[
    include_str!("../data/engines/ffmpeg.toml"),
    include_str!("../data/engines/7zip.toml"),
    include_str!("../data/engines/pandoc.toml"),
    include_str!("../data/engines/libreoffice.toml"),
    include_str!("../data/engines/calibre.toml"),
    include_str!("../data/engines/imagemagick.toml"),
    include_str!("../data/engines/libvips.toml"),
    include_str!("../data/engines/qpdf.toml"),
    include_str!("../data/engines/ghostscript.toml"),
    include_str!("../data/engines/tesseract.toml"),
    include_str!("../data/engines/exiftool.toml"),
    include_str!("../data/engines/mediainfo.toml"),
    include_str!("../data/engines/mkvtoolnix.toml"),
];

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("{}", chud_core::trf!("manifeste « {} » invalide : {}", "invalid manifest “{}”: {}", .0, .1))]
    Manifest(String, String),
    #[error("{}", chud_core::trf!("réglages invalides ({}) : {}", "invalid settings ({}): {}", .0.display(), .1))]
    Settings(PathBuf, String),
    #[error("{}", chud_core::trf!("erreur d'entrée/sortie : {}", "input/output error: {}", .0))]
    Io(#[from] std::io::Error),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Detect {
    #[serde(default)]
    pub version_args: Vec<String>,
    pub version_regex: String,
    #[serde(default)]
    pub known_paths_windows: Vec<String>,
    #[serde(default)]
    pub known_paths_linux: Vec<String>,
    #[serde(default)]
    pub known_paths_macos: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    /// Adaptateur de `chud-engines` (« ffmpeg », « 7zip ») ou « none » (détecté, pas encore intégré).
    pub adapter: String,
    pub homepage: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_en: Option<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description_en: Option<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Le premier est l'exécutable principal, les suivants des compagnons (ex. ffprobe).
    pub executables: Vec<String>,
    #[serde(default)]
    pub executables_windows: Option<Vec<String>>,
    #[serde(default)]
    pub executables_linux: Option<Vec<String>>,
    #[serde(default)]
    pub executables_macos: Option<Vec<String>>,
    pub detect: Detect,
    #[serde(default)]
    pub hints: BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Self, ManagerError> {
        let m: Manifest = toml::from_str(text).map_err(|e| ManagerError::Manifest("?".into(), e.to_string()))?;
        Regex::new(&m.detect.version_regex).map_err(|e| ManagerError::Manifest(m.id.clone(), e.to_string()))?;
        if m.executables.is_empty() {
            return Err(ManagerError::Manifest(m.id.clone(), chud_core::tr!("aucun exécutable", "no executable").into()));
        }
        Ok(m)
    }

    /// Noms d'exécutables candidats pour la plateforme courante (principal en premier).
    pub fn platform_executables(&self) -> Vec<String> {
        let specific = if cfg!(windows) {
            &self.executables_windows
        } else if cfg!(target_os = "macos") {
            &self.executables_macos
        } else {
            &self.executables_linux
        };
        specific.clone().unwrap_or_else(|| self.executables.clone())
    }

    fn known_paths(&self) -> &[String] {
        if cfg!(windows) {
            &self.detect.known_paths_windows
        } else if cfg!(target_os = "macos") {
            &self.detect.known_paths_macos
        } else {
            &self.detect.known_paths_linux
        }
    }

    /// Description dans la langue courante.
    pub fn description_l(&self) -> &str {
        match (chud_core::lang(), &self.description_en) {
            (chud_core::Lang::En, Some(en)) => en,
            _ => &self.description,
        }
    }

    /// Licence dans la langue courante.
    pub fn license_l(&self) -> &str {
        match (chud_core::lang(), &self.license_en) {
            (chud_core::Lang::En, Some(en)) => en,
            _ => &self.license,
        }
    }

    pub fn hint(&self) -> Option<&str> {
        let key = if cfg!(windows) {
            "windows"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else {
            "linux"
        };
        self.hints.get(key).map(String::as_str)
    }
}

pub fn builtin_manifests() -> Vec<Manifest> {
    BUILTIN_MANIFESTS.iter().map(|t| Manifest::parse(t).expect("manifeste intégré valide")).collect()
}

// ---------------------------------------------------------------------------------------------
// Emplacements de l'application
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// Mode portable : un fichier `portable` à côté de l'exécutable ; tout est rangé dans `data/`.
    pub portable: bool,
}

impl AppPaths {
    pub fn detect() -> Self {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf))
            && (dir.join("portable").exists() || dir.join("portable.txt").exists())
        {
            let base = dir.join("data");
            return Self { config_dir: base.join("config"), data_dir: base.clone(), cache_dir: base.join("cache"), portable: true };
        }
        match directories::ProjectDirs::from("", "", "Chud Converter") {
            Some(p) => Self {
                config_dir: p.config_dir().to_path_buf(),
                data_dir: p.data_local_dir().to_path_buf(),
                cache_dir: p.cache_dir().to_path_buf(),
                portable: false,
            },
            None => {
                let base = std::env::temp_dir().join("chudconverter");
                Self { config_dir: base.join("config"), data_dir: base.clone(), cache_dir: base.join("cache"), portable: false }
            }
        }
    }

    /// Emplacements explicites (tests, intégration).
    pub fn at(base: &Path) -> Self {
        Self { config_dir: base.join("config"), data_dir: base.join("data"), cache_dir: base.join("cache"), portable: true }
    }

    pub fn engines_dir(&self) -> PathBuf {
        self.data_dir.join("engines")
    }

    /// Préréglages de conversion (partagés par la CLI, l'application et le menu contextuel).
    pub fn presets_file(&self) -> PathBuf {
        self.config_dir.join("presets.toml")
    }

    /// Réglages de l'application (langue, moteurs).
    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("engines.toml")
    }
}

// ---------------------------------------------------------------------------------------------
// Réglages utilisateur (engines.toml)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSettings {
    /// Chemin personnalisé de l'exécutable principal.
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    /// Langue de l'interface : `system` (par défaut), `fr` ou `en`.
    #[serde(default)]
    pub language: chud_core::LangChoice,
    /// Mises à jour de l'application (pas des moteurs).
    #[serde(default)]
    pub updates: UpdateSettings,
    #[serde(default)]
    pub engines: BTreeMap<String, EngineSettings>,
}

/// Recherche de mises à jour de l'application sur GitHub.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettings {
    /// Recherche automatique au démarrage, au plus une fois par jour (désactivable).
    #[serde(default = "yes")]
    pub auto_check: bool,
    /// Dernière recherche, en secondes depuis 1970.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_check: Option<u64>,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self { auto_check: true, last_check: None }
    }
}

impl UpdateSettings {
    /// Intervalle minimal entre deux recherches automatiques.
    pub const INTERVAL_SECS: u64 = 24 * 3600;

    /// Une recherche automatique est-elle due à l'instant `now` ?
    pub fn auto_due(&self, now: u64) -> bool {
        self.auto_check && self.last_check.is_none_or(|t| now < t || now - t >= Self::INTERVAL_SECS)
    }
}

impl Settings {
    pub fn load(file: &Path) -> Result<Self, ManagerError> {
        match std::fs::read_to_string(file) {
            Ok(t) => toml::from_str(&t).map_err(|e| ManagerError::Settings(file.into(), e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Langue effective et application à tout le processus.
    pub fn apply_language(&self) -> chud_core::Lang {
        let l = self.language.resolve();
        chud_core::set_lang(l);
        l
    }

    /// Écriture atomique (fichier temporaire puis renommage).
    pub fn save(&self, file: &Path) -> Result<(), ManagerError> {
        if let Some(d) = file.parent() {
            std::fs::create_dir_all(d)?;
        }
        let text = toml::to_string_pretty(self).map_err(|e| ManagerError::Settings(file.into(), e.to_string()))?;
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, file)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Détection
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "detail", rename_all = "snake_case")]
pub enum EngineState {
    /// Trouvé, fonctionnel et intégré.
    Ready,
    /// Trouvé et fonctionnel, mais aucun adaptateur n'est encore écrit pour lui.
    NoAdapter,
    /// Trouvé mais ne répond pas correctement.
    Broken(String),
    Missing,
    Disabled,
}

#[derive(Clone, Debug, Serialize)]
pub struct EngineStatus {
    pub id: String,
    pub name: String,
    pub state: EngineState,
    pub path: Option<PathBuf>,
    pub companions: BTreeMap<String, PathBuf>,
    pub version: Option<String>,
    pub origin: Option<EngineOrigin>,
    pub license: String,
    pub homepage: String,
    pub description: String,
    /// Commande d'installation suggérée (affichée, jamais exécutée sans accord).
    pub hint: Option<String>,
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) && Path::new(base).extension().is_none() { format!("{base}.exe") } else { base.to_owned() }
}

/// Développe `%VAR%` (Windows) ou `$VAR` / `~` (Unix). Une variable absente invalide le chemin.
fn expand_vars(s: &str) -> Option<String> {
    let mut out = String::new();
    if cfg!(windows) {
        let mut rest = s;
        while let Some(start) = rest.find('%') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let end = after.find('%')?;
            let val = std::env::var(&after[..end]).ok()?;
            out.push_str(&val);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
    } else {
        let s = match s.strip_prefix("~/") {
            Some(r) => format!("{}/{r}", std::env::var("HOME").ok()?),
            None => s.to_owned(),
        };
        let re = Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").expect("regex");
        let mut missing = false;
        out = re
            .replace_all(&s, |c: &regex::Captures| {
                std::env::var(&c[1]).unwrap_or_else(|_| {
                    missing = true;
                    String::new()
                })
            })
            .into_owned();
        if missing {
            return None;
        }
    }
    Some(out)
}

/// Résout un motif avec au plus un `*` par composant (ex. `C:\Program Files\gs\gs*\bin\gswin64c.exe`).
fn glob_simple(pattern: &str) -> Vec<PathBuf> {
    let p = PathBuf::from(pattern);
    let mut current: Vec<PathBuf> = vec![PathBuf::new()];
    for comp in p.components() {
        let s = comp.as_os_str().to_string_lossy().into_owned();
        if let Some(star) = s.find('*') {
            let (pre, post) = (&s[..star], &s[star + 1..]);
            let mut next = Vec::new();
            for base in &current {
                let Ok(rd) = std::fs::read_dir(base) else { continue };
                let mut names: Vec<PathBuf> = rd
                    .flatten()
                    .filter(|e| {
                        let n = e.file_name().to_string_lossy().to_lowercase();
                        n.starts_with(&pre.to_lowercase()) && n.ends_with(&post.to_lowercase())
                    })
                    .map(|e| e.path())
                    .collect();
                names.sort();
                names.reverse(); // versions les plus récentes d'abord (ordre lexical)
                next.extend(names);
            }
            current = next;
        } else {
            for c in &mut current {
                c.push(comp.as_os_str());
            }
        }
    }
    current.into_iter().filter(|p| p.is_file()).collect()
}

fn in_path(name: &str) -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else { return Vec::new() };
    std::env::split_paths(&path)
        .filter(|d| d.is_absolute())
        .filter(|d| !d.to_string_lossy().to_lowercase().contains("windowsapps"))
        .map(|d| d.join(exe_name(name)))
        .filter(|p| p.is_file())
        .collect()
}

/// Exécutable géré : `<engines>/<id>/versions/<current>/…` (recherche sur 3 niveaux).
fn managed(paths: &AppPaths, id: &str, exe: &str) -> Option<(PathBuf, String)> {
    let root = paths.engines_dir().join(id);
    let version = std::fs::read_to_string(root.join("current")).ok()?.trim().to_owned();
    if version.is_empty() || version.contains(['/', '\\']) || version.contains("..") {
        return None;
    }
    let base = root.join("versions").join(&version);
    let mut stack = vec![(base, 0)];
    while let Some((d, depth)) = stack.pop() {
        let candidate = d.join(exe_name(exe));
        if candidate.is_file() {
            return Some((candidate, version));
        }
        if depth < 3
            && let Ok(rd) = std::fs::read_dir(&d)
        {
            stack.extend(rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| (e.path(), depth + 1)));
        }
    }
    None
}

/// Lance la commande de version et extrait le numéro.
pub fn probe_version(exe: &Path, m: &Manifest) -> Result<String, String> {
    let spec = ProcessSpec::new(exe).args(m.detect.version_args.iter()).timeout(Some(Duration::from_secs(15)));
    let out = chud_process::run_capture(&spec, 1024 * 1024).map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", out.stdout_text(), out.stderr_tail);
    let re = Regex::new(&m.detect.version_regex).map_err(|e| e.to_string())?;
    for line in text.lines() {
        if let Some(c) = re.captures(line.trim()) {
            return Ok(c[1].to_owned());
        }
    }
    Err(if out.success() {
        chud_core::tr!("version introuvable dans la réponse", "version not found in the output").into()
    } else {
        format!("code {:?} : {}", out.code, out.stderr_tail.lines().last().unwrap_or("").trim())
    })
}

pub struct EngineManager {
    manifests: Vec<Manifest>,
    paths: AppPaths,
    settings: Settings,
}

impl EngineManager {
    pub fn new(paths: AppPaths) -> Result<Self, ManagerError> {
        let settings = Settings::load(&paths.settings_file())?;
        Ok(Self { manifests: builtin_manifests(), paths, settings })
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn manifests(&self) -> &[Manifest] {
        &self.manifests
    }

    /// Candidats ordonnés pour l'exécutable principal d'un moteur.
    fn candidates(&self, m: &Manifest) -> Vec<(PathBuf, EngineOrigin, Option<String>)> {
        let exes = m.platform_executables();
        let mut out = Vec::new();
        if let Some(p) = self.settings.engines.get(&m.id).and_then(|s| s.path.clone()) {
            out.push((p, EngineOrigin::Custom, None));
        }
        for exe in &exes[..1] {
            if let Some((p, v)) = managed(&self.paths, &m.id, exe) {
                out.push((p, EngineOrigin::Managed, Some(v)));
            }
        }
        for k in m.known_paths() {
            if let Some(expanded) = expand_vars(k) {
                if expanded.contains('*') {
                    out.extend(glob_simple(&expanded).into_iter().map(|p| (p, EngineOrigin::System, None)));
                } else {
                    out.push((PathBuf::from(expanded), EngineOrigin::System, None));
                }
            }
        }
        // Sous Linux/macOS, plusieurs noms possibles (7zz, 7z) : chacun est cherché dans le PATH.
        let primary_names: Vec<&String> = if exes.len() > 1 && m.executables.len() == 1 { exes.iter().collect() } else { vec![&exes[0]] };
        for name in primary_names {
            out.extend(in_path(name).into_iter().map(|p| (p, EngineOrigin::System, None)));
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|(p, _, _)| seen.insert(p.clone()));
        out
    }

    /// Détecte un moteur.
    pub fn discover_one(&self, m: &Manifest) -> EngineStatus {
        let mut status = EngineStatus {
            id: m.id.clone(),
            name: m.name.clone(),
            state: EngineState::Missing,
            path: None,
            companions: BTreeMap::new(),
            version: None,
            origin: None,
            license: m.license_l().to_owned(),
            homepage: m.homepage.clone(),
            description: m.description_l().to_owned(),
            hint: m.hint().map(str::to_owned),
        };
        if self.settings.engines.get(&m.id).is_some_and(|s| !s.enabled) {
            status.state = EngineState::Disabled;
            return status;
        }
        let exes = m.platform_executables();
        let mut last_error = None;
        for (path, origin, managed_version) in self.candidates(m) {
            if !path.is_absolute() || !path.is_file() {
                if origin == EngineOrigin::Custom {
                    last_error = Some(chud_core::trf!("chemin personnalisé introuvable : {}", "custom path not found: {}", path.display()));
                }
                continue;
            }
            match probe_version(&path, m) {
                Ok(v) => {
                    status.path = Some(path.clone());
                    status.version = Some(managed_version.unwrap_or(v));
                    status.origin = Some(origin);
                    // Compagnons (ex. ffprobe) : d'abord à côté de l'exécutable principal.
                    if m.executables.len() > 1 {
                        for comp in &m.executables[1..] {
                            let near = path.parent().map(|d| d.join(exe_name(comp)));
                            let found = near.filter(|p| p.is_file()).or_else(|| in_path(comp).into_iter().next());
                            if let Some(p) = found {
                                status.companions.insert(comp.clone(), p);
                            }
                        }
                    }
                    status.state = if m.adapter == "none" { EngineState::NoAdapter } else { EngineState::Ready };
                    return status;
                }
                Err(e) => last_error = Some(format!("{} : {e}", path.display())),
            }
        }
        let _ = exes;
        if let Some(e) = last_error {
            status.state = EngineState::Broken(e);
        }
        status
    }

    /// Détecte tous les moteurs, en parallèle.
    pub fn discover(&self) -> Vec<EngineStatus> {
        std::thread::scope(|s| {
            let handles: Vec<_> = self.manifests.iter().map(|m| s.spawn(move || self.discover_one(m))).collect();
            handles.into_iter().map(|h| h.join().expect("détection")).collect()
        })
    }

    /// Construit le registre : moteurs natifs + adaptateurs des moteurs externes prêts.
    /// Les statuts sont mis à jour si un adaptateur échoue à sonder ses capacités.
    pub fn build_registry(&self, statuses: &mut [EngineStatus]) -> Registry {
        let mut reg = Registry::new(FormatCatalog::builtin());
        for e in chud_engines::native_engines() {
            reg.register(e);
        }
        type Built = (usize, Result<Arc<dyn Engine>, String>);
        let built: Vec<Built> = std::thread::scope(|s| {
            let handles: Vec<_> = statuses
                .iter()
                .enumerate()
                .filter(|(_, st)| st.state == EngineState::Ready)
                .map(|(i, st)| {
                    let st = st.clone();
                    s.spawn(move || (i, build_adapter(&st)))
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("adaptateur")).collect()
        });
        for (i, r) in built {
            match r {
                Ok(engine) => reg.register(engine),
                Err(e) => statuses[i].state = EngineState::Broken(e),
            }
        }
        for (id, s) in &self.settings.engines {
            if !s.enabled {
                reg.set_enabled(&EngineId::new(id.clone()), false);
            }
        }
        reg
    }
}

fn build_adapter(st: &EngineStatus) -> Result<Arc<dyn Engine>, String> {
    let path = st.path.clone().ok_or(chud_core::tr!("chemin absent", "path missing"))?;
    let origin = st.origin.unwrap_or(EngineOrigin::System);
    match st.id.as_str() {
        "ffmpeg" => {
            let ffprobe = st.companions.get("ffprobe").cloned();
            Ok(Arc::new(Ffmpeg::new(path, ffprobe, st.version.clone(), origin)?))
        }
        "7zip" => Ok(Arc::new(SevenZip::new(path, st.version.clone(), origin)?)),
        other => Err(chud_core::trf!("aucun adaptateur pour {other}", "no adapter for {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_settings_default_on_once_a_day_and_round_trip() {
        let s: Settings = toml::from_str("language = \"fr\"").unwrap();
        assert!(s.updates.auto_check && s.updates.last_check.is_none());
        assert!(s.updates.auto_due(1_000_000));
        let u = UpdateSettings { auto_check: true, last_check: Some(1_000_000) };
        assert!(!u.auto_due(1_000_000 + 3600));
        assert!(u.auto_due(1_000_000 + UpdateSettings::INTERVAL_SECS));
        assert!(u.auto_due(999_000), "horloge reculée : on ne bloque pas les recherches");
        assert!(!UpdateSettings { auto_check: false, last_check: None }.auto_due(1));
        let s = Settings { updates: UpdateSettings { auto_check: false, last_check: Some(42) }, ..Settings::default() };
        let back: Settings = toml::from_str(&toml::to_string_pretty(&s).unwrap()).unwrap();
        assert_eq!(back.updates, s.updates);
    }

    #[test]
    fn manifests_are_valid() {
        let ms = builtin_manifests();
        assert!(ms.len() >= 10);
        let mut ids = std::collections::HashSet::new();
        for m in &ms {
            assert!(ids.insert(m.id.clone()), "identifiant en double : {}", m.id);
        }
        let ff = ms.iter().find(|m| m.id == "ffmpeg").unwrap();
        let re = Regex::new(&ff.detect.version_regex).unwrap();
        assert_eq!(&re.captures("ffmpeg version 9.0.2-full_build-www.gyan.dev Copyright").unwrap()[1], "9.0.2-full_build-www.gyan.dev");
        let sz = ms.iter().find(|m| m.id == "7zip").unwrap();
        let re = Regex::new(&sz.detect.version_regex).unwrap();
        assert_eq!(&re.captures("7-Zip 26.06 (x64) : Copyright (c) 1999-2026 Igor Pavlov").unwrap()[1], "26.06");
        assert_eq!(&re.captures("7-Zip (a) 23.01 (x64) : Copyright").unwrap()[1], "23.01");
    }

    #[test]
    fn expands_variables() {
        if cfg!(windows) {
            let pf = std::env::var("ProgramFiles").unwrap();
            assert_eq!(expand_vars(r"%ProgramFiles%\7-Zip\7z.exe").unwrap(), format!(r"{pf}\7-Zip\7z.exe"));
        }
        assert!(expand_vars("%CHUD_VARIABLE_ABSENTE_123%/x").is_none() || !cfg!(windows));
    }

    #[test]
    fn settings_round_trip() {
        let dir = std::env::temp_dir().join(format!("chud-settings-{}", std::process::id()));
        let file = dir.join("engines.toml");
        let mut s = Settings::default();
        s.engines.insert("ffmpeg".into(), EngineSettings { path: Some(PathBuf::from("/opt/ffmpeg/bin/ffmpeg")), enabled: false });
        s.save(&file).unwrap();
        let back = Settings::load(&file).unwrap();
        assert!(!back.engines["ffmpeg"].enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
