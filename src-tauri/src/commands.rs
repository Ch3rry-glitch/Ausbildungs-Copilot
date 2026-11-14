//! Tauri-Befehle für die Oberfläche. M0 ist **rein lesend** – einzige Schreibvorgänge sind die
//! App-Einstellungen (außerhalb des Vaults) und das Anlegen des Starter-Vaults in einen neuen Ordner.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::config::{self, AppConfig};
use crate::error::{AppError, AppResult};
use crate::index::VaultIndex;
use crate::model::{AppStatus, FolderInfo, NoteContent, NoteSummary, StarterReport};
use crate::watcher::{self, SharedIndex, VaultWatcher};
use crate::{obsidian, starter, vault};

pub const EVENT_VAULT_CHANGED: &str = "vault-changed";

pub struct AppState {
    pub config_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    pub index: SharedIndex,
    pub watcher: Mutex<Option<VaultWatcher>>,
}

impl AppState {
    pub fn load(config_dir: PathBuf) -> Self {
        let config = config::load_from(&config_dir).unwrap_or_else(|e| {
            eprintln!("[config] {e} – starte mit Standardwerten");
            AppConfig::default()
        });
        AppState {
            config_dir,
            config: Mutex::new(config),
            index: Arc::new(RwLock::new(None)),
            watcher: Mutex::new(None),
        }
    }

    /// Index aufbauen und Wächter starten. Ersetzt einen vorher geöffneten Vault.
    pub fn open_vault(&self, app: &AppHandle, root: PathBuf) -> AppResult<()> {
        if !root.is_dir() {
            return Err(AppError::NotFound(root.display().to_string()));
        }
        // alten Wächter zuerst stoppen, damit keine Ereignisse in den neuen Index laufen
        *self.watcher.lock().unwrap() = None;
        let idx = VaultIndex::build(&root)?;
        *self.index.write().unwrap() = Some(idx);
        let handle = app.clone();
        let w = watcher::start(root, self.index.clone(), move |ch| {
            let _ = handle.emit(EVENT_VAULT_CHANGED, ch);
        })?;
        *self.watcher.lock().unwrap() = Some(w);
        Ok(())
    }

    fn remember_vault(&self, root: PathBuf) -> AppResult<()> {
        let mut cfg = self.config.lock().unwrap();
        cfg.set_vault(root);
        config::save_to(&self.config_dir, &cfg)
    }

    fn root(&self) -> AppResult<PathBuf> {
        self.index
            .read()
            .unwrap()
            .as_ref()
            .map(|i| i.root().to_path_buf())
            .ok_or(AppError::NoVault)
    }

    pub fn status(&self) -> AppStatus {
        let cfg = self.config.lock().unwrap();
        let idx = self.index.read().unwrap();
        let vault_path = cfg.vault_path.clone().or_else(|| idx.as_ref().map(|i| i.root().to_path_buf()));
        AppStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            vault_name: idx.as_ref().map(|i| obsidian::vault_name(i.root())),
            vault_path: vault_path.map(|p| p.display().to_string()),
            vault_ok: idx.is_some(),
            note_count: idx.as_ref().map(|i| i.len()).unwrap_or(0),
            watching: self.watcher.lock().unwrap().is_some(),
            default_starter_path: starter::default_target().display().to_string(),
            config_dir: self.config_dir.display().to_string(),
        }
    }
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppStatus {
    state.status()
}

/// Legt den Starter-Vault an (Standard: `Dokumente\Ausbildungs-Vault`) und öffnet ihn.
#[tauri::command]
pub async fn create_starter_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    target: Option<String>,
) -> Result<StarterReport, AppError> {
    let target = target
        .filter(|t| !t.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(starter::default_target);
    let report = starter::create(&target)?;
    state.open_vault(&app, target.clone())?;
    state.remember_vault(target)?;
    Ok(report)
}

/// Bestehenden Vault öffnen.
#[tauri::command]
pub async fn set_vault(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<AppStatus, AppError> {
    let root = PathBuf::from(path.trim());
    state.open_vault(&app, root.clone())?;
    state.remember_vault(root)?;
    Ok(state.status())
}

#[tauri::command]
pub fn list_folders(state: State<'_, AppState>) -> Result<Vec<FolderInfo>, AppError> {
    let idx = state.index.read().unwrap();
    Ok(idx.as_ref().ok_or(AppError::NoVault)?.folders())
}

#[tauri::command]
pub fn list_notes(
    state: State<'_, AppState>,
    folder: String,
    recursive: Option<bool>,
) -> Result<Vec<NoteSummary>, AppError> {
    let idx = state.index.read().unwrap();
    Ok(idx.as_ref().ok_or(AppError::NoVault)?.notes_in(&folder, recursive.unwrap_or(false)))
}

/// Liest eine Notiz frisch von der Platte (nicht aus dem Index), damit die Anzeige nie veraltet.
#[tauri::command]
pub fn read_note(state: State<'_, AppState>, path: String) -> Result<NoteContent, AppError> {
    let root = state.root()?;
    let (meta, note) = vault::scan::read_note(&root, &path)?;
    Ok(NoteContent { meta, body: note.body().to_string(), frontmatter_raw: note.yaml() })
}

#[tauri::command]
pub fn open_in_obsidian(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<String, AppError> {
    let root = state.root()?;
    // Pfad prüfen (Regel 8, nichts außerhalb des Vaults), auch wenn wir nur eine URL bauen
    crate::guard::resolve_in_vault(&root, &path)?;
    let uri = obsidian::open_uri(&root, &path);
    app.opener()
        .open_url(uri.clone(), None::<&str>)
        .map_err(|e| AppError::Other(format!("Obsidian konnte nicht geöffnet werden: {e}")))?;
    Ok(uri)
}

#[tauri::command]
pub async fn reindex(app: AppHandle, state: State<'_, AppState>) -> Result<AppStatus, AppError> {
    let root = state.root()?;
    state.open_vault(&app, root)?;
    Ok(state.status())
}
