//! Frontmatter lesen (über `serde_yaml`) und schreiben (über **Zeilenchirurgie**).
//!
//! Schreiben verändert ausschließlich die Zeile(n) des betroffenen Schlüssels. Unbekannte
//! Schlüssel, Reihenfolge, Kommentare, Listenstil (`[a, b]` vs. Blockliste), Zeilenenden (CRLF/LF)
//! und ein BOM bleiben erhalten. Lesen + unverändert schreiben ist **byte-gleich** (SPEC §5.2).

use crate::model::FrontmatterField;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteText {
    /// BOM + öffnendes `---` inkl. Zeilenende (leer, wenn keine Frontmatter)
    head: String,
    /// YAML-Zeilen, jeweils inkl. Zeilenende
    yaml_lines: Vec<String>,
    /// schließendes `---` inkl. Zeilenende (leer, wenn keine Frontmatter)
    close: String,
    body: String,
    eol: &'static str,
}

impl NoteText {
    pub fn parse(text: &str) -> Self {
        let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let (bom, rest) = match text.strip_prefix('\u{feff}') {
            Some(r) => ("\u{feff}", r),
            None => ("", text),
        };
        let no_fm = || NoteText {
            head: String::new(),
            yaml_lines: Vec::new(),
            close: String::new(),
            body: text.to_string(),
            eol,
        };

        let mut lines = rest.split_inclusive('\n');
        let Some(first) = lines.next() else { return no_fm() };
        if strip_eol(first) != "---" {
            return no_fm();
        }
        let mut yaml_lines = Vec::new();
        let mut consumed = first.len();
        for line in lines {
            consumed += line.len();
            let content = strip_eol(line);
            if content.trim_end() == "---" || content.trim_end() == "..." {
                return NoteText {
                    head: format!("{bom}{first}"),
                    yaml_lines,
                    close: line.to_string(),
                    body: rest[consumed..].to_string(),
                    eol,
                };
            }
            yaml_lines.push(line.to_string());
        }
        // nicht geschlossen → keine Frontmatter (so sieht es auch Obsidian)
        no_fm()
    }

    pub fn has_frontmatter(&self) -> bool {
        !self.head.is_empty()
    }

    /// Die YAML-Quelle ohne `---`.
    pub fn yaml(&self) -> Option<String> {
        self.has_frontmatter().then(|| self.yaml_lines.concat())
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    /// Anzahl der Zeilen vor dem Body – damit Zeilennummern aus dem Body auf die Datei passen.
    pub fn body_line_offset(&self) -> usize {
        if self.has_frontmatter() {
            self.yaml_lines.len() + 2
        } else {
            0
        }
    }

    /// Frontmatter-Felder in Dateireihenfolge. `Err` bei ungültigem YAML – die Notiz bleibt
    /// trotzdem lesbar.
    pub fn fields(&self) -> Result<Vec<FrontmatterField>, String> {
        let Some(yaml) = self.yaml() else { return Ok(Vec::new()) };
        if yaml.trim().is_empty() {
            return Ok(Vec::new());
        }
        let value: serde_yaml::Value = serde_yaml::from_str(&yaml).map_err(|e| e.to_string())?;
        match value {
            serde_yaml::Value::Null => Ok(Vec::new()),
            serde_yaml::Value::Mapping(map) => Ok(map
                .into_iter()
                .map(|(k, v)| FrontmatterField { key: yaml_key(&k), value: yaml_to_json(v) })
                .collect()),
            _ => Err("Frontmatter ist keine Schlüssel-Wert-Liste".into()),
        }
    }

    /// Setzt einen Skalar (z. B. `logbuch: kopiert`). `value` ist der **rohe YAML-Text**;
    /// für beliebige Strings vorher [`yaml_scalar`] benutzen. Gibt `true` zurück, wenn sich
    /// der Text geändert hat.
    pub fn set_scalar(&mut self, key: &str, value: &str) -> bool {
        if !self.has_frontmatter() {
            self.head = format!("---{}", self.eol);
            self.close = format!("---{}", self.eol);
            self.yaml_lines.push(format!("{}{}", key_line(key, value), self.eol));
            return true;
        }
        let Some(idx) = self.find_key(key) else {
            if let Some(last) = self.yaml_lines.last_mut() {
                if !last.ends_with('\n') {
                    last.push_str(self.eol);
                }
            }
            self.yaml_lines.push(format!("{}{}", key_line(key, value), self.eol));
            return true;
        };

        let line = &self.yaml_lines[idx];
        let line_eol = &line[strip_eol(line).len()..];
        let old_value = strip_eol(line)[key.len() + 1..].trim();
        let cont = self.continuation_len(idx);
        if cont == 0 && unquote(old_value) == unquote(value) {
            return false; // gleicher Wert → Datei bleibt byte-gleich
        }
        let new_line = format!("{}{}", key_line(key, value), line_eol);
        self.yaml_lines.splice(idx..=idx + cont, [new_line]);
        true
    }

    fn find_key(&self, key: &str) -> Option<usize> {
        self.yaml_lines.iter().position(|l| {
            let c = strip_eol(l);
            c.strip_prefix(key)
                .and_then(|r| r.strip_prefix(':'))
                .is_some_and(|r| r.is_empty() || r.starts_with([' ', '\t']))
        })
    }

    /// Zeilen, die zum Wert in Zeile `idx` gehören (eingerückt oder `- …`-Blockliste).
    fn continuation_len(&self, idx: usize) -> usize {
        self.yaml_lines[idx + 1..]
            .iter()
            .take_while(|l| {
                let c = strip_eol(l);
                c.starts_with([' ', '\t']) || c.starts_with("- ") || c == "-"
            })
            .count()
    }

    pub fn render(&self) -> String {
        let mut s = String::with_capacity(self.head.len() + self.body.len() + 256);
        s.push_str(&self.head);
        for l in &self.yaml_lines {
            s.push_str(l);
        }
        s.push_str(&self.close);
        s.push_str(&self.body);
        s
    }
}

impl std::fmt::Display for NoteText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}

fn strip_eol(line: &str) -> &str {
    line.strip_suffix('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).unwrap_or(line)
}

fn key_line(key: &str, value: &str) -> String {
    if value.is_empty() {
        format!("{key}:")
    } else {
        format!("{key}: {value}")
    }
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Macht aus einem beliebigen Text einen sicheren YAML-Skalar (Anführungszeichen nur, wenn nötig).
pub fn yaml_scalar(s: &str) -> String {
    const RESERVED: [&str; 9] = ["true", "false", "null", "yes", "no", "on", "off", "~", ""];
    let needs_quotes = RESERVED.contains(&s.to_ascii_lowercase().as_str())
        || s.starts_with(|c: char| "[]{}>|*&!%@#`'\",?:-".contains(c) || c.is_whitespace())
        || s.ends_with(char::is_whitespace)
        || s.contains(": ")
        || s.contains(" #")
        || s.contains('\n')
        || s.parse::<f64>().is_ok();
    if needs_quotes {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
    } else {
        s.to_string()
    }
}

fn yaml_key(k: &serde_yaml::Value) -> String {
    match k {
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

/// YAML → JSON ohne Fehler bei Nicht-String-Schlüsseln (die JSON nicht kennt).
pub fn yaml_to_json(v: serde_yaml::Value) -> serde_json::Value {
    use serde_json::Value as J;
    use serde_yaml::Value as Y;
    match v {
        Y::Null => J::Null,
        Y::Bool(b) => J::Bool(b),
        Y::Number(n) => {
            if let Some(i) = n.as_i64() {
                J::from(i)
            } else if let Some(u) = n.as_u64() {
                J::from(u)
            } else {
                n.as_f64().and_then(serde_json::Number::from_f64).map(J::Number).unwrap_or(J::Null)
            }
        }
        Y::String(s) => J::String(s),
        Y::Sequence(seq) => J::Array(seq.into_iter().map(yaml_to_json).collect()),
        Y::Mapping(map) => {
            J::Object(map.into_iter().map(|(k, v)| (yaml_key(&k), yaml_to_json(v))).collect())
        }
        Y::Tagged(t) => yaml_to_json(t.value),
    }
}

/// Frontmatter-Wert als einfacher Text (Links auf Alias reduziert: `[[GEC – …|GEC]]` → `GEC`).
pub fn field_as_text(v: &serde_json::Value) -> Option<String> {
    let s = match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Array(a) => return a.first().and_then(field_as_text),
        _ => return None,
    };
    if s.is_empty() {
        return None;
    }
    if let Some(inner) = s.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")) {
        let inner = inner.replace("\\|", "|");
        let shown = inner.rsplit('|').next().unwrap_or(&inner);
        return Some(shown.trim().to_string());
    }
    Some(s)
}

/// `tags` aus der Frontmatter: Liste oder Text („a, b“ / „a b“), ohne führendes `#`.
pub fn tags_from_field(v: &serde_json::Value) -> Vec<String> {
    let raw: Vec<String> = match v {
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect(),
        serde_json::Value::String(s) => {
            s.split([',', ' ']).map(str::to_string).collect()
        }
        _ => Vec::new(),
    };
    raw.into_iter()
        .map(|t| t.trim().trim_start_matches('#').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SAMPLES: &[&str] = &[
        "---\ntags: [unterricht]\ndatum: 2026-09-07\nfach: \"[[GEC – Geschäftsprozesse im E-Commerce|GEC]]\"\n---\n# Titel\n",
        "---\r\ntags:\r\n  - unterricht\r\n  - gec\r\ndatum: 2026-09-07 # Kommentar\r\n---\r\nText\r\n",
        "\u{feff}---\nunbekannt: wert\n  eingerückt: ja\n---\n",
        "Keine Frontmatter\n---\nnur eine Linie\n",
        "---\nnicht: geschlossen\n",
        "---\n---\nleere Frontmatter",
        "---\ntyp: Rechnen\nschwierigkeit: 2\ngeloest: false\n...\nBody ohne Zeilenende",
        "",
    ];

    #[test]
    fn rundlauf_ist_byte_gleich() {
        for s in SAMPLES {
            let n = NoteText::parse(s);
            assert_eq!(n.render(), *s, "Rundlauf verändert: {s:?}");
        }
    }

    #[test]
    fn gleicher_wert_laesst_datei_byte_gleich() {
        let s = "---\r\ntags: [unterricht]\r\nlogbuch: \"offen\"\r\n---\r\nText";
        let mut n = NoteText::parse(s);
        assert!(!n.set_scalar("logbuch", "offen"));
        assert_eq!(n.render(), s);
    }

    #[test]
    fn nur_die_eine_zeile_aendert_sich() {
        let s = "---\ntags: [unterricht, gec]\nlogbuch: offen   \nthema: Kalkulation\n---\nBody\n";
        let mut n = NoteText::parse(s);
        assert!(n.set_scalar("logbuch", "kopiert"));
        assert_eq!(
            n.render(),
            "---\ntags: [unterricht, gec]\nlogbuch: kopiert\nthema: Kalkulation\n---\nBody\n"
        );
    }

    #[test]
    fn blockwert_wird_ersetzt_rest_bleibt() {
        let s = "---\ntags:\n  - a\n  - b\nstatus:\n- alt\n- alt2\nkw: 41\n---\n";
        let mut n = NoteText::parse(s);
        n.set_scalar("status", "Abgeschlossen");
        assert_eq!(n.render(), "---\ntags:\n  - a\n  - b\nstatus: Abgeschlossen\nkw: 41\n---\n");
    }

    #[test]
    fn neuer_schluessel_am_ende_mit_crlf() {
        let s = "---\r\ndatum: 2026-09-07\r\n---\r\nText";
        let mut n = NoteText::parse(s);
        n.set_scalar("logbuch", "offen");
        assert_eq!(n.render(), "---\r\ndatum: 2026-09-07\r\nlogbuch: offen\r\n---\r\nText");

        let mut ohne = NoteText::parse("Nur Text\n");
        ohne.set_scalar("logbuch", "offen");
        assert_eq!(ohne.render(), "---\nlogbuch: offen\n---\nNur Text\n");
    }

    #[test]
    fn schluessel_mit_gleichem_praefix_wird_nicht_verwechselt() {
        let s = "---\nkw_alt: 1\nkw: 41\n---\n";
        let mut n = NoteText::parse(s);
        n.set_scalar("kw", "42");
        assert_eq!(n.render(), "---\nkw_alt: 1\nkw: 42\n---\n");
    }

    #[test]
    fn felder_in_dateireihenfolge() {
        let n = NoteText::parse(SAMPLES[0]);
        let f = n.fields().unwrap();
        let keys: Vec<&str> = f.iter().map(|x| x.key.as_str()).collect();
        assert_eq!(keys, vec!["tags", "datum", "fach"]);
        assert_eq!(f[0].value, json!(["unterricht"]));
        assert_eq!(f[1].value, json!("2026-09-07"));
        assert_eq!(field_as_text(&f[2].value).as_deref(), Some("GEC"));
        assert_eq!(n.body_line_offset(), 5);
    }

    #[test]
    fn uneinheitliche_werte_werden_gelesen() {
        let n = NoteText::parse(SAMPLES[6]);
        let f = n.fields().unwrap();
        assert_eq!(f[0].value, json!("Rechnen"));
        assert_eq!(f[1].value, json!(2));
        assert_eq!(f[2].value, json!(false));
    }

    #[test]
    fn kaputtes_yaml_ist_fehler_aber_kein_absturz() {
        let n = NoteText::parse("---\ntags: [offen\n---\nText");
        assert!(n.fields().is_err());
        assert_eq!(n.body(), "Text");
    }

    #[test]
    fn yaml_scalar_setzt_anfuehrungszeichen_nur_wenn_noetig() {
        assert_eq!(yaml_scalar("kopiert"), "kopiert");
        assert_eq!(yaml_scalar("[[Unterrichtsnotiz]]"), "\"[[Unterrichtsnotiz]]\"");
        assert_eq!(yaml_scalar("Thema: Teil 2"), "\"Thema: Teil 2\"");
        assert_eq!(yaml_scalar("2"), "\"2\"");
        assert_eq!(yaml_scalar("no"), "\"no\"");
    }

    #[test]
    fn tags_als_liste_oder_text() {
        assert_eq!(tags_from_field(&json!(["#a", "b"])), vec!["a", "b"]);
        assert_eq!(tags_from_field(&json!("a, b c")), vec!["a", "b", "c"]);
    }
}
