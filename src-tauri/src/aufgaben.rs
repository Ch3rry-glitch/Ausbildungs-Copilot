//! M3: Aufgaben (SPEC §6.4) – Nachbau der Seite `00 Start/Aufgaben.md`.
//!
//! * **Arbeitsaufträge**: alle Aufgaben (`- [ ] …`) aus allen Notizen außer `99 Meta` und
//!   `!…`-Dateien ([`zaehlt_als_quelle`] – gilt auch für „Heute“ und die Fragen), eingeteilt wie die
//!   Dataview-Abfragen der Seite ([`uebersicht`]).
//! * **Lernaufgaben**: Notizen in `07 Material/Aufgabensammlung/` ([`lernaufgaben`]).
//! * **Abhaken** ändert in der Datei genau das Zeichen zwischen `[` und `]` ([`abhaken`]),
//!   **gelöst** genau die Zeile `geloest:` ([`lernaufgabe_geloest`]). Alles andere bleibt byte-gleich
//!   (BOM, CRLF). Geschrieben wird nur über [`crate::write::save`] mit der frisch gelesenen Version
//!   als Basis; ändert Obsidian die Datei dazwischen, wird neu gelesen und neu geplant (höchstens
//!   dreimal).
//!
//! Abweichungen von Dataview (bewusst):
//! * `- [-]` (abgebrochen) zählt nicht als offen – wie „Heute“. Dataview zeigt sie als offen; der
//!   echte Vault kennt nur ` ` und `x`.
//! * „Zuletzt erledigt“ nutzt denselben Quellfilter wie die offenen Listen (also auch ohne
//!   `!…`-Dateien); die Dataview-Abfrage dort filtert nur `99 Meta`.
//! * „Später fällig“ (Frist nach heute + 7) gibt es nur in der App.

use std::path::Path;
use std::sync::LazyLock;

use chrono::{Duration, NaiveDate};
use regex::Regex;

use crate::error::{AppError, AppResult};
use crate::guard;
use crate::index::VaultIndex;
use crate::model::{
    AufgabeAbhaken, AufgabeErgebnis, AufgabenUebersicht, Arbeitsauftrag, FrontmatterField, LineEnding,
    Lernaufgabe, NoteMeta, NoteVersion, Task, WriteReport,
};
use crate::vault::frontmatter::{field_as_text, NoteText};
use crate::vault::{scan, tags, tasks};
use crate::write::{self, RetryPolicy, SaveOutcome, SelfWrites};

pub const AUFGABENSAMMLUNG_DIR: &str = "07 Material/Aufgabensammlung";
/// Die Übersichtsseite der Sammlung ist selbst keine Aufgabe.
pub const AUFGABENSAMMLUNG_TITEL: &str = "Aufgabensammlung";
pub const GELOEST_KEY: &str = "geloest";
/// Dataview `LIMIT 10` unter „Zuletzt erledigt“.
pub const ZULETZT_MAX: usize = 10;
/// So oft wird nach einem Konflikt (Obsidian hat dazwischen gespeichert) neu gelesen und geplant.
pub(crate) const VERSUCHE: usize = 3;

// ---------------------------------------------------------------------------------------------
// Filter wie auf der Dataview-Seite
// ---------------------------------------------------------------------------------------------

/// Zählt die Notiz als Quelle für Aufgaben, Fragen und „Heute“? Wie `00 Start/Aufgaben.md`:
/// `!contains(file.folder, "99 Meta") AND !startswith(file.name, "!")`.
pub fn zaehlt_als_quelle(m: &NoteMeta) -> bool {
    quelle_erlaubt(&m.folder, &m.title)
}

fn quelle_erlaubt(folder: &str, title: &str) -> bool {
    !folder.contains("99 Meta") && !title.starts_with('!')
}

/// Dateiname ohne `.md` (gleich welche Schreibweise der Endung).
pub(crate) fn titel_aus_pfad(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.len().checked_sub(3).and_then(|i| name.get(i..).map(|e| (i, e))) {
        Some((i, ext)) if ext.eq_ignore_ascii_case(".md") => &name[..i],
        _ => name,
    }
}

/// Gleicher Filter wie [`zaehlt_als_quelle`], nur aus dem Pfad – für die schreibenden Befehle.
pub(crate) fn pfad_zaehlt_als_quelle(rel: &str) -> bool {
    quelle_erlaubt(&scan::parent_folder(rel), titel_aus_pfad(rel))
}

/// Offen = weder erledigt (`x`/`X`) noch abgebrochen (`-`).
pub fn ist_offen(t: &Task) -> bool {
    !t.done && t.status_char != '-'
}

/// Dataview `length(text) > 3` – JavaScript zählt UTF-16-Einheiten. Filtert den
/// Vorlagen-Platzhalter `- [ ]  📅 ` (Text `📅` = 2 Einheiten).
pub fn lang_genug(text: &str) -> bool {
    text.encode_utf16().count() > 3
}

// ---------------------------------------------------------------------------------------------
// Felder lesen
// ---------------------------------------------------------------------------------------------

fn feld<'a>(m: &'a NoteMeta, key: &str) -> Option<&'a serde_json::Value> {
    m.fields.iter().find(|f| f.key.eq_ignore_ascii_case(key)).map(|f| &f.value)
}

static LF_TEXT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^LF\s*(\d{1,2})(?:\D|$)").unwrap());

/// Lernfeld als Kürzel: Wikilink → Alias (sonst Ziel); `LF4` / `LF 04` / `LF04 – …` → `LF04`;
/// Zahl `4` → `LF04`; sonst der Text getrimmt; leer → `None`.
pub fn lernfeld_label(v: &serde_json::Value) -> Option<String> {
    let text = field_as_text(v)?;
    let s = text.trim();
    if s.is_empty() {
        return None;
    }
    let nummer: Option<u32> = match LF_TEXT.captures(s) {
        Some(c) => c[1].parse().ok(),
        None if s.len() <= 2 && s.chars().all(|c| c.is_ascii_digit()) => s.parse().ok(),
        None => None,
    };
    Some(match nummer {
        Some(n) => format!("LF{n:02}"),
        None => s.to_string(),
    })
}

/// `LF04` → 4 (für die Sortierung).
fn lernfeld_nummer(label: Option<&str>) -> Option<u32> {
    label.and_then(|l| l.strip_prefix("LF")).and_then(|n| n.parse().ok())
}

/// `leicht`/`1` → 1, `mittel`/`2` → 2, `schwer`/`3` → 3, sonst 9 (Bestand ist uneinheitlich, SPEC §5.3).
pub fn schwierigkeit_rang(s: Option<&str>) -> u8 {
    match s.map(|s| s.trim().to_lowercase()).as_deref() {
        Some("leicht" | "1") => 1,
        Some("mittel" | "2") => 2,
        Some("schwer" | "3") => 3,
        _ => 9,
    }
}

// ---------------------------------------------------------------------------------------------
// Übersicht
// ---------------------------------------------------------------------------------------------

fn auftrag(m: &NoteMeta, t: &Task, lernfeld: &Option<String>) -> Arbeitsauftrag {
    Arbeitsauftrag {
        path: m.path.clone(),
        title: m.title.clone(),
        kind: m.kind,
        fach: m.fach.clone(),
        lernfeld: lernfeld.clone(),
        datum: m.datum,
        line: t.line,
        text: t.text.clone(),
        done: t.done,
        status_char: t.status_char,
        due: t.due,
        indent: t.indent,
        privat: m.private,
    }
}

/// Alle Listen der Seite „Aufgaben“ für den Tag `heute`.
pub fn uebersicht(idx: &VaultIndex, heute: NaiveDate) -> AufgabenUebersicht {
    let bis = heute + Duration::days(7);
    let mut out = AufgabenUebersicht {
        heute,
        ueberfaellig: Vec::new(),
        diese_woche: Vec::new(),
        spaeter: Vec::new(),
        ohne_frist: Vec::new(),
        zuletzt_erledigt: Vec::new(),
        lernaufgaben: lernaufgaben(idx),
    };
    let mut erledigt: Vec<(i64, Arbeitsauftrag)> = Vec::new();
    for m in idx.notes().filter(|m| zaehlt_als_quelle(m)) {
        let lernfeld = feld(m, "lernfeld").and_then(lernfeld_label);
        for t in &m.tasks {
            if t.done {
                erledigt.push((m.modified_ms, auftrag(m, t, &lernfeld)));
                continue;
            }
            if !ist_offen(t) {
                continue;
            }
            match t.due {
                Some(d) if d < heute => out.ueberfaellig.push(auftrag(m, t, &lernfeld)),
                Some(d) if d <= bis => out.diese_woche.push(auftrag(m, t, &lernfeld)),
                Some(_) => out.spaeter.push(auftrag(m, t, &lernfeld)),
                None if lang_genug(&t.text) => out.ohne_frist.push(auftrag(m, t, &lernfeld)),
                None => {}
            }
        }
    }
    for list in [&mut out.ueberfaellig, &mut out.diese_woche, &mut out.spaeter] {
        list.sort_by(|a, b| (a.due, &a.path, a.line).cmp(&(b.due, &b.path, b.line)));
    }
    out.ohne_frist.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    // Dataview `SORT file.mtime DESC` – bei gleicher Zeit fest nach Pfad und Zeile
    erledigt.sort_by(|(ma, a), (mb, b)| mb.cmp(ma).then_with(|| (&a.path, a.line).cmp(&(&b.path, b.line))));
    out.zuletzt_erledigt = erledigt.into_iter().take(ZULETZT_MAX).map(|(_, a)| a).collect();
    out
}

fn in_aufgabensammlung(folder: &str) -> bool {
    folder == AUFGABENSAMMLUNG_DIR
        || folder.strip_prefix(AUFGABENSAMMLUNG_DIR).is_some_and(|rest| rest.starts_with('/'))
}

/// Dataview `FROM "07 Material/Aufgabensammlung" WHERE file.name != "Aufgabensammlung"`.
pub fn ist_lernaufgabe(m: &NoteMeta) -> bool {
    in_aufgabensammlung(&m.folder) && m.title != AUFGABENSAMMLUNG_TITEL
}

/// Gleiche Regel aus dem Pfad – für „gelöst“ setzen.
pub fn ist_lernaufgabe_pfad(rel: &str) -> bool {
    in_aufgabensammlung(&scan::parent_folder(rel)) && titel_aus_pfad(rel) != AUFGABENSAMMLUNG_TITEL
}

/// Alle Lernaufgaben (auch gelöste – die Oberfläche blendet sie aus), sortiert nach Lernfeld
/// (ohne zuletzt), Schwierigkeit, Titel.
pub fn lernaufgaben(idx: &VaultIndex) -> Vec<Lernaufgabe> {
    let text = |m: &NoteMeta, k: &str| feld(m, k).and_then(field_as_text).filter(|s| !s.is_empty());
    let mut out: Vec<Lernaufgabe> = idx
        .notes()
        .filter(|m| ist_lernaufgabe(m))
        .map(|m| Lernaufgabe {
            path: m.path.clone(),
            title: m.title.clone(),
            fach: m.fach.clone(),
            lernfeld: feld(m, "lernfeld").and_then(lernfeld_label),
            typ: text(m, "typ"),
            schwierigkeit: text(m, "schwierigkeit"),
            // nur YAML-`true` – `"true"` als Text ist für Dataview `!= true`
            geloest: feld(m, GELOEST_KEY) == Some(&serde_json::Value::Bool(true)),
            quelle: text(m, "quelle"),
            nochmal: m.tags.iter().any(|t| tags::tag_passt(t, "nochmal")),
        })
        .collect();
    out.sort_by_cached_key(|a| {
        let nr = lernfeld_nummer(a.lernfeld.as_deref());
        (nr.is_none(), nr, schwierigkeit_rang(a.schwierigkeit.as_deref()), a.title.to_lowercase(), a.path.clone())
    });
    out
}

// ---------------------------------------------------------------------------------------------
// Schreiben: frisch lesen, eine Stelle ändern, mit Basis-Version speichern
// ---------------------------------------------------------------------------------------------

/// Liest die Datei frisch von der Platte: Text (BOM weg, LF) und Version. Kein UTF-8 →
/// `NotEditable`; gemischte Zeilenenden → `InvalidInput` (sonst würde das Speichern sie
/// vereinheitlichen und mehr als die eine Stelle ändern).
pub(crate) fn frisch_lesen(abs: &Path, rel: &str, was: &str) -> AppResult<(String, NoteVersion)> {
    let (bytes, version) = match write::read_versioned(abs, &RetryPolicy::short()) {
        Ok(x) => x,
        Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(AppError::NoteNotFound(rel.into())),
        Err(e) => return Err(e),
    };
    let d = write::decode(&bytes);
    if !d.utf8 {
        return Err(AppError::NotEditable(rel.into()));
    }
    if d.line_ending == LineEnding::Mixed {
        return Err(AppError::InvalidInput(format!(
            "„{rel}“ hat gemischte Zeilenenden – die App ändert dort nichts, damit sich sonst nichts verschiebt. Bitte in Obsidian {was}."
        )));
    }
    Ok((d.text, version))
}

pub(crate) fn bericht(rel: &str, version: NoteVersion, warnungen: Vec<String>) -> WriteReport {
    WriteReport { rel_path: rel.to_string(), created: false, version, created_folders: vec![], warnungen }
}

pub(crate) fn zu_oft_geaendert(rel: &str, was: &str) -> AppError {
    AppError::Other(format!("„{rel}“ wurde gerade mehrfach geändert – {was}. Bitte erneut versuchen."))
}

/// Bytebereich der 1-basierten Zeile `nr` (ohne `\n`).
pub(crate) fn zeilen_bereich(text: &str, nr: usize) -> Option<std::ops::Range<usize>> {
    let mut start = 0;
    for (i, z) in text.split_inclusive('\n').enumerate() {
        if i + 1 == nr {
            let inhalt = z.strip_suffix('\n').unwrap_or(z);
            return Some(start..start + inhalt.len());
        }
        start += z.len();
    }
    None
}

/// Gleicher Anfang wie `vault::tasks` – findet die Klammer, deren Inhalt das Statuszeichen ist.
static CHECKBOX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*(?:[-*+]|\d+[.)]) \[(?P<status>.)\]").unwrap());

/// Was „Abhaken“ tun würde – reine Berechnung auf dem dekodierten Text (LF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbhakenPlan {
    pub line: usize,
    pub verschoben: bool,
    /// `None` = schon im Zielzustand
    pub neu: Option<String>,
}

fn nicht_mehr_so(aufgabe: &str, rel: &str) -> AppError {
    AppError::InvalidInput(format!(
        "Die Aufgabe „{aufgabe}“ steht nicht mehr so in „{}“ – bitte Liste neu laden.",
        titel_aus_pfad(rel)
    ))
}

/// Aufgabe an `line` mit Text `aufgabe` nehmen; steht sie dort nicht mehr, genau **eine** andere mit
/// gleichem Text, die noch nicht im Zielzustand ist (sonst Fehler). Geändert wird nur das Zeichen
/// zwischen `[` und `]`: erledigt → `x`, offen → ` ` (kein `✅ Datum`). Zeilennummern wie im Index;
/// Aufgaben in Codeblöcken gibt es nicht.
pub fn plane_abhaken(text: &str, rel: &str, line: usize, aufgabe: &str, erledigt: bool) -> AppResult<AbhakenPlan> {
    let note = NoteText::parse(text);
    let liste = tasks::parse_tasks(note.body(), note.body_line_offset());
    let im_ziel = |t: &Task| t.done == erledigt;
    let (task, verschoben) = match liste.iter().find(|t| t.line == line && t.text == aufgabe) {
        Some(t) => (t, false),
        None => match liste.iter().filter(|t| t.text == aufgabe && !im_ziel(t)).collect::<Vec<_>>().as_slice() {
            [t] => (*t, true),
            _ => return Err(nicht_mehr_so(aufgabe, rel)),
        },
    };
    if im_ziel(task) {
        return Ok(AbhakenPlan { line: task.line, verschoben, neu: None });
    }
    let bereich = zeilen_bereich(text, task.line).ok_or_else(|| nicht_mehr_so(aufgabe, rel))?;
    let status = CHECKBOX
        .captures(&text[bereich.clone()])
        .and_then(|c| c.name("status"))
        .map(|m| bereich.start + m.start()..bereich.start + m.end())
        .ok_or_else(|| nicht_mehr_so(aufgabe, rel))?;
    let zeichen = if erledigt { "x" } else { " " };
    let neu = format!("{}{zeichen}{}", &text[..status.start], &text[status.end..]);
    Ok(AbhakenPlan { line: task.line, verschoben, neu: Some(neu) })
}

/// Aufgabe in der Datei abhaken bzw. wieder öffnen.
pub fn abhaken(root: &Path, req: &AufgabeAbhaken, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<AufgabeErgebnis> {
    let (abs, rel) = guard::resolve_for_write(root, &req.path)?;
    if !pfad_zaehlt_als_quelle(&rel) {
        return Err(AppError::InvalidInput(format!(
            "„{rel}“ zählt nicht als Quelle für Aufgaben („99 Meta“ oder Dateiname mit „!“) – dort hakt die App nichts ab."
        )));
    }
    for _ in 0..VERSUCHE {
        let (text, version) = frisch_lesen(&abs, &rel, "abhaken")?;
        let plan = plane_abhaken(&text, &rel, req.line, &req.text, req.erledigt)?;
        let Some(neu) = plan.neu else {
            return Ok(AufgabeErgebnis { report: None, line: plan.line, verschoben: plan.verschoben });
        };
        match write::save(root, &rel, &neu, &version, p, writes)? {
            SaveOutcome::Saved { version, warnungen } => {
                return Ok(AufgabeErgebnis {
                    report: Some(bericht(&rel, version, warnungen)),
                    line: plan.line,
                    verschoben: plan.verschoben,
                })
            }
            SaveOutcome::Unchanged(_) => return Ok(AufgabeErgebnis { report: None, line: plan.line, verschoben: plan.verschoben }),
            // Obsidian hat dazwischen gespeichert → frisch lesen und neu planen
            SaveOutcome::Conflict { .. } => continue,
            SaveOutcome::Missing => return Err(AppError::NoteNotFound(rel)),
        }
    }
    Err(zu_oft_geaendert(&rel, "nichts abgehakt"))
}

fn yaml_fehler(rel: &str, e: &str) -> AppError {
    AppError::InvalidInput(format!(
        "Die Frontmatter von „{}“ ist kein gültiges YAML ({e}) – „gelöst“ bitte in Obsidian setzen.",
        titel_aus_pfad(rel)
    ))
}

/// Neuer Text mit `geloest: true|false` – `None`, wenn schon so. Ändert nur diese eine Zeile (bzw.
/// hängt sie an die Frontmatter an) und prüft danach, dass alle anderen Felder gleich geblieben sind.
pub fn plane_geloest(text: &str, rel: &str, geloest: bool) -> AppResult<Option<String>> {
    let mut note = NoteText::parse(text);
    let vorher = note.fields().map_err(|e| yaml_fehler(rel, &e))?;
    let feld = vorher.iter().find(|f| f.key.eq_ignore_ascii_case(GELOEST_KEY));
    let aktuell = feld.is_some_and(|f| f.value == serde_json::Value::Bool(true));
    if aktuell == geloest {
        return Ok(None);
    }
    // Schreibweise des Schlüssels aus der Datei (`Geloest:` bleibt `Geloest:`)
    let key = feld.map(|f| f.key.clone()).unwrap_or_else(|| GELOEST_KEY.to_string());
    let wert = if geloest { "true" } else { "false" };
    let nicht_eindeutig = || {
        AppError::InvalidInput(format!(
            "„{key}“ lässt sich in „{}“ nicht eindeutig setzen – bitte in Obsidian auf {wert} setzen.",
            titel_aus_pfad(rel)
        ))
    };
    if !note.set_scalar(&key, wert) {
        return Err(nicht_eindeutig()); // z. B. `geloest: "true"` (Text): gleich geschrieben, aber kein Bool
    }
    let neu = note.render();
    let nachher = NoteText::parse(&neu).fields().map_err(|_| nicht_eindeutig())?;
    let ohne = |v: &[FrontmatterField]| -> Vec<FrontmatterField> {
        v.iter().filter(|f| !f.key.eq_ignore_ascii_case(GELOEST_KEY)).cloned().collect()
    };
    let treffer: Vec<&FrontmatterField> = nachher.iter().filter(|f| f.key.eq_ignore_ascii_case(GELOEST_KEY)).collect();
    let passt = ohne(&vorher) == ohne(&nachher)
        && matches!(treffer.as_slice(), [f] if f.value == serde_json::Value::Bool(geloest));
    if !passt {
        return Err(nicht_eindeutig());
    }
    Ok(Some(neu))
}

/// `geloest:` einer Lernaufgabe setzen. `None` = war schon so, nichts geschrieben.
pub fn lernaufgabe_geloest(
    root: &Path,
    path: &str,
    geloest: bool,
    writes: &SelfWrites,
    p: &RetryPolicy,
) -> AppResult<Option<WriteReport>> {
    let (abs, rel) = guard::resolve_for_write(root, path)?;
    if !ist_lernaufgabe_pfad(&rel) {
        return Err(AppError::InvalidInput(format!(
            "„{rel}“ ist keine Lernaufgabe aus „{AUFGABENSAMMLUNG_DIR}/“."
        )));
    }
    for _ in 0..VERSUCHE {
        let (text, version) = frisch_lesen(&abs, &rel, "als gelöst markieren")?;
        let Some(neu) = plane_geloest(&text, &rel, geloest)? else { return Ok(None) };
        match write::save(root, &rel, &neu, &version, p, writes)? {
            SaveOutcome::Saved { version, warnungen } => return Ok(Some(bericht(&rel, version, warnungen))),
            SaveOutcome::Unchanged(_) => return Ok(None),
            SaveOutcome::Conflict { .. } => continue,
            SaveOutcome::Missing => return Err(AppError::NoteNotFound(rel)),
        }
    }
    Err(zu_oft_geaendert(&rel, "„gelöst“ nicht gesetzt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NoteKind;
    use std::fs;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    /// Index aus (Pfad, Text, Änderungszeit) – ohne Platte, wie der Scan es täte.
    fn index_mit(notizen: &[(&str, &str, i64)]) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = VaultIndex::build(dir.path()).unwrap();
        for (rel, text, mtime) in notizen {
            idx.insert_meta(scan::analyze(rel, &NoteText::parse(text), *mtime, text.len() as u64));
        }
        (dir, idx)
    }

    fn zeilen(v: &[Arbeitsauftrag]) -> Vec<(&str, usize)> {
        v.iter().map(|a| (a.path.as_str(), a.line)).collect()
    }

    const GEC: &str = "03 Unterricht/GEC 16.11.2026.md";

    #[test]
    fn dataview_filter_und_eimer() {
        let gec = "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: GEC\nlernfeld: \"[[LF04 – Werteströme|LF04]]\"\n---\n\
                   - [ ] gestern 📅 2026-11-15\n\
                   - [ ] heute 📅 2026-11-16\n\
                   - [ ] plus sieben 📅 2026-11-23\n\
                   - [ ] plus acht 📅 2026-11-24\n\
                   - [ ]  📅 \n\
                   - [ ] Abc\n\
                   - [ ] Abcd\n\
                   - [ ] a😀\n\
                   - [ ] ab😀\n\
                   - [-] abgebrochen 📅 2026-11-01\n\
                   - [x] erledigt 📅 2026-11-01\n\
                   - [/] angefangen 📅 2026-11-16\n\
                   ```\n- [ ] im Codeblock 📅 2026-11-01\n```\n";
        let (_d, idx) = index_mit(&[
            (GEC, gec, 5),
            ("99 Meta/Templates/Unterrichtsnotiz.md", "- [ ] Vorlage 📅 2026-11-01\n- [ ] Vorlage ohne Frist\n", 1),
            ("Archiv/99 Meta alt/alt.md", "- [ ] tief in 99 Meta 📅 2026-11-01\n", 1),
            ("00 Start/!Entwurf.md", "- [ ] Entwurf 📅 2026-11-01\n- [ ] Entwurf ohne Frist\n", 1),
            ("10 Privat/Lern-Log/x.md", "- [ ] privat überfällig 📅 2026-10-01\n", 1),
        ]);
        let u = uebersicht(&idx, d(2026, 11, 16));
        assert_eq!(u.heute, d(2026, 11, 16));
        assert_eq!(zeilen(&u.ueberfaellig), vec![("10 Privat/Lern-Log/x.md", 1), (GEC, 7)]);
        assert!(u.ueberfaellig[0].privat && !u.ueberfaellig[1].privat);
        // gleiche Frist: Pfad, dann Zeile
        assert_eq!(zeilen(&u.diese_woche), vec![(GEC, 8), (GEC, 18), (GEC, 9)]);
        assert_eq!(zeilen(&u.spaeter), vec![(GEC, 10)]);
        // Platzhalter (`📅` = 2), `Abc` (3) und `a😀` (3 UTF-16-Einheiten) fallen raus
        let ohne: Vec<&str> = u.ohne_frist.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(ohne, vec!["Abcd", "ab😀"]);
        assert_eq!(zeilen(&u.zuletzt_erledigt), vec![(GEC, 17)]);
        let a = &u.diese_woche[0];
        assert_eq!((a.kind, a.fach.as_deref(), a.lernfeld.as_deref()), (NoteKind::Unterricht, Some("GEC"), Some("LF04")));
        assert_eq!((a.title.as_str(), a.datum, a.status_char, a.indent), ("GEC 16.11.2026", Some(d(2026, 11, 16)), ' ', 0));
        assert_eq!(u.diese_woche[1].status_char, '/');
        assert!(u.lernaufgaben.is_empty());
    }

    #[test]
    fn quellfilter() {
        let m = |rel: &str| scan::analyze(rel, &NoteText::parse(""), 0, 0);
        assert!(zaehlt_als_quelle(&m("03 Unterricht/GEC 05.10.2026.md")));
        assert!(zaehlt_als_quelle(&m("Home.md")));
        assert!(!zaehlt_als_quelle(&m("99 Meta/Templates/Unterrichtsnotiz.md")));
        assert!(!zaehlt_als_quelle(&m("05 Lernen/99 Meta/x.md")));
        assert!(!zaehlt_als_quelle(&m("00 Start/!Entwurf.md")));
        assert!(zaehlt_als_quelle(&m("00 Start/Ent!wurf.md")));
        assert!(pfad_zaehlt_als_quelle("03 Unterricht/GEC 05.10.2026.md"));
        assert!(!pfad_zaehlt_als_quelle("99 Meta/Templates/Unterrichtsnotiz.md"));
        assert!(!pfad_zaehlt_als_quelle("!Entwurf.md"));
        assert_eq!(titel_aus_pfad("a/b/C.MD"), "C");
        assert_eq!(titel_aus_pfad("x.md"), "x");
        assert_eq!(titel_aus_pfad("md"), "md");
    }

    #[test]
    fn zuletzt_erledigt_nach_aenderungszeit_hoechstens_zehn() {
        let texte: Vec<(String, String, i64)> = (0..12)
            .map(|i| (format!("03 Unterricht/N{i:02}.md"), "- [x] fertig\n- [X] auch fertig\n".to_string(), i as i64 / 2))
            .collect();
        let refs: Vec<(&str, &str, i64)> = texte.iter().map(|(a, b, c)| (a.as_str(), b.as_str(), *c)).collect();
        let (_d, idx) = index_mit(&refs);
        let u = uebersicht(&idx, d(2026, 11, 16));
        assert_eq!(u.zuletzt_erledigt.len(), ZULETZT_MAX);
        // neueste Zeit zuerst (N10/N11 haben 5), dann Pfad und Zeile
        assert_eq!(
            zeilen(&u.zuletzt_erledigt)[..5],
            [("03 Unterricht/N10.md", 1), ("03 Unterricht/N10.md", 2), ("03 Unterricht/N11.md", 1), ("03 Unterricht/N11.md", 2), ("03 Unterricht/N08.md", 1)]
        );
        assert!(u.zuletzt_erledigt.iter().all(|a| a.done));
    }

    #[test]
    fn lernfeld_als_kuerzel() {
        use serde_json::json;
        for (v, erwartet) in [
            (json!("[[LF04 – Werteströme erfassen|LF04]]"), Some("LF04")),
            (json!("[[LF04 – Werteströme erfassen]]"), Some("LF04")),
            (json!("LF4"), Some("LF04")),
            (json!("LF 04"), Some("LF04")),
            (json!("lf12 – Projekte"), Some("LF12")),
            (json!(4), Some("LF04")),
            (json!("4"), Some("LF04")),
            (json!(["LF02", "LF03"]), Some("LF02")),
            (json!("LF045"), Some("LF045")),
            (json!("  Sonstiges "), Some("Sonstiges")),
            (json!(""), None),
            (json!(null), None),
            (json!(true), None),
        ] {
            assert_eq!(lernfeld_label(&v).as_deref(), erwartet, "{v}");
        }
        assert_eq!(schwierigkeit_rang(Some("Mittel")), 2);
        assert_eq!(schwierigkeit_rang(Some("3")), 3);
        assert_eq!(schwierigkeit_rang(Some("leicht")), 1);
        assert_eq!(schwierigkeit_rang(Some("knifflig")), 9);
        assert_eq!(schwierigkeit_rang(None), 9);
    }

    #[test]
    fn lernaufgaben_aus_der_sammlung() {
        let la = |lf: &str, schw: &str, geloest: &str, extra: &str| {
            format!("---\ntags: [aufgabe, lernaufgabe{extra}]\nfach: \"[[GEC – Geschäftsprozesse im E-Commerce|GEC]]\"\n{lf}typ: rechnung\nschwierigkeit: {schw}\n{geloest}quelle: \"[[GEC 05.10.2026]]\"\n---\n# x\n")
        };
        let a = la("lernfeld: 4\n", "schwer", "geloest: false\n", "");
        let b = la("lernfeld: \"[[LF04 – Werteströme|LF04]]\"\n", "1", "geloest: true\n", ", nochmal");
        let c = la("", "mittel", "", "");
        let e = la("lernfeld: LF2\n", "mittel", "geloest: \"true\"\n", "");
        let (_d, idx) = index_mit(&[
            ("07 Material/Aufgabensammlung/A Zuschlag.md", &a, 0),
            ("07 Material/Aufgabensammlung/B Spanne.md", &b, 0),
            ("07 Material/Aufgabensammlung/Unterordner/C Ohne LF.md", &c, 0),
            ("07 Material/Aufgabensammlung/E Text-true.md", &e, 0),
            ("07 Material/Aufgabensammlung/Aufgabensammlung.md", "---\ntags: [moc]\n---\n", 0),
            ("07 Material/Aufgabensammlung alt/F.md", &a, 0),
            ("07 Material/Altklausuren/G.md", &a, 0),
        ]);
        let l = lernaufgaben(&idx);
        let titel: Vec<&str> = l.iter().map(|x| x.title.as_str()).collect();
        // LF02 vor LF04; in LF04 leicht (1) vor schwer; ohne Lernfeld zuletzt
        assert_eq!(titel, vec!["E Text-true", "B Spanne", "A Zuschlag", "C Ohne LF"]);
        assert!(!l[0].geloest, "`\"true\"` als Text ist nicht gelöst (Dataview `geloest != true`)");
        assert!(l[1].geloest && l[1].nochmal);
        assert!(!l[2].geloest && !l[2].nochmal);
        assert!(!l[3].geloest, "fehlt → nicht gelöst");
        assert_eq!(l[2].lernfeld.as_deref(), Some("LF04"));
        assert_eq!(l[3].lernfeld, None);
        assert_eq!(l[2].fach.as_deref(), Some("GEC"));
        assert_eq!(l[2].typ.as_deref(), Some("rechnung"));
        assert_eq!(l[1].schwierigkeit.as_deref(), Some("1"));
        assert_eq!(l[2].quelle.as_deref(), Some("GEC 05.10.2026"));
        assert!(ist_lernaufgabe_pfad("07 Material/Aufgabensammlung/Unterordner/C Ohne LF.md"));
        assert!(!ist_lernaufgabe_pfad("07 Material/Aufgabensammlung/Aufgabensammlung.md"));
        assert!(!ist_lernaufgabe_pfad("07 Material/Aufgabensammlung alt/F.md"));
        assert!(!ist_lernaufgabe_pfad("03 Unterricht/GEC 05.10.2026.md"));
    }

    // --- Schreiben ---------------------------------------------------------------------------

    fn datei(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }

    fn req(line: usize, text: &str, erledigt: bool) -> AufgabeAbhaken {
        AufgabeAbhaken { path: GEC.into(), line, text: text.into(), erledigt }
    }

    fn haken(root: &Path, r: &AufgabeAbhaken) -> AppResult<AufgabeErgebnis> {
        abhaken(root, r, &SelfWrites::new(), &RetryPolicy::none())
    }

    /// Indizes der Bytes, die sich unterscheiden (gleiche Länge vorausgesetzt).
    fn diff(a: &[u8], b: &[u8]) -> Vec<usize> {
        assert_eq!(a.len(), b.len());
        (0..a.len()).filter(|i| a[*i] != b[*i]).collect()
    }

    #[test]
    fn abhaken_aendert_genau_ein_byte_auch_mit_crlf_und_bom() {
        let lf = "---\ntags: [unterricht]\n---\n# GEC\n- [ ] Arbeitsblatt 📅 2026-11-20\n- [ ] Vokabeln\n";
        let crlf = format!("\u{FEFF}{}", lf.replace('\n', "\r\n"));
        for vorher in [lf.to_string(), crlf] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            datei(root, GEC, vorher.as_bytes());
            let r = haken(root, &req(5, "Arbeitsblatt", true)).unwrap();
            assert_eq!((r.line, r.verschoben), (5, false));
            let report = r.report.expect("geschrieben");
            assert_eq!(report.rel_path, GEC);
            let nachher = fs::read(root.join(GEC)).unwrap();
            let i = diff(vorher.as_bytes(), &nachher);
            assert_eq!(i.len(), 1, "genau ein Byte");
            assert_eq!(nachher[i[0]], b'x');
            assert_eq!(nachher[i[0] - 1], b'[');
            assert_eq!(report.version.sha256, write::sha256_hex(&nachher));
            // schon im Zielzustand → nichts geschrieben
            let r = haken(root, &req(5, "Arbeitsblatt", true)).unwrap();
            assert_eq!(r.report, None);
            assert_eq!(fs::read(root.join(GEC)).unwrap(), nachher);
            // wieder öffnen → Datei wie vorher
            let r = haken(root, &req(5, "Arbeitsblatt", false)).unwrap();
            assert!(r.report.is_some());
            assert_eq!(fs::read(root.join(GEC)).unwrap(), vorher.as_bytes());
            // offen und schon offen → None
            assert_eq!(haken(root, &req(6, "Vokabeln", false)).unwrap().report, None);
        }
    }

    #[test]
    fn abhaken_findet_verschobene_zeile() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // die App kennt „Vokabeln“ in Zeile 2, in Obsidian kamen zwei Zeilen davor dazu
        datei(root, GEC, b"- [ ] neu oben\n- [x] noch eine\n- [ ] Arbeitsblatt\n- [ ] Vokabeln\n");
        let r = haken(root, &req(2, "Vokabeln", true)).unwrap();
        assert_eq!((r.line, r.verschoben), (4, true));
        assert_eq!(fs::read_to_string(root.join(GEC)).unwrap(), "- [ ] neu oben\n- [x] noch eine\n- [ ] Arbeitsblatt\n- [x] Vokabeln\n");
        // anderer Text an der Zeile, gleicher Text schon erledigt → keine Kandidaten
        let r = haken(root, &req(1, "Vokabeln", true));
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
    }

    #[test]
    fn mehrdeutig_ist_fehler_und_schreibt_nichts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vorher = b"- [ ] anderes\n- [ ] Lesen S. 12\n- [ ] Lesen S. 12\n";
        datei(root, GEC, vorher);
        let r = haken(root, &req(1, "Lesen S. 12", true));
        let Err(AppError::InvalidInput(m)) = &r else { panic!("{r:?}") };
        assert_eq!(m, "Die Aufgabe „Lesen S. 12“ steht nicht mehr so in „GEC 16.11.2026“ – bitte Liste neu laden.");
        assert_eq!(fs::read(root.join(GEC)).unwrap(), vorher);
        // an der richtigen Zeile ist es eindeutig
        assert!(haken(root, &req(3, "Lesen S. 12", true)).unwrap().report.is_some());
        assert_eq!(fs::read_to_string(root.join(GEC)).unwrap(), "- [ ] anderes\n- [ ] Lesen S. 12\n- [x] Lesen S. 12\n");
    }

    #[test]
    fn codeblock_zeile_wird_nie_abgehakt() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let vorher = b"# x\n```\n- [ ] im Codeblock\n```\n~~~md\n- [ ] auch hier\n~~~\n";
        datei(root, GEC, vorher);
        for (line, text) in [(3, "im Codeblock"), (6, "auch hier")] {
            let r = haken(root, &req(line, text, true));
            assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        }
        assert_eq!(fs::read(root.join(GEC)).unwrap(), vorher);
        assert!(plane_abhaken(std::str::from_utf8(vorher).unwrap(), GEC, 3, "im Codeblock", true).is_err());
    }

    #[test]
    fn abhaken_lehnt_ab() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // gemischte Zeilenenden
        datei(root, GEC, b"- [ ] a1234\r\n- [ ] b1234\n");
        let r = haken(root, &req(1, "a1234", true));
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("gemischte Zeilenenden")), "{r:?}");
        // Vorlage und !-Datei
        for rel in ["99 Meta/Templates/Unterrichtsnotiz.md", "00 Start/!Entwurf.md"] {
            datei(root, rel, b"- [ ] a1234\n");
            let r = abhaken(root, &AufgabeAbhaken { path: rel.into(), line: 1, text: "a1234".into(), erledigt: true }, &SelfWrites::new(), &RetryPolicy::none());
            assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
            assert_eq!(fs::read(root.join(rel)).unwrap(), b"- [ ] a1234\n");
        }
        // .obsidian
        let r = abhaken(root, &AufgabeAbhaken { path: ".obsidian/x.md".into(), line: 1, text: "a".into(), erledigt: true }, &SelfWrites::new(), &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::Guard(_))), "{r:?}");
        // fehlt
        let r = abhaken(root, &AufgabeAbhaken { path: "03 Unterricht/fehlt.md".into(), line: 1, text: "a".into(), erledigt: true }, &SelfWrites::new(), &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::NoteNotFound(_))), "{r:?}");
    }

    #[test]
    fn nur_das_statuszeichen_aendert_sich() {
        let text = "1. [/] nummeriert\n\t* [-] abgebrochen\n";
        let p = plane_abhaken(text, GEC, 1, "nummeriert", true).unwrap();
        assert_eq!(p.neu.as_deref(), Some("1. [x] nummeriert\n\t* [-] abgebrochen\n"));
        let p = plane_abhaken(text, GEC, 2, "abgebrochen", true).unwrap();
        assert_eq!(p.neu.as_deref(), Some("1. [/] nummeriert\n\t* [x] abgebrochen\n"));
        // `[/]` ist nicht erledigt → „wieder öffnen“ ändert nichts
        assert_eq!(plane_abhaken(text, GEC, 1, "nummeriert", false).unwrap().neu, None);
    }

    const LA: &str = "07 Material/Aufgabensammlung/Handelsspanne berechnen.md";

    fn geloest_setzen(root: &Path, rel: &str, g: bool) -> AppResult<Option<WriteReport>> {
        lernaufgabe_geloest(root, rel, g, &SelfWrites::new(), &RetryPolicy::none())
    }

    #[test]
    fn geloest_setzen_laesst_rest_byte_gleich() {
        let lf = "---\ntags: [aufgabe, lernaufgabe]\ntyp: Rechnen # Altbestand\nschwierigkeit: 2\ngeloest: false\nquelle: \"[[GEC 05.10.2026]]\"\n---\n# Handelsspanne\n\n> [!success]- Lösung\n> 60 %\n";
        let crlf = format!("\u{FEFF}{}", lf.replace('\n', "\r\n"));
        for vorher in [lf.to_string(), crlf] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            datei(root, LA, vorher.as_bytes());
            assert!(geloest_setzen(root, LA, true).unwrap().is_some());
            let nachher = String::from_utf8(fs::read(root.join(LA)).unwrap()).unwrap();
            assert_eq!(nachher, vorher.replace("geloest: false", "geloest: true"));
            assert_eq!(geloest_setzen(root, LA, true).unwrap(), None, "schon gelöst");
            assert!(geloest_setzen(root, LA, false).unwrap().is_some());
            assert_eq!(fs::read(root.join(LA)).unwrap(), vorher.as_bytes());
            assert_eq!(geloest_setzen(root, LA, false).unwrap(), None);
        }
    }

    #[test]
    fn geloest_sonderfaelle() {
        // fehlt → angehängt; fehlt und `false` → nichts zu tun
        let t = "---\ntags: [aufgabe]\n---\nText\n";
        assert_eq!(plane_geloest(t, LA, false).unwrap(), None);
        assert_eq!(plane_geloest(t, LA, true).unwrap().as_deref(), Some("---\ntags: [aufgabe]\ngeloest: true\n---\nText\n"));
        // Schreibweise des Schlüssels bleibt
        let t = "---\nGeloest: false\n---\n";
        assert_eq!(plane_geloest(t, LA, true).unwrap().as_deref(), Some("---\nGeloest: true\n---\n"));
        // `"true"` als Text: Dataview sieht „nicht gelöst“, Schreiben wäre unsichtbar → Fehler
        let r = plane_geloest("---\ngeloest: \"true\"\n---\n", LA, true);
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        assert_eq!(plane_geloest("---\ngeloest: \"true\"\n---\n", LA, false).unwrap(), None);
        // ungültiges YAML
        let r = plane_geloest("---\ntags: [offen\ngeloest: false\n---\n", LA, true);
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("kein gültiges YAML")), "{r:?}");
        // Schlüssel in Anführungszeichen: Zeilenchirurgie fände ihn nicht → doppelt → abgelehnt
        let r = plane_geloest("---\n\"geloest\": false\n---\n", LA, true);
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        // keine Lernaufgabe
        let dir = tempfile::tempdir().unwrap();
        datei(dir.path(), GEC, b"---\ngeloest: false\n---\n");
        let r = geloest_setzen(dir.path(), GEC, true);
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        assert_eq!(fs::read(dir.path().join(GEC)).unwrap(), b"---\ngeloest: false\n---\n");
        let r = geloest_setzen(dir.path(), "07 Material/Aufgabensammlung/Aufgabensammlung.md", true);
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
    }
}
