//! Leerer Starter-Vault für Mitschüler. Steckt per `include_dir` im Programm (keine
//! Ressourcenpfade mit Umlauten im Installer) und wird in einen gewählten Ordner kopiert –
//! **ohne je eine vorhandene Datei zu überschreiben**.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir, DirEntry};

use crate::error::AppResult;
use crate::guard;
use crate::model::StarterReport;

static STARTER: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/starter-vault");

/// Ordner, die leer starten (Git/include_dir kennen keine leeren Ordner).
pub const EMPTY_DIRS: &[&str] = &[
    "04 Betrieb/Wochen",
    "05 Lernen/Zusammenfassungen",
    "07 Material/_Inbox",
    "10 Privat/Noten",
    "10 Privat/Lern-Log",
];

pub const DEFAULT_FOLDER_NAME: &str = "Ausbildungs-Vault";

/// `Dokumente\Ausbildungs-Vault`
pub fn default_target() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DEFAULT_FOLDER_NAME)
}

pub fn file_count() -> usize {
    fn count(d: &Dir<'_>) -> usize {
        d.entries()
            .iter()
            .map(|e| match e {
                DirEntry::Dir(sub) => count(sub),
                DirEntry::File(_) => 1,
            })
            .sum()
    }
    count(&STARTER)
}

pub fn create(target: &Path) -> AppResult<StarterReport> {
    let mut report = StarterReport {
        target: target.display().to_string(),
        created_files: 0,
        skipped_existing: 0,
        created_dirs: 0,
    };
    if !target.exists() {
        fs::create_dir_all(target)?;
        report.created_dirs += 1;
    }
    copy_dir(&STARTER, target, &mut report)?;
    for d in EMPTY_DIRS {
        let p = target.join(d);
        if !p.exists() {
            fs::create_dir_all(&p)?;
            report.created_dirs += 1;
        }
    }
    Ok(report)
}

fn copy_dir(dir: &Dir<'_>, target: &Path, report: &mut StarterReport) -> AppResult<()> {
    for entry in dir.entries() {
        let rel = entry.path();
        if guard::is_obsidian_config(rel) {
            continue; // Regel 8 – sollte nie im Starter-Vault liegen
        }
        let dest = target.join(rel);
        match entry {
            DirEntry::Dir(sub) => {
                if !dest.exists() {
                    fs::create_dir_all(&dest)?;
                    report.created_dirs += 1;
                }
                copy_dir(sub, target, report)?;
            }
            DirEntry::File(f) => {
                // create_new: schlägt fehl, wenn die Datei existiert → nie überschreiben
                match OpenOptions::new().write(true).create_new(true).open(&dest) {
                    Ok(mut file) => {
                        file.write_all(f.contents())?;
                        report.created_files += 1;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => report.skipped_existing += 1,
                    Err(e) => return Err(e.into()),
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_vault_wird_angelegt_und_nie_ueberschrieben() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Ausbildungs-Vault");
        let r = create(&target).unwrap();
        assert_eq!(r.created_files, file_count());
        assert!(r.created_files >= 40);
        assert!(target.join("00 Start/Stundenplan.md").is_file());
        assert!(target.join("99 Meta/Templates/Unterrichtsnotiz.md").is_file());
        for d in EMPTY_DIRS {
            assert!(target.join(d).is_dir(), "{d} fehlt");
        }
        assert!(!target.join(".obsidian").exists(), "Regel 8: kein .obsidian im Starter-Vault");

        // Nutzer ändert eine Datei → zweites Anlegen darf sie nicht anfassen
        let home = target.join("00 Start/Home.md");
        fs::write(&home, "meine Änderung").unwrap();
        let r2 = create(&target).unwrap();
        assert_eq!(r2.created_files, 0);
        assert_eq!(r2.skipped_existing, file_count());
        assert_eq!(fs::read_to_string(&home).unwrap(), "meine Änderung");
    }

    /// Keine Benutzerpfade, keine E-Mail-Adressen. Eigene Wörter (Name, Betrieb …) kommen aus
    /// `STARTER_FORBIDDEN_WORDS="wort1,wort2"`, damit sie nicht selbst im Repository stehen.
    #[test]
    fn starter_vault_enthaelt_keine_persoenlichen_daten() {
        let generic = regex::Regex::new(
            r"(?i)([a-z]:[\\/]users[\\/]|/c/users/|[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,})",
        )
        .unwrap();
        let extra: Vec<String> = std::env::var("STARTER_FORBIDDEN_WORDS")
            .unwrap_or_default()
            .split(',')
            .map(|w| w.trim().to_lowercase())
            .filter(|w| !w.is_empty())
            .collect();
        fn check(d: &Dir<'_>, generic: &regex::Regex, extra: &[String]) {
            for e in d.entries() {
                match e {
                    DirEntry::Dir(sub) => check(sub, generic, extra),
                    DirEntry::File(f) => {
                        let text = String::from_utf8_lossy(f.contents()).to_lowercase();
                        let path = f.path().display();
                        assert!(!generic.is_match(&text), "Benutzerpfad oder E-Mail in {path}");
                        for w in extra {
                            assert!(!text.contains(w.as_str()), "„{w}“ in {path}");
                        }
                    }
                }
            }
        }
        check(&STARTER, &generic, &extra);
    }
}
