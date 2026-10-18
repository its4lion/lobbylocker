#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod platform;
use lobbylocker_core::{engine::Engine, i18n::Catalogue, store::Store};
use std::{
    path::PathBuf,
    sync::{atomic::AtomicBool, Arc, Mutex},
};
use tauri::Manager;

fn data_path(app: &tauri::App, variable: &str, folder: &str) -> lobbylocker_core::Result<PathBuf> {
    if let Some(path) = std::env::var_os(variable) {
        return Ok(PathBuf::from(path));
    }
    let resource = app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("data")
        .join(folder);
    if resource.is_dir() {
        return Ok(resource);
    }
    #[cfg(debug_assertions)]
    {
        Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../data")
            .join(folder))
    }
    #[cfg(not(debug_assertions))]
    Err(format!("Bundled data/{folder} directory is missing."))
}

fn main() {
    #[cfg(target_os = "linux")]
    if let Some(code) = lobbylocker_core::firewall::helper_entry() {
        std::process::exit(code);
    }
    platform::configure_webview();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let games = data_path(app, "LOBBYLOCKER_DATA_DIR", "games")?;
            if !games.is_dir() {
                return Err("Default game directory is missing.".into());
            }
            let locales = data_path(app, "LOBBYLOCKER_LOCALES_DIR", "locales")?;
            let engine = Engine::new(Store::open(Store::default_directory()?, &games)?)?;
            app.manage(commands::Backend {
                engine,
                locales: Catalogue::load(&locales)?,
                busy: Arc::new(AtomicBool::new(false)),
                installed: Arc::new(Mutex::new(lobbylocker_core::installed::Scan::default())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::verify_firewall,
            commands::get_languages,
            commands::set_preferences,
            commands::set_language,
            commands::save_game,
            commands::create_game,
            commands::refresh_game,
            commands::ensure_fresh_data,
            commands::restart_as_admin,
            commands::check_ping,
            commands::apply_rules,
            commands::reset_rules,
            commands::scan_games,
            commands::set_library_games,
            commands::select_game_library,
            commands::remove_game_library,
            commands::select_game_executable,
            commands::set_game_executable,
            commands::get_custom_icons,
            commands::discover_connections,
            commands::import_game,
            commands::export_game,
        ])
        .run(tauri::generate_context!())
        .expect("Unable to start LobbyLocker");
}
