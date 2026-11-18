//! Commandes appelées par l'interface. Tout ce qui touche au disque ou aux moteurs part sur un
//! fil bloquant pour ne jamais figer la fenêtre.

use crate::backend::{
    App, AppInfo, ConvertRequest, EngineView, FileInfo, FileTargets, PlanRequest, PlanView, PresetView, PresetsView, QuickBatch, RunPreset,
    SettingsView, ShellView, Submitted,
};
use chud_core::LangChoice;
use chud_jobs::JobId;
use std::sync::Arc;
use tauri::State;
use tauri::{AppHandle, Emitter};

type Shared<'a> = State<'a, Arc<App>>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| chud_core::trf!("tâche interrompue : {e}", "task interrupted: {e}"))?
}

#[tauri::command]
pub async fn app_info(app: Shared<'_>) -> Result<AppInfo, String> {
    let app = app.inner().clone();
    blocking(move || Ok(app.engines()?.info())).await
}

#[tauri::command]
pub async fn engines(app: Shared<'_>, refresh: bool) -> Result<Vec<EngineView>, String> {
    let app = app.inner().clone();
    blocking(move || Ok(if refresh { app.reload_engines()? } else { app.engines()? }.views())).await
}

#[tauri::command]
pub async fn inspect(app: Shared<'_>, paths: Vec<String>) -> Result<Vec<FileInfo>, String> {
    let app = app.inner().clone();
    blocking(move || Ok(app.engines()?.inspect(&paths))).await
}

#[tauri::command]
pub async fn targets(app: Shared<'_>, path: String) -> Result<FileTargets, String> {
    let app = app.inner().clone();
    blocking(move || app.engines()?.targets(&path)).await
}

#[tauri::command]
pub async fn plan(app: Shared<'_>, request: PlanRequest) -> Result<PlanView, String> {
    let app = app.inner().clone();
    blocking(move || app.engines()?.plan(&request)).await
}

#[tauri::command]
pub async fn convert(app: Shared<'_>, request: ConvertRequest) -> Result<Vec<Submitted>, String> {
    let app = app.inner().clone();
    blocking(move || app.submit(&request)).await
}

/// Fichiers donnés sur la ligne de commande ; vide après le premier appel.
#[tauri::command]
pub fn initial_paths(app: Shared<'_>) -> Vec<String> {
    app.take_initial()
}

#[tauri::command]
pub fn cancel(app: Shared<'_>, id: JobId) -> bool {
    app.cancel(id)
}

#[tauri::command]
pub fn cancel_all(app: Shared<'_>) {
    app.cancel_all();
}

// ---------------------------------------------------------------------------------- préréglages

#[tauri::command]
pub async fn presets(app: Shared<'_>) -> Result<PresetsView, String> {
    let app = app.inner().clone();
    blocking(move || Ok(app.presets())).await
}

#[tauri::command]
pub async fn save_presets(app: Shared<'_>, presets: Vec<PresetView>) -> Result<PresetsView, String> {
    let app = app.inner().clone();
    blocking(move || app.save_presets(presets)).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetPresets {
    view: PresetsView,
    backup: Option<String>,
}

#[tauri::command]
pub async fn reset_presets(app: Shared<'_>) -> Result<ResetPresets, String> {
    let app = app.inner().clone();
    blocking(move || app.reset_presets().map(|(view, backup)| ResetPresets { view, backup })).await
}

#[tauri::command]
pub async fn presets_for(app: Shared<'_>, paths: Vec<String>) -> Result<Vec<PresetView>, String> {
    let app = app.inner().clone();
    blocking(move || app.presets_for(&paths)).await
}

#[tauri::command]
pub async fn run_preset(app: Shared<'_>, id: String, files: Vec<String>) -> Result<RunPreset, String> {
    let app = app.inner().clone();
    blocking(move || app.run_preset(&id, &files)).await
}

/// Lots lancés par le menu contextuel, pas encore affichés.
#[tauri::command]
pub fn take_quick(app: Shared<'_>) -> Vec<QuickBatch> {
    app.take_quick()
}

/// Ouvre (ou montre) la fenêtre principale depuis la fenêtre rapide. Asynchrone : sous Windows,
/// créer une fenêtre depuis une commande synchrone la laisse vide (WebView2 bloqué).
#[tauri::command]
pub async fn show_main(handle: tauri::AppHandle) -> Result<(), String> {
    crate::show_main(&handle, Vec::new()).map_err(|e| e.to_string())
}

// ------------------------------------------------------------------------------ menu clic droit

#[tauri::command]
pub async fn shell_status(app: Shared<'_>) -> Result<ShellView, String> {
    let app = app.inner().clone();
    blocking(move || Ok(app.shell_status())).await
}

#[tauri::command]
pub async fn shell_install(app: Shared<'_>) -> Result<ShellView, String> {
    let app = app.inner().clone();
    blocking(move || app.shell_install()).await
}

#[tauri::command]
pub async fn settings(app: Shared<'_>) -> Result<SettingsView, String> {
    let app = app.inner().clone();
    blocking(move || Ok(app.settings())).await
}

/// Change la langue et prévient toutes les fenêtres ouvertes.
#[tauri::command]
pub async fn set_language(handle: AppHandle, app: Shared<'_>, language: LangChoice) -> Result<SettingsView, String> {
    let app = app.inner().clone();
    let view = blocking(move || app.set_language(language)).await?;
    let _ = handle.emit("language", view.effective);
    Ok(view)
}

#[tauri::command]
pub async fn shell_uninstall(app: Shared<'_>) -> Result<ShellView, String> {
    let app = app.inner().clone();
    blocking(move || app.shell_uninstall()).await
}

// ---------------------------------------------------------------------- mises à jour

type Upd<'a> = State<'a, crate::updates::Updates>;

/// État des mises à jour, sans connexion.
#[tauri::command]
pub fn update_status(app: Shared<'_>, updates: Upd<'_>) -> crate::updates::UpdateView {
    updates.view(app.inner())
}

/// Recherche (`manual` : demandée par l'utilisateur ; sinon seulement si elle est due).
#[tauri::command]
pub async fn update_check(
    handle: AppHandle,
    app: Shared<'_>,
    updates: Upd<'_>,
    manual: bool,
) -> Result<crate::updates::UpdateView, String> {
    updates.check(&handle, app.inner(), manual).await
}

#[tauri::command]
pub fn update_set_auto(app: Shared<'_>, updates: Upd<'_>, enabled: bool) -> Result<crate::updates::UpdateView, String> {
    app.save_update_settings(|s| s.auto_check = enabled)?;
    Ok(updates.view(app.inner()))
}

/// Télécharge, vérifie et installe ; l'application redémarre sur la nouvelle version.
#[tauri::command]
pub async fn update_install(handle: AppHandle, app: Shared<'_>, updates: Upd<'_>) -> Result<(), String> {
    updates.install(&handle, app.inner()).await
}
