// Pas de console derrière la fenêtre dans la version publiée (Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod commands;

use backend::{App, UiListener};
use chud_enginemgr::AppPaths;
use std::sync::Arc;
use tauri::{Emitter, Manager, RunEvent};

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let listener: UiListener = Arc::new(move |ev| {
                let _ = handle.emit("queue", ev);
            });
            // Les chemins sont transmis tels quels comme données : jamais interprétés par un shell.
            let files: Vec<String> =
                std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).filter(|a| !a.is_empty()).collect();
            let state = Arc::new(App::new(AppPaths::detect(), listener).with_initial(files));
            app.manage(state.clone());
            // Détection des moteurs dès l'ouverture, sans attendre le premier fichier.
            std::thread::spawn(move || {
                let _ = state.engines();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::engines,
            commands::inspect,
            commands::targets,
            commands::plan,
            commands::convert,
            commands::initial_paths,
            commands::cancel,
            commands::cancel_all,
        ])
        .build(tauri::generate_context!())
        .expect("impossible de démarrer Chud Converter");

    app.run(|handle, event| {
        // À la fermeture : les conversions en cours sont annulées, leurs moteurs arrêtés et leurs
        // dossiers de travail supprimés avant la sortie. Aucun fichier à moitié écrit ne reste.
        if let RunEvent::Exit = event
            && let Some(state) = handle.try_state::<Arc<App>>()
        {
            state.shutdown();
        }
    });
}
