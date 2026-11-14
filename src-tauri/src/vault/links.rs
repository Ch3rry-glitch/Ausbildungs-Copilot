//! Wikilinks `[[Ziel#Überschrift|Alias]]` und Einbettungen `![[Bild.png]]`.
//! In Markdown-Tabellen steht der Trenner als `\|` – das wird wie `|` behandelt.

use std::sync::LazyLock;

use regex::Regex;

use super::prose_lines;
use crate::model::WikiLink;

static LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<embed>!?)\[\[(?P<inner>[^\[\]\n]+?)\]\]").unwrap());

/// Zerlegt den Inhalt zwischen `[[` und `]]`.
pub fn parse_link_inner(inner: &str) -> (String, Option<String>, Option<String>) {
    let inner = inner.replace("\\|", "|");
    let (dest, alias) = match inner.split_once('|') {
        Some((d, a)) => (d.to_string(), Some(a.trim().to_string()).filter(|a| !a.is_empty())),
        None => (inner.clone(), None),
    };
    let (target, heading) = match dest.split_once('#') {
        Some((t, h)) => (t.trim().to_string(), Some(h.trim().to_string()).filter(|h| !h.is_empty())),
        None => (dest.trim().to_string(), None),
    };
    (target, heading, alias)
}

pub fn parse_links(body: &str, line_offset: usize) -> Vec<WikiLink> {
    let mut out = Vec::new();
    for l in prose_lines(body) {
        for caps in LINK.captures_iter(&l.masked) {
            let (target, heading, alias) = parse_link_inner(&caps["inner"]);
            out.push(WikiLink {
                target,
                heading,
                alias,
                embed: !caps["embed"].is_empty(),
                line: l.nr + line_offset,
            });
        }
    }
    out
}

/// Links in Frontmatter-Werten (`fach: "[[GEC – …|GEC]]"`, `quelle: "[[…]]"`).
pub fn links_in_value(v: &serde_json::Value, line: usize, out: &mut Vec<WikiLink>) {
    match v {
        serde_json::Value::String(s) => {
            for caps in LINK.captures_iter(s) {
                let (target, heading, alias) = parse_link_inner(&caps["inner"]);
                out.push(WikiLink { target, heading, alias, embed: false, line });
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| links_in_value(x, line, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| links_in_value(x, line, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_arten() {
        let body = "Siehe [[GEC – Geschäftsprozesse im E-Commerce|GEC]] und [[D-DV]].\n\
                    | 2 | [[WSP – Wirtschafts- und Sozialprozesse\\|WSP]] |\n\
                    ![[Scan 2026-09-07.png]] [[LF04 – Werteströme#Bilanz|Bilanz]]\n\
                    `[[kein Link]]`";
        let l = parse_links(body, 0);
        assert_eq!(l.len(), 5, "{l:#?}");
        assert_eq!(l[0].target, "GEC – Geschäftsprozesse im E-Commerce");
        assert_eq!(l[0].alias.as_deref(), Some("GEC"));
        assert_eq!(l[1].target, "D-DV");
        assert_eq!(l[1].alias, None);
        assert_eq!(l[2].target, "WSP – Wirtschafts- und Sozialprozesse");
        assert_eq!(l[2].alias.as_deref(), Some("WSP"));
        assert_eq!(l[2].line, 2);
        assert!(l[3].embed);
        assert_eq!(l[4].heading.as_deref(), Some("Bilanz"));
    }

    #[test]
    fn links_aus_frontmatter() {
        let mut out = Vec::new();
        links_in_value(&serde_json::json!("[[GEC – Geschäftsprozesse im E-Commerce|GEC]]"), 3, &mut out);
        assert_eq!(out[0].alias.as_deref(), Some("GEC"));
        assert_eq!(out[0].line, 3);
    }
}
