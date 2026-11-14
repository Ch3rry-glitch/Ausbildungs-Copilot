//! Inline-Tags wie `#frage` und `#nochmal`.
//!
//! Wie in Obsidian: ein Tag beginnt am Zeilenanfang oder nach Leerraum, besteht aus Buchstaben,
//! Ziffern, `_`, `-`, `/` und enthält mindestens ein Nicht-Ziffer-Zeichen (`#1` ist kein Tag).
//! Durchgestrichene Tags (`~~#frage~~`) zählen nicht – so markiert die App später „geklärt".

use std::sync::LazyLock;

use regex::Regex;

use super::prose_lines;
use crate::model::InlineTag;

static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s(\[,;])#(?P<tag>[\p{L}\p{N}_/\-]+)").unwrap());

pub fn parse_inline_tags(body: &str, line_offset: usize) -> Vec<InlineTag> {
    let mut out = Vec::new();
    for l in prose_lines(body) {
        for caps in TAG.captures_iter(&l.masked) {
            let tag = &caps["tag"];
            if tag.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            // Überschrift `# Titel` hat ein Leerzeichen und wird oben nie erfasst.
            out.push(InlineTag {
                tag: tag.trim_end_matches(['/', '-']).to_string(),
                line: l.nr + line_offset,
                context: l.raw.trim().to_string(),
            });
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
}
