//! Unterrichtsnotizen: Vorlage ausfüllen, Fächer und Lernfelder kennen, Notizen anlegen und listen.
//!
//! Die Vorlage `99 Meta/Templates/Unterrichtsnotiz.md` wird per **Zeilenchirurgie** ausgefüllt:
//! nur die Werte von `datum`, `fach`, `lernfeld`, `thema`, `lehrkraft`, die Überschrift und die
//! Zeile `**Fach:** …` ändern sich. Alles andere bleibt byte-gleich (auch BOM und Zeilenenden).

use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;

use crate::error::{AppError, AppResult};
use crate::guard;
use crate::index::{self, VaultIndex};
use crate::model::{
    Block, CreateResult, Fach, FachInfo, LernfeldInfo, NeueUnterrichtsnotiz, NoteKind, NoteMeta, NoteSummary,
    NotizVorschau, RelPath, Stundenplan, StundenplanStatus, Wochentag, WriteReport,
};
use crate::vault::frontmatter::{field_as_text, yaml_scalar, NoteText};
use crate::vault::{naming, scan, stundenplan};
use crate::watcher::SharedIndex;
use crate::write::{self, CreateOutcome, RetryPolicy, SelfWrites};

pub const TEMPLATE_UNTERRICHT: &str = "99 Meta/Templates/Unterrichtsnotiz.md";
pub const UNTERRICHT_DIR: &str = "03 Unterricht";
pub const FAECHER_DIR: &str = "01 Fächer";

// ---------------------------------------------------------------------------------------------
// Vorlage ausfüllen (rein, ohne Datei-I/O)
// ---------------------------------------------------------------------------------------------

pub struct FillInput<'a> {
    pub datum: NaiveDate,
    /// `GEC`
    pub kurz: &'a str,
    /// `[[GEC – Geschäftsprozesse im E-Commerce|GEC]]` (echtes `|`, nie `\|`)
    pub fach_link: &'a str,
    /// `[[LF04 – …|LF04]]`
    pub lernfeld_link: Option<&'a str>,
    /// schon normalisiert, nicht leer
    pub thema: Option<&'a str>,
    /// nicht leer
    pub lehrkraft: Option<&'a str>,
}

const W_KEINE_UEBERSCHRIFT: &str =
    "Vorlage hat keine Überschrift „# …“ – die neue Notiz bekommt keine Überschrift mit Datum und Fach.";
const W_KEIN_DATUM: &str = "Vorlage: Überschrift ohne „JJJJ-MM-TT“ – das Datum steht nur im Frontmatter.";
const W_KEIN_FACH: &str = "Vorlage: Überschrift ohne „Fach“ – das Kürzel steht nur im Frontmatter.";
const W_KEIN_THEMA: &str = "Vorlage: Überschrift ohne „Thema“ – das Thema steht nur im Frontmatter.";
const W_KEINE_FACHZEILE: &str = "Vorlage hat keine Zeile „**Fach:**“ – Fachlink steht nur im Frontmatter.";
const W_FREMDE_PLATZHALTER: &str = "Vorlage enthält Platzhalter „{{…}}“ oder „<% … %>“ (Obsidian-Vorlagen/Templater). Die App füllt sie nicht aus – sie bleiben wörtlich stehen.";
const W_KEIN_YAML: &str = "Frontmatter der neuen Notiz ist kein gültiges YAML.";

static THEMA_MIT_TRENNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*[–—-]\s*Thema\b").unwrap());

/// Zeile in Inhalt und Zeilenende zerlegen.
fn split_eol(line: &str) -> (&str, &str) {
    let content = line.strip_suffix("\r\n").or_else(|| line.strip_suffix('\n')).unwrap_or(line);
    (content, &line[content.len()..])
}

/// Erstes `word` mit Leerzeichen/Anfang davor und Leerzeichen/Ende danach.
fn find_standalone(s: &str, word: &str) -> Option<Range<usize>> {
    s.match_indices(word)
        .find(|(i, _)| {
            let end = i + word.len();
            (*i == 0 || s[..*i].ends_with(' ')) && (end == s.len() || s[end..].starts_with(' '))
        })
        .map(|(i, _)| i..i + word.len())
}

/// Ersetzt Bytebereiche des **Originals** (nie Regex-Ersetzung: `$1` bleibt wörtlich).
/// Überlappende Bereiche werden übersprungen.
fn splice(s: &str, mut edits: Vec<(Range<usize>, String)>) -> String {
    edits.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(s.len() + 64);
    let mut pos = 0;
    for (r, rep) in edits {
        if r.start < pos {
            continue;
        }
        out.push_str(&s[pos..r.start]);
        out.push_str(&rep);
        pos = r.end;
    }
    out.push_str(&s[pos..]);
    out
}

/// `**Label:**` + `[ \t]*` + `[[]]` → Bereich von `[[]]`.
fn label_link(content: &str, label: &str) -> Option<Range<usize>> {
    let start = content.find(label)? + label.len();
    let rest = &content[start..];
    let skip = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let at = start + skip;
    content[at..].starts_with("[[]]").then(|| at..at + 4)
}

fn fill_heading(content: &str, input: &FillInput, warnungen: &mut Vec<String>) -> String {
    let rest = &content[2..]; // nach "# "
    let mut edits = Vec::new();
    match rest.find("JJJJ-MM-TT") {
        Some(i) => edits.push((i..i + 10, input.datum.format("%Y-%m-%d").to_string())),
        None => warnungen.push(W_KEIN_DATUM.to_string()),
    }
    match find_standalone(rest, "Fach") {
        Some(r) => edits.push((r, input.kurz.to_string())),
        None => warnungen.push(W_KEIN_FACH.to_string()),
    }
    let thema_pos = find_standalone(rest, "Thema");
    let mut trim = false;
    match (input.thema, thema_pos) {
        (Some(t), Some(r)) => edits.push((r, t.to_string())),
        (Some(_), None) => warnungen.push(W_KEIN_THEMA.to_string()),
        (None, pos) => {
            if let Some(m) = THEMA_MIT_TRENNER.find(rest) {
                edits.push((m.range(), String::new()));
            } else if let Some(r) = pos {
                edits.push((r, String::new()));
                trim = true;
            }
        }
    }
    let mut out = splice(rest, edits);
    if trim {
        out.truncate(out.trim_end().len());
    }
    format!("# {out}")
}

fn fill_label_line(content: &str, input: &FillInput) -> String {
    let mut edits = Vec::new();
    if let Some(r) = label_link(content, "**Fach:**") {
        edits.push((r, input.fach_link.to_string()));
    }
    if let Some(l) = input.lernfeld_link {
        if let Some(r) = label_link(content, "**Lernfeld:**") {
            edits.push((r, l.to_string()));
        }
    }
    if let Some(lk) = input.lehrkraft {
        const LABEL: &str = "**Lehrkraft:**";
        if let Some(i) = content.find(LABEL) {
            let after = i + LABEL.len();
            if content[after..].chars().all(char::is_whitespace) {
                edits.push((after..content.len(), format!(" {lk}")));
            }
        }
    }
    splice(content, edits)
}

fn fill_body(body: &str, input: &FillInput, warnungen: &mut Vec<String>) -> String {
    let mut lines: Vec<String> = body.split_inclusive('\n').map(str::to_string).collect();
    match lines.iter().position(|l| split_eol(l).0.starts_with("# ")) {
        Some(i) => {
            let (content, eol) = split_eol(&lines[i]);
            lines[i] = format!("{}{eol}", fill_heading(content, input, warnungen));
        }
        None => warnungen.push(W_KEINE_UEBERSCHRIFT.to_string()),
    }
    match lines.iter().position(|l| split_eol(l).0.contains("**Fach:**")) {
        Some(i) => {
            let (content, eol) = split_eol(&lines[i]);
            lines[i] = format!("{}{eol}", fill_label_line(content, input));
        }
        None => warnungen.push(W_KEINE_FACHZEILE.to_string()),
    }
    lines.concat()
}

/// Füllt die Vorlage aus. Liefert den neuen Text und deutsche Warnungen (für die Vorschau).
pub fn fill_template(template: &str, input: &FillInput) -> (String, Vec<String>) {
    let mut warnungen = Vec::new();
    // BOM vorne abnehmen und am Ende wieder anfügen: ohne Frontmatter landete es sonst im Body
    // (vor „# …“), und neues Frontmatter stünde davor
    let (bom, template) = match template.strip_prefix('\u{FEFF}') {
        Some(rest) => ("\u{FEFF}", rest),
        None => ("", template),
    };
    let mut note = NoteText::parse(template);
    let template_yaml_ok = note.fields().is_ok();

    // Frontmatter: nur über set_scalar; datum und fach sind Pflicht (Index, Heute)
    note.set_scalar("datum", &input.datum.format("%Y-%m-%d").to_string());
    note.set_scalar("fach", &yaml_scalar(input.fach_link));
    let mut lernfeld_filled = false;
    for (key, value) in [("lernfeld", input.lernfeld_link), ("thema", input.thema), ("lehrkraft", input.lehrkraft)] {
        if let Some(v) = value {
            if note.has_key(key) {
                note.set_scalar(key, &yaml_scalar(v));
                lernfeld_filled |= key == "lernfeld";
            }
        }
    }

    let body = fill_body(note.body(), input, &mut warnungen);
    note.set_body(body);
    let out = format!("{bom}{}", note.render());

    if out.contains("{{") || out.contains("<%") {
        warnungen.push(W_FREMDE_PLATZHALTER.to_string());
    }
    if template_yaml_ok {
        let ok = NoteText::parse(&out).fields().is_ok_and(|fields| {
            let is_link = |k: &str| {
                fields
                    .iter()
                    .find(|f| f.key == k)
                    .and_then(|f| f.value.as_str())
                    .is_some_and(|s| s.starts_with("[["))
            };
            is_link("fach") && (!lernfeld_filled || is_link("lernfeld"))
        });
        if !ok {
            warnungen.push(W_KEIN_YAML.to_string());
        }
    }
    (out, warnungen)
}

/// Thema säubern: Zeilenumbrüche/Tabs → Leerzeichen, Steuerzeichen und BOM weg, Leerraum
/// zusammenfassen, höchstens 120 Zeichen. Leer → `None`.
pub fn normalize_thema(s: &str) -> Option<String> {
    let mapped: String = s
        .chars()
        .filter_map(|c| match c {
            '\t' | '\r' | '\n' | '\u{2028}' | '\u{2029}' => Some(' '),
            '\u{FEFF}' => None,
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    let collapsed = mapped.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = collapsed.chars().take(120).collect();
    let t = cut.trim_end().to_string();
    (!t.is_empty()).then_some(t)
}

static KURZ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\p{L}][\p{L}\p{N}\-]*$").unwrap());
static LF_KURZ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(LF\d{2})(\s|$)").unwrap());

/// Kürzel muss als Dateiname taugen und von `parse_unterricht` wieder erkannt werden.
pub fn validate_kurz(kurz: &str) -> AppResult<()> {
    if KURZ.is_match(kurz) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "Das Kürzel „{kurz}“ eignet sich nicht als Dateiname (nur Buchstaben, Ziffern und „-“)."
        )))
    }
}

/// `LF04 – Werteströme …` → `LF04`
pub fn lernfeld_kurz(title: &str) -> Option<String> {
    LF_KURZ.captures(title).map(|c| c[1].to_string())
}

// ---------------------------------------------------------------------------------------------
// Vorlage und Stundenplan lesen
// ---------------------------------------------------------------------------------------------

/// Alles, was `plan_note` von der Platte braucht – vorher gelesen, außerhalb jeder Sperre.
pub struct PlanInputs {
    /// roh, BOM + Zeilenenden wie auf der Platte
    pub template: String,
    pub plan: Option<Stundenplan>,
    pub status: StundenplanStatus,
}

impl PlanInputs {
    pub fn read(root: &Path, p: &RetryPolicy) -> AppResult<Self> {
        let template = read_template(root, p)?;
        let (plan, status) = load_stundenplan(root, p);
        Ok(PlanInputs { template, plan, status })
    }
}

/// Liest **nur** die Vorlage im Vault – nie `.obsidian/templates.json`, nie die eingebaute Kopie.
pub fn read_template(root: &Path, p: &RetryPolicy) -> AppResult<String> {
    let missing = || AppError::TemplateMissing(TEMPLATE_UNTERRICHT.to_string());
    let (abs, _) = guard::resolve_for_read(root, TEMPLATE_UNTERRICHT)?;
    let bytes = match write::read_versioned(&abs, p) {
        Ok((b, _)) => b,
        Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
        Err(e) => return Err(e),
    };
    String::from_utf8(bytes).map_err(|_| AppError::NotEditable(TEMPLATE_UNTERRICHT.to_string()))
}

/// `fehlt`: Datei fehlt/unlesbar; `leer`: kein Format liefert einen Tag; sonst `ok`.
pub fn load_stundenplan(root: &Path, p: &RetryPolicy) -> (Option<Stundenplan>, StundenplanStatus) {
    let Ok((abs, _)) = guard::resolve_for_read(root, stundenplan::STUNDENPLAN_PATH) else {
        return (None, StundenplanStatus::Fehlt);
    };
    let Ok((bytes, _)) = write::read_versioned(&abs, p) else { return (None, StundenplanStatus::Fehlt) };
    let text = write::decode(&bytes).text;
    let plan = stundenplan::parse_stundenplan(NoteText::parse(&text).body());
    let status = if plan.tage.iter().any(|t| !t.stunden.is_empty()) {
        StundenplanStatus::Ok
    } else {
        StundenplanStatus::Leer
    };
    (Some(plan), status)
}

// ---------------------------------------------------------------------------------------------
// Fächer und Lernfelder
// ---------------------------------------------------------------------------------------------

fn has_tag(m: &NoteMeta, tag: &str) -> bool {
    m.tags.iter().any(|t| t.eq_ignore_ascii_case(tag))
}

fn field_text(m: &NoteMeta, key: &str) -> Option<String> {
    m.fields.iter().find(|f| f.key.eq_ignore_ascii_case(key)).and_then(|f| field_as_text(&f.value))
}

/// Notiz mit diesem Titel (bevorzugt unter `01 Fächer/`).
fn ziel_notiz<'a>(idx: &'a VaultIndex, ziel: &str) -> Option<&'a NoteMeta> {
    let prefix = format!("{FAECHER_DIR}/");
    let mut best = None;
    for m in idx.notes().filter(|m| m.title.eq_ignore_ascii_case(ziel)) {
        if m.path.starts_with(&prefix) {
            return Some(m);
        }
        best.get_or_insert(m);
    }
    best
}

fn fach_info(idx: &VaultIndex, kurz: &str, ziel: &str, lehrkraft: Option<String>, im_stundenplan: bool) -> FachInfo {
    let note = ziel_notiz(idx, ziel);
    let lehrkraft = lehrkraft.filter(|l| !stundenplan::ist_platzhalter(l));
    FachInfo {
        kurz: kurz.to_string(),
        ziel: ziel.to_string(),
        link: Fach { kurz: kurz.to_string(), ziel: ziel.to_string(), lehrkraft: lehrkraft.clone() }.link(),
        lehrkraft,
        ziel_existiert: note.is_some(),
        notiz_pfad: note.map(|m| m.path.clone()),
        im_stundenplan,
    }
}

#[derive(Default)]
struct FachAcc {
    kurz: String,
    order: Option<usize>,
    ziel_tt: Option<String>,
    lehrkraft_tt: Option<String>,
    note_ziel: Option<String>,
    note_lehrkraft: Option<String>,
}

/// Fach-Verzeichnis aus Stundenplan, Fachnotizen und Links der Stundenplan-Seite.
pub fn faecher(idx: &VaultIndex, plan: Option<&Stundenplan>) -> Vec<FachInfo> {
    let mut acc: Vec<FachAcc> = Vec::new();
    fn entry<'a>(acc: &'a mut Vec<FachAcc>, kurz: &str) -> &'a mut FachAcc {
        match acc.iter().position(|a| a.kurz.eq_ignore_ascii_case(kurz)) {
            Some(i) => &mut acc[i],
            None => {
                acc.push(FachAcc { kurz: kurz.to_string(), ..Default::default() });
                acc.last_mut().unwrap()
            }
        }
    }
    // 1. Stundenplan (Montag zuerst, nach Stunde)
    let mut n = 0;
    for tag in plan.map(|p| p.tage.as_slice()).unwrap_or(&[]) {
        for s in &tag.stunden {
            let e = entry(&mut acc, &s.fach);
            if e.order.is_none() {
                e.order = Some(n);
                n += 1;
            }
            if e.ziel_tt.is_none() {
                e.ziel_tt = s.fach_ziel.clone();
            }
            if e.lehrkraft_tt.is_none() {
                e.lehrkraft_tt = s.lehrkraft.clone().filter(|l| !stundenplan::ist_platzhalter(l));
            }
        }
    }
    // 2. Fachnotizen (Tag `fach` – schließt z. B. `Logbuch GEC.md` aus)
    for m in idx.notes().filter(|m| m.kind == NoteKind::Fach && has_tag(m, "fach")) {
        let kurz = field_text(m, "kuerzel")
            .or_else(|| m.title.split_once(" – ").map(|(k, _)| k.trim().to_string()))
            .unwrap_or_else(|| m.title.clone());
        let e = entry(&mut acc, &kurz);
        if e.note_ziel.is_none() {
            e.note_ziel = Some(m.title.clone());
            e.note_lehrkraft = field_text(m, "lehrkraft").filter(|l| !stundenplan::ist_platzhalter(l));
        }
    }
    // 3./4. zusammenführen
    let links = plan.map(|p| p.fach_links.as_slice()).unwrap_or(&[]);
    let mut out: Vec<(Option<usize>, FachInfo)> = acc
        .into_iter()
        .map(|a| {
            let ziel = a
                .ziel_tt
                .clone()
                .or_else(|| a.note_ziel.clone())
                .or_else(|| links.iter().find(|f| f.kurz.eq_ignore_ascii_case(&a.kurz)).map(|f| f.ziel.clone()))
                .unwrap_or_else(|| a.kurz.clone());
            // Stundenplan-Zelle, dann Fachnotiz, dann Lehrkräfte-Tabelle der Stundenplan-Seite (Matrix)
            let lehrkraft = a.lehrkraft_tt.clone().or(a.note_lehrkraft.clone()).or_else(|| {
                links.iter().find(|f| f.kurz.eq_ignore_ascii_case(&a.kurz)).and_then(|f| f.lehrkraft.clone())
            });
            (a.order, fach_info(idx, &a.kurz, &ziel, lehrkraft, a.order.is_some()))
        })
        .collect();
    // 5. Stundenplan-Reihenfolge, dann alphabetisch
    out.sort_by(|(oa, a), (ob, b)| match (oa, ob) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.kurz.to_lowercase().cmp(&b.kurz.to_lowercase()),
    });
    out.into_iter().map(|(_, f)| f).collect()
}

/// Ein Block kann im Stundenplan ein eigenes Linkziel haben; Lehrkraft: Block vor Fachnotiz.
/// Gilt für Heute **und** das Anlegen (eine Regel).
pub fn fach_fuer_block(idx: &VaultIndex, reg: &FachInfo, block: &Block) -> FachInfo {
    let lehrkraft = block.lehrkraft.clone().or_else(|| reg.lehrkraft.clone());
    match &block.fach_ziel {
        Some(z) => fach_info(idx, &reg.kurz, z, lehrkraft, reg.im_stundenplan),
        None => FachInfo { lehrkraft: lehrkraft.filter(|l| !stundenplan::ist_platzhalter(l)), ..reg.clone() },
    }
}

fn lernfeld_info(m: &NoteMeta) -> LernfeldInfo {
    let kurz = lernfeld_kurz(&m.title);
    LernfeldInfo {
        path: m.path.clone(),
        titel: m.title.clone(),
        link: match &kurz {
            Some(k) => format!("[[{}|{k}]]", m.title),
            None => format!("[[{}]]", m.title),
        },
        kurz,
        nummer: field_text(m, "lernfeld").and_then(|s| s.trim().parse().ok()),
        fach: field_text(m, "fach"),
    }
}

/// Lernfelder (`LFnn – …` oder Tag `lernfeld`), ohne Übersichtsseiten. Nicht nach Fach gefiltert.
pub fn lernfelder(idx: &VaultIndex) -> Vec<LernfeldInfo> {
    let mut out: Vec<LernfeldInfo> = idx
        .notes()
        .filter(|m| m.kind == NoteKind::Lernfeld && (lernfeld_kurz(&m.title).is_some() || has_tag(m, "lernfeld")))
        .map(lernfeld_info)
        .collect();
    out.sort_by(|a, b| (a.nummer.is_none(), a.nummer, &a.titel).cmp(&(b.nummer.is_none(), b.nummer, &b.titel)));
    out
}

// ---------------------------------------------------------------------------------------------
// Unterrichtsnotizen finden
// ---------------------------------------------------------------------------------------------

/// Datum + Fach, keine Vorlage, und Unterricht (Ordner/Name) oder Tag `unterricht`.
/// Die Übersicht `03 Unterricht/Unterricht.md` hat kein Datum und zählt nicht.
pub fn is_unterricht_note(m: &NoteMeta) -> bool {
    m.datum.is_some()
        && m.fach.is_some()
        && m.kind != NoteKind::Template
        && (m.kind == NoteKind::Unterricht || has_tag(m, "unterricht"))
}

/// Block aus dem Dateinamen (`GEC 2 …` → 2), sonst 1 (auch Altbestand).
pub fn block_nr(m: &NoteMeta) -> u8 {
    naming::parse_unterricht(&m.title).map(|n| n.block).unwrap_or(1)
}

pub fn fach_matches(m: &NoteMeta, kurz: &str, ziel: &str) -> bool {
    m.fach.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(kurz) || f.eq_ignore_ascii_case(ziel))
}

/// Unterrichtsnotizen, neueste zuerst; optional nach Fach gefiltert.
pub fn list_unterricht(idx: &VaultIndex, fach: Option<&FachInfo>) -> Vec<NoteSummary> {
    let mut notes: Vec<&NoteMeta> = idx
        .notes()
        .filter(|m| is_unterricht_note(m))
        .filter(|m| match fach {
            Some(f) => fach_matches(m, &f.kurz, &f.ziel),
            None => true,
        })
        .collect();
    notes.sort_by(|a, b| b.datum.cmp(&a.datum).then(b.modified_ms.cmp(&a.modified_ms)).then(a.path.cmp(&b.path)));
    notes.into_iter().map(NoteSummary::from).collect()
}

/// Gibt es diese Notiz schon? (a) gleicher Titel irgendwo im Vault, oder (b) eine
/// Unterrichtsnotiz mit gleichem Datum, Fach und Block (fängt Altbestand-Namen).
pub fn find_existing(
    idx: &VaultIndex,
    kurz: &str,
    ziel: &str,
    datum: NaiveDate,
    block: u8,
    stem: &str,
) -> Option<RelPath> {
    idx.notes()
        .find(|m| m.title.eq_ignore_ascii_case(stem))
        .or_else(|| {
            idx.notes().find(|m| {
                is_unterricht_note(m) && m.datum == Some(datum) && fach_matches(m, kurz, ziel) && block_nr(m) == block
            })
        })
        .map(|m| m.path.clone())
}

// ---------------------------------------------------------------------------------------------
// Planen und Anlegen
// ---------------------------------------------------------------------------------------------

/// Trockenlauf ohne I/O: Pfad, Inhalt, Links, Warnungen.
pub fn plan_note(idx: &VaultIndex, inputs: &PlanInputs, req: &NeueUnterrichtsnotiz) -> AppResult<NotizVorschau> {
    // 1. Anfrage prüfen
    validate_kurz(&req.fach)?;
    if !(1..=9).contains(&req.block) {
        return Err(AppError::InvalidInput("Block muss zwischen 1 und 9 liegen.".into()));
    }
    let thema = req.thema.as_deref().and_then(normalize_thema);

    // 2. Fach und Block
    let registry = faecher(idx, inputs.plan.as_ref());
    let reg = registry
        .iter()
        .find(|f| f.kurz.eq_ignore_ascii_case(&req.fach))
        .ok_or_else(|| AppError::UnknownFach(req.fach.clone()))?;
    let kurz = reg.kurz.clone();
    let block = inputs
        .plan
        .as_ref()
        .and_then(|p| Wochentag::from_chrono(req.datum.weekday()).and_then(|w| p.tag(w)))
        .and_then(|t| {
            t.bloecke().into_iter().filter(|b| b.fach.eq_ignore_ascii_case(&kurz)).nth(usize::from(req.block) - 1)
        });
    let f = match &block {
        Some(b) => fach_fuer_block(idx, reg, b),
        None => reg.clone(),
    };

    // 3. Lernfeld
    let lernfeld = match &req.lernfeld {
        Some(path) => {
            let m = idx
                .get(path)
                .filter(|m| m.kind == NoteKind::Lernfeld)
                .ok_or_else(|| AppError::InvalidInput(format!("Lernfeld „{path}“ nicht gefunden.")))?;
            Some(lernfeld_info(m))
        }
        None => None,
    };

    // 4./5. Pfad und Dubletten
    let filename = naming::unterricht_filename(&kurz, req.block, req.datum);
    let rel_path = format!("{UNTERRICHT_DIR}/{filename}");
    let stem = filename.trim_end_matches(".md");
    let existiert = find_existing(idx, &kurz, &f.ziel, req.datum, req.block, stem);

    // 6. Vorlage ausfüllen
    let lehrkraft = f.lehrkraft.clone().filter(|l| !l.trim().is_empty());
    let input = FillInput {
        datum: req.datum,
        kurz: &kurz,
        fach_link: &f.link,
        lernfeld_link: lernfeld.as_ref().map(|l| l.link.as_str()),
        thema: thema.as_deref(),
        lehrkraft: lehrkraft.as_deref(),
    };
    let (text, fill_warnungen) = fill_template(&inputs.template, &input);

    // 7. Warnungen
    let mut warnungen = Vec::new();
    if !f.ziel_existiert {
        warnungen.push(format!("Fachnotiz „{}“ fehlt im Vault – der Link zeigt noch ins Leere.", f.ziel));
    }
    if inputs.status != StundenplanStatus::Ok {
        warnungen.push("Stundenplan nicht gefunden – Lehrkraft und Fachlink aus „01 Fächer/“.".to_string());
    }
    warnungen.extend(fill_warnungen);

    Ok(NotizVorschau {
        rel_path,
        text,
        fach_link: f.link.clone(),
        lernfeld_link: lernfeld.map(|l| l.link),
        lehrkraft,
        existiert,
        warnungen,
    })
}

/// Ordner von `rel`, die es auf der Platte noch nicht gibt – von oben nach unten.
fn missing_folders(root: &Path, rel: &str) -> Vec<RelPath> {
    let folder = scan::parent_folder(rel);
    if folder.is_empty() {
        return Vec::new();
    }
    let parts: Vec<&str> = folder.split('/').collect();
    (1..=parts.len())
        .map(|i| parts[..i].join("/"))
        .filter(|f| guard::resolve_in_vault(root, f).is_ok_and(|p| !p.is_dir()))
        .collect()
}

/// Legt die Unterrichtsnotiz an (der Befehl hält dabei die globale Schreibsperre).
pub fn create_note(
    index: &SharedIndex,
    root: &Path,
    req: &NeueUnterrichtsnotiz,
    writes: &SelfWrites,
    p: &RetryPolicy,
) -> AppResult<CreateResult> {
    // 1. außerhalb jeder Sperre lesen
    let inputs = PlanInputs::read(root, &RetryPolicy::short())?;
    // 2. planen
    let v = {
        let guard = index.read().unwrap_or_else(|e| e.into_inner());
        let idx = guard.as_ref().ok_or(AppError::NoVault)?;
        plan_note(idx, &inputs, req)?
    };
    if let Some(existing) = v.existiert {
        return Ok(CreateResult::Exists { rel_path: existing });
    }
    let (_, rel) = guard::resolve_for_write(root, &v.rel_path)?;

    // 3. neue Ordner vorher bekannt machen → ihr Ereignis löst keinen Vollscan aus
    let missing = missing_folders(root, &rel);
    let with_index = |f: &dyn Fn(&mut VaultIndex)| {
        let mut guard = index.write().unwrap_or_else(|e| e.into_inner());
        if let Some(idx) = guard.as_mut().filter(|i| i.root() == root) {
            f(idx);
        }
    };
    if !missing.is_empty() {
        with_index(&|idx| idx.add_folders(&missing));
    }
    let undo_folders = || {
        let still: Vec<RelPath> = missing_folders(root, &rel).into_iter().filter(|f| missing.contains(f)).collect();
        if !still.is_empty() {
            with_index(&|idx| idx.remove_folders(&still));
        }
    };

    // 4. schreiben (ohne Index-Sperre)
    let version = match write::create_new(root, &rel, &v.text, p, writes) {
        Ok(CreateOutcome::Created(version)) => version,
        Ok(CreateOutcome::Exists(disk)) => {
            undo_folders();
            return Ok(CreateResult::Exists { rel_path: disk });
        }
        Err(e) => {
            undo_folders();
            return Err(e);
        }
    };

    // 5. sofort in den Index – nur, wenn der Vault inzwischen nicht gewechselt wurde
    match index::read_meta(root, &rel) {
        Ok(meta) => with_index(&|idx| idx.insert_meta(meta.clone())),
        Err(e) => eprintln!("[unterricht] {rel} angelegt, aber nicht eingelesen: {e}"),
    }

    Ok(CreateResult::Created {
        report: WriteReport { rel_path: rel, created: true, version, created_folders: missing, warnungen: v.warnungen },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FrontmatterField, Schultag, Stunde, Task};
    use crate::write::sha256_hex;

    const TEMPLATE: &str = include_str!("../../fixtures/vault/99 Meta/Templates/Unterrichtsnotiz.md");
    const GEC_LINK: &str = "[[GEC – Geschäftsprozesse im E-Commerce|GEC]]";
    const LF04_LINK: &str = "[[LF04 – Werteströme erfassen, auswerten und beurteilen|LF04]]";

    const REST: &str = "\n## Status\n\
        - [ ] **Notiz abgeschlossen** · danach beim Wochenabschluss nach `Abgeschlossene Notizen` verschieben\n\n\
        ## Inhalt der Stunde\n- \n\n## Wichtige Begriffe\n- [[]]\n\n## Merksätze / Formeln\n> \n\n\
        ## Aufgaben / Hausaufgaben\n- [ ]  📅 \n\n## Offene Fragen\n- #frage \n\n\
        ## Noch nicht verstanden\n- #nochmal \n\n## Verknüpft\n- [[Unterricht]]\n";

    fn golden_a() -> String {
        format!(
            "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"{GEC_LINK}\"\nlernfeld: \"{LF04_LINK}\"\n\
             thema: Kalkulation\nlehrkraft: HEIN\n---\n# 2026-11-16 GEC – Kalkulation\n\n\
             **Fach:** {GEC_LINK} · **Lernfeld:** {LF04_LINK} · **Lehrkraft:** HEIN\n{REST}"
        )
    }

    fn golden_b() -> String {
        format!(
            "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"{GEC_LINK}\"\nlernfeld: \nthema: \n\
             lehrkraft: HEIN\n---\n# 2026-11-16 GEC\n\n\
             **Fach:** {GEC_LINK} · **Lernfeld:** [[]] · **Lehrkraft:** HEIN\n{REST}"
        )
    }

    fn golden_c() -> String {
        format!(
            "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"[[D – Deutsch|D]]\"\nlernfeld: \nthema: \n\
             lehrkraft: \n---\n# 2026-11-16 D\n\n\
             **Fach:** [[D – Deutsch|D]] · **Lernfeld:** [[]] · **Lehrkraft:** \n{REST}"
        )
    }

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    fn input_a() -> FillInput<'static> {
        FillInput {
            datum: d(2026, 11, 16),
            kurz: "GEC",
            fach_link: GEC_LINK,
            lernfeld_link: Some(LF04_LINK),
            thema: Some("Kalkulation"),
            lehrkraft: Some("HEIN"),
        }
    }

    fn input_b() -> FillInput<'static> {
        FillInput { lernfeld_link: None, thema: None, ..input_a() }
    }

    #[test]
    fn vorlage_golden_gec() {
        let (out, w) = fill_template(TEMPLATE, &input_a());
        assert_eq!(out, golden_a());
        assert_eq!(out.len(), 749);
        assert_eq!(sha256_hex(out.as_bytes()), "cfa66253fd023c8898dd7a4e958935984b30b8335696881440dc6f36d732e535");
        assert!(w.is_empty(), "{w:?}");
        // nur die Zeilen 3–7, 9 und 11 unterscheiden sich von der Vorlage
        let diff: Vec<usize> = TEMPLATE
            .lines()
            .zip(out.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i + 1)
            .collect();
        assert_eq!(diff, vec![3, 4, 5, 6, 7, 9, 11]);
        assert_eq!(TEMPLATE.lines().count(), out.lines().count());
    }

    #[test]
    fn vorlage_ohne_thema_und_lernfeld() {
        let (out, w) = fill_template(TEMPLATE, &input_b());
        assert_eq!(out, golden_b());
        assert_eq!(out.len(), 594);
        assert_eq!(sha256_hex(out.as_bytes()), "bd9cdea81659b7de3bbd2694e47da5222ef46d7224d21f5b474d944f06dc0d1e");
        assert!(!w.iter().any(|x| x == W_KEIN_YAML), "{w:?}");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn vorlage_ohne_lehrkraft() {
        let input = FillInput { kurz: "D", fach_link: "[[D – Deutsch|D]]", lehrkraft: None, ..input_b() };
        let (out, w) = fill_template(TEMPLATE, &input);
        assert_eq!(out, golden_c());
        assert_eq!(out.len(), 526);
        assert_eq!(sha256_hex(out.as_bytes()), "e80a252535dd6e1609217c0856a1e6001e0d8d6eddaa772a32d22640da35f742");
        assert!(out.contains("\nlehrkraft: \n"));
        assert!(out.contains("**Lehrkraft:** \n"));
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn vorlage_crlf_und_bom() {
        let crlf = TEMPLATE.replace('\n', "\r\n");
        let (out, _) = fill_template(&crlf, &input_a());
        assert_eq!(out, golden_a().replace('\n', "\r\n"));
        let bom = format!("\u{FEFF}{TEMPLATE}");
        let (out, _) = fill_template(&bom, &input_a());
        assert_eq!(out, format!("\u{FEFF}{}", golden_a()));
        // BOM ohne Frontmatter: BOM bleibt ganz vorne, die Überschrift wird trotzdem gefüllt
        let (out, w) = fill_template("\u{FEFF}# JJJJ-MM-TT Fach – Thema\n", &input_a());
        assert!(out.starts_with("\u{FEFF}---\ndatum: 2026-11-16\n"), "{out:?}");
        assert!(out.ends_with("---\n# 2026-11-16 GEC – Kalkulation\n"), "{out:?}");
        assert_eq!(out.matches('\u{FEFF}').count(), 1);
        assert!(!w.contains(&W_KEINE_UEBERSCHRIFT.to_string()), "{w:?}");
    }

    #[test]
    fn thema_mit_doppelpunkt() {
        let input = FillInput { thema: Some("Kalkulation: Teil 2"), ..input_a() };
        let (out, w) = fill_template(TEMPLATE, &input);
        assert!(out.contains("\nthema: \"Kalkulation: Teil 2\"\n"), "{out}");
        assert!(out.contains("\n# 2026-11-16 GEC – Kalkulation: Teil 2\n"), "{out}");
        assert!(NoteText::parse(&out).fields().is_ok());
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn thema_mit_dollar() {
        let input = FillInput { thema: Some("Kosten $1 \\1"), ..input_a() };
        let (out, _) = fill_template(TEMPLATE, &input);
        assert!(out.contains("\n# 2026-11-16 GEC – Kosten $1 \\1\n"), "{out}");
        let fields = NoteText::parse(&out).fields().unwrap();
        let thema = fields.iter().find(|f| f.key == "thema").unwrap();
        assert_eq!(thema.value, serde_json::json!("Kosten $1 \\1"));
    }

    #[test]
    fn thema_normalisiert() {
        assert_eq!(normalize_thema("a\u{2028}b\u{0007}c\u{FEFF}").as_deref(), Some("a bc"));
        assert_eq!(normalize_thema("  Kalkulation\t\r\n Teil  2 ").as_deref(), Some("Kalkulation Teil 2"));
        assert_eq!(normalize_thema(" \u{FEFF}\n"), None);
        assert_eq!(normalize_thema(&"x".repeat(200)).unwrap().chars().count(), 120);
    }

    #[test]
    fn fremde_platzhalter_und_fehlende_tokens() {
        let t = "---\ndatum: {{date}}\nfach: \n---\n# Notiz\n\n**Fach:** [[]]\n<% tp.file.title %>\n";
        let (out, w) = fill_template(t, &input_a());
        assert!(w.contains(&W_FREMDE_PLATZHALTER.to_string()), "{w:?}");
        assert!(w.contains(&W_KEIN_DATUM.to_string()), "{w:?}");
        assert!(w.contains(&W_KEIN_FACH.to_string()), "{w:?}");
        assert!(w.contains(&W_KEIN_THEMA.to_string()), "{w:?}");
        assert!(out.contains("<% tp.file.title %>"));
        assert!(out.contains("\ndatum: 2026-11-16\n"), "{out}");
        // nur Obsidian-Kernvorlage „{{…}}“ (kein „<%“), die das Ausfüllen überlebt
        let t = "---\ndatum: \nfach: \n---\n# JJJJ-MM-TT Fach\n\n**Fach:** [[]]\n{{title}}\n";
        let (out, w) = fill_template(t, &input_b());
        assert!(out.contains("\n{{title}}\n") && !out.contains("<%"), "{out}");
        assert!(w.contains(&W_FREMDE_PLATZHALTER.to_string()), "{w:?}");
        let (_, w) = fill_template(TEMPLATE, &input_b());
        assert!(!w.contains(&W_FREMDE_PLATZHALTER.to_string()), "{w:?}");
        // ohne Überschrift und ohne Fachzeile
        let (out, w) = fill_template("Nur Text\n", &input_a());
        assert!(w.contains(&W_KEINE_UEBERSCHRIFT.to_string()));
        assert!(w.contains(&W_KEINE_FACHZEILE.to_string()));
        assert!(out.starts_with("---\ndatum: 2026-11-16\nfach: \""), "{out}");
        // Vorlage ohne fach-/datum-Schlüssel: werden angehängt
        let (out, _) = fill_template("---\ntags: [unterricht]\n---\n# JJJJ-MM-TT Fach\n", &input_b());
        assert!(out.starts_with(&format!("---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"{GEC_LINK}\"\n---\n")));
        assert!(out.ends_with("# 2026-11-16 GEC\n"), "{out}");
    }

    #[test]
    fn kein_escaped_pipe() {
        let (out, _) = fill_template(TEMPLATE, &input_a());
        assert!(!out.contains("\\|"));
        // Link aus der Stundenplan-Zelle (`\|`) wird ohne Backslash geschrieben
        let plan = stundenplan::parse_stundenplan(include_str!("../../fixtures/vault/00 Start/Stundenplan.md"));
        let idx = VaultIndex::build(&fixture()).unwrap();
        let f = faecher(&idx, Some(&plan));
        assert!(f.iter().all(|x| !x.link.contains('\\')), "{f:#?}");
        assert_eq!(f[0].link, GEC_LINK);
    }

    fn fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/vault")
    }

    fn stunde(nr: u8, fach: &str) -> Stunde {
        Stunde { nr, zeit: "08:35 – 09:20".into(), fach: fach.into(), fach_ziel: None, lehrkraft: None }
    }

    fn plan_mit(stunden: Vec<Stunde>, links: Vec<Fach>) -> Stundenplan {
        Stundenplan { tage: vec![Schultag { tag: Wochentag::Montag, stunden }], fach_links: links }
    }

    #[test]
    fn fach_aus_klartextzelle() {
        let idx = VaultIndex::build(&fixture()).unwrap();
        let plan = plan_mit(vec![stunde(2, "GEC")], vec![]);
        let reg = faecher(&idx, Some(&plan));
        let gec = &reg[0];
        assert_eq!(gec.kurz, "GEC");
        assert_eq!(gec.link, GEC_LINK);
        assert_eq!(gec.lehrkraft.as_deref(), Some("HEIN"));
        assert!(gec.ziel_existiert && gec.im_stundenplan);
        assert_eq!(gec.notiz_pfad.as_deref(), Some("01 Fächer/GEC/GEC – Geschäftsprozesse im E-Commerce.md"));
        // Fachnotizen ohne Stundenplan-Eintrag kommen alphabetisch danach
        let rest: Vec<&str> = reg[1..].iter().map(|f| f.kurz.as_str()).collect();
        assert_eq!(rest, vec!["D-DV", "WSP"]);
        assert!(!reg[1].im_stundenplan);
        let inputs = PlanInputs { template: TEMPLATE.into(), plan: Some(plan), status: StundenplanStatus::Ok };
        let req = NeueUnterrichtsnotiz { datum: d(2026, 11, 16), fach: "gec".into(), block: 1, lernfeld: None, thema: None };
        let v = plan_note(&idx, &inputs, &req).unwrap();
        assert!(v.warnungen.is_empty(), "{:?}", v.warnungen);
        assert_eq!(v.rel_path, "03 Unterricht/GEC 16.11.2026.md");
        assert_eq!(v.text, golden_b());
    }

    #[test]
    fn fach_aus_stundenplan_links() {
        let idx = VaultIndex::build(&fixture()).unwrap();
        let links = vec![Fach { kurz: "D".into(), ziel: "D – Deutsch".into(), lehrkraft: None }];
        let reg = faecher(&idx, Some(&plan_mit(vec![stunde(8, "D")], links)));
        assert_eq!(reg[0].link, "[[D – Deutsch|D]]");
        assert!(!reg[0].ziel_existiert);
        let plan = plan_mit(vec![stunde(8, "D")], vec![]);
        let reg = faecher(&idx, Some(&plan));
        assert_eq!(reg[0].link, "[[D]]");
        let inputs = PlanInputs { template: TEMPLATE.into(), plan: Some(plan), status: StundenplanStatus::Ok };
        let req = NeueUnterrichtsnotiz { datum: d(2026, 11, 16), fach: "D".into(), block: 1, lernfeld: None, thema: None };
        let v = plan_note(&idx, &inputs, &req).unwrap();
        assert_eq!(v.warnungen, vec!["Fachnotiz „D“ fehlt im Vault – der Link zeigt noch ins Leere.".to_string()]);
        // unbekanntes Fach
        let req = NeueUnterrichtsnotiz { fach: "XY".into(), ..req };
        assert!(matches!(plan_note(&idx, &inputs, &req), Err(AppError::UnknownFach(_))));
    }

    fn meta(path: &str, kind: NoteKind, fields: Vec<(&str, serde_json::Value)>, tags: &[&str]) -> NoteMeta {
        let fields: Vec<FrontmatterField> =
            fields.into_iter().map(|(k, v)| FrontmatterField { key: k.into(), value: v }).collect();
        let title = path.rsplit('/').next().unwrap().trim_end_matches(".md").to_string();
        let fach = fields.iter().find(|f| f.key == "fach").and_then(|f| field_as_text(&f.value));
        let datum = fields
            .iter()
            .find(|f| f.key == "datum")
            .and_then(|f| f.value.as_str())
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok());
        NoteMeta {
            path: path.into(),
            folder: scan::parent_folder(path),
            title,
            kind,
            fields,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            inline_tags: vec![],
            tasks: Vec::<Task>::new(),
            links: vec![],
            datum,
            fach,
            modified_ms: 0,
            size: 0,
            frontmatter_error: None,
            private: false,
        }
    }

    fn index_mit(notes: Vec<NoteMeta>) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = VaultIndex::build(dir.path()).unwrap();
        for n in notes {
            idx.insert_meta(n);
        }
        (dir, idx)
    }

    #[test]
    fn lernfelder_ohne_hub() {
        use serde_json::json;
        let (_d, idx) = index_mit(vec![
            meta("02 Lernfelder/Lernfelder.md", NoteKind::Lernfeld, vec![], &["moc"]),
            meta("02 Lernfelder/LF10 – Optimieren.md", NoteKind::Lernfeld, vec![("lernfeld", json!(10))], &[]),
            meta(
                "02 Lernfelder/LF04 – Werteströme erfassen, auswerten und beurteilen.md",
                NoteKind::Lernfeld,
                vec![("lernfeld", json!(4)), ("fach", json!("KSK"))],
                &["lernfeld"],
            ),
            meta("02 Lernfelder/Sonderfeld.md", NoteKind::Lernfeld, vec![], &["lernfeld"]),
        ]);
        let l = lernfelder(&idx);
        let titel: Vec<&str> = l.iter().map(|x| x.titel.as_str()).collect();
        assert_eq!(titel, vec!["LF04 – Werteströme erfassen, auswerten und beurteilen", "LF10 – Optimieren", "Sonderfeld"]);
        assert_eq!(l[0].link, LF04_LINK);
        assert_eq!((l[0].kurz.as_deref(), l[0].nummer, l[0].fach.as_deref()), (Some("LF04"), Some(4), Some("KSK")));
        assert_eq!(l[2].link, "[[Sonderfeld]]");
    }

    #[test]
    fn liste_filtert_auch_vollen_titel() {
        use serde_json::json;
        let (_d, idx) = index_mit(vec![
            meta(
                "03 Unterricht/GEC 01.10.2026.md",
                NoteKind::Unterricht,
                vec![("datum", json!("2026-10-01")), ("fach", json!("[[GEC – Geschäftsprozesse im E-Commerce]]"))],
                &["unterricht"],
            ),
            meta(
                "03 Unterricht/WSP 02.10.2026.md",
                NoteKind::Unterricht,
                vec![("datum", json!("2026-10-02")), ("fach", json!("WSP"))],
                &["unterricht"],
            ),
            meta("03 Unterricht/Unterricht.md", NoteKind::Unterricht, vec![], &["moc", "unterricht"]),
            meta(
                "99 Meta/Templates/Unterrichtsnotiz.md",
                NoteKind::Template,
                vec![("datum", json!("2026-10-03")), ("fach", json!("GEC"))],
                &["unterricht"],
            ),
        ]);
        let gec = FachInfo {
            kurz: "GEC".into(),
            ziel: "GEC – Geschäftsprozesse im E-Commerce".into(),
            link: GEC_LINK.into(),
            lehrkraft: None,
            ziel_existiert: true,
            notiz_pfad: None,
            im_stundenplan: true,
        };
        let l = list_unterricht(&idx, Some(&gec));
        assert_eq!(l.iter().map(|n| n.path.as_str()).collect::<Vec<_>>(), vec!["03 Unterricht/GEC 01.10.2026.md"]);
        let alle = list_unterricht(&idx, None);
        assert_eq!(
            alle.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
            vec!["WSP 02.10.2026", "GEC 01.10.2026"],
            "neueste zuerst, ohne Übersicht und Vorlage"
        );
    }

    #[test]
    fn lernfeld_kurz_und_link() {
        assert_eq!(lernfeld_kurz("LF04 – Werteströme").as_deref(), Some("LF04"));
        assert_eq!(lernfeld_kurz("LF12").as_deref(), Some("LF12"));
        assert_eq!(lernfeld_kurz("LF4 – x"), None);
        assert_eq!(lernfeld_kurz("Lernfelder"), None);
        assert_eq!(lernfeld_kurz("LF045 x"), None);
    }

    #[test]
    fn kuerzel_validierung() {
        for bad in ["G EC", "GEC/1", "", "1GEC", "GEC.", "-GEC"] {
            assert!(matches!(validate_kurz(bad), Err(AppError::InvalidInput(_))), "{bad:?}");
        }
        for ok in ["D-DV", "GEC", "D", "Ksk", "Ü1"] {
            assert!(validate_kurz(ok).is_ok(), "{ok}");
        }
        let msg = validate_kurz("G EC").unwrap_err().to_string();
        assert_eq!(msg, "Das Kürzel „G EC“ eignet sich nicht als Dateiname (nur Buchstaben, Ziffern und „-“).");
    }
}
