//! Wächter für die harten Regeln aus SPEC §3, soweit sie Pfade und ausgehende Texte betreffen.
//!
//! * Regel 6  – interne Einkaufspreise (EK) verlassen die App nie → [`strip_purchase_prices`]
//! * Regel 8  – `.obsidian/` wird nie gelesen oder geschrieben → [`resolve_in_vault`], [`is_obsidian_config`]
//! * Regel 10 – `10 Privat/` ist standardmäßig vom KI-Versand ausgeschlossen → [`select_for_ai`]
//!
//! Jeder Dateizugriff auf den Vault läuft über [`resolve_in_vault`].

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::model::RelPath;

pub const OBSIDIAN_DIR: &str = ".obsidian";
pub const PRIVATE_DIR: &str = "10 Privat";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    #[error("„.obsidian/“ wird nie gelesen oder geschrieben ({0}).")]
    ObsidianConfig(String),
    #[error("Pfad liegt außerhalb des Vaults ({0}).")]
    OutsideVault(String),
    #[error("„10 Privat/“ ist vom KI-Versand ausgeschlossen ({0}). Nur pro Vorgang ausdrücklich einschaltbar.")]
    Private(String),
    #[error("Ungültiger Dateiname ({0}).")]
    InvalidName(String),
    #[error("Nur Markdown-Dateien (.md) werden geöffnet oder geschrieben ({0}).")]
    NotMarkdown(String),
    #[error("Versteckte Dateien und Ordner werden nicht geöffnet oder geschrieben ({0}).")]
    Hidden(String),
    #[error("Verknüpfungen (Symlink oder Junction) werden nicht geöffnet oder geschrieben ({0}).")]
    Link(String),
}

/// Liegt der (relative oder absolute) Pfad in einem `.obsidian`-Ordner?
pub fn is_obsidian_config(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .components()
        .any(|c| matches!(c, Component::Normal(n) if n.eq_ignore_ascii_case(OBSIDIAN_DIR)))
}

/// Versteckte Ordner/Dateien (`.obsidian`, `.trash`, `.git` …) – werden beim Scannen übersprungen.
pub fn is_hidden_component(name: &str) -> bool {
    name.starts_with('.')
}

/// Liegt der vault-relative Pfad unter `10 Privat/`?
pub fn is_private(rel: impl AsRef<Path>) -> bool {
    rel.as_ref()
        .components()
        .find_map(|c| match c {
            Component::Normal(n) => Some(n.to_string_lossy().eq_ignore_ascii_case(PRIVATE_DIR)),
            _ => None,
        })
        .unwrap_or(false)
}

/// Macht aus einem vault-relativen Pfad einen absoluten – oder verweigert:
/// keine absoluten Pfade, kein `..`, nichts in `.obsidian/`.
pub fn resolve_in_vault(root: &Path, rel: &str) -> Result<PathBuf, GuardError> {
    let rel_path = Path::new(rel);
    if is_obsidian_config(rel_path) {
        return Err(GuardError::ObsidianConfig(rel.to_string()));
    }
    let mut out = root.to_path_buf();
    for c in rel_path.components() {
        match c {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(GuardError::OutsideVault(rel.to_string()))
            }
        }
    }
    Ok(out)
}

/// Pfad einer Notiz zum **Lesen** im Editor prüfen. Gleiche Prüfungen wie [`resolve_for_write`].
/// Liefert (absoluter kanonischer Pfad, relativer Pfad in der Schreibweise auf der Platte).
pub fn resolve_for_read(root: &Path, rel: &str) -> Result<(PathBuf, RelPath), GuardError> {
    resolve_checked(root, rel)
}

/// Pfad einer Notiz zum **Schreiben** prüfen (Speichern, Anlegen). Schließt neben `..` und
/// `.obsidian` auch 8.3-Kurznamen, Junctions und Symlinks aus (Regel 8).
pub fn resolve_for_write(root: &Path, rel: &str) -> Result<(PathBuf, RelPath), GuardError> {
    resolve_checked(root, rel)
}

/// Windows-Gerätenamen, die als Dateiname nicht funktionieren.
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
    "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// `.obsidian` in jeder Schreibweise, auch mit angehängten Punkten/Leerzeichen (`.obsidian.`).
fn is_obsidian_name(name: &str) -> bool {
    name.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(OBSIDIAN_DIR)
}

/// Sieht aus wie ein 8.3-Kurzname (`OBSIDI~1`)? Echte Notizen heißen nie so.
fn looks_like_short_name(name: &str) -> bool {
    let chars: Vec<char> = name.chars().collect();
    chars.len() <= 12 && chars.windows(2).any(|w| w[0] == '~' && w[1].is_ascii_digit())
}

/// Regel-8- und Versteckt-Prüfung für eine Liste von Pfadteilen.
fn check_obsidian_and_hidden<'a>(
    comps: impl Iterator<Item = &'a str> + Clone,
    rel: &str,
) -> Result<(), GuardError> {
    if comps.clone().any(is_obsidian_name) {
        return Err(GuardError::ObsidianConfig(rel.to_string()));
    }
    if comps.into_iter().any(|c| c.starts_with('.') && c != "." && c != "..") {
        return Err(GuardError::Hidden(rel.to_string()));
    }
    Ok(())
}

/// Lexikalische Prüfungen (ohne Dateisystem), Schritte 1–5.
fn check_lexical(root: &Path, rel: &str) -> Result<PathBuf, GuardError> {
    let invalid = || GuardError::InvalidName(rel.to_string());
    if rel.trim().is_empty() || rel.ends_with(['/', '\\']) {
        return Err(invalid());
    }
    let comps: Vec<&str> = rel.split(['/', '\\']).collect();
    // `.`, `..` und leere Teile (führendes `/`) prüft resolve_in_vault (→ OutsideVault)
    let named = comps.iter().copied().filter(|c| !c.is_empty() && *c != "." && *c != "..");
    check_obsidian_and_hidden(named.clone(), rel)?;
    for c in named {
        if c.ends_with(['.', ' ']) {
            return Err(invalid());
        }
        if c.chars().any(|ch| "<>:\"|?*".contains(ch) || (ch as u32) < 0x20) {
            return Err(invalid());
        }
        let stem = c.split('.').next().unwrap_or(c);
        if RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
            return Err(invalid());
        }
        if looks_like_short_name(c) {
            return Err(invalid());
        }
    }
    let last = comps.last().copied().unwrap_or("");
    let is_md = last.len() > 3 && last.is_char_boundary(last.len() - 3) && last[last.len() - 3..].eq_ignore_ascii_case(".md");
    if !is_md {
        return Err(GuardError::NotMarkdown(rel.to_string()));
    }
    resolve_in_vault(root, rel)
}

fn resolve_checked(root: &Path, rel: &str) -> Result<(PathBuf, RelPath), GuardError> {
    let lexical = check_lexical(root, rel)?;
    let outside = || GuardError::OutsideVault(rel.to_string());

    // 6. Anker = Ziel oder nächster vorhandener Vorfahr; kanonisch muss er im Vault liegen.
    let r = root.canonicalize().map_err(|_| outside())?;
    let tail: Vec<String> = lexical
        .strip_prefix(root)
        .map_err(|_| outside())?
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let mut existing = tail.len();
    while existing > 0 {
        let p = tail[..existing].iter().fold(root.to_path_buf(), |acc, c| acc.join(c));
        if fs::symlink_metadata(&p).is_ok() {
            break;
        }
        existing -= 1;
    }
    let anchor = tail[..existing].iter().fold(root.to_path_buf(), |acc, c| acc.join(c));
    let c = anchor.canonicalize().map_err(|_| outside())?;
    let canon_rel = c.strip_prefix(&r).map_err(|_| outside())?;
    let canon_comps: Vec<String> =
        canon_rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();

    // 7. Regel 8 und Versteckt noch einmal auf den echten Namen (Junction → .obsidian, Kurznamen)
    check_obsidian_and_hidden(canon_comps.iter().map(String::as_str), rel)?;

    // 8. Keine Symlinks/Junctions – weder das Ziel noch ein Ordner darüber (unterhalb des Vaults).
    //    `is_symlink` ist für Cloud-Platzhalter (OneDrive) falsch, die bleiben also schreibbar.
    for i in 1..=existing {
        let p = tail[..i].iter().fold(root.to_path_buf(), |acc, c| acc.join(c));
        if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(GuardError::Link(rel.to_string()));
        }
    }

    // 9. Schreibweise auf der Platte + restliche (noch nicht vorhandene) Teile
    let on_disk: RelPath = canon_comps.iter().chain(tail[existing..].iter()).cloned().collect::<Vec<_>>().join("/");
    let abs = on_disk.split('/').fold(r.clone(), |acc, c| acc.join(c));
    Ok((abs, on_disk))
}

/// Prüft, welche Dateien an die KI gehen dürfen. `allow_private` gilt nur für **diesen** Vorgang.
pub fn select_for_ai(files: &[String], allow_private: bool) -> Result<Vec<&str>, GuardError> {
    let mut out = Vec::new();
    for f in files {
        if is_obsidian_config(f) {
            return Err(GuardError::ObsidianConfig(f.clone()));
        }
        if is_private(f) && !allow_private {
            continue;
        }
        out.push(f.as_str());
    }
    Ok(out)
}

/// Frontmatter-Schlüssel, die EK enthalten (`ek`, `einkaufspreis`, `ek_preis`, `ek-netto` …).
static EK_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\s*(ek([_\-]\w+)?|einkaufspreis\w*)\s*:").unwrap()
});
/// Fließtext mit „EK:“, „EK =“, „Einkaufspreis: …“, „EK-Preis 12 €“.
static EK_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(\bEK\b[\s\-]*(preis|netto|brutto)?\s*[:=]|\bEinkaufspreis(e)?\b|\bEK-Preis)").unwrap()
});

/// Entfernt jede Zeile, die einen Einkaufspreis enthält (Regel 6). Lieber eine Zeile zu viel
/// weg als ein EK in einem KI-Prompt oder Kundentext.
pub fn strip_purchase_prices(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if EK_KEY.is_match(line) || EK_TEXT.is_match(line) {
            continue;
        }
        out.push_str(line);
    }
    out
}

pub fn contains_purchase_price(text: &str) -> bool {
    text.lines().any(|l| EK_KEY.is_match(l) || EK_TEXT.is_match(l))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Regel 8 ----
    #[test]
    fn regel8_obsidian_ordner_wird_verweigert() {
        let root = Path::new("C:/Vault");
        assert_eq!(
            resolve_in_vault(root, ".obsidian/app.json"),
            Err(GuardError::ObsidianConfig(".obsidian/app.json".into()))
        );
        assert!(resolve_in_vault(root, "03 Unterricht/.obsidian/x.json").is_err());
        assert!(resolve_in_vault(root, ".OBSIDIAN/workspace.json").is_err());
        assert!(is_obsidian_config("C:/Vault/.obsidian/plugins/x/main.js"));
        assert!(!is_obsidian_config("03 Unterricht/obsidian-tipps.md"));
    }

    #[test]
    fn pfade_ausserhalb_des_vaults_werden_verweigert() {
        let root = Path::new("C:/Vault");
        assert!(matches!(resolve_in_vault(root, "../geheim.md"), Err(GuardError::OutsideVault(_))));
        assert!(matches!(resolve_in_vault(root, "03 Unterricht/../../x.md"), Err(GuardError::OutsideVault(_))));
        assert!(matches!(resolve_in_vault(root, "C:/Windows/win.ini"), Err(GuardError::OutsideVault(_))));
        assert_eq!(
            resolve_in_vault(root, "03 Unterricht/GEC 07.09.2026.md").unwrap(),
            Path::new("C:/Vault/03 Unterricht/GEC 07.09.2026.md")
        );
    }

    #[test]
    fn schreibpfade_werden_geprueft() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        let abgelehnt = [
            ".obsidian/app.json",
            ".OBSIDIAN/x.md",
            ".obsidian./app.json",
            "a/.obsidian/x.md",
            "../x.md",
            "a/../../x.md",
            "C:/x.md",
            "/x.md",
            "",
            "03 Unterricht/",
            ".trash/x.md",
            "x.md:stream",
            "CON.md",
            "nul.md",
            "a /x.md",
            "a./x.md",
            "x.txt",
            "GEC 16.11.2026.md ",
            "OBSIDI~1/x.md",
            "OBSIDI~1/app.json",
            "03 Unterricht\\..\\..\\x.md",
        ];
        for rel in abgelehnt {
            assert!(resolve_for_write(root, rel).is_err(), "Schreiben: {rel:?}");
            assert!(resolve_for_read(root, rel).is_err(), "Lesen: {rel:?}");
        }
        assert!(matches!(resolve_for_write(root, ".obsidian/app.json"), Err(GuardError::ObsidianConfig(_))));
        assert!(matches!(resolve_for_write(root, ".obsidian./app.json"), Err(GuardError::ObsidianConfig(_))));
        assert!(matches!(resolve_for_write(root, ".trash/x.md"), Err(GuardError::Hidden(_))));
        assert!(matches!(resolve_for_write(root, "../x.md"), Err(GuardError::OutsideVault(_))));
        assert!(matches!(resolve_for_write(root, "x.txt"), Err(GuardError::NotMarkdown(_))));
        assert!(matches!(resolve_for_write(root, "OBSIDI~1/x.md"), Err(GuardError::InvalidName(_))));
        assert!(matches!(resolve_for_write(root, "CON.md"), Err(GuardError::InvalidName(_))));

        let (abs, rel) = resolve_for_write(root, "03 Unterricht/GEC 16.11.2026.md").unwrap();
        assert_eq!(rel, "03 Unterricht/GEC 16.11.2026.md");
        assert!(abs.ends_with("03 Unterricht/GEC 16.11.2026.md"));
        assert!(abs.starts_with(root.canonicalize().unwrap()));
        let (_, rel) = resolve_for_read(root, "10 Privat/Noten/x.md").unwrap();
        assert_eq!(rel, "10 Privat/Noten/x.md");
        let (_, rel) = resolve_for_write(root, "03 Unterricht\\GEC.MD").unwrap();
        assert_eq!(rel, "03 Unterricht/GEC.MD");
    }

    #[cfg(windows)]
    fn mklink_junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success())
    }

    #[cfg(windows)]
    #[test]
    fn junction_nach_obsidian() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        fs::write(root.join(".obsidian/falle.md"), "nie lesen").unwrap();
        fs::write(outside.path().join("y.md"), "draußen").unwrap();
        assert!(mklink_junction(&root.join("03 Unterricht/cfg"), &root.join(".obsidian")), "mklink /J fehlgeschlagen");
        assert!(mklink_junction(&root.join("03 Unterricht/raus"), outside.path()), "mklink /J fehlgeschlagen");
        assert!(matches!(resolve_for_write(root, "03 Unterricht/cfg/x.md"), Err(GuardError::ObsidianConfig(_))));
        assert!(matches!(resolve_for_read(root, "03 Unterricht/cfg/falle.md"), Err(GuardError::ObsidianConfig(_))));
        assert!(matches!(resolve_for_write(root, "03 Unterricht/raus/x.md"), Err(GuardError::OutsideVault(_))));
        assert!(matches!(resolve_for_read(root, "03 Unterricht/raus/y.md"), Err(GuardError::OutsideVault(_))));
        // Junction innerhalb des Vaults auf einen normalen Ordner: ebenfalls abgelehnt (R11)
        fs::create_dir_all(root.join("04 Betrieb")).unwrap();
        assert!(mklink_junction(&root.join("03 Unterricht/betrieb"), &root.join("04 Betrieb")));
        assert!(matches!(resolve_for_write(root, "03 Unterricht/betrieb/x.md"), Err(GuardError::Link(_))));
        // Junctions mit rmdir entfernen (nie rekursiv durch den Link löschen)
        for j in ["cfg", "raus", "betrieb"] {
            fs::remove_dir(root.join("03 Unterricht").join(j)).unwrap();
        }
        assert_eq!(fs::read_to_string(root.join(".obsidian/falle.md")).unwrap(), "nie lesen");
    }

    #[cfg(windows)]
    #[test]
    fn symlink_ziel() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        fs::write(root.join("03 Unterricht/echt.md"), "x").unwrap();
        let link = root.join("03 Unterricht/link.md");
        if let Err(e) = std::os::windows::fs::symlink_file(root.join("03 Unterricht/echt.md"), &link) {
            eprintln!("symlink_ziel übersprungen (kein Entwicklermodus?): {e}");
            return;
        }
        assert!(matches!(resolve_for_write(root, "03 Unterricht/link.md"), Err(GuardError::Link(_))));
        assert!(matches!(resolve_for_read(root, "03 Unterricht/link.md"), Err(GuardError::Link(_))));
        assert!(resolve_for_write(root, "03 Unterricht/echt.md").is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn schreibweise_von_der_platte() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("03 Unterricht")).unwrap();
        fs::write(root.join("03 Unterricht/GEC 05.10.2026.md"), "x").unwrap();
        let (abs, rel) = resolve_for_write(root, "03 unterricht\\gec 05.10.2026.md").unwrap();
        assert_eq!(rel, "03 Unterricht/GEC 05.10.2026.md");
        assert!(abs.ends_with("03 Unterricht/GEC 05.10.2026.md"));
        // noch nicht vorhandene Datei: vorhandener Teil auf der Platte, Rest wie angegeben
        let (_, rel) = resolve_for_write(root, "03 UNTERRICHT/Neu/gec 2 05.10.2026.md").unwrap();
        assert_eq!(rel, "03 Unterricht/Neu/gec 2 05.10.2026.md");
    }

    // ---- Regel 10 ----
    #[test]
    fn regel10_privat_ist_standardmaessig_ausgeschlossen() {
        let files = vec![
            "03 Unterricht/GEC 07.09.2026.md".to_string(),
            "10 Privat/Noten/Noten.md".to_string(),
            "10 Privat/Reflexion/KW45.md".to_string(),
        ];
        assert_eq!(select_for_ai(&files, false).unwrap(), vec!["03 Unterricht/GEC 07.09.2026.md"]);
        assert_eq!(select_for_ai(&files, true).unwrap().len(), 3);
        assert!(is_private("10 Privat/x.md"));
        assert!(!is_private("03 Unterricht/10 Privat.md"));
    }

    #[test]
    fn regel8_auch_beim_ki_versand() {
        let files = vec![".obsidian/app.json".to_string()];
        assert!(select_for_ai(&files, true).is_err());
    }

    // ---- Regel 6 ----
    #[test]
    fn regel6_ek_wird_vor_ki_versand_entfernt() {
        let text = "---\ntitel: Jeans 01\nek: 34,50\nEinkaufspreis: 34,50 €\nvk: 129\n---\n\
                    Die Jeans ist aus 14,5 oz Selvedge-Denim.\nEK: 34,50 € netto\n\
                    Intern: EK-Preis beim Lieferanten nachfragen\nVK 129 €\n";
        let clean = strip_purchase_prices(text);
        assert!(!contains_purchase_price(&clean), "{clean}");
        assert!(!clean.contains("34,50"));
        assert!(clean.contains("vk: 129"));
        assert!(clean.contains("Selvedge-Denim"));
        assert!(clean.contains("VK 129 €"));
    }

    #[test]
    fn regel6_keine_falschen_treffer() {
        // „Ekel“, „Dekor“ oder „Check“ sind keine EKs.
        let text = "Dekoration im Check-out\nEkel vor Spam\n";
        assert_eq!(strip_purchase_prices(text), text);
    }
}
