//! Alles, was Markdown-Dateien im Vault versteht. Reine Funktionen ohne Tauri-Abhängigkeit,
//! damit sie ohne App testbar sind.

pub mod frontmatter;
pub mod links;
pub mod naming;
pub mod scan;
pub mod stundenplan;
pub mod tags;
pub mod tasks;

/// Eine Textzeile außerhalb von Codeblöcken, mit ausgeblendetem Inline-Code.
pub(crate) struct ProseLine<'a> {
    /// 1-basierte Zeilennummer relativ zum übergebenen Text
    pub nr: usize,
    /// die Originalzeile (ohne Zeilenende)
    pub raw: &'a str,
    /// die Zeile mit `` `code` `` durch Leerzeichen ersetzt (gleiche Länge)
    pub masked: String,
}

/// Liefert alle Zeilen außerhalb von ```/~~~-Codeblöcken. Inline-Code wird maskiert, damit
/// `#include` oder `[[x]]` in Code nicht als Tag/Link zählen.
pub(crate) fn prose_lines(text: &str) -> Vec<ProseLine<'_>> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (i, raw) in text.lines().enumerate() {
        let trimmed = raw.trim_start();
        let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if let Some(c) = fence_char {
            let run = trimmed.chars().take_while(|x| *x == c).count();
            if run >= 3 {
                match fence {
                    None => {
                        fence = Some((c, run));
                        continue;
                    }
                    Some((fc, frun)) if fc == c && run >= frun && trimmed[run..].trim().is_empty() => {
                        fence = None;
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if fence.is_some() {
            continue;
        }
        out.push(ProseLine { nr: i + 1, raw, masked: mask_inline_code(raw) });
    }
    out
}

fn mask_inline_code(line: &str) -> String {
    if !line.contains('`') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
            out.push(' ');
        } else if in_code {
            // gleiche Bytelänge beibehalten, damit Positionen stimmen
            for _ in 0..ch.len_utf8() {
                out.push(' ');
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codebloecke_und_inline_code_werden_ausgeblendet() {
        let text = "a #eins\n```\n#nicht\n```\nb `#auch_nicht` #zwei\n~~~~\n```\n~~~~\nc";
        let lines = prose_lines(text);
        let nrs: Vec<usize> = lines.iter().map(|l| l.nr).collect();
        assert_eq!(nrs, vec![1, 5, 9]);
        assert!(!lines[1].masked.contains("auch_nicht"));
        assert!(lines[1].masked.contains("#zwei"));
        assert_eq!(lines[1].masked.len(), lines[1].raw.len());
    }
}
