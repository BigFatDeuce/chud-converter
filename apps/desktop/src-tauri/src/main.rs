// Pas de console derrière la fenêtre dans la version publiée (Windows).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;
mod commands;
mod updates;

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
        .title(chud_core::tr!("Chud Converter — conversion", "Chud Converter — conversion"))
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
        eprintln!("chud-converter: {e}");
    }
}

/// `chud-converter --uninstall-menu` : appelé par le désinstalleur. Retire le menu clic droit
/// seulement s'il lance ce programme-ci (le menu d'une copie portable reste en place).
/// Code de sortie : 0 retiré ou rien à faire, 1 erreur.
fn uninstall_menu_if_ours() -> i32 {
    let same = |a: &std::path::Path, b: &std::path::Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    let Ok(me) = std::env::current_exe() else { return 1 };
    match chud_shell::status() {
        Ok(s) if s.installed && s.exe.as_deref().is_some_and(|e| same(e, &me)) => match chud_shell::uninstall() {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("chud-converter: {e}");
                1
            }
        },
        Ok(_) => 0,
        Err(chud_shell::ShellError::Unsupported) => 0,
        Err(e) => {
            eprintln!("chud-converter: {e}");
            1
        }
    }
}

fn main() {
    if std::env::args_os().nth(1).is_some_and(|a| a == "--uninstall-menu") {
        std::process::exit(uninstall_menu_if_ours());
    }
    let app = tauri::Builder::default()
        // En premier : une seconde instance transmet ses arguments puis se termine.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Ce rappel peut arriver pendant la création d'une fenêtre (vingt fichiers choisis dans
            // l'Explorateur = vingt lancements simultanés) : créer une fenêtre ici bloquerait tout.
            // On se contente de mettre le travail dans la file de la boucle d'événements, depuis un
            // autre fil : appelé depuis le fil principal, `run_on_main_thread` exécuterait tout de
            // suite, donc toujours à l'intérieur de ce rappel.
            let launch = Launch::parse(argv.into_iter().skip(1).map(Into::into));
            let handle = app.clone();
            std::thread::spawn(move || {
                let h = handle.clone();
                if let Err(e) = handle.run_on_main_thread(move || handle_launch(&h, launch)) {
                    eprintln!("chud-converter : lancement transmis perdu : {e}");
                }
            });
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::default())
        .setup(|app| {
            let handle = app.handle().clone();
            let listener: UiListener = Arc::new(move |ev| {
                let _ = handle.emit("queue", ev);
            });
            let state = Arc::new(App::new(AppPaths::detect(), listener));
            app.manage(state.clone());
            // Les chemins sont transmis tels quels comme données : jamais interprétés par un shell.
            handle_launch(app.handle(), Launch::parse(std::env::args_os().skip(1)));
            // Recherche de mise à jour (si activée et pas faite depuis un jour).
            updates::check_at_startup(app.handle().clone());
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
            commands::shell_status,
            commands::shell_install,
            commands::shell_uninstall,
            commands::settings,
            commands::set_language,
            commands::update_status,
            commands::update_check,
            commands::update_set_auto,
            commands::update_install,
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
