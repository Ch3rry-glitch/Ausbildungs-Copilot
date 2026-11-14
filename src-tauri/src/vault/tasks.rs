//! Aufgaben im Format des Tasks-Plugins: `- [ ] Text 📅 2026-11-05`.

use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;

use super::prose_lines;
use crate::model::Task;

static TASK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<indent>[ \t]*)(?:[-*+]|\d+[.)]) \[(?P<status>.)\](?:[ \t]+(?P<text>.*))?$").unwrap());
static DUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*📅\s*(?P<date>\d{4}-\d{2}-\d{2})").unwrap());

/// Liest alle Aufgaben aus `body`. `line_offset` = Zeilen vor dem Body (Frontmatter), damit
/// [`Task::line`] die echte Zeile in der Datei ist.
pub fn parse_tasks(body: &str, line_offset: usize) -> Vec<Task> {
    prose_lines(body)
        .into_iter()
        .filter_map(|l| {
            let caps = TASK.captures(l.raw)?;
            let status_char = caps["status"].chars().next()?;
            let raw_text = caps.name("text").map(|m| m.as_str()).unwrap_or("");
            let due = DUE
                .captures(raw_text)
                .and_then(|c| NaiveDate::parse_from_str(&c["date"], "%Y-%m-%d").ok());
            let text = DUE.replace(raw_text, "").trim().to_string();
            let indent = caps["indent"].chars().map(|c| if c == '\t' { 4 } else { 1 }).sum();
            Some(Task {
                line: l.nr + line_offset,
                text,
                done: matches!(status_char, 'x' | 'X'),
                status_char,
                due,
                indent,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aufgaben_mit_und_ohne_frist() {
        let body = "## Aufgaben / Hausaufgaben\n\
                    - [ ] Arbeitsblatt Kalkulation 📅 2026-11-05\n\
                    - [x] Vokabeln lernen ✅ 2026-11-01\n\
                    \t- [ ] Unteraufgabe\n\
                    * [X] Sternchen\n\
                    1. [ ] nummeriert📅2026-12-01\n\
                    - [] keine Aufgabe\n\
                    - [ ]\n\
                    ```\n- [ ] im Codeblock\n```\n";
        let t = parse_tasks(body, 5);
        assert_eq!(t.len(), 6, "{t:#?}");
        assert_eq!(t[0].text, "Arbeitsblatt Kalkulation");
        assert_eq!(t[0].due, NaiveDate::from_ymd_opt(2026, 11, 5));
        assert_eq!(t[0].line, 7);
        assert!(!t[0].done);
        assert!(t[1].done);
        assert_eq!(t[1].due, None);
        assert_eq!(t[2].indent, 4);
        assert!(t[3].done);
        assert_eq!(t[4].text, "nummeriert");
        assert_eq!(t[4].due, NaiveDate::from_ymd_opt(2026, 12, 1));
        assert_eq!(t[5].text, "");
    }

    #[test]
    fn ungueltiges_datum_ist_keine_frist() {
        let t = parse_tasks("- [ ] Test 📅 2026-13-45", 0);
        assert_eq!(t[0].due, None);
    }

    #[test]
    fn andere_statuszeichen_bleiben_erhalten() {
        let t = parse_tasks("- [/] halb\n- [-] abgebrochen", 0);
        assert_eq!(t[0].status_char, '/');
        assert!(!t[0].done);
        assert_eq!(t[1].status_char, '-');
    }
}
