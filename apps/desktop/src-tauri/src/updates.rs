//! Mises à jour de l'application (ADR-021).
//!
//! - Recherche : un seul fichier, `latest.json`, lu sur la dernière publication GitHub du projet.
//!   Rien d'autre n'est envoyé : ni fichier, ni nom de fichier, ni identifiant.
//! - Automatique au démarrage, au plus une fois par jour, désactivable dans Réglages.
//! - Le paquet téléchargé est refusé si sa signature Ed25519 ne correspond pas à la clé publique
//!   intégrée au programme, ou si la version signée n'est pas celle annoncée.
//! - Rien n'est installé sans un clic de l'utilisateur, ni pendant une conversion.
//! - Version portable, paquets .deb/.rpm et version de développement : la nouvelle version est
//!   signalée avec un lien vers sa page, mais n'est pas installée par le programme (il ne
//!   remplacerait pas proprement ce qu'un autre outil a installé).

use crate::backend::App;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

pub const REPO_URL: &str = "https://github.com/BigFatDeuce/chud-converter";

/// Comment ce programme-ci peut être mis à jour.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Install {
    /// Installé par l'installeur Windows (NSIS) : l'installeur de la nouvelle version s'en charge.
    WindowsInstaller,
    /// AppImage : le fichier est remplacé.
    AppImage,
    /// À faire par l'utilisateur (raison affichée).
    Manual(String),
}

fn install_kind(app: &App) -> Install {
    if cfg!(debug_assertions) {
        return Install::Manual(chud_core::tr!("version de développement", "development build").into());
    }
    if app.is_portable() {
        return Install::Manual(
            chud_core::tr!(
                "version portable : téléchargez la nouvelle version et remplacez les programmes du dossier",
                "portable version: download the new version and replace the programs in the folder"
            )
            .into(),
        );
    }
    if cfg!(target_os = "linux") {
        if std::env::var_os("APPIMAGE").is_some() {
            return Install::AppImage;
        }
        return Install::Manual(
            chud_core::tr!(
                "installé par un paquet .deb ou .rpm : installez le paquet de la nouvelle version",
                "installed from a .deb or .rpm package: install the package of the new version"
            )
            .into(),
        );
    }
    if cfg!(windows) && std::env::current_exe().is_ok_and(|e| e.with_file_name("uninstall.exe").is_file()) {
        return Install::WindowsInstaller;
    }
    Install::Manual(chud_core::tr!("programme lancé hors de son installation", "program started outside its installation").into())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Nouvelle version trouvée.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableView {
    pub version: String,
    /// Date de publication (RFC 3339).
    pub date: Option<String>,
    pub notes: Option<String>,
    /// Page de la publication sur GitHub.
    pub page: String,
}

/// État des mises à jour montré dans l'écran Réglages.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateView {
    pub current: String,
    pub auto_check: bool,
    /// Dernière recherche réussie (secondes depuis 1970).
    pub last_check: Option<u64>,
    pub available: Option<AvailableView>,
    /// Le programme peut installer la nouvelle version lui-même.
    pub can_install: bool,
    /// Sinon, pourquoi (affiché tel quel).
    pub manual_reason: Option<String>,
    /// Page de toutes les publications (versions précédentes comprises).
    pub releases: String,
    /// Fichier de réglages illisible : rien n'est enregistré.
    pub settings_error: Option<String>,
    /// Opération en cours (une fenêtre ouverte en cours de route l'affiche aussi).
    pub phase: Phase,
}

/// Progression envoyée à l'interface pendant l'installation.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum UpdateProgress {
    Downloading {
        received: u64,
        total: Option<u64>,
    },
    /// Téléchargé et signature vérifiée.
    Verified,
    Installing,
}

/// Machine à états des mises à jour : `Idle → Checking → Idle`, `Idle → Installing → (redémarrage)`.
/// Une seule opération à la fois ; un échec ramène à `Idle` (l'erreur est renvoyée à l'appelant).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Installing,
}

#[derive(Default)]
pub struct Updates {
    /// Version trouvée par la dernière recherche réussie.
    pending: Mutex<Option<Update>>,
    phase: Mutex<Phase>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ramène la machine au repos quoi qu'il arrive (erreur, panique, tâche abandonnée).
struct Running<'a>(&'a Mutex<Phase>);
impl Drop for Running<'_> {
    fn drop(&mut self) {
        *lock(self.0) = Phase::Idle;
    }
}

impl Updates {
    /// Seule transition depuis le repos ; refusée si une opération est déjà en cours.
    fn begin(&self, to: Phase) -> Result<Running<'_>, String> {
        let mut p = lock(&self.phase);
        if *p != Phase::Idle {
            return Err(chud_core::tr!(
                "une recherche ou une installation est déjà en cours",
                "a check or an installation is already running"
            )
            .into());
        }
        *p = to;
        Ok(Running(&self.phase))
    }

    pub fn view(&self, app: &App) -> UpdateView {
        let (settings, settings_error) = match app.update_settings() {
            Ok(s) => (s, None),
            Err(e) => (Default::default(), Some(e)),
        };
        let kind = install_kind(app);
        let available = lock(&self.pending).as_ref().map(|u| AvailableView {
            version: u.version.clone(),
            date: u.raw_json.get("pub_date").and_then(|d| d.as_str()).map(str::to_owned),
            notes: u.body.clone().filter(|b| !b.trim().is_empty()),
            page: format!("{REPO_URL}/releases/tag/v{}", u.version),
        });
        UpdateView {
            current: env!("CARGO_PKG_VERSION").into(),
            auto_check: settings.auto_check,
            last_check: settings.last_check,
            available,
            can_install: !matches!(kind, Install::Manual(_)),
            manual_reason: match kind {
                Install::Manual(r) => Some(r),
                _ => None,
            },
            releases: format!("{REPO_URL}/releases"),
            settings_error,
            phase: *lock(&self.phase),
        }
    }

    /// Recherche. `manual` : demandée par l'utilisateur ; sinon seulement si elle est due
    /// (recherche automatique activée et pas faite depuis un jour).
    pub async fn check(&self, handle: &AppHandle, app: &Arc<App>, manual: bool) -> Result<UpdateView, String> {
        if !manual {
            let due = app.update_settings().map(|s| s.auto_due(now())).unwrap_or(false);
            if !due {
                return Ok(self.view(app));
            }
        }
        let _running = self.begin(Phase::Checking)?;
        let updater = handle
            .updater_builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| chud_core::trf!("recherche de mise à jour impossible : {e}", "cannot check for updates: {e}"))?;
        let found = updater
            .check()
            .await
            .map_err(|e| chud_core::trf!("recherche de mise à jour impossible : {e}", "cannot check for updates: {e}"))?;
        *lock(&self.pending) = found;
        // Enregistré seulement après une recherche réussie ; un fichier de réglages illisible
        // n'empêche pas la recherche mais n'est pas écrasé.
        let _ = app.save_update_settings(|s| s.last_check = Some(now()));
        Ok(self.view(app))
    }

    /// Télécharge, vérifie la signature et installe la version trouvée par [`Self::check`].
    /// Windows : l'installeur de la nouvelle version remplace le programme puis le relance.
    pub async fn install(&self, handle: &AppHandle, app: &Arc<App>) -> Result<(), String> {
        let _running = self.begin(Phase::Installing)?;
        let kind = install_kind(app);
        if let Install::Manual(reason) = kind {
            return Err(reason);
        }
        if !app.is_idle() {
            return Err(chud_core::tr!(
                "des conversions sont en cours : attendez qu'elles soient terminées (ou annulez-les) avant de mettre à jour",
                "conversions are running: wait for them to finish (or cancel them) before updating"
            )
            .into());
        }
        let update = lock(&self.pending).clone().ok_or_else(|| {
            chud_core::tr!("aucune mise à jour trouvée : relancez la recherche", "no update found: check again").to_string()
        })?;
        let mut received = 0u64;
        let h = handle.clone();
        let bytes = update
            .download(
                |chunk, total| {
                    received += chunk as u64;
                    let _ = h.emit("update-progress", UpdateProgress::Downloading { received, total });
                },
                || {},
            )
            .await
            .map_err(|e| chud_core::trf!("mise à jour refusée : {e}", "update refused: {e}"))?;
        let _ = handle.emit("update-progress", UpdateProgress::Verified);
        let _ = handle.emit("update-progress", UpdateProgress::Installing);
        // Les conversions sont arrêtées proprement avant que l'installeur ne remplace le programme.
        app.shutdown();
        update.install(bytes).map_err(|e| chud_core::trf!("installation impossible : {e}", "installation failed: {e}"))?;
        // Windows ne revient pas ici (l'installeur a pris le relais) ; AppImage : relance.
        handle.restart();
    }
}

/// Recherche automatique au démarrage (si elle est due), sans rien afficher en cas d'échec :
/// l'interface est prévenue seulement quand une nouvelle version existe.
pub fn check_at_startup(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Laisse la fenêtre s'ouvrir et les moteurs se détecter d'abord.
        tokio_sleep(Duration::from_secs(5)).await;
        let (Some(app), Some(updates)) = (handle.try_state::<Arc<App>>(), handle.try_state::<Updates>()) else { return };
        let app = app.inner().clone();
        // Toujours prévenue de la fin (même sans nouvelle version, même en cas d'échec) : une
        // fenêtre qui affichait « recherche… » revient au repos.
        let _ = updates.check(&handle, &app, false).await;
        let _ = handle.emit("update-available", updates.view(&app));
    });
}

async fn tokio_sleep(d: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
}
