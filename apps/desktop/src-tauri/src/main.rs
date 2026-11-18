// Pas de console derrière la fenêtre dans la version publiée (Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod commands;

use backend::{App, Launch, UiListener};
use chud_enginemgr::AppPaths;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

/// Fenêtre principale ; créée au besoin (on peut n'avoir ouvert que la fenêtre rapide).
/// Les fichiers donnés sont ajoutés à sa liste.
pub fn show_main(app: &AppHandle, files: Vec<String>) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("main") {
        if !files.is_empty() {
            app.emit_to("main", "open-files", files)?;
        }
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    if !files.is_empty()
        && let Some(state) = app.try_state::<Arc<App>>()
    {
        state.add_initial(files);
    }
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Chud Converter")
        .inner_size(1180.0, 760.0)
        .min_inner_size(920.0, 600.0)
        .center()
        .build()?;
    Ok(())
}

/// Petite fenêtre de suivi des conversions lancées par le menu contextuel.
fn show_quick(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("quick") {
        let _ = w.unminimize();
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "quick", WebviewUrl::App("index.html".into()))
        .title("Chud Converter — conversion")
        .inner_size(520.0, 400.0)
        .min_inner_size(380.0, 260.0)
        .center()
        .build()?;
    Ok(())
}

/// Lancement direct ou relayé par une seconde instance (menu contextuel sur plusieurs fichiers :
/// l'Explorateur lance un processus par fichier, tous rejoignent la première instance).
fn handle_launch(app: &AppHandle, launch: Launch) {
    let result = match launch {
        Launch::Open(files) => show_main(app, files),
        Launch::Preset { id, files } => {
            let Some(state) = app.try_state::<Arc<App>>() else { return };
            let state = state.inner().clone();
            // La détection des moteurs peut prendre une seconde : la fenêtre s'ouvre tout de suite.
            let shown = show_quick(app);
            let app = app.clone();
            std::thread::spawn(move || {
                state.quick(&id, files);
                // Si la fenêtre n'a pas encore chargé son interface, elle lira le lot au démarrage.
                let _ = app.emit_to("quick", "quick", ());
            });
            shown
        }
    };
    if let Err(e) = result {
        eprintln!("chud-converter : fenêtre impossible à ouvrir : {e}");
    }
}

fn main() {
    let app = tauri::Builder::default()
        // En premier : une seconde instance transmet ses arguments puis se termine.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_launch(app, Launch::parse(argv.into_iter().skip(1).map(Into::into)));
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let listener: UiListener = Arc::new(move |ev| {
                let _ = handle.emit("queue", ev);
            });
            let state = Arc::new(App::new(AppPaths::detect(), listener));
            app.manage(state.clone());
            // Les chemins sont transmis tels quels comme données : jamais interprétés par un shell.
            handle_launch(app.handle(), Launch::parse(std::env::args_os().skip(1)));
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
            commands::presets,
            commands::save_presets,
            commands::reset_presets,
            commands::presets_for,
            commands::run_preset,
            commands::take_quick,
            commands::show_main,
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
