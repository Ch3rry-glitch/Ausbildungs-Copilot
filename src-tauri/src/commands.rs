//! Tauri-Befehle für die Oberfläche.
//!
//! Schreiben in den Vault geschieht **nur** über [`crate::write`] (Speichern, Anlegen) – unter
//! einer globalen Schreibsperre, damit zwei Speichervorgänge nie gegeneinander laufen. Jeder
//! Befehl, der die Platte anfasst, ist `async` und arbeitet in `spawn_blocking`: Wiederholungen
//! bei gesperrten Dateien blockieren so nie die Oberfläche, und gelesen wird nie unter der
//! Index-Sperre. Außerhalb des Vaults schreibt die App nur ihre Einstellungen und den Starter-Vault.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use chrono::{NaiveDate, NaiveDateTime};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::config::{self, AppConfig};
use crate::error::{AppError, AppResult};
use crate::index::{self, VaultIndex};
use crate::model::{
    AppStatus, CreateResult, FachInfo, FolderInfo, HeuteTag, KiStatus, LernfeldInfo, LogbuchAnfrage, LogbuchEntwurf,
    LogbuchFach, LogbuchKopieren, LogbuchKopiert, LogbuchStatus, LogbuchStatusReport, LogbuchTextAnfrage,
    LogbuchUebersicht, NeueUnterrichtsnotiz, NoteContent, NoteMeta, NoteSummary, NoteVersion, NotizVorschau, RelPath,
    SaveResult, StarterReport, VaultChanged, WriteReport,
};
use crate::watcher::{self, SharedIndex, VaultWatcher};
use crate::write::{self, RetryPolicy, SaveOutcome, SelfWrites};
use crate::{guard, heute as heute_mod, ki, logbuch, obsidian, starter, unterricht};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub const EVENT_VAULT_CHANGED: &str = "vault-changed";

pub struct AppState {
    pub config_dir: PathBuf,
    pub config: Mutex<AppConfig>,
    pub index: SharedIndex,
    pub watcher: Mutex<Option<VaultWatcher>>,
    /// eigene Schreibvorgänge (Pfad + Hash) – der Wächter meldet sie als `own`
    pub writes: Arc<SelfWrites>,
    /// eine globale Sperre für alle Schreibvorgänge der App
    pub write_lock: Arc<Mutex<()>>,
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
            writes: Arc::new(SelfWrites::new()),
            write_lock: Arc::new(Mutex::new(())),
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
        let folders = idx.folder_paths();
        *self.index.write().unwrap() = Some(idx);
        let handle = app.clone();
        let w = watcher::start(root.clone(), self.index.clone(), self.writes.clone(), move |ch| {
            let _ = handle.emit(EVENT_VAULT_CHANGED, ch);
        })?;
        *self.watcher.lock().unwrap() = Some(w);
        // liegengebliebene eigene Temporärdateien (Absturz) im Hintergrund wegräumen
        std::thread::spawn(move || {
            write::sweep_temp_files(&root, &folders);
        });
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

/// Arbeit im Hintergrund-Thread (Platte, Wiederholungen) – nie im UI-Thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Other(format!("Hintergrundaufgabe fehlgeschlagen: {e}")))?
}

fn emit_changed(app: &AppHandle, ch: VaultChanged) {
    let _ = app.emit(EVENT_VAULT_CHANGED, ch);
}

/// Nach Vault-Wechsel oder Neuaufbau: dasselbe Ereignis mit `fullRescan`.
fn emit_full_rescan(app: &AppHandle) {
    emit_changed(app, VaultChanged { changed: vec![], removed: vec![], own: vec![], full_rescan: true });
}

/// Index lesen; `NoVault`, wenn keiner geöffnet ist.
fn with_index<T>(index: &SharedIndex, f: impl FnOnce(&VaultIndex) -> AppResult<T>) -> AppResult<T> {
    let guard = index.read().unwrap_or_else(|e| e.into_inner());
    f(guard.as_ref().ok_or(AppError::NoVault)?)
}

/// Nach einem eigenen Schreibvorgang: sofort in den Index (nur wenn der Vault nicht gewechselt
/// wurde) und `own` melden. Die Oberfläche verlässt sich nie auf den Wächter.
fn index_own_write(app: &AppHandle, index: &SharedIndex, root: &Path, rel: &str) {
    let meta = index::read_meta(root, rel);
    let same_root = {
        let mut guard = index.write().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some(idx) if idx.root() == root => {
                match meta {
                    Ok(m) => idx.insert_meta(m),
                    Err(e) => eprintln!("[schreiben] {rel} nicht eingelesen: {e}"),
                }
                true
            }
            _ => false,
        }
    };
    if same_root {
        emit_changed(app, VaultChanged { changed: vec![], removed: vec![], own: vec![rel.to_string()], full_rescan: false });
    }
}

/// Entwicklungs-Builds schreiben nur in eine Vault-Kopie unter `%TEMP%` (oder mit
/// `COCKPIT_DEV_WRITE=1`). Release-Builds erlauben immer.
pub fn ensure_write_allowed(root: &Path) -> AppResult<()> {
    #[cfg(debug_assertions)]
    {
        if std::env::var("COCKPIT_DEV_WRITE").as_deref() == Ok("1") {
            return Ok(());
        }
        let erlaubt = match (root.canonicalize(), std::env::temp_dir().canonicalize()) {
            (Ok(r), Ok(t)) => r.starts_with(&t),
            _ => false,
        };
        if !erlaubt {
            return Err(AppError::DevWriteBlocked);
        }
    }
    let _ = root;
    Ok(())
}

/// Jetzt – im Debug-Build über `COCKPIT_NOW=YYYY-MM-DDTHH:MM` vorgebbar (Checkliste).
fn jetzt() -> NaiveDateTime {
    #[cfg(debug_assertions)]
    if let Ok(s) = std::env::var("COCKPIT_NOW") {
        match NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M") {
            Ok(t) => return t,
            Err(_) => eprintln!("[dev] COCKPIT_NOW „{s}“ ignoriert (Format JJJJ-MM-TTTHH:MM)"),
        }
    }
    chrono::Local::now().naive_local()
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
    emit_full_rescan(&app);
    Ok(report)
}

/// Bestehenden Vault öffnen.
#[tauri::command]
pub async fn set_vault(app: AppHandle, state: State<'_, AppState>, path: String) -> Result<AppStatus, AppError> {
    let root = PathBuf::from(path.trim());
    state.open_vault(&app, root.clone())?;
    state.remember_vault(root)?;
    emit_full_rescan(&app);
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

/// Liest eine Notiz frisch von der Platte (nicht aus dem Index), mit Version für den Editor.
#[tauri::command]
pub async fn read_note(state: State<'_, AppState>, path: String) -> Result<NoteContent, AppError> {
    let root = state.root()?;
    blocking(move || write::read_for_edit(&root, &path, &RetryPolicy::standard())).await
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
    emit_full_rescan(&app);
    Ok(state.status())
}

// ---------------------------------------------------------------------------------------------
// M1: Heute und Unterricht
// ---------------------------------------------------------------------------------------------

/// Tagesansicht. `datum` = `JJJJ-MM-TT`, `None` = heute.
#[tauri::command]
pub async fn heute(state: State<'_, AppState>, datum: Option<String>) -> Result<HeuteTag, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    let jetzt = jetzt();
    let datum = match datum.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| AppError::InvalidInput(format!("Ungültiges Datum „{d}“.")))?,
        None => jetzt.date(),
    };
    blocking(move || {
        let (plan, status) = unterricht::load_stundenplan(&root, &RetryPolicy::short());
        with_index(&index, |idx| Ok(heute_mod::heute(idx, plan.as_ref(), status, datum, jetzt)))
    })
    .await
}

#[tauri::command]
pub async fn list_faecher(state: State<'_, AppState>) -> Result<Vec<FachInfo>, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    blocking(move || {
        let (plan, _) = unterricht::load_stundenplan(&root, &RetryPolicy::short());
        with_index(&index, |idx| Ok(unterricht::faecher(idx, plan.as_ref())))
    })
    .await
}

#[tauri::command]
pub fn list_lernfelder(state: State<'_, AppState>) -> Result<Vec<LernfeldInfo>, AppError> {
    with_index(&state.index, |idx| Ok(unterricht::lernfelder(idx)))
}

/// Unterrichtsnotizen, neueste zuerst. `fach` = Kürzel; ein unbekanntes Kürzel filtert nur danach.
#[tauri::command]
pub async fn list_unterricht(state: State<'_, AppState>, fach: Option<String>) -> Result<Vec<NoteSummary>, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    blocking(move || {
        let plan = match fach {
            Some(_) => unterricht::load_stundenplan(&root, &RetryPolicy::short()).0,
            None => None,
        };
        with_index(&index, |idx| {
            let filter = fach.filter(|f| !f.trim().is_empty()).map(|kurz| {
                unterricht::faecher(idx, plan.as_ref())
                    .into_iter()
                    .find(|f| f.kurz.eq_ignore_ascii_case(&kurz))
                    .unwrap_or(FachInfo {
                        link: format!("[[{kurz}]]"),
                        ziel: kurz.clone(),
                        kurz,
                        lehrkraft: None,
                        ziel_existiert: false,
                        notiz_pfad: None,
                        im_stundenplan: false,
                    })
            });
            Ok(unterricht::list_unterricht(idx, filter.as_ref()))
        })
    })
    .await
}

/// Trockenlauf für den Anlegen-Dialog – schreibt nichts.
#[tauri::command]
pub async fn preview_unterricht_note(
    state: State<'_, AppState>,
    req: NeueUnterrichtsnotiz,
) -> Result<NotizVorschau, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    blocking(move || {
        let inputs = unterricht::PlanInputs::read(&root, &RetryPolicy::short())?;
        with_index(&index, |idx| unterricht::plan_note(idx, &inputs, &req))
    })
    .await
}

#[tauri::command]
pub async fn create_unterricht_note(
    app: AppHandle,
    state: State<'_, AppState>,
    req: NeueUnterrichtsnotiz,
) -> Result<CreateResult, AppError> {
    let root = state.root()?;
    ensure_write_allowed(&root)?;
    let (index, writes, lock) = (state.index.clone(), state.writes.clone(), state.write_lock.clone());
    blocking(move || {
        let _schreibsperre = lock.lock().unwrap_or_else(|e| e.into_inner());
        let result = unterricht::create_note(&index, &root, &req, &writes, &RetryPolicy::write())?;
        if let CreateResult::Created { report } = &result {
            let same_root = index.read().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|i| i.root() == root);
            if same_root {
                emit_changed(
                    &app,
                    VaultChanged { changed: vec![], removed: vec![], own: vec![report.rel_path.clone()], full_rescan: false },
                );
            }
        }
        Ok(result)
    })
    .await
}

/// Speichern mit Konfliktschutz. Konflikt und fehlende Datei sind Ergebnisse, keine Fehler.
#[tauri::command]
pub async fn save_note(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    text: String,
    base: NoteVersion,
) -> Result<SaveResult, AppError> {
    let root = state.root()?;
    ensure_write_allowed(&root)?;
    let (index, writes, lock) = (state.index.clone(), state.writes.clone(), state.write_lock.clone());
    blocking(move || {
        let _schreibsperre = lock.lock().unwrap_or_else(|e| e.into_inner());
        let (_, rel) = guard::resolve_for_write(&root, &path)?;
        let outcome = write::save(&root, &rel, &text, &base, &RetryPolicy::write(), &writes)?;
        Ok(match outcome {
            SaveOutcome::Saved { version, warnungen } => {
                index_own_write(&app, &index, &root, &rel);
                SaveResult::Saved {
                    report: WriteReport { rel_path: rel, created: false, version, created_folders: vec![], warnungen },
                }
            }
            SaveOutcome::Unchanged(version) => SaveResult::Unchanged { version },
            SaveOutcome::Conflict { disk_text, disk_version } => SaveResult::Conflict { disk_text, disk_version },
            SaveOutcome::Missing => SaveResult::Missing { rel_path: rel },
        })
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// M2: Logbuch
// ---------------------------------------------------------------------------------------------

impl AppState {
    fn logbuch_konfig(&self) -> Vec<LogbuchFach> {
        self.config.lock().unwrap().logbuch.clone()
    }

    fn ki_modell(&self) -> String {
        ki::modell_oder_standard(&self.config.lock().unwrap().ki.modell)
    }
}

/// Fach-Verzeichnis (Stundenplan + `01 Fächer/`); der Stundenplan wird außerhalb der Sperre gelesen.
fn faecher_lesen(root: &Path, index: &SharedIndex) -> AppResult<Vec<FachInfo>> {
    let (plan, _) = unterricht::load_stundenplan(root, &RetryPolicy::short());
    with_index(index, |idx| Ok(unterricht::faecher(idx, plan.as_ref())))
}

#[tauri::command]
pub async fn logbuch_einstellungen(state: State<'_, AppState>) -> Result<Vec<LogbuchFach>, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    let konfig = state.logbuch_konfig();
    blocking(move || Ok(logbuch::einstellungen_liste(&konfig, &faecher_lesen(&root, &index)?))).await
}

#[tauri::command]
pub async fn logbuch_einstellungen_speichern(
    state: State<'_, AppState>,
    faecher: Vec<LogbuchFach>,
) -> Result<Vec<LogbuchFach>, AppError> {
    let geprueft = logbuch::pruefe_einstellungen(faecher)?;
    {
        let mut cfg = state.config.lock().unwrap();
        let alt = std::mem::replace(&mut cfg.logbuch, geprueft);
        if let Err(e) = config::save_to(&state.config_dir, &cfg) {
            cfg.logbuch = alt;
            return Err(e);
        }
    }
    logbuch_einstellungen(state).await
}

#[tauri::command]
pub async fn logbuch_uebersicht(state: State<'_, AppState>) -> Result<LogbuchUebersicht, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    let konfig = state.logbuch_konfig();
    blocking(move || {
        let faecher = faecher_lesen(&root, &index)?;
        with_index(&index, |idx| Ok(logbuch::uebersicht(idx, &konfig, &faecher)))
    })
    .await
}

/// Konfiguration eines **aktiven** Fachs, sein Linkziel und sein Name für die KI.
fn aktives_fach(konfig: &[LogbuchFach], faecher: &[FachInfo], kurz: &str) -> AppResult<(LogbuchFach, String, String)> {
    let f = logbuch::fach_konfig(konfig, kurz.trim());
    if !f.aktiv {
        return Err(AppError::InvalidInput(format!("Für {} ist kein Logbuch aktiv (Einstellungen → Logbuch).", f.kurz)));
    }
    let ziel = faecher
        .iter()
        .find(|x| x.kurz.eq_ignore_ascii_case(&f.kurz))
        .map(|x| x.ziel.clone())
        .unwrap_or_else(|| f.kurz.clone());
    Ok((f, ziel.clone(), ziel))
}

/// Entwurf erzeugen – mit KI nur auf ausdrücklichen Klick (`mitKi`). Schreibt nichts.
#[tauri::command]
pub async fn logbuch_entwurf(state: State<'_, AppState>, req: LogbuchAnfrage) -> Result<LogbuchEntwurf, AppError> {
    let root = state.root()?;
    let index = state.index.clone();
    let konfig = state.logbuch_konfig();
    let config_dir = state.config_dir.clone();
    let modell = state.ki_modell();
    blocking(move || {
        let faecher = faecher_lesen(&root, &index)?;
        let (cfg, ziel, name) = aktives_fach(&konfig, &faecher, &req.fach)?;
        // ohne KI bleibt alles lokal; mit KI nur, was gesendet werden darf (Regel 10)
        let mit_privat = !req.mit_ki || req.privat_erlauben;
        let (metas, klausuren): (Vec<NoteMeta>, Vec<String>) = with_index(&index, |idx| {
            Ok((
                logbuch::quellnotizen(idx, &cfg.kurz, &ziel, req.datum).into_iter().cloned().collect(),
                logbuch::klausuren(idx, &cfg.kurz, &ziel, req.datum, mit_privat),
            ))
        })?;
        let quellen = logbuch::quellen_lesen(&root, &metas)?;
        if !req.mit_ki {
            return logbuch::entwurf_erstellen(&cfg, &name, req.datum, &quellen, &klausuren, None, req.privat_erlauben);
        }
        let schluessel = ki::schluessel_laden()?.ok_or(AppError::KiKeinSchluessel)?;
        let provider = ki::AnthropicProvider::new(schluessel, &modell)?;
        let system = ki::prompt_logbuch(&config_dir);
        let zugang = logbuch::KiZugang { provider: &provider, system: &system, verlauf_dir: Some(&config_dir) };
        logbuch::entwurf_erstellen(&cfg, &name, req.datum, &quellen, &klausuren, Some(zugang), req.privat_erlauben)
    })
    .await
}

/// Vorschau des Moodle-Textes (ohne Zahl: `__/10`). Schreibt nichts.
#[tauri::command]
pub fn logbuch_text(state: State<'_, AppState>, req: LogbuchTextAnfrage) -> Result<String, AppError> {
    let cfg = logbuch::fach_konfig(&state.logbuch_konfig(), req.fach.trim());
    logbuch::render_text(&cfg, req.datum, &req.abschnitte, req.bewertung)
}

/// Geprüfte Notiz: Pfad auf der Platte, Fach, Datum.
type GeprueftePfade = Vec<(RelPath, Option<String>, Option<NaiveDate>)>;

/// Jeder Pfad muss eine indizierte Unterrichtsnotiz sein (optional: dieses Fachs und Tages).
fn pruefe_pfade(index: &SharedIndex, pfade: &[RelPath], fach_tag: Option<(&str, &str, NaiveDate)>) -> AppResult<GeprueftePfade> {
    if pfade.is_empty() {
        return Err(AppError::InvalidInput("Keine Notiz angegeben.".into()));
    }
    with_index(index, |idx| {
        let mut out = Vec::new();
        for p in pfade {
            let m = idx
                .get(p)
                .or_else(|| idx.notes().find(|m| m.path.eq_ignore_ascii_case(p)))
                .filter(|m| unterricht::is_unterricht_note(m))
                .ok_or_else(|| AppError::InvalidInput(format!("„{p}“ ist keine Unterrichtsnotiz im Vault.")))?;
            if let Some((kurz, ziel, datum)) = fach_tag {
                if !unterricht::fach_matches(m, kurz, ziel) || m.datum != Some(datum) {
                    return Err(AppError::InvalidInput(format!(
                        "„{}“ gehört nicht zu {kurz} am {}.",
                        m.title,
                        datum.format("%d.%m.%Y")
                    )));
                }
            }
            out.push((m.path.clone(), m.fach.clone(), m.datum));
        }
        Ok(out)
    })
}

/// Die Teile des App-Zustands, die ein Statuswechsel braucht.
struct StatusZiel {
    index: SharedIndex,
    root: PathBuf,
    writes: Arc<SelfWrites>,
    lock: Arc<Mutex<()>>,
    config_dir: PathBuf,
}

impl StatusZiel {
    fn von(state: &AppState, root: PathBuf) -> Self {
        StatusZiel {
            index: state.index.clone(),
            root,
            writes: state.writes.clone(),
            lock: state.write_lock.clone(),
            config_dir: state.config_dir.clone(),
        }
    }

    /// Status schreiben (unter der globalen Schreibsperre) und sofort in den Index übernehmen.
    fn schreiben(&self, app: &AppHandle, pfade: &[RelPath], status: LogbuchStatus, nur_hoeher: bool) -> LogbuchStatusReport {
        let _schreibsperre = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let backups = self.config_dir.join("backups");
        let report =
            logbuch::status_setzen(&self.root, pfade, status, nur_hoeher, &self.writes, Some(&backups), &RetryPolicy::write());
        for w in &report.geschrieben {
            index_own_write(app, &self.index, &self.root, &w.rel_path);
        }
        report
    }
}

/// „Kopieren und Moodle öffnen“: Text rendern (Regel 2), in die Zwischenablage, Moodle im Browser,
/// Status `kopiert`. Der Text wird danach verworfen – nirgends gespeichert (Regel 1).
#[tauri::command]
pub async fn logbuch_kopieren(app: AppHandle, state: State<'_, AppState>, req: LogbuchKopieren) -> Result<LogbuchKopiert, AppError> {
    let root = state.root()?;
    ensure_write_allowed(&root)?;
    let konfig = state.logbuch_konfig();
    let ziel_status = StatusZiel::von(&state, root);
    blocking(move || {
        let faecher = faecher_lesen(&ziel_status.root, &ziel_status.index)?;
        let (cfg, ziel, _) = aktives_fach(&konfig, &faecher, &req.fach)?;
        let text = logbuch::kopiertext(&cfg, req.datum, &req.abschnitte, req.bewertung)?;
        let pfade: Vec<RelPath> = pruefe_pfade(&ziel_status.index, &req.pfade, Some((&cfg.kurz, &ziel, req.datum)))?
            .into_iter()
            .map(|(p, _, _)| p)
            .collect();
        app.clipboard()
            .write_text(logbuch::fuer_zwischenablage(&text))
            .map_err(|e| AppError::Zwischenablage(e.to_string()))?;
        drop(text);
        let url = logbuch::moodle_url(&cfg);
        let moodle_fehler =
            app.opener().open_url(url.clone(), None::<&str>).err().map(|e| format!("Moodle ließ sich nicht öffnen: {e}"));
        let status = ziel_status.schreiben(&app, &pfade, LogbuchStatus::Kopiert, true);
        if let Err(e) = logbuch::verlauf_anhaengen(&ziel_status.config_dir, &cfg.kurz, req.datum, LogbuchStatus::Kopiert) {
            eprintln!("[logbuch] Verlauf nicht geschrieben: {e}");
        }
        Ok(LogbuchKopiert { moodle_url: url, moodle_geoeffnet: moodle_fehler.is_none(), moodle_fehler, status })
    })
    .await
}

/// Status von Hand setzen (z. B. `abgegeben`).
#[tauri::command]
pub async fn logbuch_status_setzen(
    app: AppHandle,
    state: State<'_, AppState>,
    pfade: Vec<RelPath>,
    status: LogbuchStatus,
) -> Result<LogbuchStatusReport, AppError> {
    let root = state.root()?;
    ensure_write_allowed(&root)?;
    let ziel_status = StatusZiel::von(&state, root);
    blocking(move || {
        let geprueft = pruefe_pfade(&ziel_status.index, &pfade, None)?;
        let liste: Vec<RelPath> = geprueft.iter().map(|(p, _, _)| p.clone()).collect();
        let report = ziel_status.schreiben(&app, &liste, status, false);
        // Verlauf: je Fach und Tag eine Zeile, wenn sich dort etwas geändert hat
        let mut gemeldet: Vec<(String, NaiveDate)> = Vec::new();
        for (p, fach, datum) in &geprueft {
            let (Some(f), Some(d)) = (fach, datum) else { continue };
            let geaendert = report.geschrieben.iter().any(|w| w.rel_path.eq_ignore_ascii_case(p));
            if geaendert && !gemeldet.iter().any(|(gf, gd)| gf == f && gd == d) {
                gemeldet.push((f.clone(), *d));
                if let Err(e) = logbuch::verlauf_anhaengen(&ziel_status.config_dir, f, *d, status) {
                    eprintln!("[logbuch] Verlauf nicht geschrieben: {e}");
                }
            }
        }
        Ok(report)
    })
    .await
}

fn ki_status_jetzt(state: &AppState) -> KiStatus {
    let (gespeichert, fehler) = match ki::schluessel_laden() {
        Ok(s) => (s.is_some(), None),
        Err(e) => (false, Some(e.to_string())),
    };
    KiStatus { schluessel_gespeichert: gespeichert, modell: state.ki_modell(), modelle: ki::modelle(), fehler }
}

#[tauri::command]
pub async fn ki_status(state: State<'_, AppState>) -> Result<KiStatus, AppError> {
    Ok(ki_status_jetzt(&state))
}

/// Schlüssel in den Windows-Anmeldeinformationsspeicher. Er geht nie an die Oberfläche zurück.
#[tauri::command]
pub async fn ki_schluessel_speichern(state: State<'_, AppState>, schluessel: String) -> Result<KiStatus, AppError> {
    ki::schluessel_speichern(&schluessel)?;
    Ok(ki_status_jetzt(&state))
}

#[tauri::command]
pub async fn ki_schluessel_loeschen(state: State<'_, AppState>) -> Result<KiStatus, AppError> {
    ki::schluessel_loeschen()?;
    Ok(ki_status_jetzt(&state))
}

#[tauri::command]
pub async fn ki_modell_setzen(state: State<'_, AppState>, modell: String) -> Result<KiStatus, AppError> {
    let modell = modell.trim().to_string();
    if !ki::modelle().iter().any(|m| m.id == modell) {
        return Err(AppError::InvalidInput(format!("Unbekanntes Modell „{modell}“.")));
    }
    {
        let mut cfg = state.config.lock().unwrap();
        cfg.ki.modell = modell;
        config::save_to(&state.config_dir, &cfg)?;
    }
    Ok(ki_status_jetzt(&state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schreiben_nur_in_temp_kopie() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ensure_write_allowed(dir.path()).is_ok());
        if std::env::var("COCKPIT_DEV_WRITE").as_deref() != Ok("1") {
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/vault");
            assert!(matches!(ensure_write_allowed(&fixtures), Err(AppError::DevWriteBlocked)));
        }
    }

    #[test]
    fn ergebnisse_im_vertrag() {
        let v = NoteVersion { sha256: "ab".into(), modified_ms: 1, size: 2 };
        let s = serde_json::to_value(SaveResult::Conflict { disk_text: "x".into(), disk_version: v.clone() }).unwrap();
        assert_eq!(
            s,
            serde_json::json!({"status": "conflict", "diskText": "x", "diskVersion": {"sha256": "ab", "modifiedMs": 1, "size": 2}})
        );
        let s = serde_json::to_value(SaveResult::Missing { rel_path: "a.md".into() }).unwrap();
        assert_eq!(s, serde_json::json!({"status": "missing", "relPath": "a.md"}));
        let r = WriteReport { rel_path: "a.md".into(), created: true, version: v, created_folders: vec![], warnungen: vec![] };
        let s = serde_json::to_value(CreateResult::Created { report: r }).unwrap();
        assert_eq!(s["status"], "created");
        assert_eq!(s["report"]["relPath"], "a.md");
        assert_eq!(s["report"]["createdFolders"], serde_json::json!([]));
        let ch = serde_json::to_value(VaultChanged { changed: vec![], removed: vec![], own: vec!["a.md".into()], full_rescan: false })
            .unwrap();
        assert_eq!(ch, serde_json::json!({"changed": [], "removed": [], "own": ["a.md"], "fullRescan": false}));
        let le = serde_json::to_value(crate::model::LineEnding::Crlf).unwrap();
        assert_eq!(le, "crlf");
        let art = serde_json::to_value(crate::model::TagArt::Betriebstag).unwrap();
        assert_eq!(art, "betriebstag");
        let req: NeueUnterrichtsnotiz =
            serde_json::from_value(serde_json::json!({"datum": "2026-11-16", "fach": "GEC", "block": 1, "lernfeld": null, "thema": null}))
                .unwrap();
        assert_eq!(req.block, 1);
    }
}
