//! Ausbildungs-Cockpit – Rust-Backend.
//!
//! Aufbau:
//! * [`model`]   – Datenmodelle (Rust ↔ Oberfläche)
//! * [`vault`]   – Markdown verstehen: Scan, Frontmatter, Aufgaben, Tags, Links, Namen, Stundenplan
//! * [`index`]   – In-Memory-Index, jederzeit verwerfbar
//! * [`watcher`] – Dateiwächter (`notify`)
//! * [`guard`]   – harte Regeln (6, 8, 10) für Pfade und ausgehende Texte
//! * [`write`]   – der einzige Schreibweg in den Vault (atomar, Konfliktschutz, eigene Schreibvorgänge)
//! * [`unterricht`] – Unterrichtsnotizen: Vorlage ausfüllen, Fächer, Lernfelder, anlegen
//! * [`heute`]   – Tagesansicht: Blöcke, Notizen, fällige Aufgaben
//! * [`starter`] – leerer Starter-Vault für Mitschüler
//! * [`obsidian`] – `obsidian://`-Links
//! * [`config`]  – Einstellungen in `%APPDATA%\ausbildungs-cockpit`
//! * [`logbuch`] – Logbucheinträge für Moodle: Entwurf, Text, Status (nur `logbuch:` im Vault)
//! * [`ki`]      – KI hinter `LlmProvider` (Anthropic), Schlüssel im Anmeldeinformationsspeicher
//! * [`aufgaben`] – Arbeitsaufträge und Lernaufgaben wie `00 Start/Aufgaben.md`, Abhaken, gelöst
//! * [`fragen`]  – `#frage` / `#nochmal`, „geklärt“ (durchstreichen oder in „Geklärt“ verschieben)
//! * [`commands`] – Tauri-Befehle

pub mod aufgaben;
pub mod commands;
pub mod config;
pub mod error;
pub mod fragen;
pub mod guard;
pub mod heute;
pub mod index;
pub mod ki;
pub mod logbuch;
pub mod model;
pub mod obsidian;
pub mod starter;
pub mod unterricht;
pub mod vault;
pub mod watcher;
pub mod write;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tauri::Manager;

use commands::AppState;

/// Entwicklungs-Builds benutzen einen eigenen Einstellungsordner und öffnen standardmäßig eine
/// **Kopie** des Testvaults `fixtures/vault` unter `%TEMP%` – so wird nie versehentlich gegen den
/// echten Vault oder in die Fixtures geschrieben.
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
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault");
    if !src.is_dir() {
        return None;
    }
    match dev_vault_copy(&src, &std::env::temp_dir()) {
        Ok(p) => {
            let p = p.canonicalize().unwrap_or(p);
            eprintln!("[dev] Testvault-Kopie: {}", strip_verbatim(p.clone()).display());
            Some(p)
        }
        Err(e) => {
            eprintln!("[dev] Testvault-Kopie fehlgeschlagen: {e}");
            None
        }
    }
}

/// Dateien des Testvaults (relativ, sortiert) – ohne alles, was mit `.` beginnt (Regel 8).
fn fixture_files(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>, dirs: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let ft = entry.file_type()?;
        let path = entry.path();
        let rel = vault::scan::rel_path(root, &path);
        if ft.is_dir() {
            dirs.push(rel);
            fixture_files(root, &path, files, dirs)?;
        } else if ft.is_file() {
            files.push((rel, path));
        }
    }
    Ok(())
}

/// Kopiert den Testvault nach `<temp>/ausbildungs-cockpit-dev-vault-<hash>`. Der Hash (8 Hex)
/// läuft über die sortierten (Pfad, Bytes) – geänderte Fixtures ergeben eine frische Kopie.
/// Vorhandene Kopien werden wiederverwendet und nie gelöscht.
pub fn dev_vault_copy(src: &Path, temp: &Path) -> io::Result<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = Vec::new();
    fixture_files(src, src, &mut files, &mut dirs)?;
    files.sort();
    dirs.sort();
    let mut hasher = Sha256::new();
    let mut contents = Vec::with_capacity(files.len());
    for (rel, path) in &files {
        let bytes = fs::read(path)?;
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
        contents.push(bytes);
    }
    let hash: String = hasher.finalize().iter().take(4).map(|b| format!("{b:02x}")).collect();
    let target = temp.join(format!("ausbildungs-cockpit-dev-vault-{hash}"));
    if target.exists() {
        return Ok(target);
    }
    // erst vollständig in einen Zwischenordner, dann umbenennen – nie eine halbe Kopie benutzen
    let partial = temp.join(format!("ausbildungs-cockpit-dev-vault-{hash}.partial-{}", std::process::id()));
    for d in &dirs {
        fs::create_dir_all(partial.join(d))?;
    }
    for ((rel, _), bytes) in files.iter().zip(&contents) {
        let p = partial.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(p, bytes)?;
    }
    fs::create_dir_all(&partial)?;
    match fs::rename(&partial, &target) {
        Ok(()) => Ok(target),
        // gleichzeitig von einem zweiten Prozess angelegt
        Err(_) if target.exists() => Ok(target),
        Err(e) => Err(e),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = AppState::load(config_dir());
            let configured = state.config.lock().unwrap().vault_path.clone();
            if cfg!(debug_assertions) {
                if let Some(v) = &configured {
                    let temp = std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir());
                    let unter_temp = v.canonicalize().map(|c| c.starts_with(&temp)).unwrap_or(false);
                    if !unter_temp {
                        eprintln!("[dev] Dev-Build: gespeicherter Vault liegt nicht unter %TEMP% – Schreiben wird mit DevWriteBlocked abgelehnt.");
                    }
                }
            }
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
            commands::heute,
            commands::list_faecher,
            commands::list_lernfelder,
            commands::list_unterricht,
            commands::preview_unterricht_note,
            commands::create_unterricht_note,
            commands::save_note,
            commands::logbuch_einstellungen,
            commands::logbuch_einstellungen_speichern,
            commands::logbuch_uebersicht,
            commands::logbuch_entwurf,
            commands::logbuch_text,
            commands::logbuch_kopieren,
            commands::logbuch_status_setzen,
            commands::ki_status,
            commands::ki_schluessel_speichern,
            commands::ki_schluessel_loeschen,
            commands::ki_modell_setzen,
            commands::aufgaben_uebersicht,
            commands::aufgabe_abhaken,
            commands::lernaufgabe_geloest,
            commands::fragen_uebersicht,
            commands::frage_klaeren,
            commands::fragen_einstellungen,
            commands::fragen_einstellungen_speichern,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_kopie_mit_hash_und_ohne_obsidian() {
        let src = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(src.path().join("03 Unterricht")).unwrap();
        fs::create_dir_all(src.path().join("07 Material/_Inbox")).unwrap();
        fs::create_dir_all(src.path().join(".obsidian")).unwrap();
        fs::write(src.path().join("03 Unterricht/a.md"), "a").unwrap();
        fs::write(src.path().join(".obsidian/app.json"), "{}").unwrap();
        fs::write(src.path().join("07 Material/_Inbox/.gitkeep"), "").unwrap();

        let a = dev_vault_copy(src.path(), temp.path()).unwrap();
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("ausbildungs-cockpit-dev-vault-") && name.len() == 30 + 8, "{name}");
        assert_eq!(fs::read_to_string(a.join("03 Unterricht/a.md")).unwrap(), "a");
        assert!(a.join("07 Material/_Inbox").is_dir());
        assert!(!a.join(".obsidian").exists());
        assert!(!a.join("07 Material/_Inbox/.gitkeep").exists());
        // gleiche Fixtures → gleiche Kopie; geänderte → neue, alte bleibt
        assert_eq!(dev_vault_copy(src.path(), temp.path()).unwrap(), a);
        fs::write(src.path().join("03 Unterricht/a.md"), "b").unwrap();
        let b = dev_vault_copy(src.path(), temp.path()).unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir());
        // .obsidian-Inhalt ändert den Hash nicht (wird nie gelesen)
        fs::write(src.path().join(".obsidian/app.json"), "{\"x\":1}").unwrap();
        assert_eq!(dev_vault_copy(src.path(), temp.path()).unwrap(), b);
    }
}
