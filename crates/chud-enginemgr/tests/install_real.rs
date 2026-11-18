//! Installation réelle depuis Internet (FFmpeg ~115 Mo, 7-Zip ~2 Mo) dans un dossier temporaire.
//! Ignorés par défaut ; à lancer à la main : `cargo test -p chud-enginemgr --test install_real -- --ignored`.

use chud_core::EngineOrigin;
use chud_enginemgr::install::{self, InstallError};
use chud_enginemgr::{AppPaths, EngineManager, EngineState, InstallProgress};
use std::sync::atomic::AtomicBool;

fn sandbox(name: &str) -> (std::path::PathBuf, AppPaths) {
    let dir = std::env::temp_dir().join(format!("chud-install-real-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = AppPaths::at(&dir);
    (dir, paths)
}

fn install_and_check(id: &str) {
    let (dir, paths) = sandbox(id);
    let mgr = EngineManager::new(paths.clone()).unwrap();
    let m = mgr.manifests().iter().find(|m| m.id == id).unwrap().clone();
    if m.install_source().is_none() {
        eprintln!("{id} : pas de source pour cette plateforme");
        return;
    }
    let mut phases = Vec::new();
    let done = install::install(&paths, &m, &AtomicBool::new(false), &mut |p| {
        let name = match p {
            InstallProgress::Downloading { .. } => "downloading",
            InstallProgress::Verifying => "verifying",
            InstallProgress::Extracting => "extracting",
            InstallProgress::Checking => "checking",
        };
        if phases.last() != Some(&name) {
            phases.push(name);
        }
    })
    .unwrap_or_else(|e| panic!("{id} : {e}"));
    assert_eq!(phases, ["downloading", "verifying", "extracting", "checking"]);
    assert!(done.path.is_file(), "{}", done.path.display());
    assert!(!paths.engines_dir().join(id).join("downloads").exists(), "téléchargements retirés");

    // L'installation gérée passe avant celle du système, et le moteur est réellement utilisable.
    let mgr = EngineManager::new(paths.clone()).unwrap();
    let mut statuses = mgr.discover();
    let st = statuses.iter().find(|s| s.id == id).unwrap();
    assert_eq!(st.state, EngineState::Ready, "{st:?}");
    assert_eq!(st.origin, Some(EngineOrigin::Managed));
    assert_eq!(st.version.as_deref(), Some(done.version.as_str()));
    let reg = mgr.build_registry(&mut statuses);
    assert!(reg.engines().iter().any(|e| e.id().as_str() == id), "adaptateur construit");

    // Retour en arrière impossible sans version précédente ; désinstallation complète.
    assert!(install::rollback(&paths, &m).is_err());
    install::uninstall(&paths, &m).unwrap();
    assert!(!paths.engines_dir().join(id).join("versions").exists());
    let st = EngineManager::new(paths.clone()).unwrap().discover_one(&m);
    assert_ne!(st.origin, Some(EngineOrigin::Managed));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "réseau"]
fn installs_7zip() {
    install_and_check("7zip");
}

#[test]
#[ignore = "réseau, 115 Mo"]
fn installs_ffmpeg() {
    install_and_check("ffmpeg");
}

#[test]
#[ignore = "réseau"]
fn tampered_hash_is_refused_and_nothing_is_left() {
    let (dir, paths) = sandbox("tampered");
    let mgr = EngineManager::new(paths.clone()).unwrap();
    let mut m = mgr.manifests().iter().find(|m| m.id == "7zip").unwrap().clone();
    let here = install::current_platform();
    let Some(src) = m.install.iter_mut().find(|s| s.platform == here) else { return };
    let last = src.files.len() - 1;
    src.files[last].sha256 = "0".repeat(64);
    let err = install::install(&paths, &m, &AtomicBool::new(false), &mut |_| {}).unwrap_err();
    assert!(matches!(&err, InstallError::Failed(e) if e.contains("SHA-256")), "{err}");
    let root = paths.engines_dir().join("7zip");
    assert!(!root.join("current").exists());
    assert!(!root.join("downloads").exists());
    assert!(std::fs::read_dir(root.join("versions")).map(|mut d| d.next().is_none()).unwrap_or(true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
#[ignore = "réseau"]
fn cancelled_install_leaves_nothing() {
    let (dir, paths) = sandbox("cancel");
    let mgr = EngineManager::new(paths.clone()).unwrap();
    let m = mgr.manifests().iter().find(|m| m.id == "7zip").unwrap().clone();
    if m.install_source().is_none() {
        return;
    }
    let cancel = AtomicBool::new(false);
    let err = install::install(&paths, &m, &cancel, &mut |p| {
        if matches!(p, InstallProgress::Downloading { received, .. } if received > 0) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    })
    .unwrap_err();
    assert!(matches!(err, InstallError::Cancelled), "{err}");
    assert!(!paths.engines_dir().join("7zip").join("current").exists());
    assert!(!paths.engines_dir().join("7zip").join("downloads").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
