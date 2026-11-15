//! Inline-Tags wie `#frage` und `#nochmal`.
//!
//! Wie in Obsidian: ein Tag beginnt am Zeilenanfang oder nach Leerraum, besteht aus Buchstaben,
//! Ziffern, `_`, `-`, `/` und enthält mindestens ein Nicht-Ziffer-Zeichen (`#1` ist kein Tag).
//! Durchgestrichene Tags (`~~#frage~~`) zählen nicht – so markiert die App später „geklärt".

use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;

use super::prose_lines;
use crate::model::InlineTag;

static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s(\[,;])#(?P<tag>[\p{L}\p{N}_/\-]+)").unwrap());

/// Ein Tag in einer Zeile: Bytebereich von `#` bis zum Ende des Tags und der Name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TagToken {
    /// `#frage/offen` – inklusive `#` und eventueller `/`/`-` am Ende
    pub range: Range<usize>,
    /// ohne `#`, ohne `/`/`-` am Ende, Schreibweise wie in der Datei
    pub tag: String,
}

/// Alle Tags einer Zeile. `masked` = Zeile mit ausgeblendetem Inline-Code (gleiche Bytelänge wie
/// das Original, die Bereiche passen also auf beide).
pub(crate) fn tag_tokens(masked: &str) -> Vec<TagToken> {
    TAG.captures_iter(masked)
        .filter_map(|caps| {
            let m = caps.name("tag")?;
            if m.as_str().chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            // Überschrift `# Titel` hat ein Leerzeichen und wird oben nie erfasst.
            Some(TagToken { range: m.start() - 1..m.end(), tag: m.as_str().trim_end_matches(['/', '-']).to_string() })
        })
        .collect()
}

/// `frage` passt auf `frage`, `Frage` und `frage/offen` – nicht auf `fragebogen`.
pub fn tag_passt(tag: &str, name: &str) -> bool {
    match tag.get(..name.len()) {
        Some(kopf) if kopf.eq_ignore_ascii_case(name) => {
            let rest = &tag[name.len()..];
            rest.is_empty() || rest.starts_with('/')
        }
        _ => false,
    }
}

pub fn parse_inline_tags(body: &str, line_offset: usize) -> Vec<InlineTag> {
    let mut out = Vec::new();
    for l in prose_lines(body) {
        for t in tag_tokens(&l.masked) {
            out.push(InlineTag { tag: t.tag, line: l.nr + line_offset, context: l.raw.trim().to_string() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(s: &str) -> Vec<String> {
        parse_inline_tags(s, 0).into_iter().map(|t| t.tag).collect()
    }

    #[test]
    fn frage_und_nochmal() {
        let body = "# Überschrift\nWas ist Skonto? #frage\n#nochmal Deckungsbeitrag\n";
        let t = parse_inline_tags(body, 4);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].tag, "frage");
        assert_eq!(t[0].line, 6);
        assert_eq!(t[0].context, "Was ist Skonto? #frage");
        assert_eq!(t[1].tag, "nochmal");
    }

    #[test]
    fn keine_falschen_tags() {
        assert!(tags("## Kapitel\n[[Notiz#Abschnitt]]\nhttps://x.de/#anker\nAufgabe #1\nC#\n").is_empty());
        assert!(tags("`#include` und\n```\n#code\n```").is_empty());
        assert!(tags("~~#frage~~ geklärt").is_empty());
    }

    #[test]
    fn umlaute_und_verschachtelt() {
        assert_eq!(tags("#prüfung/lf04 und (#übung)"), vec!["prüfung/lf04", "übung"]);
    }

    #[test]
    fn tag_bereiche_und_passen() {
        let zeile = "- Was? #Frage und #fragebogen, #frage/offen";
        let t = tag_tokens(zeile);
        let teile: Vec<&str> = t.iter().map(|x| &zeile[x.range.clone()]).collect();
        assert_eq!(teile, vec!["#Frage", "#fragebogen", "#frage/offen"]);
        assert!(tag_passt(&t[0].tag, "frage"));
        assert!(!tag_passt(&t[1].tag, "frage"));
        assert!(tag_passt(&t[2].tag, "frage"));
        assert!(tag_passt("Nochmal", "nochmal") && !tag_passt("noch", "nochmal") && !tag_passt("nochmals", "nochmal"));
        assert!(!tag_passt("früh", "frage"));
    }
}
