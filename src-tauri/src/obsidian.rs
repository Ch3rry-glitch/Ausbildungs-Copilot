//! „In Obsidian öffnen“ über `obsidian://open?vault=<Name>&file=<relativer Pfad>`.

use std::path::Path;

/// Vaultname = Name des Vault-Ordners (so registriert Obsidian ihn).
pub fn vault_name(root: &Path) -> String {
    root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn open_uri(root: &Path, rel: &str) -> String {
    format!(
        "obsidian://open?vault={}&file={}",
        encode_component(&vault_name(root)),
        encode_component(rel.trim_end_matches(".md"))
    )
}

/// Wie JavaScripts `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_mit_leerzeichen_umlauten_und_ordnern() {
        let root = Path::new("C:/Users/x/Documents/E-Commerce");
        assert_eq!(
            open_uri(root, "03 Unterricht/GEC 07.09.2026.md"),
            "obsidian://open?vault=E-Commerce&file=03%20Unterricht%2FGEC%2007.09.2026"
        );
        assert_eq!(
            open_uri(root, "01 Fächer/GEC/GEC – Geschäftsprozesse im E-Commerce.md"),
            "obsidian://open?vault=E-Commerce&file=01%20F%C3%A4cher%2FGEC%2FGEC%20%E2%80%93%20Gesch%C3%A4ftsprozesse%20im%20E-Commerce"
        );
        assert_eq!(encode_component("a&b=c#d"), "a%26b%3Dc%23d");
    }
}
