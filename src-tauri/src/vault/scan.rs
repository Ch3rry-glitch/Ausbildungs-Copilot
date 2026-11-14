//! Vault durchsuchen und einzelne Notizen analysieren. Liest nur – schreibt nie.
//!
//! `.obsidian/` und alle anderen versteckten Ordner (`.trash`, `.git`) werden nicht betreten
//! (Regel 8). Symlinks werden nicht verfolgt.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::NaiveDate;

use super::frontmatter::{field_as_text, tags_from_field, NoteText};
use super::{links, naming, tags, tasks};
use crate::guard;
use crate::model::{NoteKind, NoteMeta, RelPath};

#[derive(Debug, Default)]
pub struct ScanResult {
    /// alle Ordner relativ zum Vault (ohne Wurzel `""`)
    pub folders: Vec<RelPath>,
    /// alle `.md`-Dateien relativ zum Vault
    pub notes: Vec<RelPath>,
}

pub fn scan_vault(root: &Path) -> std::io::Result<ScanResult> {
    let mut res = ScanResult::default();
    walk(root, root, &mut res)?;
    res.folders.sort();
    res.notes.sort();
    Ok(res)
}

fn walk(root: &Path, dir: &Path, res: &mut ScanResult) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if guard::is_hidden_component(&name) {
            continue; // .obsidian, .trash, .git …
        }
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            res.folders.push(rel_path(root, &path));
            walk(root, &path, res)?;
        } else if ft.is_file() && is_markdown(&path) {
            res.notes.push(rel_path(root, &path));
        }
    }
    Ok(())
}

pub fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Vault-relativer Pfad mit `/`.
pub fn rel_path(root: &Path, path: &Path) -> RelPath {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Ordner eines relativen Pfads (`""` für die Wurzel).
pub fn parent_folder(rel: &str) -> RelPath {
    rel.rsplit_once('/').map(|(p, _)| p.to_string()).unwrap_or_default()
}

/// Liest eine Notiz und liefert Metadaten + Text. Läuft über den Wächter (Regel 8).
pub fn read_note(root: &Path, rel: &str) -> Result<(NoteMeta, NoteText), crate::error::AppError> {
    let abs: PathBuf = guard::resolve_in_vault(root, rel)?;
    let bytes = fs::read(&abs)?;
    let text = String::from_utf8_lossy(&bytes);
    let md = fs::metadata(&abs)?;
    let modified_ms = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let note = NoteText::parse(&text);
    let meta = analyze(rel, &note, modified_ms, md.len());
    Ok((meta, note))
}

/// Baut die Metadaten aus dem geparsten Text. Reine Funktion – gut testbar.
pub fn analyze(rel: &str, note: &NoteText, modified_ms: i64, size: u64) -> NoteMeta {
    let folder = parent_folder(rel);
    let title = rel
        .rsplit('/')
        .next()
        .unwrap_or(rel)
        .trim_end_matches(".md")
        .trim_end_matches(".MD")
        .to_string();

    let (fields, frontmatter_error) = match note.fields() {
        Ok(f) => (f, None),
        Err(e) => (Vec::new(), Some(e)),
    };
    let field = |k: &str| fields.iter().find(|f| f.key.eq_ignore_ascii_case(k)).map(|f| &f.value);

    let offset = note.body_line_offset();
    let body = note.body();
    let inline_tags = tags::parse_inline_tags(body, offset);
    let task_list = tasks::parse_tasks(body, offset);
    let mut link_list = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        // Zeile ungefähr: Feldindex + 2 (öffnendes ---). Genau genug für Frontmatter-Links.
        links::links_in_value(&f.value, i + 2, &mut link_list);
    }
    link_list.extend(links::parse_links(body, offset));

    let mut all_tags: Vec<String> = field("tags").map(tags_from_field).unwrap_or_default();
    for t in &inline_tags {
        if !all_tags.iter().any(|x| x.eq_ignore_ascii_case(&t.tag)) {
            all_tags.push(t.tag.clone());
        }
    }

    let unterricht_name = naming::parse_unterricht(&title);
    let datum = field("datum")
        .and_then(field_as_text)
        .and_then(|s| NaiveDate::parse_from_str(s.get(..10).unwrap_or(&s), "%Y-%m-%d").ok())
        .or_else(|| unterricht_name.as_ref().map(|n| n.datum));
    let fach = field("fach")
        .and_then(field_as_text)
        .or_else(|| unterricht_name.as_ref().map(|n| n.fach.clone()));

    let kind = classify(&folder, &title, &all_tags, unterricht_name.is_some());

    NoteMeta {
        path: rel.to_string(),
        private: guard::is_private(rel),
        folder,
        title,
        kind,
        fields,
        tags: all_tags,
        inline_tags,
        tasks: task_list,
        links: link_list,
        datum,
        fach,
        modified_ms,
        size,
        frontmatter_error,
    }
}

fn classify(folder: &str, title: &str, tags: &[String], unterricht_name: bool) -> NoteKind {
    let top = folder.split('/').next().unwrap_or("");
    let has = |t: &str| tags.iter().any(|x| x.eq_ignore_ascii_case(t));
    if top == "99 Meta" {
        NoteKind::Template
    } else if top == "03 Unterricht" && (unterricht_name || has("unterricht")) {
        NoteKind::Unterricht
    } else if folder == "04 Betrieb/Wochen" && naming::parse_woche(title).is_some() {
        NoteKind::BerichtsheftWoche
    } else if folder == "07 Material/Aufgabensammlung" && (has("aufgabe") || has("lernaufgabe")) {
        NoteKind::Aufgabe
    } else if top == "06 Glossar" && has("begriff") {
        NoteKind::Glossar
    } else if top == "01 Fächer" {
        NoteKind::Fach
    } else if top == "02 Lernfelder" {
        NoteKind::Lernfeld
    } else {
        NoteKind::Sonstige
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyse_einer_unterrichtsnotiz() {
        let text = "---\ntags: [unterricht]\ndatum: 2026-09-07\nfach: \"[[GEC – Geschäftsprozesse im E-Commerce|GEC]]\"\n---\n\
                    # 2026-09-07 GEC – Kalkulation\n- [ ] AB 📅 2026-09-10\nWas ist Skonto? #frage\n";
        let note = NoteText::parse(text);
        let m = analyze("03 Unterricht/GEC 07.09.2026.md", &note, 0, 0);
        assert_eq!(m.kind, NoteKind::Unterricht);
        assert_eq!(m.title, "GEC 07.09.2026");
        assert_eq!(m.folder, "03 Unterricht");
        assert_eq!(m.fach.as_deref(), Some("GEC"));
        assert_eq!(m.datum, NaiveDate::from_ymd_opt(2026, 9, 7));
        assert_eq!(m.tasks[0].line, 7);
        assert_eq!(m.inline_tags[0].line, 8);
        assert_eq!(m.tags, vec!["unterricht", "frage"]);
        assert_eq!(m.links[0].alias.as_deref(), Some("GEC"));
        assert!(!m.private);
    }

    #[test]
    fn datum_und_fach_aus_altbestand_dateiname() {
        let note = NoteText::parse("Kein Frontmatter\n");
        let m = analyze("03 Unterricht/Unterrichtsnotiz GEC 07.09.2026.md", &note, 0, 0);
        assert_eq!(m.kind, NoteKind::Unterricht);
        assert_eq!(m.fach.as_deref(), Some("GEC"));
        assert_eq!(m.datum, NaiveDate::from_ymd_opt(2026, 9, 7));
    }

    #[test]
    fn rel_pfade_mit_schraegstrich() {
        let root = Path::new("C:/Vault");
        assert_eq!(rel_path(root, Path::new("C:/Vault/03 Unterricht/GEC 07.09.2026.md")), "03 Unterricht/GEC 07.09.2026.md");
        assert_eq!(parent_folder("Home.md"), "");
        assert_eq!(parent_folder("04 Betrieb/Wochen/2026-KW41.md"), "04 Betrieb/Wochen");
    }
}
