//! Ausbildungs-Cockpit – Rust-Backend.
//!
//! Aufbau:
//! * [`model`]   – Datenmodelle (Rust ↔ Oberfläche)
//! * [`vault`]   – Markdown verstehen: Scan, Frontmatter, Aufgaben, Tags, Links, Namen, Stundenplan
//! * [`index`]   – In-Memory-Index, jederzeit verwerfbar
//! * [`watcher`] – Dateiwächter (`notify`)
//! * [`guard`]   – harte Regeln (6, 8, 10) für Pfade und ausgehende Texte
//! * [`starter`] – leerer Starter-Vault für Mitschüler
//! * [`obsidian`] – `obsidian://`-Links
//! * [`config`]  – Einstellungen in `%APPDATA%\ausbildungs-cockpit`
//! * [`commands`] – Tauri-Befehle

pub mod commands;
pub mod config;
pub mod error;
pub mod guard;
pub mod index;
pub mod model;
pub mod obsidian;
pub mod starter;
pub mod vault;
pub mod watcher;

use std::path::PathBuf;

use tauri::Manager;

use commands::AppState;

/// Entwicklungs-Builds benutzen einen eigenen Einstellungsordner und öffnen standardmäßig den
/// Testvault `fixtures/vault` – so wird nie versehentlich gegen den echten Vault entwickelt.
fn config_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        config::app_dir().with_file_name(format!("{}-dev", config::APP_DIR_NAME))
    } else {
        config::app_dir()
    }
}

fn dev_fixture_vault() -> Option<PathBuf> {
    if !cfg!(debug_assertions) {
        return None;
    }
    if let Ok(p) = std::env::var("COCKPIT_VAULT") {
        return Some(PathBuf::from(p));
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault");
    p.is_dir().then(|| p.canonicalize().unwrap_or(p))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::load(config_dir());
            let configured = state.config.lock().unwrap().vault_path.clone();
            if let Some(root) = configured.or_else(dev_fixture_vault) {
                if let Err(e) = state.open_vault(app.handle(), strip_verbatim(root)) {
                    eprintln!("[vault] {e}");
                }
            }
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::create_starter_vault,
            commands::set_vault,
            commands::list_folders,
            commands::list_notes,
            commands::read_note,
            commands::open_in_obsidian,
            commands::reindex,
        ])
        .run(tauri::generate_context!())
        .expect("Ausbildungs-Cockpit konnte nicht gestartet werden");
}

/// `canonicalize` liefert unter Windows `\\?\C:\…` – das wollen wir weder anzeigen noch speichern.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}
