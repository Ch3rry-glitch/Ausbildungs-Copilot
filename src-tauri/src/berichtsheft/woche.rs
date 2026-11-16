//! Wochennotiz lesen: Tage, Stichpunkte, Nachweis-Stand, Lernfeld-Tabelle, offene Punkte – und die
//! Übersicht aller Wochen.
//!
//! Format (echter Vault, KW36–KW46): `## Berufsschule` / `## Betrieb` (Reihenfolge wechselt), darunter
//! `### Montag, 09.11.` (auch `TT.MM.JJJJ`). Das Datum ergibt sich aus Wochentag + KW; eine
//! abweichende Datumsangabe in der Überschrift ist nur eine Warnung.

use std::path::Path;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use regex::Regex;

use super::regeln::{self, diagnose_wort, falten, ist_luecke};
use super::tabelle::{self, Rolle};
use super::{nachweis, tag_der_woche, tag_kurz, wochentag_am_anfang, wochentag_name, Gliederung, Kontext, WOCHENTAGE};
use crate::error::{AppError, AppResult};
use crate::model::{
    BerichtsTag, BerichtsTagArt, BerichtsWoche, BerichtsheftUebersicht, LernfeldZeile, LineEnding, NachweisStand,
    NoteMeta, NoteVersion, OffenerPunkt, RelPath, Stichpunkt, TagAbschnitt, WochenUnterricht, WochenZeile,
};
use crate::vault::frontmatter::{field_as_text, NoteText};
use crate::vault::{links, naming};
use crate::write::{self, RetryPolicy};

pub const ABSCHNITT_BERUFSSCHULE: &str = "Berufsschule";
pub const ABSCHNITT_BETRIEB: &str = "Betrieb";
pub const HINWEIS_NICHT_BEARBEITBAR: &str = "enthält mehr als Stichpunkte – bitte in Obsidian bearbeiten";

pub fn abschnitt_titel(a: TagAbschnitt) -> &'static str {
    match a {
        TagAbschnitt::Berufsschule => ABSCHNITT_BERUFSSCHULE,
        TagAbschnitt::Betrieb => ABSCHNITT_BETRIEB,
    }
}

// ---------------------------------------------------------------------------------------------
// Zeilen eines Tages
// ---------------------------------------------------------------------------------------------

/// Was eine Zeile unter einer Tages-Überschrift ist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Zeilenart {
    /// `- GEC · D-DV · WSP · D` – nur Kürzel, kein Inhalt
    Uebersicht,
    /// `- *(noch einzutragen)*`, `⟨…⟩`
    Platzhalter,
    /// Fach-Zeile der Berufsschule; `inhalt` = Text nach dem Fach (`None` = leer/Platzhalter)
    Fach { kurz: String, inhalt: Option<String> },
    Sondertag(BerichtsTagArt),
    Inhalt,
}

impl Zeilenart {
    /// Zählt als Inhalt (gegen `fehlt`)?
    pub fn hat_inhalt(&self) -> bool {
        matches!(self, Zeilenart::Inhalt | Zeilenart::Sondertag(_) | Zeilenart::Fach { inhalt: Some(_), .. })
    }
}

static PLATZHALTER_KLAMMER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"⟨[^⟩]*⟩").unwrap());

/// Nur Platzhalter: leer, `⟨…⟩`, „*(noch einzutragen)*“, „*noch einzutragen.*“, „—“.
pub fn ist_platzhalter_text(s: &str) -> bool {
    let ohne = PLATZHALTER_KLAMMER.replace_all(s, "");
    let rest: String = falten(&ohne).chars().filter(|c| c.is_alphanumeric()).collect();
    matches!(rest.as_str(), "" | "nocheinzutragen" | "nocheintragen" | "themanocheintragen" | "nochanzulegen" | "nochoffen")
}

static UEBERSICHT_TEIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-ZÄÖÜ][A-ZÄÖÜ0-9\-]*$").unwrap());
static KUERZEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-ZÄÖÜ][A-ZÄÖÜ0-9\-]{0,7}$").unwrap());
static FACH_FETT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\*\*([^*]+?)\*\*[ \t]*(?:[—–:-][ \t]*(.*))?$").unwrap());
static FACH_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[\[([^\]|\\]*)(?:\\?\|([^\]]*))?\]\][ \t]*(?:[:—–][ \t]*(.*))?$").unwrap());
static FACH_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([\p{L}][\p{L}\p{N}\-]*)(?:[ \t]*:|[ \t]+[—–])[ \t]*(.*)$").unwrap());
static PFEIL_LINKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]*→[ \t]*\[\[.*$").unwrap());

/// `- GEC · D-DV · WSP · D`: mindestens zwei Kürzel, getrennt durch `·`.
pub fn ist_uebersicht(text: &str) -> bool {
    let teile: Vec<&str> = text.split('·').map(str::trim).collect();
    teile.len() >= 2 && teile.iter().all(|t| UEBERSICHT_TEIL.is_match(t))
}

/// Kürzel eines Fachs? Bekannt aus dem Stundenplan oder in Großbuchstaben (`GEC`, `D-DV`, `E`).
fn ist_kuerzel(x: &str, bekannte: &[String]) -> bool {
    let x = x.trim();
    bekannte.iter().any(|b| b.eq_ignore_ascii_case(x)) || KUERZEL.is_match(x)
}

/// Text nach dem Fach ohne `→ [[…]]` am Ende.
pub fn ohne_pfeil_links(s: &str) -> String {
    PFEIL_LINKS.replace(s, "").trim().to_string()
}

fn fach_inhalt(rest: Option<&str>) -> Option<String> {
    let r = rest.unwrap_or("").trim();
    (!ist_platzhalter_text(r)).then(|| r.to_string())
}

/// Fach-Zeile erkennen: `**X** — …`, `[[…|X]]: …`, `X: …` / `X — …`, `X` allein.
pub fn fach_zeile(text: &str, bekannte: &[String]) -> Option<(String, Option<String>)> {
    let t = text.trim();
    if let Some(c) = FACH_FETT.captures(t) {
        let x = c[1].trim();
        if ist_kuerzel(x, bekannte) {
            return Some((x.to_string(), fach_inhalt(c.get(2).map(|m| m.as_str()))));
        }
        return None;
    }
    if let Some(c) = FACH_LINK.captures(t) {
        let ziel = c[1].trim();
        let x = c.get(2).map(|m| m.as_str().trim()).filter(|a| !a.is_empty()).unwrap_or_else(|| {
            ziel.split_once(" – ").map(|(k, _)| k.trim()).unwrap_or(ziel)
        });
        if ist_kuerzel(x, bekannte) && c.get(3).is_some() {
            return Some((x.to_string(), fach_inhalt(c.get(3).map(|m| m.as_str()))));
        }
        return None;
    }
    if let Some(c) = FACH_TEXT.captures(t) {
        if ist_kuerzel(&c[1], bekannte) {
            return Some((c[1].to_string(), fach_inhalt(Some(&c[2]))));
        }
        return None;
    }
    ist_kuerzel(t, bekannte).then(|| (t.to_string(), None))
}

/// Zeilenart eines Stichpunkt-Textes (ohne `- `).
pub fn zeilenart(text: &str, abschnitt: TagAbschnitt, bekannte: &[String]) -> Zeilenart {
    if let Some(a) = regeln::sondertag(text) {
        return Zeilenart::Sondertag(a);
    }
    if ist_uebersicht(text) {
        return Zeilenart::Uebersicht;
    }
    if abschnitt == TagAbschnitt::Berufsschule {
        if let Some((kurz, inhalt)) = fach_zeile(text, bekannte) {
            return Zeilenart::Fach { kurz, inhalt };
        }
    }
    if ist_platzhalter_text(text) {
        return Zeilenart::Platzhalter;
    }
    Zeilenart::Inhalt
}

static LISTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([ \t]*)([-*+]|\d+[.)])(?:[ \t]+(.*))?$").unwrap());

/// Eine Zeile eines Tagesblocks: (Text ohne Listenzeichen, einfacher `- `-Punkt ohne Einrückung?).
/// `None` = leer oder bloßes `- `.
pub fn listenzeile(zeile: &str) -> Option<(String, bool)> {
    if zeile.trim().is_empty() {
        return None;
    }
    match LISTE.captures(zeile) {
        Some(c) => {
            let text = c.get(3).map(|m| m.as_str().trim()).unwrap_or("").to_string();
            let einfach = c[1].is_empty() && &c[2] == "-";
            (!text.is_empty()).then_some((text, einfach))
        }
        None => Some((zeile.trim().to_string(), false)),
    }
}

/// Ein gelesener Tag samt Zeilenarten und Bereich in der Datei.
#[derive(Debug, Clone)]
pub struct TagRoh {
    pub tag: BerichtsTag,
    /// parallel zu `tag.stichpunkte`
    pub arten: Vec<Zeilenart>,
    /// (Index der Überschrift, Ende exklusiv) – 0-basiert
    pub bereich: Option<(usize, usize)>,
}

static DATUM_KOPF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,2})\.(\d{1,2})\.").unwrap());

/// Stundenplan-Kürzel (für die Fach-Erkennung).
pub fn bekannte_faecher(ctx: &Kontext) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for t in ctx.plan.iter().flat_map(|p| p.tage.iter()) {
        for s in &t.stunden {
            if !v.iter().any(|x| x.eq_ignore_ascii_case(&s.fach)) {
                v.push(s.fach.clone());
            }
        }
    }
    v
}

/// Kürzel einer Unterrichtsnotiz: `fach` (Alias, sonst Teil vor „ – “), sonst aus dem Dateinamen.
pub fn notiz_kurz(m: &NoteMeta) -> Option<String> {
    let aus_feld = m.fach.as_deref().map(|f| f.split_once(" – ").map(|(k, _)| k).unwrap_or(f).trim().to_string());
    aus_feld.filter(|f| !f.is_empty()).or_else(|| naming::parse_unterricht(&m.title).map(|n| n.fach))
}

fn feld_text(m: &NoteMeta, key: &str) -> Option<String> {
    m.fields.iter().find(|f| f.key.eq_ignore_ascii_case(key)).and_then(|f| field_as_text(&f.value)).filter(|s| !s.trim().is_empty())
}

pub fn thema_von(m: &NoteMeta) -> Option<String> {
    feld_text(m, "thema")
}

/// Liest die Zeilen eines Tagesblocks (`von` = erste Zeile nach der Überschrift, `bis` exklusiv).
pub fn tag_zeilen(
    zeilen: &[String],
    von: usize,
    bis: usize,
    abschnitt: TagAbschnitt,
    bekannte: &[String],
) -> (Vec<Stichpunkt>, Vec<Zeilenart>, bool) {
    let mut sp = Vec::new();
    let mut arten = Vec::new();
    let mut einfach_alle = true;
    for (i, z) in zeilen.iter().enumerate().take(bis).skip(von) {
        let Some((text, einfach)) = listenzeile(z) else {
            // ein bloßes `- ` (Vorlage) ist ein einfacher Listenpunkt ohne Text
            if !z.trim().is_empty() && z.trim_end() != "-" {
                einfach_alle = false;
            }
            continue;
        };
        if !einfach {
            einfach_alle = false;
        }
        let art = zeilenart(&text, abschnitt, bekannte);
        let fach = match &art {
            Zeilenart::Fach { kurz, .. } => Some(kurz.clone()),
            _ => None,
        };
        let luecke = art == Zeilenart::Inhalt && ist_luecke(&text);
        let ziele = links::parse_links(&text, 0).into_iter().filter(|l| !l.target.is_empty()).map(|l| l.target).collect();
        sp.push(Stichpunkt { line: i + 1, diagnose: diagnose_wort(&text), text, luecke, fach, links: ziele });
        arten.push(art);
    }
    (sp, arten, einfach_alle)
}

/// Tagesart: erste Zeile mit Inhalt entscheidet.
pub fn tagesart(arten: &[Zeilenart]) -> BerichtsTagArt {
    match arten.iter().find(|a| a.hat_inhalt()) {
        Some(Zeilenart::Sondertag(a)) => *a,
        _ => BerichtsTagArt::Stichpunkte,
    }
}

/// Eine gefundene Tages-Überschrift.
struct Fund {
    abschnitt: TagAbschnitt,
    wochentag: Weekday,
    kopf: usize,
    ende: usize,
    /// nach dem Block folgt eine Unterüberschrift → nicht nur Stichpunkte
    unterkopf: bool,
}

/// Alle Tages-Überschriften unter `## Berufsschule` / `## Betrieb`.
fn tage_finden(gl: &Gliederung, zeilen: &[String], montag: NaiveDate, warnungen: &mut Vec<String>) -> Vec<Fund> {
    let mut out: Vec<Fund> = Vec::new();
    for a in [TagAbschnitt::Berufsschule, TagAbschnitt::Betrieb] {
        let Some((start, ende)) = gl.abschnitt(abschnitt_titel(a), zeilen.len()) else { continue };
        for k in gl.koepfe.iter().filter(|k| k.idx > start && k.idx < ende && k.ebene >= 3) {
            let Some(w) = wochentag_am_anfang(&k.text) else { continue };
            if out.iter().any(|f| f.abschnitt == a && f.wochentag == w) {
                warnungen.push(format!("„{}“ steht doppelt unter „{}“ – die App nimmt die erste.", k.text, abschnitt_titel(a)));
                continue;
            }
            let datum = tag_der_woche(montag, w);
            if let Some(c) = DATUM_KOPF.captures(&k.text) {
                let (t, m): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
                if (t, m) != (datum.day(), datum.month()) {
                    warnungen.push(format!(
                        "„{}“: Das Datum passt nicht zur KW – es gilt {}, {}.",
                        k.text,
                        wochentag_name(w),
                        datum.format("%d.%m.")
                    ));
                }
            }
            let naechster = gl.naechster_kopf(k.idx);
            let block_ende = naechster.map(|n| n.idx).unwrap_or(zeilen.len()).min(ende);
            let unterkopf = naechster.is_some_and(|n| n.idx < ende && n.ebene > k.ebene);
            out.push(Fund { abschnitt: a, wochentag: w, kopf: k.idx, ende: block_ende, unterkopf });
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Frontmatter, Lernfeld-Tabelle, offene Punkte
// ---------------------------------------------------------------------------------------------

/// `lernfelder: [2, 4]` / Blockliste / `"LF04"` → Nummern. `None`, wenn ein Eintrag keine Nummer ist.
pub fn lernfelder_aus(v: &serde_json::Value) -> Option<Vec<u32>> {
    let eintraege: Vec<&serde_json::Value> = match v {
        serde_json::Value::Array(a) => a.iter().collect(),
        serde_json::Value::Null => vec![],
        x => vec![x],
    };
    let mut out = Vec::new();
    for e in eintraege {
        let n = match e {
            serde_json::Value::Number(n) => n.as_u64().map(|n| n as u32),
            serde_json::Value::String(s) if s.trim().is_empty() => continue,
            serde_json::Value::String(s) => s.trim().parse().ok().or_else(|| tabelle::lernfeld_nummern(s).first().copied()),
            _ => None,
        }?;
        if !out.contains(&n) {
            out.push(n);
        }
    }
    Some(out)
}

/// Tabelle unter „Bezug zu Lernfeldern“ und ihre Zeilen mit Inhalt.
pub fn lernfeld_tabelle(zeilen: &[String], gl: &Gliederung) -> (Option<tabelle::Tabelle>, Vec<LernfeldZeile>) {
    let Some((start, ende, _)) = gl.abschnitt_beginnt("Bezug zu Lernfeld", zeilen.len()) else { return (None, vec![]) };
    let Some(t) = tabelle::tabellen(zeilen, start + 1, ende).into_iter().next() else { return (None, vec![]) };
    let sp_t = t.spalte(Rolle::Taetigkeit).unwrap_or(0);
    let sp_l = t.spalte(Rolle::Lernfeld).unwrap_or(1);
    let mut out = Vec::new();
    for &i in &t.zeilen {
        let z = tabelle::zellen(&zeilen[i]).unwrap_or_default();
        if tabelle::ist_leer(&z) {
            continue;
        }
        let zelle = z.get(sp_l).cloned().unwrap_or_default();
        out.push(LernfeldZeile {
            line: i + 1,
            taetigkeit: z.get(sp_t).cloned().unwrap_or_default(),
            lernfelder: tabelle::lernfeld_nummern(&zelle),
            zelle,
        });
    }
    (Some(t), out)
}

static OFFEN_PUNKT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t]*[-*+] \[ \][ \t]+(.+)$").unwrap());
static CALLOUT_KOPF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^>[ \t]*\[![^\]]+\][+-]?[ \t]*(.*)$").unwrap());

const OFFEN_TITEL: &str = "offen vor dem abzeichnen";

/// Offene `- [ ]` unter „Offen vor dem Abzeichnen“ – als Abschnitt oder als Callout.
pub fn offene_punkte(zeilen: &[String], gl: &Gliederung) -> Vec<OffenerPunkt> {
    let mut out = Vec::new();
    let nehmen = |i: usize, z: &str, out: &mut Vec<OffenerPunkt>| {
        if let Some(c) = OFFEN_PUNKT.captures(z) {
            out.push(OffenerPunkt { line: i + 1, text: c[1].trim().to_string() });
        }
    };
    if let Some((start, ende, _)) = gl.abschnitt_beginnt(OFFEN_TITEL, zeilen.len()) {
        for (i, z) in zeilen.iter().enumerate().take(ende).skip(start + 1) {
            if gl.prosa.contains(&i) && !z.trim_start().starts_with('>') {
                nehmen(i, z, &mut out);
            }
        }
    }
    let mut im_callout = false;
    for (i, z) in zeilen.iter().enumerate() {
        if !gl.prosa.contains(&i) || !z.starts_with('>') {
            im_callout = false;
            continue;
        }
        if let Some(c) = CALLOUT_KOPF.captures(z) {
            im_callout = falten(&c[1]).contains(OFFEN_TITEL);
            continue;
        }
        if im_callout {
            nehmen(i, z.trim_start_matches('>'), &mut out);
        }
    }
    out.sort_by_key(|p| p.line);
    out
}

// ---------------------------------------------------------------------------------------------
// Woche parsen
// ---------------------------------------------------------------------------------------------

/// Gelesene Woche: Ergebnis für die Oberfläche plus Zeilenarten und Bereiche der Tage.
#[derive(Debug, Clone)]
pub struct WocheRoh {
    pub woche: BerichtsWoche,
    /// gleiche Reihenfolge wie `woche.tage`
    pub tage: Vec<TagRoh>,
}

impl WocheRoh {
    pub fn tag(&self, abschnitt: TagAbschnitt, datum: NaiveDate) -> Option<&TagRoh> {
        self.tage.iter().find(|t| t.tag.abschnitt == abschnitt && t.tag.datum == datum)
    }
}

/// (ISO-Jahr, KW, Montag) aus dem Dateinamen `2026-KW46.md`.
pub fn woche_aus_pfad(rel: &str) -> AppResult<(i32, u32, NaiveDate)> {
    let stem = crate::aufgaben::titel_aus_pfad(rel);
    naming::parse_woche(stem)
        .and_then(|(j, kw)| super::montag_von(j, kw).map(|m| (j, kw, m)))
        .ok_or_else(|| AppError::InvalidInput(format!("„{stem}“ ist kein Wochenname wie „2026-KW46“.")))
}

fn unterricht_eintrag(m: &NoteMeta, block: &[String]) -> WochenUnterricht {
    let verlinkt = block.iter().any(|z| links::parse_links(z, 0).iter().any(|l| l.target.eq_ignore_ascii_case(&m.title)));
    WochenUnterricht {
        path: m.path.clone(),
        title: m.title.clone(),
        fach: notiz_kurz(m),
        thema: thema_von(m),
        datum: m.datum.unwrap_or_default(),
        verlinkt,
        privat: m.private,
    }
}

/// Liest die Woche aus dem dekodierten Text (`\n`).
pub fn parsen(rel: &str, text: &str, version: NoteVersion, ctx: &Kontext) -> AppResult<WocheRoh> {
    let (jahr, kw, montag) = woche_aus_pfad(rel)?;
    let sonntag = montag + Duration::days(6);
    let (zeilen, _) = super::zeilen_von(text);
    let gl = Gliederung::von(text, &zeilen);
    let mut warnungen = Vec::new();

    // Frontmatter
    let note = NoteText::parse(text);
    let felder = note.fields().unwrap_or_else(|e| {
        warnungen.push(format!("Die Frontmatter ist kein gültiges YAML ({e})."));
        Vec::new()
    });
    let feld = |k: &str| felder.iter().find(|f| f.key.eq_ignore_ascii_case(k)).map(|f| &f.value);
    let text_von = |k: &str| feld(k).and_then(field_as_text).filter(|s| !s.trim().is_empty());
    let abgezeichnet = match feld("abgezeichnet") {
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => matches!(s.trim().to_lowercase().as_str(), "true" | "ja"),
        _ => false,
    };
    let lernfelder = feld("lernfelder").and_then(lernfelder_aus).unwrap_or_default();
    if let Some(k) = feld("kw").and_then(|v| v.as_u64()) {
        if k != u64::from(kw) {
            warnungen.push(format!("In der Frontmatter steht „kw: {k}“, der Dateiname sagt KW {kw}."));
        }
    }

    // Tage
    let bekannte = bekannte_faecher(ctx);
    let funde = tage_finden(&gl, &zeilen, montag, &mut warnungen);
    for a in [TagAbschnitt::Berufsschule, TagAbschnitt::Betrieb] {
        if gl.abschnitt(abschnitt_titel(a), zeilen.len()).is_none() {
            warnungen.push(format!("Die Woche hat keinen Abschnitt „## {}“.", abschnitt_titel(a)));
        }
    }
    let notizen = ctx.unterricht_der_woche(montag);
    let mut erwartet: Vec<(TagAbschnitt, NaiveDate)> = Vec::new();
    match &ctx.plan {
        Some(plan) => {
            for w in WOCHENTAGE.into_iter().take(5) {
                let d = tag_der_woche(montag, w);
                let schule = crate::model::Wochentag::from_chrono(w).is_some_and(|t| plan.ist_schultag(t));
                erwartet.push((if schule { TagAbschnitt::Berufsschule } else { TagAbschnitt::Betrieb }, d));
            }
        }
        None => warnungen.push("Kein Stundenplan gefunden – es zählen nur die Tage, die in der Datei stehen.".into()),
    }
    let mut tage: Vec<TagRoh> = Vec::new();
    for f in &funde {
        let datum = tag_der_woche(montag, f.wochentag);
        let (stichpunkte, arten, einfach) = tag_zeilen(&zeilen, f.kopf + 1, f.ende, f.abschnitt, &bekannte);
        let bearbeitbar = einfach && !f.unterkopf;
        let block = &zeilen[f.kopf..f.ende];
        let unterricht = if f.abschnitt == TagAbschnitt::Berufsschule {
            notizen.iter().filter(|m| m.datum == Some(datum)).map(|m| unterricht_eintrag(m, block)).collect()
        } else {
            vec![]
        };
        tage.push(TagRoh {
            tag: BerichtsTag {
                abschnitt: f.abschnitt,
                datum,
                wochentag: wochentag_name(f.wochentag).to_string(),
                ueberschrift_zeile: Some(f.kopf + 1),
                art: tagesart(&arten),
                fehlt: !arten.iter().any(Zeilenart::hat_inhalt),
                bearbeitbar,
                hinweis: (!bearbeitbar).then(|| HINWEIS_NICHT_BEARBEITBAR.to_string()),
                stichpunkte,
                unterricht,
                nachweis: NachweisStand::Fehlt,
            },
            arten,
            bereich: Some((f.kopf, f.ende)),
        });
    }
    for (a, d) in erwartet {
        if tage.iter().any(|t| t.tag.abschnitt == a && t.tag.datum == d) {
            continue;
        }
        let unterricht = if a == TagAbschnitt::Berufsschule {
            notizen.iter().filter(|m| m.datum == Some(d)).map(|m| unterricht_eintrag(m, &[])).collect()
        } else {
            vec![]
        };
        tage.push(TagRoh {
            tag: BerichtsTag {
                abschnitt: a,
                datum: d,
                wochentag: wochentag_name(d.weekday()).to_string(),
                ueberschrift_zeile: None,
                art: BerichtsTagArt::Stichpunkte,
                stichpunkte: vec![],
                fehlt: true,
                bearbeitbar: true,
                hinweis: None,
                unterricht,
                nachweis: NachweisStand::Fehlt,
            },
            arten: vec![],
            bereich: None,
        });
    }
    tage.sort_by_key(|t| (t.tag.datum, t.tag.abschnitt));
    for m in &notizen {
        let d = m.datum.unwrap_or_default();
        if !tage.iter().any(|t| t.tag.abschnitt == TagAbschnitt::Berufsschule && t.tag.datum == d) {
            warnungen.push(format!(
                "Die Unterrichtsnotiz „{}“ ({}) steht an keinem Schultag der Woche – „Aus Unterrichtsnotizen übernehmen“ legt den Tag an.",
                m.title,
                tag_kurz(d)
            ));
        }
    }

    // Nachweis-Stand je Tag
    let teil = nachweis::teil(&zeilen, &gl, montag);
    let schluessel: Vec<(TagAbschnitt, NaiveDate)> = tage.iter().map(|t| (t.tag.abschnitt, t.tag.datum)).collect();
    for t in &mut tage {
        t.tag.nachweis = nachweis::stand(&teil, &zeilen, (t.tag.abschnitt, t.tag.datum), &schluessel);
    }
    for t in &tage {
        for s in t.tag.stichpunkte.iter().filter(|s| s.diagnose.is_some()) {
            warnungen.push(format!(
                "{} {}: „{}“ ist eine Gesundheitsangabe – bitte in Obsidian entfernen. Sie geht nie an die KI und nie in den Nachweis (Regel 3).",
                abschnitt_titel(t.tag.abschnitt),
                tag_kurz(t.tag.datum),
                s.diagnose.as_deref().unwrap_or_default()
            ));
        }
    }

    let (_, lernfeld_tabelle) = lernfeld_tabelle(&zeilen, &gl);
    let offene_punkte = offene_punkte(&zeilen, &gl);
    let woche = BerichtsWoche {
        path: rel.to_string(),
        title: crate::aufgaben::titel_aus_pfad(rel).to_string(),
        jahr,
        kw,
        montag,
        sonntag,
        status: text_von("status"),
        abgezeichnet,
        stunden: text_von("stunden"),
        lernfelder,
        tage: tage.iter().map(|t| t.tag.clone()).collect(),
        lernfeld_tabelle,
        offene_punkte,
        warnungen,
        version,
    };
    Ok(WocheRoh { woche, tage })
}

/// Woche frisch von der Platte lesen.
pub fn lesen(root: &Path, rel: &str, ctx: &Kontext) -> AppResult<WocheRoh> {
    let (abs, rel) = crate::guard::resolve_for_read(root, rel)?;
    let (bytes, version) = match write::read_versioned(&abs, &RetryPolicy::short()) {
        Ok(x) => x,
        Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(AppError::NoteNotFound(rel)),
        Err(e) => return Err(e),
    };
    let d = write::decode(&bytes);
    let mut roh = parsen(&rel, &d.text, version, ctx)?;
    if !d.utf8 {
        roh.woche.warnungen.insert(0, "Keine gültige UTF-8-Datei – die App ändert hier nichts.".into());
    } else if d.line_ending == LineEnding::Mixed {
        roh.woche.warnungen.insert(0, "Die Datei hat gemischte Zeilenenden – die App ändert hier nichts (bitte in Obsidian bearbeiten).".into());
    }
    Ok(roh)
}

// ---------------------------------------------------------------------------------------------
// Übersicht
// ---------------------------------------------------------------------------------------------

fn zeile_aus(roh: Option<&WocheRoh>, jahr: i32, kw: u32, montag: NaiveDate, path: Option<RelPath>, aktuell: bool) -> WochenZeile {
    let mut z = WochenZeile {
        path,
        stem: super::stem(jahr, kw),
        jahr,
        kw,
        montag,
        sonntag: montag + Duration::days(6),
        status: None,
        abgezeichnet: false,
        stunden: None,
        fehlende_tage: vec![],
        offene_punkte: 0,
        luecken: 0,
        diagnosen: 0,
        aktuell,
    };
    if let Some(r) = roh {
        let w = &r.woche;
        z.status = w.status.clone();
        z.abgezeichnet = w.abgezeichnet;
        z.stunden = w.stunden.clone();
        let mut fehlend: Vec<String> = Vec::new();
        for t in w.tage.iter().filter(|t| t.fehlt) {
            let k = tag_kurz(t.datum);
            if !fehlend.contains(&k) {
                fehlend.push(k);
            }
        }
        z.fehlende_tage = fehlend;
        z.offene_punkte = w.offene_punkte.len();
        z.luecken = w.tage.iter().flat_map(|t| &t.stichpunkte).filter(|s| s.luecke).count();
        z.diagnosen = w.tage.iter().flat_map(|t| &t.stichpunkte).filter(|s| s.diagnose.is_some()).count();
    }
    z
}

/// Zählt die Woche für das Badge? Vorwoche mit Notiz, nicht abgezeichnet, und etwas ist offen.
fn ist_offen(z: &WochenZeile) -> bool {
    z.path.is_some() && !z.abgezeichnet && (!z.fehlende_tage.is_empty() || z.offene_punkte > 0 || z.luecken > 0 || z.diagnosen > 0)
}

/// Alle Wochen von der ältesten Notiz bis heute (und spätere Notizen), neueste zuerst. Wochen ohne
/// Notiz (Ferien?) werden eingereiht. Liest jede Wochennotiz frisch von der Platte.
pub fn uebersicht(root: &Path, ctx: &Kontext, heute: NaiveDate) -> BerichtsheftUebersicht {
    let (hj, hkw, hmontag) = super::woche_von(heute);
    let mut notizen: Vec<(NaiveDate, &NoteMeta)> = ctx
        .wochen
        .iter()
        .filter_map(|m| woche_aus_pfad(&m.path).ok().map(|(_, _, mo)| (mo, m)))
        .collect();
    notizen.sort_by(|a, b| (a.0, &a.1.path).cmp(&(b.0, &b.1.path)));
    let aeltester = notizen.first().map(|(m, _)| *m).unwrap_or(hmontag).min(hmontag);
    let neuester = notizen.last().map(|(m, _)| *m).unwrap_or(hmontag).max(hmontag);
    let mut wochen = Vec::new();
    let mut mo = neuester;
    while mo >= aeltester {
        let (j, kw, _) = super::woche_von(mo);
        let aktuell = mo == hmontag;
        match notizen.iter().find(|(m, _)| *m == mo) {
            Some((_, meta)) => {
                let roh = lesen(root, &meta.path, ctx).map_err(|e| eprintln!("[berichtsheft] {}: {e}", meta.path)).ok();
                wochen.push(zeile_aus(roh.as_ref(), j, kw, mo, Some(meta.path.clone()), aktuell));
            }
            None => wochen.push(zeile_aus(None, j, kw, mo, None, aktuell)),
        }
        mo -= Duration::days(7);
    }
    let offen_vorwochen = wochen.iter().filter(|z| z.montag < hmontag && ist_offen(z)).count();
    BerichtsheftUebersicht {
        heute,
        aktuelle_woche: super::stem(hj, hkw),
        aktuelle_vorhanden: notizen.iter().any(|(m, _)| *m == hmontag),
        wochen,
        offen_vorwochen,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{self, standard, P_KW41, P_KW42};
    use super::*;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    fn kurz(w: &BerichtsWoche) -> Vec<(TagAbschnitt, NaiveDate, bool, Option<usize>)> {
        w.tage.iter().map(|t| (t.abschnitt, t.datum, t.fehlt, t.ueberschrift_zeile)).collect()
    }

    #[test]
    fn altformat_kw41_berufsschule_vor_betrieb() {
        let (dir, ctx) = standard();
        let w = lesen(dir.path(), P_KW41, &ctx).unwrap().woche;
        assert_eq!((w.jahr, w.kw, w.montag, w.sonntag), (2026, 41, d(2026, 10, 5), d(2026, 10, 11)));
        assert_eq!((w.status.as_deref(), w.stunden.as_deref(), w.abgezeichnet), (Some("In Arbeit"), Some("38"), false));
        assert_eq!(w.lernfelder, vec![2, 4]);
        use TagAbschnitt::{Berufsschule as S, Betrieb as B};
        assert_eq!(
            kurz(&w),
            vec![
                (S, d(2026, 10, 5), false, Some(13)),
                (B, d(2026, 10, 6), false, Some(17)),
                (B, d(2026, 10, 7), true, None),
                (S, d(2026, 10, 8), true, None),
                (B, d(2026, 10, 9), true, None),
            ]
        );
        let mo = &w.tage[0];
        assert_eq!(mo.wochentag, "Montag");
        assert_eq!(mo.stichpunkte.len(), 1);
        let s = &mo.stichpunkte[0];
        assert_eq!((s.line, s.fach.as_deref(), s.luecke), (14, Some("GEC"), false), "Altformat `[[…|GEC]]:` ist eine Fach-Zeile");
        assert_eq!(s.links, vec!["GEC – Geschäftsprozesse im E-Commerce".to_string(), "GEC 05.10.2026".to_string()]);
        let u: Vec<(&str, bool)> = mo.unterricht.iter().map(|u| (u.title.as_str(), u.verlinkt)).collect();
        assert_eq!(u, vec![("D-DV 05.10.2026", false), ("GEC 05.10.2026", true), ("GEC 2 05.10.2026", false)]);
        assert_eq!(mo.nachweis, NachweisStand::Fehlt);
        let di = &w.tage[1];
        let sp: Vec<(&str, bool)> = di.stichpunkte.iter().map(|s| (s.text.as_str(), s.luecke)).collect();
        assert_eq!(sp, vec![("Artikel im Shop gepflegt", false), ("Einschulung", true)]);
        assert!(di.bearbeitbar && di.art == BerichtsTagArt::Stichpunkte);
        assert_eq!(di.nachweis, NachweisStand::Vorhanden);
        assert!(di.unterricht.is_empty());
        // Lernfeld-Tabelle und offene Punkte (Abschnitt **und** Callout)
        assert_eq!(w.lernfeld_tabelle.len(), 1);
        assert_eq!((w.lernfeld_tabelle[0].line, w.lernfeld_tabelle[0].taetigkeit.as_str()), (28, "Artikel im Shop gepflegt"));
        assert_eq!(w.lernfeld_tabelle[0].lernfelder, vec![4]);
        let op: Vec<(usize, &str)> = w.offene_punkte.iter().map(|p| (p.line, p.text.as_str())).collect();
        assert_eq!(op, vec![(22, "Mittwoch und Freitag ergänzen"), (37, "Stundenzahl prüfen")]);
    }

    #[test]
    fn neues_format_kw42_krank_diagnose_platzhalter_sammelblock() {
        let (dir, ctx) = standard();
        let w = lesen(dir.path(), P_KW42, &ctx).unwrap().woche;
        assert_eq!(w.lernfelder, vec![4], "Blockliste");
        assert!(w.lernfeld_tabelle.is_empty(), "Platzhalterzeile `|  | [[]] |` zählt nicht");
        let tag = |a, t| w.tage.iter().find(|x| x.abschnitt == a && x.datum == d(2026, 10, t)).unwrap();
        use TagAbschnitt::{Berufsschule as S, Betrieb as B};
        // Montag: Übersicht und Platzhalter stehen in den Stichpunkten, zählen aber nicht als Inhalt
        let mo = tag(S, 12);
        let texte: Vec<&str> = mo.stichpunkte.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texte, vec!["GEC · D-DV · WSP · D", "**GEC** — *(noch einzutragen)*", "**D-DV** —"]);
        assert!(mo.stichpunkte.iter().all(|s| !s.luecke));
        assert_eq!(mo.stichpunkte[1].fach.as_deref(), Some("GEC"));
        assert!(mo.fehlt && mo.bearbeitbar);
        assert_eq!(mo.nachweis, NachweisStand::Platzhalter);
        // Krank-Tage, Sammelblock „Dienstag … und Donnerstag … — Arbeitsunfähigkeit“
        assert_eq!((tag(B, 13).art, tag(B, 13).fehlt, tag(B, 13).nachweis), (BerichtsTagArt::Krank, false, NachweisStand::Vorhanden));
        assert_eq!((tag(S, 15).art, tag(S, 15).nachweis), (BerichtsTagArt::Krank, NachweisStand::Vorhanden));
        assert!(!tag(B, 13).stichpunkte[0].luecke, "Sondertag ist nie eine Lücke");
        // Gesundheitsangabe aus Obsidian: markiert, nicht still verschluckt
        let mi = tag(B, 14);
        assert_eq!(mi.stichpunkte[0].diagnose.as_deref(), Some("Grippe"));
        assert_eq!((mi.fehlt, mi.art, mi.nachweis), (false, BerichtsTagArt::Stichpunkte, NachweisStand::Fehlt));
        assert!(w.warnungen.iter().any(|x| x.contains("„Grippe“ ist eine Gesundheitsangabe")), "{:?}", w.warnungen);
        assert_eq!(tag(B, 16).nachweis, NachweisStand::Platzhalter);
        assert_eq!(w.offene_punkte.len(), 2, "nur offene Punkte des Callouts, keine anderen Callouts");
    }

    #[test]
    fn bearbeitbar_datum_und_ohne_stundenplan() {
        let text = "## Betrieb\n### Dienstag, 11.11.\n- Retouren\n\t- 5 Pakete\n### Freitag, 13.11.2026\n- Inventur\n\nText dazu\n";
        let (_dir, mut ctx) = testhilfe::vault(&[]);
        let v = NoteVersion { sha256: String::new(), modified_ms: 0, size: 0 };
        let w = parsen("04 Betrieb/Wochen/2026-KW46.md", text, v.clone(), &ctx).unwrap().woche;
        let di = w.tage.iter().find(|t| t.datum == d(2026, 11, 10)).unwrap();
        assert!(!di.bearbeitbar);
        assert_eq!(di.hinweis.as_deref(), Some(HINWEIS_NICHT_BEARBEITBAR));
        assert_eq!(di.stichpunkte.len(), 2, "eingerückte Punkte stehen mit drin");
        assert!(w.warnungen.iter().any(|x| x.contains("passt nicht zur KW")), "{:?}", w.warnungen);
        let fr = w.tage.iter().find(|t| t.datum == d(2026, 11, 13)).unwrap();
        assert!(!fr.bearbeitbar, "Text unter den Stichpunkten");
        // ohne Stundenplan: nur die Tage der Datei + Warnung
        ctx.plan = None;
        let w = parsen("04 Betrieb/Wochen/2026-KW46.md", text, v, &ctx).unwrap().woche;
        assert_eq!(w.tage.len(), 2);
        assert!(w.warnungen.iter().any(|x| x.contains("Kein Stundenplan")));
        assert!(w.warnungen.iter().any(|x| x.contains("„## Berufsschule“")));
    }

    #[test]
    fn zeilenarten() {
        let b = vec!["GEC".to_string(), "D-DV".to_string(), "D".to_string()];
        use TagAbschnitt::Berufsschule as S;
        assert_eq!(zeilenart("GEC · D-DV · WSP · D", S, &b), Zeilenart::Uebersicht);
        assert_eq!(zeilenart("*(noch einzutragen)*", S, &b), Zeilenart::Platzhalter);
        assert_eq!(zeilenart("⟨…⟩", TagAbschnitt::Betrieb, &b), Zeilenart::Platzhalter);
        assert_eq!(zeilenart("**WSP** — ", S, &b), Zeilenart::Fach { kurz: "WSP".into(), inhalt: None });
        assert_eq!(zeilenart("**WSP** — *(noch einzutragen)*", S, &b), Zeilenart::Fach { kurz: "WSP".into(), inhalt: None });
        assert_eq!(
            zeilenart("**D-DV** — Kachel 3: SVERWEIS → [[D-DV 09.11.2026]]", S, &b),
            Zeilenart::Fach { kurz: "D-DV".into(), inhalt: Some("Kachel 3: SVERWEIS → [[D-DV 09.11.2026]]".into()) }
        );
        assert_eq!(zeilenart("D: Kommunikation", S, &b), Zeilenart::Fach { kurz: "D".into(), inhalt: Some("Kommunikation".into()) });
        assert_eq!(zeilenart("WSP und E", S, &b), Zeilenart::Inhalt);
        assert_eq!(zeilenart("**Arbeitsunfähig erkrankt** — ganztägig", S, &b), Zeilenart::Sondertag(BerichtsTagArt::Krank));
        assert_eq!(zeilenart("**GEC** — x", TagAbschnitt::Betrieb, &b), Zeilenart::Inhalt, "Fach-Zeilen nur in der Berufsschule");
        assert_eq!(ohne_pfeil_links("Thema → [[GEC 09.11.2026]] · [[GEC 2 09.11.2026]]"), "Thema");
        assert_eq!(listenzeile("- "), None);
        assert_eq!(listenzeile("-  Entfallen"), Some(("Entfallen".into(), true)));
    }
}
