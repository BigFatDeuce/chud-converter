//! Commandes appelées par l'interface. Tout ce qui touche au disque ou aux moteurs part sur un
//! fil bloquant pour ne jamais figer la fenêtre.

use crate::backend::{App, AppInfo, ConvertRequest, EngineView, FileInfo, FileTargets, PlanRequest, PlanView, Submitted};
use chud_jobs::JobId;
use std::sync::Arc;
use tauri::State;

type Shared<'a> = State<'a, Arc<App>>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("tâche interrompue : {e}"))?
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
