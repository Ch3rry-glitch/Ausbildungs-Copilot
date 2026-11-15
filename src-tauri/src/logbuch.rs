//! M2: Logbuch-Automatisierung (SPEC §6.3).
//!
//! * **Regel 1:** Der Eintragstext wird nie in den Vault geschrieben – nicht in eine Notiz, nicht in
//!   den Index, nicht in ein Log. Das Einzige, was dieses Modul schreibt, ist das Feld
//!   `logbuch: offen | kopiert | abgegeben` in der Frontmatter der Unterrichtsnotiz – über
//!   [`crate::write::save`], Zeile für Zeile, alles andere bleibt byte-gleich.
//! * **Regel 2:** Die Bewertung füllt nie die App aus. Ohne Zahl kein Kopieren ([`kopiertext`]).
//! * **Regel 6:** EK-Zeilen werden aus den Quellen entfernt, ein Text mit EK wird nicht kopiert.
//! * **Regel 7:** Die Vorlagenbefüllung übernimmt nur, was in der Notiz steht.
//!
//! Erlaubt ist ein Verlauf „Fach · Datum · Status · Zeitpunkt“ im App-Datenordner ([`verlauf_anhaengen`]).

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use chrono::{Duration, NaiveDate};
use regex::Regex;

use crate::error::{AppError, AppResult};
use crate::guard;
use crate::index::VaultIndex;
use crate::ki;
use crate::model::{
    AbschnittQuelle, BewertungFeld, EntwurfAbschnitt, FachInfo, LineEnding, LogbuchAbschnitt, LogbuchEntwurf,
    LogbuchFach, LogbuchFachTage, LogbuchNotiz, LogbuchQuelle, LogbuchStatus, LogbuchStatusReport, LogbuchTag,
    LogbuchUebersicht, NoteMeta, Punkt, RelPath, StatusFehler, TextAbschnitt, Umlaute, WriteReport,
};
use crate::unterricht::{block_nr, fach_matches, is_unterricht_note};
use crate::vault::frontmatter::{field_as_text, NoteText};
use crate::vault::prose_lines;
use crate::write::{self, RetryPolicy, SaveOutcome, SelfWrites};

pub const STATUS_KEY: &str = "logbuch";
pub const MOODLE_START: &str = "https://www.lebk-moodle.de/";
pub const VERLAUF_DATEI: &str = "logbuch-verlauf.jsonl";
pub const BEWERTUNG_TITEL: &str = "Bewertung dieser Lerneinheit";
/// Klausuren so viele Tage nach dem Unterrichtstag kommen unter „als Nächstes“.
const KLAUSUR_TAGE: i64 = 42;

// ---------------------------------------------------------------------------------------------
// Einstellungen
// ---------------------------------------------------------------------------------------------

/// Die drei Abschnitte, die HEIN für GEC vorgibt (Muster in `01 Fächer/GEC/Logbuch GEC.md`).
pub fn standard_abschnitte() -> Vec<LogbuchAbschnitt> {
    vec![
        LogbuchAbschnitt { titel: "Dies habe ich erarbeitet".into(), quelle: AbschnittQuelle::Erarbeitet },
        LogbuchAbschnitt { titel: "Diese Fragen haben sich ergeben".into(), quelle: AbschnittQuelle::Fragen },
        LogbuchAbschnitt { titel: "Das ist als naechstes zu tun".into(), quelle: AbschnittQuelle::Naechstes },
    ]
}

/// Voreinstellung, solange ein Fach nicht konfiguriert ist: GEC aktiv mit Bewertung 1–10 und zwei
/// Einträgen pro Woche, alle anderen Fächer aus (SPEC §6.3).
pub fn voreinstellung(kurz: &str) -> LogbuchFach {
    let gec = kurz.eq_ignore_ascii_case("GEC");
    LogbuchFach {
        kurz: kurz.to_string(),
        aktiv: gec,
        moodle_url: String::new(),
        abschnitte: standard_abschnitte(),
        selbstbewertung: gec.then_some(10),
        bewertung_titel: BEWERTUNG_TITEL.into(),
        eintraege_pro_woche: if gec { 2 } else { 1 },
        umlaute: Umlaute::Erhalten,
        markdown_entfernen: true,
        offen_ab: None,
    }
}

/// Konfiguration eines Fachs: gespeicherter Eintrag oder Voreinstellung.
pub fn fach_konfig(konfig: &[LogbuchFach], kurz: &str) -> LogbuchFach {
    konfig
        .iter()
        .find(|f| f.kurz.eq_ignore_ascii_case(kurz))
        .cloned()
        .unwrap_or_else(|| voreinstellung(kurz))
}

/// Alle bekannten Fächer (Reihenfolge des Fach-Verzeichnisses), dann nur konfigurierte.
pub fn einstellungen_liste(konfig: &[LogbuchFach], faecher: &[FachInfo]) -> Vec<LogbuchFach> {
    let mut out: Vec<LogbuchFach> = faecher.iter().map(|f| fach_konfig(konfig, &f.kurz)).collect();
    for f in konfig {
        if !out.iter().any(|o| o.kurz.eq_ignore_ascii_case(&f.kurz)) {
            out.push(f.clone());
        }
    }
    // Voreinstellung für GEC auch dann, wenn es (noch) nicht im Vault vorkommt
    if !out.iter().any(|o| o.kurz.eq_ignore_ascii_case("GEC")) && faecher.is_empty() {
        out.push(voreinstellung("GEC"));
    }
    out
}

/// Wirksame Moodle-Adresse.
pub fn moodle_url(f: &LogbuchFach) -> String {
    let u = f.moodle_url.trim();
    if u.is_empty() {
        MOODLE_START.to_string()
    } else {
        u.to_string()
    }
}

fn titel_norm(s: &str) -> String {
    s.trim().trim_end_matches(':').trim().to_string()
}

/// Prüft und vereinheitlicht die Einstellungen vor dem Speichern.
pub fn pruefe_einstellungen(liste: Vec<LogbuchFach>) -> AppResult<Vec<LogbuchFach>> {
    let ungueltig = |s: String| Err(AppError::InvalidInput(s));
    let mut out: Vec<LogbuchFach> = Vec::with_capacity(liste.len());
    for mut f in liste {
        f.kurz = f.kurz.trim().to_string();
        // Das Kürzel ist hier nur ein Schlüssel in config.json (nie ein Dateiname): es kommt so aus
        // dem Fach-Verzeichnis (Stundenplan-Zelle, Titel in „01 Fächer“) und muss unverändert zurück.
        if f.kurz.is_empty() || f.kurz.chars().count() > 80 || f.kurz.contains(['\n', '\r']) {
            return ungueltig(format!("Das Fach „{}“ hat kein brauchbares Kürzel (eine Zeile, höchstens 80 Zeichen).", f.kurz));
        }
        if out.iter().any(|o| o.kurz.eq_ignore_ascii_case(&f.kurz)) {
            return ungueltig(format!("Das Fach „{}“ steht doppelt in den Einstellungen.", f.kurz));
        }
        f.moodle_url = f.moodle_url.trim().to_string();
        let u = &f.moodle_url;
        if !u.is_empty()
            && (!u.to_ascii_lowercase().starts_with("https://") || u.len() < 12 || u.len() > 500 || u.chars().any(char::is_whitespace))
        {
            return ungueltig(format!("{}: Die Moodle-Adresse muss mit https:// beginnen (z. B. https://www.lebk-moodle.de/course/view.php?id=…).", f.kurz));
        }
        if f.abschnitte.is_empty() || f.abschnitte.len() > 8 {
            return ungueltig(format!("{}: Ein Logbuch braucht 1 bis 8 Abschnitte.", f.kurz));
        }
        for a in &mut f.abschnitte {
            a.titel = titel_norm(&a.titel);
            if a.titel.is_empty() || a.titel.chars().count() > 80 || a.titel.contains(['\n', '\r']) {
                return ungueltig(format!("{}: Jeder Abschnitt braucht einen Titel (höchstens 80 Zeichen, eine Zeile).", f.kurz));
            }
        }
        if let Some(s) = f.selbstbewertung {
            if !(2..=100).contains(&s) {
                return ungueltig(format!("{}: Die Skala der Selbstbewertung muss zwischen 2 und 100 liegen.", f.kurz));
            }
        }
        f.bewertung_titel = titel_norm(&f.bewertung_titel);
        if f.bewertung_titel.is_empty() {
            f.bewertung_titel = BEWERTUNG_TITEL.into();
        }
        if f.bewertung_titel.contains(['\n', '\r']) || f.bewertung_titel.chars().count() > 80 {
            return ungueltig(format!("{}: Der Titel der Bewertung muss eine Zeile sein (höchstens 80 Zeichen).", f.kurz));
        }
        if f.eintraege_pro_woche > 7 {
            return ungueltig(format!("{}: Höchstens 7 Einträge pro Woche.", f.kurz));
        }
        out.push(f);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------------------------

/// Status einer Notiz aus dem Index (`logbuch:` fehlt → `offen`).
pub fn status_von(m: &NoteMeta) -> LogbuchStatus {
    m.fields
        .iter()
        .find(|f| f.key.eq_ignore_ascii_case(STATUS_KEY))
        .and_then(|f| field_as_text(&f.value))
        .map(|s| LogbuchStatus::from_text(&s))
        .unwrap_or(LogbuchStatus::Offen)
}

fn status_im_text(note: &NoteText) -> AppResult<LogbuchStatus> {
    let fields = note.fields().map_err(|e| {
        AppError::InvalidInput(format!("Frontmatter ist kein gültiges YAML ({e}) – Status bitte in Obsidian setzen."))
    })?;
    Ok(fields
        .iter()
        .find(|f| f.key.eq_ignore_ascii_case(STATUS_KEY))
        .and_then(|f| field_as_text(&f.value))
        .map(|s| LogbuchStatus::from_text(&s))
        .unwrap_or(LogbuchStatus::Offen))
}

/// Neuer Text mit gesetztem Status – `None`, wenn nichts zu ändern ist. Ändert ausschließlich die
/// `logbuch:`-Zeile (bzw. hängt sie an die Frontmatter an).
pub fn text_mit_status(text: &str, status: LogbuchStatus, nur_hoeher: bool) -> AppResult<Option<String>> {
    let mut note = NoteText::parse(text);
    let aktuell = status_im_text(&note)?;
    if aktuell == status || (nur_hoeher && aktuell > status) {
        return Ok(None);
    }
    if !note.set_scalar(STATUS_KEY, status.as_str()) {
        return Ok(None);
    }
    Ok(Some(note.render()))
}

/// Was beim Setzen einer Datei herauskam.
enum EinStatus {
    Geschrieben(WriteReport),
    Unveraendert,
}

/// Liest die Datei frisch und berechnet den neuen Text (Konfliktschutz über die Version).
fn plane_status(abs: &Path, rel: &str, status: LogbuchStatus, nur_hoeher: bool) -> AppResult<Option<(String, crate::model::NoteVersion, Vec<u8>)>> {
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
            "„{rel}“ hat gemischte Zeilenenden – die App setzt dort keinen Status, damit sich nichts anderes ändert. Bitte in Obsidian setzen."
        )));
    }
    Ok(text_mit_status(&d.text, status, nur_hoeher)?.map(|t| (t, version, bytes)))
}

fn setze_eine(root: &Path, rel: &str, status: LogbuchStatus, nur_hoeher: bool, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<EinStatus> {
    let (abs, rel) = guard::resolve_for_write(root, rel)?;
    for _ in 0..3 {
        let Some((neu, version, _)) = plane_status(&abs, &rel, status, nur_hoeher)? else {
            return Ok(EinStatus::Unveraendert);
        };
        match write::save(root, &rel, &neu, &version, p, writes)? {
            SaveOutcome::Saved { version, warnungen } => {
                return Ok(EinStatus::Geschrieben(WriteReport {
                    rel_path: rel,
                    created: false,
                    version,
                    created_folders: vec![],
                    warnungen,
                }))
            }
            SaveOutcome::Unchanged(_) => return Ok(EinStatus::Unveraendert),
            // inzwischen geändert (Obsidian) → frisch lesen und nur den Status erneut setzen
            SaveOutcome::Conflict { .. } => continue,
            SaveOutcome::Missing => return Err(AppError::NoteNotFound(rel)),
        }
    }
    Err(AppError::Other(format!("„{rel}“ wurde gerade mehrfach geändert – Status nicht gesetzt. Bitte erneut versuchen.")))
}

/// Status in mehreren Notizen setzen. `nur_hoeher`: nie zurückstufen (Kopieren macht aus
/// `abgegeben` kein `kopiert`). Ändert sich mehr als eine Datei, liegt vorher eine Sicherung in
/// `backup_root/<Zeitstempel>/` (SPEC §9). Fehler je Datei landen im Bericht.
pub fn status_setzen(
    root: &Path,
    pfade: &[RelPath],
    status: LogbuchStatus,
    nur_hoeher: bool,
    writes: &SelfWrites,
    backup_root: Option<&Path>,
    p: &RetryPolicy,
) -> LogbuchStatusReport {
    let mut report = LogbuchStatusReport::default();
    // Vorlauf: welche Dateien ändern sich?
    let mut zu_aendern: Vec<(RelPath, Vec<u8>)> = Vec::new();
    for rel in pfade {
        let plan = guard::resolve_for_write(root, rel)
            .map_err(AppError::from)
            .and_then(|(abs, rel)| plane_status(&abs, &rel, status, nur_hoeher).map(|x| (rel, x)));
        match plan {
            Ok((rel, Some((_, _, bytes)))) => zu_aendern.push((rel, bytes)),
            Ok((rel, None)) => report.unveraendert.push(rel),
            Err(e) => report.fehler.push(StatusFehler { path: rel.clone(), meldung: e.to_string() }),
        }
    }
    if zu_aendern.len() > 1 {
        if let Some(b) = backup_root {
            match sicherung(b, &zu_aendern) {
                Ok(dir) => report.sicherung = Some(dir.display().to_string()),
                Err(e) => {
                    // ohne Sicherung keine Massenänderung
                    for (rel, _) in zu_aendern {
                        report.fehler.push(StatusFehler { path: rel, meldung: format!("Sicherung fehlgeschlagen, nichts geändert: {e}") });
                    }
                    return report;
                }
            }
        }
    }
    for (rel, _) in zu_aendern {
        match setze_eine(root, &rel, status, nur_hoeher, writes, p) {
            Ok(EinStatus::Geschrieben(r)) => report.geschrieben.push(r),
            Ok(EinStatus::Unveraendert) => report.unveraendert.push(rel),
            Err(e) => report.fehler.push(StatusFehler { path: rel, meldung: e.to_string() }),
        }
    }
    report
}

/// Kopien der Originale vor einer Änderung an mehreren Dateien.
fn sicherung(backup_root: &Path, dateien: &[(RelPath, Vec<u8>)]) -> std::io::Result<PathBuf> {
    let stempel = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S%.3f").to_string();
    let dir = backup_root.join(stempel);
    for (rel, bytes) in dateien {
        let ziel = rel.split('/').fold(dir.clone(), |acc, c| acc.join(c));
        if let Some(parent) = ziel.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(ziel, bytes)?;
    }
    Ok(dir)
}

// ---------------------------------------------------------------------------------------------
// Übersicht und Quellen
// ---------------------------------------------------------------------------------------------

fn thema_von(m: &NoteMeta) -> Option<String> {
    m.fields
        .iter()
        .find(|f| f.key.eq_ignore_ascii_case("thema"))
        .and_then(|f| field_as_text(&f.value))
        .filter(|s| !s.trim().is_empty())
}

fn ziel_von(faecher: &[FachInfo], kurz: &str) -> String {
    faecher.iter().find(|f| f.kurz.eq_ignore_ascii_case(kurz)).map(|f| f.ziel.clone()).unwrap_or_else(|| kurz.to_string())
}

/// Unterrichtsnotizen eines Fachs an einem Tag, nach Block sortiert.
pub fn quellnotizen<'a>(idx: &'a VaultIndex, kurz: &str, ziel: &str, datum: NaiveDate) -> Vec<&'a NoteMeta> {
    let mut v: Vec<&NoteMeta> = idx
        .notes()
        .filter(|m| is_unterricht_note(m) && m.datum == Some(datum) && fach_matches(m, kurz, ziel))
        .collect();
    v.sort_by(|a, b| (block_nr(a), &a.path).cmp(&(block_nr(b), &b.path)));
    v
}

pub fn logbuch_notiz(m: &NoteMeta) -> LogbuchNotiz {
    LogbuchNotiz {
        path: m.path.clone(),
        title: m.title.clone(),
        thema: thema_von(m),
        status: status_von(m),
        block: block_nr(m),
        privat: m.private,
    }
}

/// Je aktivem Fach die Unterrichtstage mit Status (SPEC §6.3 „Übersicht Logbuch“).
pub fn uebersicht(idx: &VaultIndex, konfig: &[LogbuchFach], faecher: &[FachInfo]) -> LogbuchUebersicht {
    let mut out = Vec::new();
    for f in einstellungen_liste(konfig, faecher).into_iter().filter(|f| f.aktiv) {
        let ziel = ziel_von(faecher, &f.kurz);
        let mut tage: BTreeMap<NaiveDate, Vec<&NoteMeta>> = BTreeMap::new();
        for m in idx.notes().filter(|m| is_unterricht_note(m) && fach_matches(m, &f.kurz, &ziel)) {
            if let Some(d) = m.datum {
                tage.entry(d).or_default().push(m);
            }
        }
        let mut liste: Vec<LogbuchTag> = tage
            .into_iter()
            .map(|(datum, mut notes)| {
                notes.sort_by(|a, b| (block_nr(a), &a.path).cmp(&(block_nr(b), &b.path)));
                let notizen: Vec<LogbuchNotiz> = notes.into_iter().map(logbuch_notiz).collect();
                let status = notizen.iter().map(|n| n.status).min().unwrap_or(LogbuchStatus::Offen);
                LogbuchTag { datum, status, zaehlt: f.offen_ab.map_or(true, |a| datum >= a), notizen }
            })
            .collect();
        liste.reverse();
        let offen = liste.iter().filter(|t| t.zaehlt && t.status == LogbuchStatus::Offen).count();
        let kopiert = liste.iter().filter(|t| t.zaehlt && t.status == LogbuchStatus::Kopiert).count();
        out.push(LogbuchFachTage {
            moodle_url: moodle_url(&f),
            kurz: f.kurz.clone(),
            ziel,
            eintraege_pro_woche: f.eintraege_pro_woche,
            offen_ab: f.offen_ab,
            tage: liste,
            offen,
            kopiert,
        });
    }
    LogbuchUebersicht { faecher: out }
}

/// Anstehende Klausuren des Fachs (Frontmatter `klausur:`) bis sechs Wochen nach `datum`.
pub fn klausuren(idx: &VaultIndex, kurz: &str, ziel: &str, datum: NaiveDate, mit_privat: bool) -> Vec<String> {
    let bis = datum + Duration::days(KLAUSUR_TAGE);
    let mut daten: Vec<NaiveDate> = Vec::new();
    for m in idx.notes() {
        if (m.private && !mit_privat) || !(fach_matches(m, kurz, ziel) || m.title.eq_ignore_ascii_case(ziel)) {
            continue;
        }
        let Some(feld) = m.fields.iter().find(|f| f.key.eq_ignore_ascii_case("klausur")) else { continue };
        let werte: Vec<String> = match &feld.value {
            serde_json::Value::Array(a) => a.iter().filter_map(field_as_text).collect(),
            v => field_as_text(v).into_iter().collect(),
        };
        for w in werte {
            if let Some(d) = datum_aus(&w) {
                if d > datum && d <= bis && !daten.contains(&d) {
                    daten.push(d);
                }
            }
        }
    }
    daten.sort();
    daten.into_iter().map(|d| format!("Klausur am {} vorbereiten", d.format("%d.%m.%Y"))).collect()
}

fn datum_aus(s: &str) -> Option<NaiveDate> {
    let s = s.trim();
    let iso = s.get(..10).and_then(|p| NaiveDate::parse_from_str(p, "%Y-%m-%d").ok());
    iso.or_else(|| s.get(..10).and_then(|p| NaiveDate::parse_from_str(p, "%d.%m.%Y").ok()))
}

// ---------------------------------------------------------------------------------------------
// Inhalt aus der Notiz ziehen (Vorlagenbefüllung ohne KI)
// ---------------------------------------------------------------------------------------------

/// Was eine Unterrichtsnotiz für den Eintrag hergibt – wörtlich, nur von Markdown-Zeichen befreit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quellinhalt {
    /// „Inhalt der Stunde“
    pub inhalt: Vec<String>,
    /// „Merksätze / Formeln“
    pub merksaetze: Vec<String>,
    /// „Wichtige Begriffe“ (einzelne Begriffe)
    pub begriffe: Vec<String>,
    /// `#frage`, „Offene Fragen“, „Noch nicht verstanden“ / `#nochmal`
    pub fragen: Vec<String>,
    /// offene Aufgaben (ohne „Status“)
    pub aufgaben: Vec<String>,
}

impl Quellinhalt {
    /// Mehrere Notizen desselben Tages zusammenführen (ohne Doppelte).
    pub fn anhaengen(&mut self, o: Quellinhalt) {
        fn add(ziel: &mut Vec<String>, neu: Vec<String>) {
            for n in neu {
                if !ziel.iter().any(|z| gleich(z, &n)) {
                    ziel.push(n);
                }
            }
        }
        add(&mut self.inhalt, o.inhalt);
        add(&mut self.merksaetze, o.merksaetze);
        add(&mut self.begriffe, o.begriffe);
        add(&mut self.fragen, o.fragen);
        add(&mut self.aufgaben, o.aufgaben);
    }

    /// Punkte für „Dies habe ich erarbeitet“: Inhalt, Merksätze, dann eine Zeile mit den Begriffen.
    pub fn erarbeitet(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inhalt.iter().chain(&self.merksaetze).cloned().collect();
        if !self.begriffe.is_empty() {
            v.push(format!("Wichtige Begriffe: {}", self.begriffe.join(", ")));
        }
        v
    }

    pub fn ist_leer(&self) -> bool {
        self.inhalt.is_empty() && self.merksaetze.is_empty() && self.begriffe.is_empty() && self.fragen.is_empty() && self.aufgaben.is_empty()
    }
}

fn gleich(a: &str, b: &str) -> bool {
    let n = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    n(a) == n(b)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bereich {
    Kopf,
    Inhalt,
    Begriffe,
    Merksaetze,
    Aufgaben,
    Fragen,
    Nochmal,
    Status,
    Verknuepft,
    Sonst,
}

fn bereich_von(ueberschrift: &str) -> Bereich {
    let t: String = ueberschrift
        .trim()
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    if t.starts_with("inhalt") {
        Bereich::Inhalt
    } else if t.starts_with("wichtige begriffe") || t.starts_with("begriffe") {
        Bereich::Begriffe
    } else if t.starts_with("merks") || t.starts_with("formeln") {
        Bereich::Merksaetze
    } else if t.starts_with("aufgaben") || t.starts_with("hausaufgaben") {
        Bereich::Aufgaben
    } else if t.starts_with("offene fragen") || t.starts_with("fragen") {
        Bereich::Fragen
    } else if t.starts_with("noch nicht verstanden") {
        Bereich::Nochmal
    } else if t.starts_with("status") {
        Bereich::Status
    } else if t.starts_with("verknüpft") || t.starts_with("verknuepft") || t.starts_with("links") {
        Bereich::Verknuepft
    } else {
        Bereich::Sonst
    }
}

static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.*)$").unwrap());
static LISTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[-*+•]|\d+[.)])(?:\s+(.*))?$").unwrap());
static AUFGABE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:[-*+]|\d+[.)])\s+\[(.)\](?:\s+(.*))?$").unwrap());
static CALLOUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[![^\]]+\][+-]?\s*(.*)$").unwrap());
static TERMIN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(📅|⏳|🛫|✅|➕|❌)\s*(\d{4}-\d{2}-\d{2})?").unwrap());
static WIEDERHOLT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*🔁[^📅⏳🛫✅➕❌]*").unwrap());
static FRAGE_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(^|[\s(\[,;])#frage\b").unwrap());
static NOCHMAL_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(^|[\s(\[,;])#nochmal\b").unwrap());
static FAELLIG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"📅\s*(\d{4}-\d{2}-\d{2})").unwrap());

/// Zeileninhalt ohne Listen-/Zitatzeichen. `None` = Leerzeile, Tabellentrenner o. Ä.
fn zeileninhalt(t: &str) -> Option<(bool, String)> {
    if let Some(rest) = t.strip_prefix('>') {
        let rest = rest.trim_start_matches(['>', ' ', '\t']);
        return Some((true, rest.trim().to_string()));
    }
    if t.starts_with('|') {
        let zellen: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).filter(|z| !z.is_empty()).collect();
        if zellen.iter().all(|z| z.chars().all(|c| matches!(c, '-' | ':' | ' '))) {
            return None;
        }
        return Some((false, zellen.join(" · ")));
    }
    if let Some(c) = LISTE.captures(t) {
        return Some((false, c.get(1).map(|m| m.as_str().trim().to_string()).unwrap_or_default()));
    }
    Some((false, t.to_string()))
}

/// Tags `#frage`/`#nochmal` und leere Links entfernen, Leerraum zusammenfassen.
fn bereinigen(s: &str) -> String {
    let s = FRAGE_TAG.replace_all(s, "$1");
    let s = NOCHMAL_TAG.replace_all(&s, "$1");
    let s = s.replace("[[]]", "");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    s.trim_matches(|c: char| c.is_whitespace() || c == '-' || c == '·').to_string()
}

/// Inhalt einer Unterrichtsnotiz (Body ohne Frontmatter) für den Eintrag. Codeblöcke zählen nicht.
pub fn quellinhalt(body: &str) -> Quellinhalt {
    let mut q = Quellinhalt::default();
    let mut sonst: Vec<String> = Vec::new();
    let mut bereich = Bereich::Kopf;
    let mut callout_titel: Option<String> = None;
    for l in prose_lines(body) {
        let t = l.raw.trim();
        if let Some(c) = HEADING.captures(t) {
            callout_titel = None;
            bereich = if c[1].len() == 1 { Bereich::Kopf } else { bereich_von(&c[2]) };
            continue;
        }
        if t.is_empty() {
            callout_titel = None;
            continue;
        }
        // Aufgaben (offen: ` ` oder `/`), nie als „erarbeitet“
        if let Some(c) = AUFGABE.captures(t) {
            let offen = matches!(&c[1], " " | "/");
            let roh = c.get(2).map(|m| m.as_str()).unwrap_or("");
            let faellig = FAELLIG.captures(roh).and_then(|d| NaiveDate::parse_from_str(&d[1], "%Y-%m-%d").ok());
            let text = bereinigen(&TERMIN.replace_all(&WIEDERHOLT.replace_all(roh, ""), ""));
            if offen && bereich != Bereich::Status && !text.is_empty() {
                q.aufgaben.push(match faellig {
                    Some(d) => format!("{text} (bis {})", d.format("%d.%m.%Y")),
                    None => text,
                });
            }
            continue;
        }
        let Some((zitat, roh)) = zeileninhalt(t) else { continue };
        if !zitat {
            callout_titel = None;
        }
        // Callout-Kopf `> [!tip] Titel`
        if zitat {
            if let Some(c) = CALLOUT.captures(&roh) {
                let titel = bereinigen(&c[1]);
                callout_titel = (!titel.is_empty()).then_some(titel);
                continue;
            }
        }
        let frage = FRAGE_TAG.is_match(&l.masked);
        let nochmal = NOCHMAL_TAG.is_match(&l.masked);
        let text = bereinigen(&roh);
        if text.is_empty() {
            continue;
        }
        if frage || bereich == Bereich::Fragen {
            q.fragen.push(text);
            continue;
        }
        if nochmal || bereich == Bereich::Nochmal {
            q.fragen.push(format!("Noch nicht verstanden: {text}"));
            continue;
        }
        match bereich {
            Bereich::Inhalt => q.inhalt.push(text),
            Bereich::Merksaetze => q.merksaetze.push(match &callout_titel {
                Some(ti) if !text.to_lowercase().starts_with(&ti.to_lowercase()) => format!("{ti}: {text}"),
                _ => text,
            }),
            Bereich::Begriffe => {
                for b in ohne_markdown(&text).split([',', ';']).map(str::trim).filter(|b| !b.is_empty()) {
                    if !q.begriffe.iter().any(|x| gleich(x, b)) {
                        q.begriffe.push(b.to_string());
                    }
                }
            }
            Bereich::Sonst if LISTE.is_match(t) => sonst.push(text),
            _ => {}
        }
    }
    // Altbestand ohne „Inhalt der Stunde“: Listenpunkte aus anderen Abschnitten
    if q.inhalt.is_empty() && q.merksaetze.is_empty() {
        q.inhalt = sonst;
    }
    let mut dedup = Quellinhalt::default();
    dedup.anhaengen(q);
    dedup
}

// ---------------------------------------------------------------------------------------------
// Text: Markdown entfernen, Umlaute, Rendern
// ---------------------------------------------------------------------------------------------

macro_rules! re {
    ($name:ident, $pat:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pat).unwrap());
    };
}
re!(MD_EMBED, r"!\[\[[^\]]*\]\]");
re!(MD_WIKI, r"\[\[([^\]|#]*)(?:#[^\]|]*)?(?:\|([^\]]*))?\]\]");
re!(MD_IMG, r"!\[[^\]]*\]\([^)]*\)");
re!(MD_LINK, r"\[([^\]]+)\]\([^)]*\)");
re!(MD_BOLD, r"\*\*(.+?)\*\*");
re!(MD_BOLD_US, r"__(.+?)__");
re!(MD_STRIKE, r"~~(.+?)~~");
re!(MD_MARK, r"==(.+?)==");
re!(MD_CODE, r"`([^`]*)`");
re!(MD_EM, r"(^|[\s(\[])\*([^\s*](?:[^*]*[^\s*])?)\*");
re!(MD_EM_US, r"(^|[\s(\[])_([^\s_](?:[^_]*[^\s_])?)_");
re!(MD_TAG, r"(^|\s)#[\p{L}\p{N}_/\-]*\p{L}[\p{L}\p{N}_/\-]*");
re!(MD_HTML, r"</?[a-zA-Z][^>]*>");
re!(MD_CALLOUT, r"^\s*\[![^\]]+\][+-]?\s*");
re!(MD_HEAD, r"^#{1,6}\s+");

/// Moodle bekommt reinen Text: Links → Linktext, Hervorhebungen, Tags und HTML weg.
pub fn ohne_markdown(s: &str) -> String {
    let s = MD_EMBED.replace_all(s, "");
    let s = MD_WIKI.replace_all(&s, |c: &regex::Captures| {
        c.get(2).filter(|a| !a.as_str().trim().is_empty()).or(c.get(1)).map(|m| m.as_str().trim().to_string()).unwrap_or_default()
    });
    let s = MD_IMG.replace_all(&s, "");
    let s = MD_LINK.replace_all(&s, "$1");
    let s = MD_BOLD.replace_all(&s, "$1");
    let s = MD_BOLD_US.replace_all(&s, "$1");
    let s = MD_STRIKE.replace_all(&s, "$1");
    let s = MD_MARK.replace_all(&s, "$1");
    let s = MD_CODE.replace_all(&s, "$1");
    let s = MD_EM.replace_all(&s, "${1}${2}");
    let s = MD_EM_US.replace_all(&s, "${1}${2}");
    let s = MD_TAG.replace_all(&s, "$1");
    let s = MD_HTML.replace_all(&s, " ");
    let s = MD_CALLOUT.replace_all(&s, "");
    let s = MD_HEAD.replace_all(&s, "");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ä → ae, ö → oe, ü → ue, ß → ss (auch Großbuchstaben).
pub fn umlaute_umschreiben(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'Ä' => out.push_str("Ae"),
            'Ö' => out.push_str("Oe"),
            'Ü' => out.push_str("Ue"),
            'ß' => out.push_str("ss"),
            'ẞ' => out.push_str("SS"),
            _ => out.push(c),
        }
    }
    out
}

/// Ein Punkt für Moodle: eine Zeile, ohne Aufzählungszeichen, ggf. ohne Markdown.
pub fn punkt_text(roh: &str, cfg: &LogbuchFach) -> String {
    let eine = roh.replace(['\r', '\n'], " ");
    let mut t = eine.trim();
    for p in ["- ", "* ", "• ", "+ "] {
        if let Some(r) = t.strip_prefix(p) {
            t = r.trim_start();
        }
    }
    let t = if cfg.markdown_entfernen { ohne_markdown(t) } else { t.split_whitespace().collect::<Vec<_>>().join(" ") };
    t.trim().to_string()
}

/// Abschnitte des Entwurfs aus den drei Inhaltslisten (je nach Quelle des Abschnitts).
pub fn abschnitte_fuellen(cfg: &LogbuchFach, erarbeitet: &[Punkt], fragen: &[Punkt], naechstes: &[Punkt]) -> Vec<EntwurfAbschnitt> {
    let saeubern = |v: &[Punkt]| -> Vec<Punkt> {
        let mut out: Vec<Punkt> = Vec::new();
        for p in v {
            let text = punkt_text(&p.text, cfg);
            if !text.is_empty() && !out.iter().any(|o| gleich(&o.text, &text)) {
                out.push(Punkt { text, vorschlag: p.vorschlag });
            }
        }
        out
    };
    cfg.abschnitte
        .iter()
        .map(|a| EntwurfAbschnitt {
            titel: a.titel.clone(),
            quelle: a.quelle,
            punkte: match a.quelle {
                AbschnittQuelle::Erarbeitet => saeubern(erarbeitet),
                AbschnittQuelle::Fragen => saeubern(fragen),
                AbschnittQuelle::Naechstes => saeubern(naechstes),
                AbschnittQuelle::Leer => vec![],
            },
        })
        .collect()
}

pub fn punkte(v: &[String]) -> Vec<Punkt> {
    v.iter().map(|t| Punkt { text: t.clone(), vorschlag: false }).collect()
}

/// Die Abschnitte der Anfrage müssen genau die der Einstellungen sein (Titel, Reihenfolge).
fn pruefe_abschnitte(cfg: &LogbuchFach, abschnitte: &[TextAbschnitt]) -> AppResult<()> {
    let passt = abschnitte.len() == cfg.abschnitte.len()
        && abschnitte.iter().zip(&cfg.abschnitte).all(|(a, c)| titel_norm(&a.titel) == titel_norm(&c.titel));
    if passt {
        Ok(())
    } else {
        Err(AppError::InvalidInput(
            "Die Abschnitte passen nicht mehr zu den Einstellungen – bitte den Entwurf neu erzeugen.".into(),
        ))
    }
}

/// Text für Moodle. Ohne Bewertung steht `__/10` (Vorschau). Die Struktur kommt immer aus den
/// Einstellungen – egal, was die Oberfläche oder die KI liefert.
pub fn render_text(cfg: &LogbuchFach, datum: NaiveDate, abschnitte: &[TextAbschnitt], bewertung: Option<u8>) -> AppResult<String> {
    pruefe_abschnitte(cfg, abschnitte)?;
    if let (Some(b), Some(skala)) = (bewertung, cfg.selbstbewertung) {
        if b < 1 || b > skala {
            return Err(AppError::InvalidInput(format!("Die Bewertung muss zwischen 1 und {skala} liegen.")));
        }
    }
    let mut s = format!("{}\n\n", datum.format("%d.%m.%Y"));
    for (a, c) in abschnitte.iter().zip(&cfg.abschnitte) {
        s.push_str(&titel_norm(&c.titel));
        s.push_str(":\n");
        let punkte: Vec<String> = a.punkte.iter().map(|p| punkt_text(p, cfg)).filter(|p| !p.is_empty()).collect();
        if punkte.is_empty() {
            s.push_str("- \n");
        }
        for p in punkte {
            s.push_str("- ");
            s.push_str(&p);
            s.push('\n');
        }
        s.push('\n');
    }
    if let Some(skala) = cfg.selbstbewertung {
        let zahl = bewertung.map(|b| b.to_string()).unwrap_or_else(|| "__".into());
        s.push_str(&format!("{}: {zahl}/{skala}", titel_norm(&cfg.bewertung_titel)));
    }
    let s = s.trim_end().to_string();
    Ok(match cfg.umlaute {
        Umlaute::Erhalten => s,
        Umlaute::Umschreiben => umlaute_umschreiben(&s),
    })
}

/// Text zum Kopieren. **Regel 2:** verlangt das Fach eine Bewertung, gibt es ohne Zahl keinen
/// Text. **Regel 6:** ein Text mit Einkaufspreis wird nicht kopiert.
pub fn kopiertext(cfg: &LogbuchFach, datum: NaiveDate, abschnitte: &[TextAbschnitt], bewertung: Option<u8>) -> AppResult<String> {
    if let (Some(skala), None) = (cfg.selbstbewertung, bewertung) {
        return Err(AppError::BewertungFehlt(skala));
    }
    let text = render_text(cfg, datum, abschnitte, bewertung)?;
    if guard::contains_purchase_price(&text) {
        return Err(AppError::InvalidInput(
            "Der Text enthält einen Einkaufspreis (EK) – der verlässt die App nie (Regel 6). Bitte die Stelle entfernen.".into(),
        ));
    }
    Ok(text)
}

/// Für die Zwischenablage unter Windows: CRLF.
pub fn fuer_zwischenablage(text: &str) -> String {
    if cfg!(windows) {
        text.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        text.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// Entwurf
// ---------------------------------------------------------------------------------------------

/// Eine Quellnotiz, frisch von der Platte gelesen.
#[derive(Debug, Clone)]
pub struct EntwurfQuelle {
    pub meta: NoteMeta,
    /// Text nach der Frontmatter
    pub body: String,
}

/// Quellnotizen frisch lesen (nicht aus dem Index – der kennt keinen Text).
pub fn quellen_lesen(root: &Path, metas: &[NoteMeta]) -> AppResult<Vec<EntwurfQuelle>> {
    metas
        .iter()
        .map(|m| {
            let c = write::read_for_edit(root, &m.path, &RetryPolicy::short())?;
            Ok(EntwurfQuelle { meta: c.meta, body: c.body })
        })
        .collect()
}

/// Zugang zur KI für einen Entwurf.
pub struct KiZugang<'a> {
    pub provider: &'a dyn ki::LlmProvider,
    pub system: &'a str,
    pub verlauf_dir: Option<&'a Path>,
}

fn dazu(ziel: &mut Vec<String>, neu: &[String]) {
    for n in neu {
        if !ziel.iter().any(|z| gleich(z, n)) {
            ziel.push(n.clone());
        }
    }
}

/// Entwurf mit KI (`ki = Some`) oder als Vorlagenbefüllung. `klausuren` kommen vom Aufrufer
/// (mit KI ohne `10 Privat/`, außer `privat_erlauben`).
#[allow(clippy::too_many_arguments)]
pub fn entwurf_erstellen(
    cfg: &LogbuchFach,
    fach_name: &str,
    datum: NaiveDate,
    quellen: &[EntwurfQuelle],
    klausuren: &[String],
    ki: Option<KiZugang>,
    privat_erlauben: bool,
) -> AppResult<LogbuchEntwurf> {
    let kopfzeile = datum.format("%d.%m.%Y").to_string();
    if quellen.is_empty() {
        return Err(AppError::InvalidInput(format!("Keine Unterrichtsnotiz für {} am {kopfzeile}.", cfg.kurz)));
    }
    let mit_ki = ki.is_some();
    let mut warnungen = Vec::new();
    let mut info = Vec::new();
    let mut alle = Quellinhalt::default();
    let mut fuer_ki = Quellinhalt::default();
    let mut ki_quellen = Vec::new();
    for q in quellen {
        let m = &q.meta;
        // Regel 6: EK-Zeilen verlassen die App nie – weder zur KI noch nach Moodle
        let ek = guard::contains_purchase_price(&q.body);
        let body = guard::strip_purchase_prices(&q.body);
        let gesendet = !m.private || privat_erlauben;
        let inhalt = quellinhalt(&body);
        alle.anhaengen(inhalt.clone());
        if gesendet {
            fuer_ki.anhaengen(inhalt);
            let thema = thema_von(m).map(|t| format!("Thema: {}\n\n", guard::strip_purchase_prices(&t))).unwrap_or_default();
            ki_quellen.push(ki::KiQuelle { titel: m.title.clone(), text: format!("{thema}{body}") });
        }
        if ek {
            warnungen.push(format!("EK-Angaben aus „{}“ wurden entfernt (Regel 6).", m.title));
        }
        if mit_ki && !gesendet {
            warnungen.push(format!("„{}“ liegt in „10 Privat/“ und wurde nicht an die KI gesendet.", m.title));
        }
        if m.frontmatter_error.is_some() {
            warnungen.push(format!("„{}“: Die Frontmatter ist kein gültiges YAML – dort kann die App keinen Status setzen.", m.title));
        }
        info.push(LogbuchQuelle {
            path: m.path.clone(),
            title: m.title.clone(),
            status: status_von(m),
            privat: m.private,
            gesendet,
            ek_entfernt: ek,
        });
    }
    let mut naechstes = alle.aufgaben.clone();
    dazu(&mut naechstes, klausuren);

    let (erarbeitet, fragen, naechstes, aufruf) = match ki {
        Some(z) => {
            if ki_quellen.is_empty() {
                return Err(AppError::InvalidInput(
                    "Alle Quellnotizen liegen in „10 Privat/“ – für diesen Vorgang einschließen oder „Ohne KI befüllen“.".into(),
                ));
            }
            let mut naechstes_ki = fuer_ki.aufgaben.clone();
            dazu(&mut naechstes_ki, klausuren);
            let kontext = ki::KiKontext {
                fach: fach_name.to_string(),
                datum: kopfzeile.clone(),
                quellen: ki_quellen,
                gefundene_fragen: fuer_ki.fragen.clone(),
                naechstes: naechstes_ki,
            };
            let (k, aufruf) = ki::logbuch_ki(z.provider, z.system, &kontext, z.verlauf_dir)?;
            let mut fragen = punkte(&k.fragen);
            fragen.extend(k.vorgeschlagene_fragen.iter().map(|t| Punkt { text: t.clone(), vorschlag: true }));
            if !k.vorgeschlagene_fragen.is_empty() {
                warnungen.push("Die markierten Fragen sind Vorschläge der KI, nicht aus deiner Notiz – bitte prüfen oder löschen.".into());
            }
            (punkte(&k.erarbeitet), fragen, punkte(&k.naechstes), Some(aufruf))
        }
        None => (punkte(&alle.erarbeitet()), punkte(&alle.fragen), punkte(&naechstes), None),
    };
    let abschnitte = abschnitte_fuellen(cfg, &erarbeitet, &fragen, &naechstes);
    if !mit_ki && alle.fragen.is_empty() && cfg.abschnitte.iter().any(|a| a.quelle == AbschnittQuelle::Fragen) {
        warnungen.push(
            "In der Notiz stehen keine Fragen (#frage, „Noch nicht verstanden“). Mit KI schlägt die App bis zu drei vor – als Vorschlag markiert."
                .into(),
        );
    }
    for a in abschnitte.iter().filter(|a| a.punkte.is_empty() && a.quelle != AbschnittQuelle::Leer) {
        warnungen.push(format!("Abschnitt „{}“ ist leer – bitte ergänzen.", a.titel));
    }
    Ok(LogbuchEntwurf {
        fach: cfg.kurz.clone(),
        datum,
        kopfzeile,
        abschnitte,
        bewertung: cfg.selbstbewertung.map(|skala| BewertungFeld { titel: titel_norm(&cfg.bewertung_titel), skala }),
        status: info.iter().map(|q| q.status).min().unwrap_or(LogbuchStatus::Offen),
        quellen: info,
        moodle_url: moodle_url(cfg),
        mit_ki,
        ki: aufruf,
        warnungen,
    })
}

// ---------------------------------------------------------------------------------------------
// Verlauf (App-Datenordner, **ohne** Text)
// ---------------------------------------------------------------------------------------------

/// Hängt „Fach · Datum · Status · Zeitpunkt“ an `logbuch-verlauf.jsonl` an. Nie der Eintragstext.
pub fn verlauf_anhaengen(dir: &Path, fach: &str, datum: NaiveDate, status: LogbuchStatus) -> AppResult<()> {
    fs::create_dir_all(dir)?;
    let zeile = serde_json::json!({
        "zeit": chrono::Local::now().to_rfc3339(),
        "fach": fach,
        "datum": datum.format("%Y-%m-%d").to_string(),
        "status": status.as_str(),
    });
    let mut f = OpenOptions::new().create(true).append(true).open(dir.join(VERLAUF_DATEI))?;
    writeln!(f, "{zeile}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    const GEC_BODY: &str = "# 2026-10-05 GEC – Handelskalkulation\n\n## Status\n- [ ] **Notiz abgeschlossen** · danach verschieben\n\n\
## Inhalt der Stunde\n- Vorwärtskalkulation vom Listeneinkaufspreis bis zum Listenverkaufspreis\n- \n\n\
## Wichtige Begriffe\n- [[Deckungsbeitrag]], Handelsspanne, [[Kalkulationszuschlag|Zuschlag]]\n- [[]]\n\n\
## Merksätze / Formeln\n> [!tip] Handelsspanne\n> Spanne = (LVP − BP) / LVP · 100\n\n\
## Aufgaben / Hausaufgaben\n- [ ] Arbeitsblatt ausfüllen 📅 2026-10-08\n- [x] Erledigt\n- [ ]  📅 \n\n\
## Offene Fragen\n- Warum Skonto vom Zielverkaufspreis? #frage\n- #frage \n\n\
## Noch nicht verstanden\n- Rückwärtskalkulation mit Rabatt #nochmal\n- #nochmal \n\n\
## Verknüpft\n- [[Unterricht]]\n\n```text\n- [ ] keine Aufgabe #frage\n```\n";

    #[test]
    fn quellinhalt_aus_unterrichtsnotiz() {
        let q = quellinhalt(GEC_BODY);
        assert_eq!(q.inhalt, vec!["Vorwärtskalkulation vom Listeneinkaufspreis bis zum Listenverkaufspreis"]);
        assert_eq!(q.merksaetze, vec!["Handelsspanne: Spanne = (LVP − BP) / LVP · 100"]);
        assert_eq!(q.begriffe, vec!["Deckungsbeitrag", "Handelsspanne", "Zuschlag"]);
        assert_eq!(q.fragen, vec!["Warum Skonto vom Zielverkaufspreis?", "Noch nicht verstanden: Rückwärtskalkulation mit Rabatt"]);
        // Status-Aufgabe, erledigte Aufgabe, leerer Platzhalter und Codeblock zählen nicht
        assert_eq!(q.aufgaben, vec!["Arbeitsblatt ausfüllen (bis 08.10.2026)"]);
        assert_eq!(q.erarbeitet().last().unwrap(), "Wichtige Begriffe: Deckungsbeitrag, Handelsspanne, Zuschlag");
    }

    #[test]
    fn leere_vorlage_ergibt_keinen_inhalt() {
        let leer = "# 2026-11-16 GEC\n\n## Status\n- [ ] **Notiz abgeschlossen**\n\n## Inhalt der Stunde\n- \n\n## Wichtige Begriffe\n- [[]]\n\n\
## Merksätze / Formeln\n> \n\n## Aufgaben / Hausaufgaben\n- [ ]  📅 \n\n## Offene Fragen\n- #frage \n\n## Noch nicht verstanden\n- #nochmal \n\n## Verknüpft\n- [[Unterricht]]\n";
        assert!(quellinhalt(leer).ist_leer(), "{:?}", quellinhalt(leer));
    }

    #[test]
    fn markdown_und_umlaute() {
        assert_eq!(ohne_markdown("**Fett** und [[Ziel|Alias]] mit `code` #tag ![[bild.png]] [Link](http://x)"), "Fett und Alias mit code Link");
        assert_eq!(ohne_markdown("a * b und C# bleibt"), "a * b und C# bleibt");
        assert_eq!(umlaute_umschreiben("Fürs nächste Größe ÄÖÜ"), "Fuers naechste Groesse AeOeUe");
    }

    fn gec() -> LogbuchFach {
        voreinstellung("GEC")
    }

    fn abschnitte(e: &[&str], f: &[&str], n: &[&str]) -> Vec<TextAbschnitt> {
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
        vec![
            TextAbschnitt { titel: "Dies habe ich erarbeitet".into(), punkte: t(e) },
            TextAbschnitt { titel: "Diese Fragen haben sich ergeben".into(), punkte: t(f) },
            TextAbschnitt { titel: "Das ist als naechstes zu tun".into(), punkte: t(n) },
        ]
    }

    #[test]
    fn gec_eintrag_hat_genau_vier_abschnitte_und_leere_bewertung() {
        let a = abschnitte(&["Ich habe kalkuliert."], &["Warum?"], &[]);
        let t = render_text(&gec(), d(2026, 10, 5), &a, None).unwrap();
        assert_eq!(
            t,
            "05.10.2026\n\nDies habe ich erarbeitet:\n- Ich habe kalkuliert.\n\nDiese Fragen haben sich ergeben:\n- Warum?\n\n\
Das ist als naechstes zu tun:\n- \n\nBewertung dieser Lerneinheit: __/10"
        );
        let koepfe: Vec<&str> = t.lines().filter(|l| l.ends_with(':') || l.contains("/10")).collect();
        assert_eq!(koepfe.len(), 4, "{koepfe:?}");
    }

    #[test]
    fn regel2_ohne_zahl_kein_kopieren() {
        let a = abschnitte(&["x"], &["y"], &["z"]);
        assert!(matches!(kopiertext(&gec(), d(2026, 10, 5), &a, None), Err(AppError::BewertungFehlt(10))));
        assert!(kopiertext(&gec(), d(2026, 10, 5), &a, Some(11)).is_err());
        assert!(kopiertext(&gec(), d(2026, 10, 5), &a, Some(0)).is_err());
        let t = kopiertext(&gec(), d(2026, 10, 5), &a, Some(7)).unwrap();
        assert!(t.ends_with("Bewertung dieser Lerneinheit: 7/10"));
        // ohne Selbstbewertung: kein Bewertungsabschnitt, Kopieren ohne Zahl erlaubt
        let mut ddv = voreinstellung("D-DV");
        ddv.aktiv = true;
        let t = kopiertext(&ddv, d(2026, 10, 5), &a, None).unwrap();
        assert!(!t.contains("Bewertung"));
    }

    #[test]
    fn regel6_kein_ek_im_kopiertext() {
        let a = abschnitte(&["EK: 12,50 € je Tasche"], &["y"], &["z"]);
        assert!(kopiertext(&gec(), d(2026, 10, 5), &a, Some(5)).is_err());
    }

    #[test]
    fn abschnitte_muessen_zur_konfiguration_passen() {
        let mut a = abschnitte(&["x"], &["y"], &["z"]);
        a.push(TextAbschnitt { titel: "Extra".into(), punkte: vec![] });
        assert!(render_text(&gec(), d(2026, 10, 5), &a, None).is_err());
        let mut a = abschnitte(&["x"], &["y"], &["z"]);
        a.swap(0, 1);
        assert!(render_text(&gec(), d(2026, 10, 5), &a, None).is_err());
    }

    #[test]
    fn umlaute_umschreiben_und_markdown_beim_rendern() {
        let mut f = gec();
        f.umlaute = Umlaute::Umschreiben;
        let a = abschnitte(&["Für **mich** [[Ziel|wichtig]]\nzweite Zeile"], &[], &[]);
        let t = render_text(&f, d(2026, 10, 5), &a, Some(8)).unwrap();
        assert!(t.contains("- Fuer mich wichtig zweite Zeile\n"), "{t}");
    }

    #[test]
    fn status_aendert_nur_die_logbuch_zeile() {
        let alt = "---\ntags: [unterricht]\ndatum: 2026-10-05\nfach: \"[[GEC – Geschäftsprozesse im E-Commerce|GEC]]\"\nthema: x\n---\n# Kopf\n\nText\n";
        let neu = text_mit_status(alt, LogbuchStatus::Kopiert, true).unwrap().unwrap();
        assert_eq!(neu, alt.replace("thema: x\n", "thema: x\nlogbuch: kopiert\n"));
        let neu2 = text_mit_status(&neu, LogbuchStatus::Abgegeben, false).unwrap().unwrap();
        assert_eq!(neu2, alt.replace("thema: x\n", "thema: x\nlogbuch: abgegeben\n"));
        // nie zurückstufen beim Kopieren; gleicher Status → nichts
        assert_eq!(text_mit_status(&neu2, LogbuchStatus::Kopiert, true).unwrap(), None);
        assert_eq!(text_mit_status(alt, LogbuchStatus::Offen, false).unwrap(), None);
        // kaputtes YAML wird nicht angefasst
        assert!(text_mit_status("---\na: [\n---\n", LogbuchStatus::Kopiert, false).is_err());
    }

    #[test]
    fn einstellungen_pruefen() {
        let mut f = gec();
        f.moodle_url = " http://unsicher.de ".into();
        assert!(pruefe_einstellungen(vec![f.clone()]).is_err());
        f.moodle_url = "https://www.lebk-moodle.de/course/view.php?id=2644".into();
        f.abschnitte[0].titel = " Dies habe ich erarbeitet: ".into();
        let ok = pruefe_einstellungen(vec![f.clone()]).unwrap();
        assert_eq!(ok[0].abschnitte[0].titel, "Dies habe ich erarbeitet");
        assert!(pruefe_einstellungen(vec![f.clone(), f.clone()]).is_err());
        f.selbstbewertung = Some(1);
        assert!(pruefe_einstellungen(vec![f]).is_err());
        // Kürzel aus dem Fach-Verzeichnis, die kein Dateiname sein könnten, lassen sich trotzdem speichern
        let mut sport = voreinstellung("Sport / Gesundheit");
        sport.aktiv = true;
        let englisch = voreinstellung("Englisch Business");
        let ok = pruefe_einstellungen(vec![gec(), sport, englisch]).unwrap();
        assert_eq!(ok[1].kurz, "Sport / Gesundheit");
        assert!(pruefe_einstellungen(vec![voreinstellung("  ")]).is_err());
        assert!(pruefe_einstellungen(vec![voreinstellung("a\nb")]).is_err());
        assert_eq!(moodle_url(&gec()), MOODLE_START);
    }

    #[test]
    fn verlauf_enthaelt_keinen_text() {
        let dir = tempfile::tempdir().unwrap();
        verlauf_anhaengen(dir.path(), "GEC", d(2026, 10, 5), LogbuchStatus::Kopiert).unwrap();
        let s = fs::read_to_string(dir.path().join(VERLAUF_DATEI)).unwrap();
        let v: serde_json::Value = serde_json::from_str(s.trim()).unwrap();
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 4, "{keys:?}");
        assert_eq!(v["status"], "kopiert");
    }
}
