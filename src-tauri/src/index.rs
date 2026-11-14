//! In-Memory-Index über den Vault. Jederzeit verwerfbar – die Wahrheit sind die Dateien.
//! (SQLite-Cache laut SPEC ist erlaubt, aber für M0 nicht nötig: ein voller Scan dauert bei
//! einigen hundert Notizen Millisekunden.)

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::{AppError, AppResult};
use crate::guard;
use crate::model::{FolderInfo, NoteMeta, NoteSummary, RelPath, VaultChanged};
use crate::vault::scan;

#[derive(Debug)]
pub struct VaultIndex {
    root: PathBuf,
    notes: BTreeMap<RelPath, NoteMeta>,
    folders: BTreeSet<RelPath>,
}

impl VaultIndex {
    pub fn build(root: &Path) -> AppResult<Self> {
        if !root.is_dir() {
            return Err(AppError::NotFound(root.display().to_string()));
        }
        let res = scan::scan_vault(root)?;
        let mut idx = VaultIndex { root: root.to_path_buf(), notes: BTreeMap::new(), folders: res.folders.into_iter().collect() };
        for rel in res.notes {
            // Eine unlesbare Datei darf nicht den ganzen Index verhindern.
            let _ = idx.upsert(&rel);
        }
        Ok(idx)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn get(&self, rel: &str) -> Option<&NoteMeta> {
        self.notes.get(rel)
    }

    pub fn notes(&self) -> impl Iterator<Item = &NoteMeta> {
        self.notes.values()
    }

    /// Notiz (neu) einlesen. Gesperrte Dateien (Obsidian schreibt gerade, Cloud-Sync) werden
    /// kurz wiederholt statt aufzugeben (SPEC §9).
    pub fn upsert(&mut self, rel: &str) -> AppResult<()> {
        let mut last_err = None;
        for attempt in 0..4 {
            match scan::read_note(&self.root, rel) {
                Ok((meta, _)) => {
                    let mut folder = meta.folder.clone();
                    while !folder.is_empty() {
                        self.folders.insert(folder.clone());
                        folder = scan::parent_folder(&folder);
                    }
                    self.notes.insert(rel.to_string(), meta);
                    return Ok(());
                }
                Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                    self.notes.remove(rel);
                    return Err(AppError::Io(e));
                }
                Err(AppError::Io(e)) => {
                    last_err = Some(AppError::Io(e));
                    thread::sleep(Duration::from_millis(50 * (attempt + 1)));
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::Other("unbekannter Lesefehler".into())))
    }

    pub fn remove(&mut self, rel: &str) -> bool {
        self.notes.remove(rel).is_some()
    }

    /// Wendet Dateiereignisse an. Unklare Fälle (Ordner neu/umbenannt/gelöscht) → voller Neuaufbau.
    pub fn apply_fs_events(&mut self, paths: &[PathBuf]) -> AppResult<VaultChanged> {
        let mut changed = BTreeSet::new();
        let mut removed = BTreeSet::new();
        let mut full = false;
        for p in paths {
            let Ok(stripped) = p.strip_prefix(&self.root) else { continue };
            if stripped.as_os_str().is_empty() {
                continue;
            }
            if guard::is_obsidian_config(stripped)
                || stripped.components().any(|c| guard::is_hidden_component(&c.as_os_str().to_string_lossy()))
            {
                continue; // Regel 8: .obsidian-Ereignisse werden nicht einmal angesehen
            }
            let rel = scan::rel_path(&self.root, p);
            if p.is_dir() {
                if !self.folders.contains(&rel) {
                    full = true;
                }
            } else if p.is_file() {
                if scan::is_markdown(p) && self.upsert(&rel).is_ok() {
                    changed.insert(rel);
                }
            } else if self.remove(&rel) {
                removed.insert(rel);
            } else if self.folders.contains(&rel) {
                full = true; // Ordner gelöscht oder umbenannt
            }
        }
        if full {
            *self = VaultIndex::build(&self.root.clone())?;
        }
        Ok(VaultChanged {
            changed: changed.into_iter().collect(),
            removed: removed.into_iter().collect(),
            full_rescan: full,
        })
    }

    /// Alle Ordner (Wurzel zuerst), mit Zahl der Notizen.
    pub fn folders(&self) -> Vec<FolderInfo> {
        let mut direct: BTreeMap<&str, usize> = BTreeMap::new();
        for m in self.notes.values() {
            *direct.entry(m.folder.as_str()).or_default() += 1;
        }
        let total = |folder: &str| {
            if folder.is_empty() {
                return self.notes.len();
            }
            let prefix = format!("{folder}/");
            self.notes.values().filter(|m| m.folder == folder || m.folder.starts_with(&prefix)).count()
        };
        std::iter::once(String::new())
            .chain(self.folders.iter().cloned())
            .map(|path| FolderInfo {
                name: path.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or("Vault").to_string(),
                depth: if path.is_empty() { 0 } else { path.matches('/').count() + 1 },
                note_count: direct.get(path.as_str()).copied().unwrap_or(0),
                total_count: total(&path),
                path,
            })
            .collect()
    }

    /// Notizen eines Ordners, neueste zuerst.
    pub fn notes_in(&self, folder: &str, recursive: bool) -> Vec<NoteSummary> {
        let prefix = format!("{folder}/");
        let mut out: Vec<NoteSummary> = self
            .notes
            .values()
            .filter(|m| m.folder == folder || (recursive && (folder.is_empty() || m.folder.starts_with(&prefix))))
            .map(NoteSummary::from)
            .collect();
        out.sort_by(|a, b| b.datum.cmp(&a.datum).then(b.modified_ms.cmp(&a.modified_ms)).then(a.title.cmp(&b.title)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn index_ueber_testvault() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/vault");
        let idx = VaultIndex::build(&root).unwrap();
        assert!(idx.len() >= 10, "Testvault zu klein: {}", idx.len());
        // Regel 8: nichts aus .obsidian im Index
        assert!(idx.notes().all(|m| !guard::is_obsidian_config(&m.path)));
        assert!(idx.folders().iter().all(|f| !f.path.contains(".obsidian")));
        let unterricht = idx.notes_in("03 Unterricht", false);
        assert!(unterricht.iter().any(|n| n.title == "Unterrichtsnotiz GEC 07.09.2026"));
        let wurzel = &idx.folders()[0];
        assert_eq!((wurzel.path.as_str(), wurzel.total_count), ("", idx.len()));
    }

    #[test]
    fn ereignisse_aktualisieren_den_index() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        fs::write(root.join("03 Unterricht/GEC 07.09.2026.md"), "# a").unwrap();
        let mut idx = VaultIndex::build(root).unwrap();
        assert_eq!(idx.len(), 1);

        fs::write(root.join("03 Unterricht/WSP 07.09.2026.md"), "- [ ] x").unwrap();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join(".obsidian/geheim.md"), "nie lesen").unwrap();
        let ch = idx
            .apply_fs_events(&[root.join("03 Unterricht/WSP 07.09.2026.md"), root.join(".obsidian/geheim.md")])
            .unwrap();
        assert_eq!(ch.changed, vec!["03 Unterricht/WSP 07.09.2026.md"]);
        assert_eq!(idx.len(), 2);

        fs::remove_file(root.join("03 Unterricht/GEC 07.09.2026.md")).unwrap();
        let ch = idx.apply_fs_events(&[root.join("03 Unterricht/GEC 07.09.2026.md")]).unwrap();
        assert_eq!(ch.removed, vec!["03 Unterricht/GEC 07.09.2026.md"]);

        fs::rename(root.join("03 Unterricht"), root.join("03 Schule")).unwrap();
        let ch = idx.apply_fs_events(&[root.join("03 Unterricht"), root.join("03 Schule")]).unwrap();
        assert!(ch.full_rescan);
        assert!(idx.get("03 Schule/WSP 07.09.2026.md").is_some());
    }
}
