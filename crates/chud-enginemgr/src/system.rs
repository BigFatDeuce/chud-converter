//! Installation par le gestionnaire de paquets du système (Linux), quand aucun paquet portable
//! officiel n'existe pour la plateforme (Ghostscript, Tesseract, LibreOffice…).
//!
//! - Les noms de paquets viennent du manifeste intégré (jamais de l'utilisateur ni d'un fichier
//!   téléchargé) et sont validés ; ils sont passés comme arguments séparés à un script fixe.
//! - Les droits administrateur sont demandés par la fenêtre de mot de passe du système
//!   (`pkexec`) ; sans elle, un terminal s'ouvre tout seul avec `sudo` : l'utilisateur tape son
//!   mot de passe, il n'a rien à copier.
//! - Le paquet installé appartient au système : Chud Converter ne le retire jamais et ne
//!   remplace rien (une installation gérée par la distribution est mise à jour par elle).
//! - La réussite n'est jamais déduite du code de retour : le moteur est détecté et essayé ensuite.

use crate::install::{InstallError, InstallProgress, Installed};
use crate::{AppPaths, EngineManager, EngineState, Manifest};
use chud_core::{tr, trf};
use chud_process::ProcessSpec;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Paquets à installer, par gestionnaire.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Packages {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apt: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dnf: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pacman: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zypper: Vec<String>,
}

impl Packages {
    pub fn is_empty(&self) -> bool {
        self.apt.is_empty() && self.dnf.is_empty() && self.pacman.is_empty() && self.zypper.is_empty()
    }

    pub fn validate(&self) -> Result<(), String> {
        for p in self.apt.iter().chain(&self.dnf).chain(&self.pacman).chain(&self.zypper) {
            if !valid_package(p) {
                return Err(trf!("nom de paquet refusé : {}", "package name refused: {}", p));
            }
        }
        Ok(())
    }

    fn for_manager(&self, pm: Manager) -> &[String] {
        match pm {
            Manager::Apt => &self.apt,
            Manager::Dnf => &self.dnf,
            Manager::Pacman => &self.pacman,
            Manager::Zypper => &self.zypper,
        }
    }
}

/// `^[a-z0-9][a-z0-9.+-]*$` : un nom de paquet, jamais une option ni un chemin.
fn valid_package(p: &str) -> bool {
    let b = p.as_bytes();
    !b.is_empty()
        && b.len() <= 100
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".+-".contains(c))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
}

impl Manager {
    pub fn name(self) -> &'static str {
        match self {
            Manager::Apt => "apt",
            Manager::Dnf => "dnf",
            Manager::Pacman => "pacman",
            Manager::Zypper => "zypper",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Manager::Apt => "apt-get",
            Manager::Dnf => "dnf",
            Manager::Pacman => "pacman",
            Manager::Zypper => "zypper",
        }
    }

    /// Script fixe ; les paquets arrivent en arguments positionnels (`"$@"`).
    fn script(self) -> &'static str {
        match self {
            Manager::Apt => "export DEBIAN_FRONTEND=noninteractive; apt-get update -q && apt-get install -y -q \"$@\"",
            Manager::Dnf => "dnf install -y \"$@\"",
            Manager::Pacman => "pacman -S --needed --noconfirm \"$@\"",
            Manager::Zypper => "zypper --non-interactive install \"$@\"",
        }
    }
}

fn find_system(names: &[&str]) -> Option<PathBuf> {
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].iter().flat_map(|d| names.iter().map(move |n| Path::new(d).join(n))).find(|p| p.is_file())
}

/// Gestionnaire de paquets de ce système (Linux seulement).
pub fn detect_manager() -> Option<Manager> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    [Manager::Apt, Manager::Dnf, Manager::Pacman, Manager::Zypper].into_iter().find(|m| find_system(&[m.program()]).is_some())
}

/// Ce qui serait installé par le système.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SystemRoute {
    /// « apt », « dnf »…
    pub manager: String,
    pub packages: Vec<String>,
}

/// Voie « paquets du système » pour ce moteur, s'il n'a pas de paquet portable ici.
pub fn route(m: &Manifest) -> Option<(Manager, SystemRoute)> {
    if m.install_source().is_some() {
        return None;
    }
    let pm = detect_manager()?;
    let pkgs = m.packages.for_manager(pm);
    if pkgs.is_empty() {
        return None;
    }
    Some((pm, SystemRoute { manager: pm.name().into(), packages: pkgs.to_vec() }))
}

/// Installe les paquets du moteur, puis vérifie qu'il est détecté et qu'il répond.
pub fn install(
    paths: &AppPaths,
    m: &Manifest,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<Installed, InstallError> {
    let (pm, r) = route(m).ok_or_else(|| {
        InstallError::Failed(trf!(
            "{} ne peut pas être installé automatiquement sur ce système",
            "{} cannot be installed automatically on this system",
            m.name
        ))
    })?;
    if cancel.load(Ordering::Relaxed) {
        return Err(InstallError::Cancelled);
    }
    progress(InstallProgress::System { line: trf!("demande d'autorisation ({})…", "asking for permission ({})…", pm.name()) });
    let mut emit = |l: &str| {
        let l = l.trim();
        if !l.is_empty() {
            progress(InstallProgress::System { line: l.chars().take(200).collect() });
        }
    };
    match find_system(&["pkexec"]) {
        Some(pkexec) => {
            let sh = find_system(&["sh"]).ok_or_else(|| InstallError::Failed("sh".into()))?;
            let spec = ProcessSpec::new(&pkexec)
                .arg(&sh)
                .arg("-c")
                .arg(pm.script())
                .arg("sh")
                .args(&r.packages)
                .timeout(Some(Duration::from_secs(3600)));
            let mut out_line = |l: &str| emit(l);
            let mut err_tail = Vec::<String>::new();
            let out = chud_process::run(&spec, &|| false, &mut out_line, &mut |l| {
                err_tail.push(l.to_owned());
                if err_tail.len() > 20 {
                    err_tail.remove(0);
                }
            })
            .map_err(|e| InstallError::Failed(trf!("pkexec : {}", "pkexec: {}", e)))?;
            match out.code {
                Some(0) => {}
                // 126 : fenêtre fermée ou mot de passe refusé.
                Some(126) => {
                    return Err(tr!("autorisation refusée : rien n'a été installé", "permission declined: nothing was installed")
                        .to_owned()
                        .into());
                }
                // 127 : pas d'agent d'authentification graphique → terminal.
                Some(127) => run_in_terminal(pm, &r.packages)?,
                code => {
                    let last = err_tail.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_default();
                    return Err(InstallError::Failed(trf!(
                        "{} a échoué (code {:?}) : {}",
                        "{} failed (code {:?}): {}",
                        pm.name(),
                        code,
                        last.trim()
                    )));
                }
            }
        }
        None => run_in_terminal(pm, &r.packages)?,
    }
    progress(InstallProgress::Checking);
    let st = EngineManager::new(paths.clone()).map_err(|e| InstallError::Failed(e.to_string()))?.discover_one(m);
    match (&st.state, &st.path) {
        (EngineState::Ready | EngineState::NoAdapter, Some(p)) => {
            Ok(Installed { path: p.clone(), version: st.version.unwrap_or_default(), previous: None })
        }
        (EngineState::Broken(e), _) => {
            Err(InstallError::Failed(trf!("installé, mais ne répond pas : {}", "installed, but not responding: {}", e)))
        }
        _ => Err(InstallError::Failed(trf!(
            "{} reste introuvable après l'installation des paquets ({})",
            "{} is still not found after installing the packages ({})",
            m.name,
            r.packages.join(" ")
        ))),
    }
}

/// Sans fenêtre de mot de passe graphique : un terminal s'ouvre seul et lance `sudo`.
fn run_in_terminal(pm: Manager, pkgs: &[String]) -> Result<(), InstallError> {
    let sh = find_system(&["sh"]).ok_or_else(|| InstallError::Failed("sh".into()))?;
    let sudo = find_system(&["sudo"]).ok_or_else(|| {
        InstallError::Failed(
            tr!(
                "ni pkexec ni sudo sur ce système : installation impossible d'ici",
                "neither pkexec nor sudo on this system: cannot install from here"
            )
            .into(),
        )
    })?;
    // Script fixe : `sudo sh -c SCRIPT sh paquets…`, puis une pause pour lire le résultat.
    let inner = format!(
        "{} {} -c '{}' sh \"$@\"; s=$?; echo; printf '%s' \"{}\"; read _; exit $s",
        sudo.display(),
        sh.display(),
        pm.script().replace('\'', "'\\''"),
        tr!("Appuyez sur Entrée pour fermer cette fenêtre.", "Press Enter to close this window.")
    );
    let terminals: [(&str, &[&str]); 5] = [
        ("gnome-terminal", &["--wait", "--"]),
        ("konsole", &["--nofork", "-e"]),
        ("xfce4-terminal", &["--disable-server", "-x"]),
        ("xterm", &["-e"]),
        ("x-terminal-emulator", &["-e"]),
    ];
    let (term, targs) = terminals.iter().find_map(|(t, a)| find_system(&[t]).map(|p| (p, *a))).ok_or_else(|| {
        InstallError::Failed(tr!("aucun terminal trouvé pour demander le mot de passe", "no terminal found to ask for the password").into())
    })?;
    let spec = ProcessSpec::new(&term)
        .args(targs.iter())
        .arg(&sh)
        .arg("-c")
        .arg(inner)
        .arg("sh")
        .args(pkgs)
        .timeout(Some(Duration::from_secs(3600)));
    chud_process::run_capture(&spec, 64 * 1024).map_err(|e| InstallError::Failed(trf!("terminal : {}", "terminal: {}", e)))?;
    // Le code du terminal ne dit rien de fiable : la détection qui suit tranche.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_are_strict() {
        for ok in ["ghostscript", "libimage-exiftool-perl", "tesseract-ocr-fra", "g++", "libreoffice-fresh", "7zip"] {
            assert!(valid_package(ok), "{ok}");
        }
        for bad in ["", "-y", "--option", "a b", "x;rm", "../x", "Pkg", "$(id)", "a/b"] {
            assert!(!valid_package(bad), "{bad}");
        }
    }

    #[test]
    fn scripts_take_packages_as_arguments() {
        for pm in [Manager::Apt, Manager::Dnf, Manager::Pacman, Manager::Zypper] {
            assert!(pm.script().ends_with("\"$@\""), "{}", pm.name());
        }
    }
}
