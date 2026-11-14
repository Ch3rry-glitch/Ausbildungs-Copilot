//! Dateiwächter: Änderungen aus Obsidian landen in < 2 s im Index (Ziel von M0).
//! Ereignisse werden 300 ms gesammelt, damit ein Speichervorgang nicht zehnmal einliest.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};

use crate::error::{AppError, AppResult};
use crate::index::VaultIndex;
use crate::model::VaultChanged;

pub type SharedIndex = Arc<RwLock<Option<VaultIndex>>>;

pub const DEBOUNCE: Duration = Duration::from_millis(300);

pub struct VaultWatcher {
    root: PathBuf,
    _debouncer: Debouncer<RecommendedWatcher>,
}

impl VaultWatcher {
    pub fn root(&self) -> &PathBuf {
        &self.root
    }
}

/// Startet den Wächter. `on_change` wird nach jeder Index-Aktualisierung aufgerufen
/// (in der App: Ereignis `vault-changed` an die Oberfläche).
pub fn start<F>(root: PathBuf, index: SharedIndex, on_change: F) -> AppResult<VaultWatcher>
where
    F: Fn(VaultChanged) + Send + 'static,
{
    let mut debouncer = new_debouncer(DEBOUNCE, move |res: DebounceEventResult| match res {
        Ok(events) => {
            let paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
            let result = {
                let Ok(mut guard) = index.write() else { return };
                let Some(idx) = guard.as_mut() else { return };
                idx.apply_fs_events(&paths)
            };
            match result {
                Ok(ch) if ch.full_rescan || !ch.changed.is_empty() || !ch.removed.is_empty() => on_change(ch),
                Ok(_) => {}
                Err(e) => eprintln!("[watcher] {e}"),
            }
        }
        Err(e) => eprintln!("[watcher] {e:?}"),
    })
    .map_err(|e| AppError::Watch(e.to_string()))?;

    debouncer
        .watcher()
        .watch(&root, RecursiveMode::Recursive)
        .map_err(|e| AppError::Watch(e.to_string()))?;

    Ok(VaultWatcher { root, _debouncer: debouncer })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn neue_datei_erscheint_in_unter_zwei_sekunden() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        let index: SharedIndex = Arc::new(RwLock::new(Some(VaultIndex::build(&root).unwrap())));
        let (tx, rx) = mpsc::channel();
        let _w = start(root.clone(), index.clone(), move |ch| {
            let _ = tx.send(ch);
        })
        .unwrap();

        // Wächter braucht unter Windows einen Moment zum Anlaufen
        std::thread::sleep(Duration::from_millis(200));
        let t0 = Instant::now();
        fs::write(root.join(".obsidian/workspace.json"), "{}").unwrap();
        fs::write(root.join("03 Unterricht/GEC 09.11.2026.md"), "---\nfach: GEC\n---\n- [ ] neu").unwrap();

        let ch = rx.recv_timeout(Duration::from_secs(2)).expect("keine Änderung innerhalb von 2 s");
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert!(ch.changed.iter().all(|p| !p.contains(".obsidian")));
        let guard = index.read().unwrap();
        let idx = guard.as_ref().unwrap();
        assert!(idx.get("03 Unterricht/GEC 09.11.2026.md").is_some());
        assert!(idx.notes().all(|m| !m.path.contains(".obsidian")));
    }
}
