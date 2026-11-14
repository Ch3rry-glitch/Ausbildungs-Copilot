//! Wächter für die harten Regeln aus SPEC §3, soweit sie Pfade und ausgehende Texte betreffen.
//!
//! * Regel 6  – interne Einkaufspreise (EK) verlassen die App nie → [`strip_purchase_prices`]
//! * Regel 8  – `.obsidian/` wird nie gelesen oder geschrieben → [`resolve_in_vault`], [`is_obsidian_config`]
//! * Regel 10 – `10 Privat/` ist standardmäßig vom KI-Versand ausgeschlossen → [`select_for_ai`]
//!
//! Jeder Dateizugriff auf den Vault läuft über [`resolve_in_vault`].

use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

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
