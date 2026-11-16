//! „Ausformulierter Nachweis“: Stand je Tag, Entwurf ohne und mit KI, Schreiben in die Woche.
//!
//! * **Regel 3:** Krank/Urlaub/Feiertag/Entfallen bekommen einen festen Satz und gehen nie an die KI;
//!   Zeilen mit Gesundheitsangabe werden weggelassen; geschrieben wird nichts mit Diagnose-Wort.
//! * **Regel 4:** Lücken werden nie ausformuliert, sondern als `⟨Lücke: „…“ – Gegenstand fehlt⟩`
//!   ans Ende des Tagesblocks gesetzt. Ohne KI ist der Text nur umgeformt (Satzanfang groß, Punkt);
//!   mit KI wird jede Antwort verworfen, die einen Satz ohne Beleg, eine fremde Zahl, einen fremden
//!   Fettbegriff oder eine nicht markierte Lücke enthält ([`pruefe_antwort`]).
//!
//! Nachweis-Teil = Zeilen nach `## Ausformulierter Nachweis` bis zur ersten Callout-Zeile (`>`) oder
//! zur nächsten Überschrift der Ebene ≤ 2. Callouts dahinter („Offen vor dem Abzeichnen“ …) bleiben
//! byte-gleich.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate};
use regex::Regex;
use serde::Deserialize;

use super::regeln::{self, diagnose_meldung, diagnose_wort, falten, normalisiert};
use super::woche::{self, abschnitt_titel, ohne_pfeil_links, TagRoh, WocheRoh, Zeilenart};
use super::{tag_kurz, wochentag_im_text, wochentag_name, Gliederung, Kontext};
use crate::error::{AppError, AppResult};
use crate::guard;
use crate::ki;
use crate::logbuch::KiZugang;
use crate::model::{
    Aenderung, BerichtsTagArt, NachweisAnfrage, NachweisEntwurf, NachweisQuelle, NachweisSchreiben, NachweisStand,
    NachweisTag, NoteMeta, NoteVersion, TagAbschnitt,
};
use crate::write::{self, RetryPolicy, SelfWrites};

pub const ABSCHNITT_NACHWEIS: &str = "Ausformulierter Nachweis";
pub const UEBERSCHRIFT_BETRIEB: &str = "Tätigkeiten im Betrieb";

type Schluessel = (TagAbschnitt, NaiveDate);

// ---------------------------------------------------------------------------------------------
// Nachweis-Teil und Blöcke
// ---------------------------------------------------------------------------------------------

/// Ein Tagesblock: fette Kopfzeile bis vor die nächste Kopfzeile.
#[derive(Debug, Clone)]
pub struct Block {
    pub start: usize,
    pub ende: usize,
    /// alle `TT.MM.` der Kopfzeile, die in der Woche liegen
    pub daten: Vec<NaiveDate>,
    /// Kopfzeile beginnt mit „Berufsschule“
    pub berufsschule: bool,
}

#[derive(Debug, Clone)]
pub struct Teil {
    /// Index der Überschrift `## Ausformulierter Nachweis` (`None` = fehlt)
    pub kopf: Option<usize>,
    pub start: usize,
    pub ende: usize,
    pub bloecke: Vec<Block>,
}

static DATUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{1,2})\.(\d{1,2})\.").unwrap());

/// Kopfzeile eines Tagesblocks? `**…**` mit Wochentag und `TT.MM.`.
fn blockkopf(z: &str, montag: NaiveDate) -> Option<(Vec<NaiveDate>, bool)> {
    if !z.starts_with("**") || wochentag_im_text(z).is_none() || !DATUM.is_match(z) {
        return None;
    }
    let sonntag = montag + Duration::days(6);
    let mut daten = Vec::new();
    for c in DATUM.captures_iter(z) {
        let (t, m): (u32, u32) = (c[1].parse().unwrap_or(0), c[2].parse().unwrap_or(0));
        let mut d = montag;
        while d <= sonntag {
            if d.day() == t && d.month() == m && !daten.contains(&d) {
                daten.push(d);
            }
            d += Duration::days(1);
        }
    }
    let berufsschule = falten(z.trim_start_matches('*').trim()).starts_with("berufsschule");
    Some((daten, berufsschule))
}

pub fn teil(zeilen: &[String], gl: &Gliederung, montag: NaiveDate) -> Teil {
    let Some((kopf, ende2)) = gl.abschnitt(ABSCHNITT_NACHWEIS, zeilen.len()) else {
        return Teil { kopf: None, start: zeilen.len(), ende: zeilen.len(), bloecke: vec![] };
    };
    let start = kopf + 1;
    let ende = (start..ende2).find(|i| gl.prosa.contains(i) && zeilen[*i].starts_with('>')).unwrap_or(ende2);
    let mut bloecke: Vec<Block> = Vec::new();
    for (i, z) in zeilen.iter().enumerate().take(ende).skip(start) {
        if let Some((daten, berufsschule)) = blockkopf(z, montag) {
            if let Some(b) = bloecke.last_mut() {
                b.ende = i;
            }
            bloecke.push(Block { start: i, ende, daten, berufsschule });
        }
    }
    Teil { kopf: Some(kopf), start, ende, bloecke }
}

/// Zu welchen Tagen gehört der Block? Je Datum der Kopfzeile: Kopf „Berufsschule …“ → der
/// Schultag; sonst der Betriebstag, wenn es am Datum beide gibt.
pub fn besitzer(b: &Block, tage: &[Schluessel]) -> Vec<Schluessel> {
    let mut out = Vec::new();
    for d in &b.daten {
        let kand: Vec<Schluessel> = tage.iter().filter(|(_, x)| x == d).copied().collect();
        let gewaehlt: Vec<Schluessel> = if kand.is_empty() {
            vec![(if b.berufsschule { TagAbschnitt::Berufsschule } else { TagAbschnitt::Betrieb }, *d)]
        } else if b.berufsschule && kand.iter().any(|(a, _)| *a == TagAbschnitt::Berufsschule) {
            kand.into_iter().filter(|(a, _)| *a == TagAbschnitt::Berufsschule).collect()
        } else if kand.len() > 1 {
            kand.into_iter().filter(|(a, _)| *a == TagAbschnitt::Betrieb).collect()
        } else {
            kand
        };
        for g in gewaehlt {
            if !out.contains(&g) {
                out.push(g);
            }
        }
    }
    out
}

/// Text nach der fetten Kopfzeile und im Block nur Platzhalter?
fn block_stand(zeilen: &[String], b: &Block) -> NachweisStand {
    let kopf = &zeilen[b.start];
    let rest = kopf[2..].find("**").map(|p| &kopf[p + 4..]).unwrap_or("");
    let teile = std::iter::once(rest).chain(zeilen[b.start + 1..b.ende].iter().map(String::as_str));
    if teile.filter(|t| !t.trim().is_empty()).all(woche::ist_platzhalter_text) {
        NachweisStand::Platzhalter
    } else {
        NachweisStand::Vorhanden
    }
}

/// Stand des Nachweises für einen Tag.
pub fn stand(t: &Teil, zeilen: &[String], tag: Schluessel, tage: &[Schluessel]) -> NachweisStand {
    let mut stand = NachweisStand::Fehlt;
    for b in t.bloecke.iter().filter(|b| besitzer(b, tage).contains(&tag)) {
        match block_stand(zeilen, b) {
            NachweisStand::Vorhanden => return NachweisStand::Vorhanden,
            s => stand = s,
        }
    }
    stand
}

// ---------------------------------------------------------------------------------------------
// Entwurf ohne KI
// ---------------------------------------------------------------------------------------------

/// Satz aus einem Stichpunkt: erster Buchstabe groß, Punkt am Ende, Markdown bleibt.
pub fn satz(text: &str) -> String {
    let t = text.trim();
    let mut out = String::with_capacity(t.len() + 1);
    let mut gross = false;
    for c in t.chars() {
        if !gross && c.is_alphanumeric() {
            gross = true;
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// „a“, „a und b“, „a, b und c“
fn aufzaehlung(v: &[String]) -> String {
    match v {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., letzter] => format!("{} und {letzter}", rest.join(", ")),
    }
}

/// Fester Text für Sondertage (Regel 3: nie mehr als „arbeitsunfähig erkrankt“).
fn sondertag_text(art: BerichtsTagArt) -> (&'static str, &'static str) {
    match art {
        BerichtsTagArt::Krank => ("Arbeitsunfähigkeit", "Ich war arbeitsunfähig erkrankt."),
        BerichtsTagArt::Urlaub => ("Urlaub", "Urlaub."),
        BerichtsTagArt::Feiertag => ("Feiertag", "Feiertag."),
        BerichtsTagArt::Entfallen => ("Entfallen", "Der Unterricht ist entfallen."),
        BerichtsTagArt::Stichpunkte => ("", ""),
    }
}

/// Darf der Stichpunkt in den Nachweis (bzw. zur KI)? Keine Gesundheitsangabe, kein Platzhalter.
fn nutzbar(t: &TagRoh, i: usize) -> bool {
    t.tag.stichpunkte[i].diagnose.is_none() && matches!(t.arten[i], Zeilenart::Inhalt | Zeilenart::Sondertag(_) | Zeilenart::Fach { inhalt: Some(_), .. })
}

/// Lücken des Tages (ohne Gesundheitsangaben).
fn luecken_von(t: &TagRoh) -> Vec<String> {
    t.tag.stichpunkte.iter().filter(|s| s.luecke && s.diagnose.is_none()).map(|s| s.text.clone()).collect()
}

/// `In **GEC**: Handelskalkulation.`
fn fach_absatz(kurz: &str, inhalt: &str) -> Option<String> {
    let text = ohne_pfeil_links(inhalt);
    let text = text.trim().trim_end_matches(['.', ':', ';', ',']).trim();
    (!text.is_empty()).then(|| format!("In **{kurz}**: {text}."))
}

/// Nachweis eines Tages ohne KI – nur Umformung, nichts dazu. `None` = nichts zu formulieren.
pub fn ohne_ki(t: &TagRoh) -> Option<NachweisTag> {
    if t.tag.fehlt {
        return None;
    }
    let mut n = NachweisTag {
        datum: t.tag.datum,
        abschnitt: t.tag.abschnitt,
        wochentag: t.tag.wochentag.clone(),
        ueberschrift: String::new(),
        absaetze: vec![],
        luecken: vec![],
        vorhanden: t.tag.nachweis,
        uebernehmen: t.tag.nachweis != NachweisStand::Vorhanden,
        ki: false,
    };
    if t.tag.art != BerichtsTagArt::Stichpunkte {
        let (ue, absatz) = sondertag_text(t.tag.art);
        n.ueberschrift = ue.into();
        n.absaetze = vec![absatz.into()];
        return Some(n);
    }
    let mut saetze = Vec::new();
    for (i, s) in t.tag.stichpunkte.iter().enumerate() {
        if !nutzbar(t, i) || s.luecke {
            continue;
        }
        match &t.arten[i] {
            Zeilenart::Fach { kurz, inhalt: Some(inhalt) } => n.absaetze.extend(fach_absatz(kurz, inhalt)),
            _ => saetze.push(satz(&s.text)),
        }
    }
    if !saetze.is_empty() {
        n.absaetze.push(saetze.join(" "));
    }
    if t.tag.abschnitt == TagAbschnitt::Berufsschule {
        let titel: Vec<String> = t.tag.unterricht.iter().map(|u| format!("[[{}]]", u.title)).collect();
        if !titel.is_empty() {
            n.absaetze.push(format!("Ausgearbeitet in {}.", aufzaehlung(&titel)));
        }
    } else {
        n.ueberschrift = UEBERSCHRIFT_BETRIEB.into();
    }
    n.luecken = luecken_von(t);
    (!n.absaetze.is_empty() || !n.luecken.is_empty()).then_some(n)
}

// ---------------------------------------------------------------------------------------------
// Mit KI: was gesendet wird und wie die Antwort geprüft wird
// ---------------------------------------------------------------------------------------------

/// Eine Unterrichtsnotiz, wie sie an die KI ginge: ohne EK, ohne Zeilen mit Gesundheitsangabe.
#[derive(Debug, Clone)]
pub struct NotizQuelle {
    pub meta: NoteMeta,
    pub text: String,
    pub ek_entfernt: bool,
    pub gesendet: bool,
}

/// Ein Tag, wie er an die KI geht, mit allem, was zur Prüfung der Antwort gehört.
#[derive(Debug, Clone)]
pub struct KiTag {
    pub schluessel: Schluessel,
    /// Text des Tages im Nutzertext (ohne Notizen)
    pub zeilen: Vec<String>,
    pub notizen: Vec<(String, String)>,
    /// erkannte Lücken – müssen in `luecken` zurückkommen
    pub luecken: Vec<String>,
    /// Stichpunkte, die die KI zusätzlich als Lücke melden darf
    pub kandidaten: Vec<String>,
    pub staemme: BTreeSet<String>,
    pub zahlen: BTreeSet<String>,
    pub quelltext: String,
    /// es gibt etwas zu formulieren (nicht nur Lücken)
    pub hat_inhalt: bool,
}

/// Zeilen eines Notiztextes ohne Gesundheitsangabe (Regel 3) – lieber eine Zeile zu viel weg.
fn ohne_diagnose(text: &str) -> String {
    text.lines().filter(|l| diagnose_wort(l).is_none()).collect::<Vec<_>>().join("\n")
}

/// Unterrichtsnotizen der Schultage frisch lesen (Body, EK entfernt).
pub fn notizen_lesen(root: &Path, roh: &WocheRoh, ctx: &Kontext, privat_erlauben: bool) -> AppResult<Vec<NotizQuelle>> {
    let mut out = Vec::new();
    for t in roh.tage.iter().filter(|t| t.tag.abschnitt == TagAbschnitt::Berufsschule && !t.tag.fehlt) {
        for u in &t.tag.unterricht {
            let Some(meta) = ctx.unterricht.iter().find(|m| m.path == u.path) else { continue };
            let c = write::read_for_edit(root, &meta.path, &RetryPolicy::short())?;
            let ek = guard::contains_purchase_price(&c.body);
            let thema = woche::thema_von(meta).map(|t| format!("Thema: {}\n\n", guard::strip_purchase_prices(&t))).unwrap_or_default();
            let text = ohne_diagnose(&format!("{thema}{}", guard::strip_purchase_prices(&c.body)));
            out.push(NotizQuelle { meta: meta.clone(), text, ek_entfernt: ek, gesendet: !meta.private || privat_erlauben });
        }
    }
    Ok(out)
}

/// Tage für die KI: nur Art „Stichpunkte“ mit Inhalt; ohne Gesundheitsangaben, ohne EK; Lücken mit
/// `[LÜCKE]` markiert. Sondertage, Frontmatter und andere Wochen gehen nie mit.
pub fn ki_tage(roh: &WocheRoh, notizen: &[NotizQuelle], ek_entfernt: &mut bool) -> Vec<KiTag> {
    let mut out = Vec::new();
    for t in roh.tage.iter().filter(|t| !t.tag.fehlt && t.tag.art == BerichtsTagArt::Stichpunkte) {
        let mut zeilen = Vec::new();
        let mut belege: Vec<String> = Vec::new();
        let mut luecken = Vec::new();
        let mut kandidaten = Vec::new();
        for (i, s) in t.tag.stichpunkte.iter().enumerate() {
            if !nutzbar(t, i) {
                continue;
            }
            if guard::contains_purchase_price(&s.text) {
                *ek_entfernt = true;
                continue;
            }
            if s.luecke {
                zeilen.push(format!("- [LÜCKE] {}", s.text));
                luecken.push(s.text.clone());
            } else {
                zeilen.push(format!("- {}", s.text));
                belege.push(s.text.clone());
                kandidaten.push(s.text.clone());
            }
        }
        let mut gesendete_notizen = Vec::new();
        if t.tag.abschnitt == TagAbschnitt::Berufsschule {
            for n in notizen.iter().filter(|n| n.gesendet && n.meta.datum == Some(t.tag.datum)) {
                gesendete_notizen.push((n.meta.title.clone(), n.text.clone()));
                belege.push(n.meta.title.clone());
                belege.push(n.text.clone());
                if let Some(f) = n.meta.fields.iter().find(|f| f.key.eq_ignore_ascii_case("fach")).and_then(|f| f.value.as_str()) {
                    belege.push(f.to_string());
                }
            }
        }
        let hat_inhalt = !belege.is_empty();
        if zeilen.is_empty() && gesendete_notizen.is_empty() {
            continue;
        }
        let quelltext = belege.join("\n");
        let d = t.tag.datum;
        let mut zahlen = regeln::zahlen(&quelltext);
        zahlen.extend([d.day().to_string(), format!("{:02}", d.day()), d.month().to_string(), format!("{:02}", d.month()), d.year().to_string()]);
        out.push(KiTag {
            schluessel: (t.tag.abschnitt, d),
            zeilen,
            notizen: gesendete_notizen,
            luecken,
            kandidaten,
            staemme: regeln::staemme(&quelltext),
            zahlen,
            quelltext: falten(&quelltext),
            hat_inhalt,
        });
    }
    out
}

fn abschnitt_wert(a: TagAbschnitt) -> &'static str {
    match a {
        TagAbschnitt::Berufsschule => "berufsschule",
        TagAbschnitt::Betrieb => "betrieb",
    }
}

/// Nutzertext für die KI. Notizen stehen zwischen `<notiz>`-Markierungen (Grenze neutralisiert).
pub fn nutzer_text(roh: &WocheRoh, tage: &[KiTag]) -> String {
    let w = &roh.woche;
    let mut s = format!("Woche: KW {} ({} – {})\n", w.kw, w.montag.format("%d.%m.%Y"), w.sonntag.format("%d.%m.%Y"));
    for t in tage {
        let (a, d) = t.schluessel;
        s.push_str(&format!("\n## Tag {} · {} · abschnitt: {}\n", d.format("%Y-%m-%d"), wochentag_name(d.weekday()), abschnitt_wert(a)));
        if t.zeilen.is_empty() {
            s.push_str("(keine Stichpunkte)\n");
        }
        for z in &t.zeilen {
            s.push_str(z);
            s.push('\n');
        }
        for (titel, text) in &t.notizen {
            let text = text.replace("</notiz>", "</ notiz>");
            s.push_str(&format!("<notiz titel=\"{}\">\n{}\n</notiz>\n", titel.replace('"', "'"), text.trim()));
        }
    }
    s
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AntwortRoh {
    tage: Vec<AntwortTag>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AntwortTag {
    datum: String,
    abschnitt: String,
    ueberschrift: String,
    absaetze: Vec<String>,
    luecken: Vec<String>,
}

/// Geprüfte KI-Antwort eines Tages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KiErgebnis {
    pub schluessel: Schluessel,
    pub ueberschrift: String,
    pub absaetze: Vec<String>,
    /// erkannte Lücken + von der KI gemeldete Stichpunkte
    pub luecken: Vec<String>,
}

fn eine_zeile(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn gleiche_luecke(a: &str, b: &str) -> bool {
    let (x, y) = (normalisiert(a), normalisiert(b));
    !x.is_empty() && (x == y || y.contains(&x))
}

/// Prüft eine KI-Antwort (Regeln 3, 4, 6). `Err` = Verstoß (Begründung für die Neuanforderung).
pub fn pruefe_antwort(text: &str, erwartet: &[KiTag]) -> Result<Vec<KiErgebnis>, String> {
    let roh: AntwortRoh =
        serde_json::from_str(text.trim()).map_err(|e| format!("Die Antwort ist kein gültiges JSON nach dem Schema ({e})."))?;
    let mut out: Vec<KiErgebnis> = Vec::new();
    for t in roh.tage {
        let datum = NaiveDate::parse_from_str(t.datum.trim(), "%Y-%m-%d")
            .map_err(|_| format!("„{}“ ist kein Datum JJJJ-MM-TT.", t.datum))?;
        let abschnitt = match t.abschnitt.trim().to_lowercase().as_str() {
            "berufsschule" => TagAbschnitt::Berufsschule,
            "betrieb" => TagAbschnitt::Betrieb,
            x => return Err(format!("Unbekannter Abschnitt „{x}“.")),
        };
        let name = format!("{} {}", abschnitt_titel(abschnitt), tag_kurz(datum));
        let Some(e) = erwartet.iter().find(|e| e.schluessel == (abschnitt, datum)) else {
            return Err(format!("{name} gehört nicht zur Anfrage."));
        };
        if out.iter().any(|o| o.schluessel == e.schluessel) {
            return Err(format!("{name} steht doppelt in der Antwort."));
        }
        let ueberschrift = eine_zeile(&t.ueberschrift).replace("**", "");
        let absaetze: Vec<String> =
            t.absaetze.iter().map(|a| eine_zeile(a)).filter(|a| !a.is_empty() && !a.starts_with("⟨Lücke")).collect();
        let ki_luecken: Vec<String> = t.luecken.iter().map(|l| eine_zeile(l)).filter(|l| !l.is_empty()).collect();
        if absaetze.len() > 8 || absaetze.iter().any(|a| a.chars().count() > 1200) {
            return Err(format!("{name}: zu lang (höchstens 8 Absätze, 1200 Zeichen je Absatz)."));
        }
        let alle: Vec<&String> = std::iter::once(&ueberschrift).chain(&absaetze).chain(&ki_luecken).collect();
        if let Some(w) = alle.iter().find_map(|s| diagnose_wort(s)) {
            return Err(format!("{name}: „{w}“ ist eine Gesundheitsangabe – im Berichtsheft steht nur „arbeitsunfähig erkrankt“."));
        }
        if alle.iter().any(|s| guard::contains_purchase_price(s)) {
            return Err(format!("{name}: Die Antwort enthält einen Einkaufspreis (EK)."));
        }
        for l in &e.luecken {
            if !ki_luecken.iter().any(|k| gleiche_luecke(l, k) || gleiche_luecke(k, l)) {
                return Err(format!("{name}: Die Lücke „{l}“ steht nicht in „luecken“ – Lücken nicht ausformulieren, sondern zurückgeben."));
            }
        }
        if e.hat_inhalt && absaetze.is_empty() {
            return Err(format!("{name}: Der Text fehlt."));
        }
        let texte: Vec<&String> = std::iter::once(&ueberschrift).filter(|u| !u.is_empty()).chain(&absaetze).collect();
        for s in texte.iter().flat_map(|a| regeln::saetze(a)) {
            if !regeln::hat_beleg(&s, &e.staemme) {
                return Err(format!("{name}: Der Satz „{s}“ hat keinen Beleg in den Stichpunkten (nichts hinzufügen)."));
            }
        }
        for z in texte.iter().flat_map(|a| regeln::zahlen(a)) {
            if !e.zahlen.contains(&z) {
                return Err(format!("{name}: Die Zahl „{z}“ steht nicht in den Stichpunkten."));
            }
        }
        for f in texte.iter().flat_map(|a| regeln::fette_begriffe(a)) {
            if !e.quelltext.contains(&falten(&f)) {
                return Err(format!("{name}: „**{f}**“ steht nicht in den Quellen."));
            }
        }
        let mut luecken = e.luecken.clone();
        for k in &ki_luecken {
            if let Some(stp) = e.kandidaten.iter().find(|c| normalisiert(c) == normalisiert(k)) {
                if !luecken.iter().any(|l| gleiche_luecke(l, stp)) {
                    luecken.push(stp.clone());
                }
            }
        }
        out.push(KiErgebnis {
            schluessel: e.schluessel,
            ueberschrift: if abschnitt == TagAbschnitt::Betrieb { ueberschrift } else { String::new() },
            absaetze,
            luecken,
        });
    }
    if let Some(f) = erwartet.iter().find(|e| !out.iter().any(|o| o.schluessel == e.schluessel)) {
        return Err(format!("{} {} fehlt in der Antwort.", abschnitt_titel(f.schluessel.0), tag_kurz(f.schluessel.1)));
    }
    out.sort_by_key(|o| (o.schluessel.1, o.schluessel.0));
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Entwurf
// ---------------------------------------------------------------------------------------------

/// Nachweis-Entwurf ohne KI (`ki = None`) oder mit KI. Schreibt nichts.
pub fn entwurf(root: &Path, ctx: &Kontext, req: &NachweisAnfrage, ki: Option<KiZugang>) -> AppResult<NachweisEntwurf> {
    let roh = woche::lesen(root, &req.path, ctx)?;
    let mit_ki = ki.is_some();
    let mut warnungen = Vec::new();
    let mut fehlende_tage = Vec::new();
    let mut tage: Vec<NachweisTag> = Vec::new();
    for t in &roh.tage {
        match ohne_ki(t) {
            Some(n) => tage.push(n),
            None => {
                let k = tag_kurz(t.tag.datum);
                if !t.tag.fehlt {
                    warnungen.push(format!("{} {k}: nichts zu formulieren (nur Gesundheitsangaben oder Platzhalter).", abschnitt_titel(t.tag.abschnitt)));
                }
                if !fehlende_tage.contains(&k) {
                    fehlende_tage.push(k);
                }
            }
        }
    }
    for t in &roh.tage {
        if t.tag.stichpunkte.iter().any(|s| s.diagnose.is_some()) {
            warnungen.push(format!(
                "{} {}: Zeilen mit Gesundheitsangabe wurden weggelassen (Regel 3).",
                abschnitt_titel(t.tag.abschnitt),
                tag_kurz(t.tag.datum)
            ));
        }
    }
    let notizen = notizen_lesen(root, &roh, ctx, req.privat_erlauben)?;
    let mut ek_woche = false;
    let ki_tage = ki_tage(&roh, &notizen, &mut ek_woche);
    let mut aufruf = None;
    if let Some(z) = ki {
        if ki_tage.is_empty() {
            warnungen.push("Kein Tag mit Stichpunkten – nichts an die KI gesendet.".into());
        } else {
            let nutzer = nutzer_text(&roh, &ki_tage);
            let (ergebnis, a) = ki::berichtsheft_ki(z.provider, z.system, &nutzer, z.verlauf_dir, |text| pruefe_antwort(text, &ki_tage))?;
            for e in ergebnis {
                if let Some(n) = tage.iter_mut().find(|n| (n.abschnitt, n.datum) == e.schluessel) {
                    n.ueberschrift = e.ueberschrift;
                    n.absaetze = e.absaetze;
                    n.luecken = e.luecken;
                    n.ki = true;
                }
            }
            aufruf = Some(a);
        }
    }
    let gesendet = mit_ki && aufruf.is_some();
    let mut quellen = vec![NachweisQuelle {
        path: roh.woche.path.clone(),
        title: roh.woche.title.clone(),
        privat: guard::is_private(&roh.woche.path),
        gesendet: if mit_ki { gesendet } else { !ki_tage.is_empty() },
        ek_entfernt: ek_woche,
    }];
    for n in &notizen {
        if mit_ki && !n.gesendet {
            warnungen.push(format!("„{}“ liegt in „10 Privat/“ und wurde nicht an die KI gesendet.", n.meta.title));
        }
        if n.ek_entfernt {
            warnungen.push(format!("EK-Angaben aus „{}“ wurden entfernt (Regel 6).", n.meta.title));
        }
        quellen.push(NachweisQuelle {
            path: n.meta.path.clone(),
            title: n.meta.title.clone(),
            privat: n.meta.private,
            gesendet: n.gesendet && (!mit_ki || gesendet),
            ek_entfernt: n.ek_entfernt,
        });
    }
    if ek_woche {
        warnungen.push("Stichpunkte mit EK-Angabe gehen nicht an die KI (Regel 6).".into());
    }
    Ok(NachweisEntwurf { path: roh.woche.path.clone(), tage, quellen, mit_ki, ki: aufruf, fehlende_tage, warnungen })
}

// ---------------------------------------------------------------------------------------------
// Schreiben
// ---------------------------------------------------------------------------------------------

/// Absatz für die Datei: eine Zeile; `>`/`#` am Anfang wird maskiert (sonst Callout/Überschrift).
fn absatz_zeile(s: &str) -> String {
    let z = eine_zeile(s);
    if z.starts_with('>') || z.starts_with('#') {
        format!("\\{z}")
    } else {
        z
    }
}

/// Zeilen eines neuen Tagesblocks (endet mit einer Leerzeile).
pub fn block_zeilen(t: &NachweisTag) -> Vec<String> {
    let datum = t.datum.format("%d.%m.%Y");
    let wt = wochentag_name(t.datum.weekday());
    let ue = eine_zeile(&t.ueberschrift).replace("**", "");
    let kopf = match (t.abschnitt, ue.is_empty()) {
        (TagAbschnitt::Berufsschule, _) => format!("**Berufsschule — {wt}, {datum}**"),
        (TagAbschnitt::Betrieb, true) => format!("**{wt}, {datum}**"),
        (TagAbschnitt::Betrieb, false) => format!("**{wt}, {datum} — {ue}**"),
    };
    let mut out = vec![kopf, String::new()];
    for a in t.absaetze.iter().map(|a| absatz_zeile(a)).filter(|a| !a.is_empty()) {
        out.push(a);
        out.push(String::new());
    }
    for l in t.luecken.iter().map(|l| eine_zeile(l)).filter(|l| !l.is_empty()) {
        out.push(format!("⟨Lücke: „{l}“ – Gegenstand fehlt⟩"));
        out.push(String::new());
    }
    out
}

enum Einheit {
    Alt(usize),
    Neu(Vec<usize>),
}

/// Regel 3 über alle Texte, die geschrieben werden sollen.
fn pruefe_diagnose(tage: &[NachweisTag]) -> AppResult<()> {
    for t in tage {
        let alle = std::iter::once(&t.ueberschrift).chain(&t.absaetze).chain(&t.luecken);
        if let Some(w) = alle.filter_map(|s| diagnose_wort(s)).next() {
            return Err(AppError::InvalidInput(diagnose_meldung(&w)));
        }
    }
    Ok(())
}

/// Neuer Text mit den Tagesblöcken: vorhandene Blöcke ersetzen, sonst chronologisch einfügen. Ein
/// Sammelblock wird nur ersetzt, wenn alle seine Tage mitgeschrieben werden.
pub fn plane_schreiben(text: &str, roh: &WocheRoh, tage: &[NachweisTag]) -> AppResult<(String, Vec<String>)> {
    if tage.is_empty() {
        return Err(AppError::InvalidInput("Kein Tag zum Schreiben ausgewählt.".into()));
    }
    pruefe_diagnose(tage)?;
    let (montag, sonntag) = (roh.woche.montag, roh.woche.sonntag);
    let mut w: Vec<Schluessel> = Vec::new();
    for t in tage {
        if t.datum < montag || t.datum > sonntag {
            return Err(AppError::InvalidInput(format!("{} gehört nicht zu KW {}.", t.datum.format("%d.%m.%Y"), roh.woche.kw)));
        }
        if w.contains(&(t.abschnitt, t.datum)) {
            return Err(AppError::InvalidInput(format!("{} {} steht doppelt in der Anfrage.", abschnitt_titel(t.abschnitt), tag_kurz(t.datum))));
        }
        w.push((t.abschnitt, t.datum));
    }
    let (mut zeilen, endet) = super::zeilen_von(text);
    let gl = Gliederung::von(text, &zeilen);
    let teil = teil(&zeilen, &gl, montag);
    let mut sortiert: Vec<usize> = (0..tage.len()).collect();
    sortiert.sort_by_key(|i| (tage[*i].datum, tage[*i].abschnitt));

    // Abschnitt fehlt: vor `## Verknüpft`, sonst am Ende anlegen
    if teil.kopf.is_none() {
        let pos = gl
            .koepfe
            .iter()
            .find(|k| k.ebene == 2 && super::titel_norm(&k.text).starts_with("verknupft"))
            .map(|k| k.idx)
            .unwrap_or(zeilen.len());
        let mut neu = Vec::new();
        if pos > 0 && !zeilen[pos - 1].trim().is_empty() {
            neu.push(String::new());
        }
        neu.push(format!("## {ABSCHNITT_NACHWEIS}"));
        neu.push(String::new());
        for i in &sortiert {
            neu.extend(block_zeilen(&tage[*i]));
        }
        let am_ende = pos == zeilen.len();
        if am_ende {
            neu.pop(); // die Datei endet mit genau einem Zeilenumbruch
        }
        zeilen.splice(pos..pos, neu);
        let endet = endet || am_ende;
        return Ok((super::zusammen(&zeilen, endet), vec![format!("Abschnitt „## {ABSCHNITT_NACHWEIS}“ angelegt.")]));
    }

    let mut alle: Vec<Schluessel> = roh.tage.iter().map(|t| (t.tag.abschnitt, t.tag.datum)).collect();
    for k in &w {
        if !alle.contains(k) {
            alle.push(*k);
        }
    }
    let besitz: Vec<Vec<Schluessel>> = teil.bloecke.iter().map(|b| besitzer(b, &alle)).collect();
    // Anker: erster Block, der zu einem geschriebenen Tag gehört
    let mut anker: Vec<Option<usize>> = vec![None; tage.len()];
    let mut weg = vec![false; teil.bloecke.len()];
    for (bi, owners) in besitz.iter().enumerate() {
        if !owners.iter().any(|o| w.contains(o)) {
            continue;
        }
        if let Some(fremd) = owners.iter().find(|o| !w.contains(o)) {
            let kopf = zeilen[teil.bloecke[bi].start].trim();
            return Err(AppError::InvalidInput(format!(
                "Der Block „{kopf}“ gehört auch zu {} {} – bitte alle seine Tage zusammen schreiben oder in Obsidian bearbeiten.",
                abschnitt_titel(fremd.0),
                tag_kurz(fremd.1)
            )));
        }
        weg[bi] = true;
        for (ti, k) in w.iter().enumerate() {
            if owners.contains(k) && anker[ti].is_none() {
                anker[ti] = Some(bi);
            }
        }
    }
    let mut einheiten: Vec<Einheit> = Vec::new();
    for (bi, &entfaellt) in weg.iter().enumerate() {
        if !entfaellt {
            einheiten.push(Einheit::Alt(bi));
            continue;
        }
        let hier: Vec<usize> = sortiert.iter().copied().filter(|ti| anker[*ti] == Some(bi)).collect();
        if !hier.is_empty() {
            einheiten.push(Einheit::Neu(hier));
        }
    }
    let schluessel_von = |e: &Einheit| -> Option<(NaiveDate, TagAbschnitt)> {
        match e {
            Einheit::Alt(bi) => besitz[*bi].iter().map(|(a, d)| (*d, *a)).min(),
            Einheit::Neu(v) => v.iter().map(|ti| (tage[*ti].datum, tage[*ti].abschnitt)).min(),
        }
    };
    for ti in sortiert.iter().copied().filter(|ti| anker[*ti].is_none()) {
        let k = (tage[ti].datum, tage[ti].abschnitt);
        let pos = einheiten.iter().position(|e| schluessel_von(e).is_some_and(|s| s > k)).unwrap_or(einheiten.len());
        einheiten.insert(pos, Einheit::Neu(vec![ti]));
    }

    let erster = teil.bloecke.first().map(|b| b.start).unwrap_or(teil.ende);
    let mut neu: Vec<String> = zeilen[teil.start..erster].to_vec();
    for e in &einheiten {
        match e {
            Einheit::Alt(bi) => {
                let b = &teil.bloecke[*bi];
                neu.extend(zeilen[b.start..b.ende].iter().cloned());
            }
            Einheit::Neu(v) => {
                for ti in v {
                    if neu.last().is_some_and(|z| !z.trim().is_empty()) {
                        neu.push(String::new());
                    }
                    neu.extend(block_zeilen(&tage[*ti]));
                }
            }
        }
    }
    zeilen.splice(teil.start..teil.ende, neu);
    Ok((super::zusammen(&zeilen, endet), vec![]))
}

/// Tage in den Nachweis der Woche schreiben (Vorschau zuerst, Regel 9).
pub fn schreiben(root: &Path, ctx: &Kontext, req: &NachweisSchreiben, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<Aenderung> {
    // Regel 3 schon vor dem Lesen: nichts mit Diagnose-Wort, auch nicht in der Vorschau
    pruefe_diagnose(&req.tage)?;
    super::aendern(root, &req.path, req.vorschau, req.basis.as_ref(), writes, p, |text| {
        let leer = NoteVersion { sha256: String::new(), modified_ms: 0, size: 0 };
        let roh = woche::parsen(&req.path, text, leer, ctx)?;
        plane_schreiben(text, &roh, &req.tage)
    })
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{self, bytes, standard, version, KW41, KW42, P_KW41, P_KW42};
    use super::*;
    use crate::ki::attrappe::Attrappe;
    use crate::model::TagAbschnitt::{Berufsschule, Betrieb};

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    fn tag(a: TagAbschnitt, datum: NaiveDate, ue: &str, absaetze: &[&str], luecken: &[&str]) -> NachweisTag {
        NachweisTag {
            datum,
            abschnitt: a,
            wochentag: String::new(),
            ueberschrift: ue.into(),
            absaetze: absaetze.iter().map(|s| s.to_string()).collect(),
            luecken: luecken.iter().map(|s| s.to_string()).collect(),
            vorhanden: NachweisStand::Fehlt,
            uebernehmen: true,
            ki: false,
        }
    }

    fn roh_von(rel: &str, text: &str, ctx: &Kontext) -> WocheRoh {
        woche::parsen(rel, text, NoteVersion { sha256: String::new(), modified_ms: 0, size: 0 }, ctx).unwrap()
    }

    /// Alles ab dem ersten Callout im Nachweis.
    fn callouts(t: &str) -> &str {
        &t[t.find("> [!warning]").unwrap()..]
    }

    #[test]
    fn bloecke_ersetzen_einfuegen_und_sammelblock() {
        let (_dir, ctx) = standard();
        let roh = roh_von(P_KW42, KW42, &ctx);
        // Freitag: Platzhalter-Block wird ersetzt, Callouts bleiben byte-gleich
        let fr = tag(Betrieb, d(2026, 10, 16), "Retouren und Versand", &["Retouren angenommen und erfasst."], &[]);
        let (t, _) = plane_schreiben(KW42, &roh, std::slice::from_ref(&fr)).unwrap();
        assert_eq!(
            t,
            KW42.replace(
                "**Freitag, 16.10.2026** — *noch einzutragen.*\n",
                "**Freitag, 16.10.2026 — Retouren und Versand**\n\nRetouren angenommen und erfasst.\n"
            )
        );
        assert_eq!(callouts(&t), callouts(KW42));
        // Sammelblock nur mit allen seinen Tagen
        let di = tag(Betrieb, d(2026, 10, 13), "Arbeitsunfähigkeit", &["Ich war arbeitsunfähig erkrankt."], &[]);
        let e = plane_schreiben(KW42, &roh, std::slice::from_ref(&di));
        assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("gehört auch zu Berufsschule Do 15.10.")), "{e:?}");
        let mut doo = di.clone();
        doo.abschnitt = Berufsschule;
        doo.datum = d(2026, 10, 15);
        let (t, _) = plane_schreiben(KW42, &roh, &[doo.clone(), di.clone()]).unwrap();
        assert!(t.contains(
            "**Dienstag, 13.10.2026 — Arbeitsunfähigkeit**\n\nIch war arbeitsunfähig erkrankt.\n\n\
             **Berufsschule — Donnerstag, 15.10.2026**\n\nIch war arbeitsunfähig erkrankt.\n\n**Freitag, 16.10.2026** —"
        ), "{t}");
        assert!(!t.contains("und Donnerstag, 15.10.2026"));
        assert_eq!(callouts(&t), callouts(KW42));
        // Mittwoch hat keinen Block: chronologisch vor Freitag, mit Lücken-Absatz
        let mi = tag(Betrieb, d(2026, 10, 14), "", &[], &["Einschulung"]);
        let (t, _) = plane_schreiben(KW42, &roh, &[mi]).unwrap();
        assert!(t.contains(
            "Ich war arbeitsunfähig erkrankt.\n\n**Mittwoch, 14.10.2026**\n\n⟨Lücke: „Einschulung“ – Gegenstand fehlt⟩\n\n**Freitag, 16.10.2026** —"
        ), "{t}");
        // KW41: Montag (Berufsschule) vor dem Dienstags-Block
        let roh41 = roh_von(P_KW41, KW41, &ctx);
        let mo = tag(Berufsschule, d(2026, 10, 5), "", &["In **GEC**: Handelskalkulation."], &[]);
        let (t, _) = plane_schreiben(KW41, &roh41, &[mo]).unwrap();
        assert!(t.contains(
            "## Ausformulierter Nachweis\n\n**Berufsschule — Montag, 05.10.2026**\n\nIn **GEC**: Handelskalkulation.\n\n**Betrieb — Dienstag, 06.10.2026**"
        ), "{t}");
        assert_eq!(callouts(&t), callouts(KW41));
    }

    #[test]
    fn abschnitt_fehlt_wird_vor_verknuepft_angelegt() {
        let (_dir, ctx) = standard();
        let text = "## Betrieb\n### Dienstag, 06.10.\n- Retouren\n\n## Verknüpft\n- [[Betrieb]]\n";
        let roh = roh_von(P_KW41, text, &ctx);
        let (t, w) = plane_schreiben(text, &roh, &[tag(Betrieb, d(2026, 10, 6), "", &["Retouren."], &[])]).unwrap();
        assert_eq!(
            t,
            "## Betrieb\n### Dienstag, 06.10.\n- Retouren\n\n## Ausformulierter Nachweis\n\n**Dienstag, 06.10.2026**\n\nRetouren.\n\n## Verknüpft\n- [[Betrieb]]\n"
        );
        assert_eq!(w.len(), 1);
        // außerhalb der Woche → Fehler
        assert!(plane_schreiben(text, &roh, &[tag(Betrieb, d(2026, 10, 13), "", &["x"], &[])]).is_err());
    }

    #[test]
    fn regel3_nachweis_mit_getippter_migraene_blockiert() {
        let (dir, ctx) = standard();
        let root = dir.path();
        for vorschau in [true, false] {
            let req = NachweisSchreiben {
                path: P_KW41.into(),
                tage: vec![tag(Betrieb, d(2026, 10, 6), "Shop", &["Trotz Migräne habe ich Artikel gepflegt."], &[])],
                vorschau,
                basis: Some(version(root, P_KW41)),
            };
            let e = schreiben(root, &ctx, &req, &SelfWrites::new(), &RetryPolicy::none());
            assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.starts_with("„Migräne“ ist eine Gesundheitsangabe")), "{e:?}");
            assert_eq!(bytes(root, P_KW41), KW41.as_bytes());
        }
    }

    const FEST: &[&str] = &[
        "tätigkeiten", "im", "betrieb", "in", "ausgearbeitet", "und", "ich", "war", "arbeitsunfähig", "erkrankt", "der",
        "unterricht", "ist", "entfallen", "urlaub", "feiertag", "arbeitsunfähigkeit",
    ];

    fn woerter(s: &str) -> Vec<String> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect()
    }

    #[test]
    fn regel4_ohne_ki_nur_woerter_aus_stichpunkten_und_luecke_einschulung() {
        let (dir, ctx) = standard();
        let req = NachweisAnfrage { path: P_KW41.into(), mit_ki: false, privat_erlauben: false };
        let e = entwurf(dir.path(), &ctx, &req, None).unwrap();
        assert!(e.ki.is_none() && !e.mit_ki);
        assert_eq!(e.fehlende_tage, vec!["Mi 07.10.", "Do 08.10.", "Fr 09.10."]);
        let roh = woche::lesen(dir.path(), P_KW41, &ctx).unwrap();
        let mut erlaubt: BTreeSet<String> = FEST.iter().map(|s| s.to_string()).collect();
        for t in &roh.woche.tage {
            for s in &t.stichpunkte {
                erlaubt.extend(woerter(&s.text));
            }
            for u in &t.unterricht {
                erlaubt.extend(woerter(&u.title));
            }
        }
        for t in &e.tage {
            for w in std::iter::once(&t.ueberschrift).chain(&t.absaetze).flat_map(|a| woerter(a)) {
                assert!(erlaubt.contains(&w), "„{w}“ steht nicht in den Stichpunkten");
            }
        }
        let di = e.tage.iter().find(|t| t.datum == d(2026, 10, 6)).unwrap();
        assert_eq!((di.ueberschrift.as_str(), di.absaetze.clone()), (UEBERSCHRIFT_BETRIEB, vec!["Artikel im Shop gepflegt.".to_string()]));
        assert_eq!(di.luecken, vec!["Einschulung"]);
        assert_eq!((di.vorhanden, di.uebernehmen), (NachweisStand::Vorhanden, false));
        let mo = e.tage.iter().find(|t| t.datum == d(2026, 10, 5)).unwrap();
        assert_eq!(
            mo.absaetze,
            vec![
                "In **GEC**: Handelskalkulation.".to_string(),
                "Ausgearbeitet in [[D-DV 05.10.2026]], [[GEC 05.10.2026]] und [[GEC 2 05.10.2026]].".to_string()
            ]
        );
        // geschrieben steht die Lücke als eigener Absatz am Ende des Blocks
        let block = block_zeilen(di).join("\n");
        assert_eq!(block, "**Dienstag, 06.10.2026 — Tätigkeiten im Betrieb**\n\nArtikel im Shop gepflegt.\n\n⟨Lücke: „Einschulung“ – Gegenstand fehlt⟩\n");
        // Quellen: Woche + Unterrichtsnotizen des Schultags („würde gesendet“)
        let q: Vec<(&str, bool)> = e.quellen.iter().map(|q| (q.title.as_str(), q.gesendet)).collect();
        assert_eq!(q, vec![("2026-KW41", true), ("D-DV 05.10.2026", true), ("GEC 05.10.2026", true), ("GEC 2 05.10.2026", true)]);
    }

    #[test]
    fn ohne_ki_sondertage_fester_text_und_diagnose_weggelassen() {
        let (dir, ctx) = standard();
        let req = NachweisAnfrage { path: P_KW42.into(), mit_ki: false, privat_erlauben: false };
        let e = entwurf(dir.path(), &ctx, &req, None).unwrap();
        let di = e.tage.iter().find(|t| t.datum == d(2026, 10, 13)).unwrap();
        assert_eq!((di.ueberschrift.as_str(), di.absaetze.clone()), ("Arbeitsunfähigkeit", vec!["Ich war arbeitsunfähig erkrankt.".to_string()]));
        assert!(e.tage.iter().all(|t| t.datum != d(2026, 10, 14)), "Mittwoch hat nur eine Gesundheitsangabe");
        assert!(e.fehlende_tage.contains(&"Mi 14.10.".to_string()) && e.fehlende_tage.contains(&"Mo 12.10.".to_string()));
        let alles = serde_json::to_string(&e.tage).unwrap();
        assert!(!alles.contains("Grippe") && !alles.contains("Arzt"));
        assert!(e.warnungen.iter().any(|w| w.contains("Gesundheitsangabe")));
        let fr = e.tage.iter().find(|t| t.datum == d(2026, 10, 16)).unwrap();
        assert_eq!(fr.absaetze, vec!["Retouren angenommen, geprüft und im Warenwirtschaftssystem erfasst. Pakete für den Versand gepackt."]);
        assert_eq!(satz("**shopify**: produkte"), "**Shopify**: produkte.");
    }

    const KI_MO: &str = r#"{"datum":"2026-10-05","abschnitt":"berufsschule","ueberschrift":"","absaetze":["In **GEC** wurde die Handelskalkulation mit der Vorwärtskalkulation bis zum Listenverkaufspreis behandelt."],"luecken":[]}"#;

    fn ki_kw41(di: &str) -> String {
        format!(r#"{{"tage":[{KI_MO},{di}]}}"#)
    }

    const DI_GUT: &str = r#"{"datum":"2026-10-06","abschnitt":"betrieb","ueberschrift":"Artikelpflege im Shop","absaetze":["Ich habe Artikel im Shop gepflegt."],"luecken":["Einschulung"]}"#;

    fn mit_ki(dir: &Path, ctx: &Kontext, rel: &str, p: &Attrappe) -> AppResult<NachweisEntwurf> {
        let req = NachweisAnfrage { path: rel.into(), mit_ki: true, privat_erlauben: false };
        entwurf(dir, ctx, &req, Some(KiZugang { provider: p, system: "sys", verlauf_dir: None }))
    }

    #[test]
    fn ki_gueltige_antwort_und_lueckenabsatz() {
        let (dir, ctx) = standard();
        let p = Attrappe::neu(&[&ki_kw41(DI_GUT)]);
        let e = mit_ki(dir.path(), &ctx, P_KW41, &p).unwrap();
        assert_eq!(e.ki.as_ref().unwrap().versuche, 1);
        let di = e.tage.iter().find(|t| t.datum == d(2026, 10, 6)).unwrap();
        assert!(di.ki);
        assert_eq!((di.ueberschrift.as_str(), di.luecken.clone()), ("Artikelpflege im Shop", vec!["Einschulung".to_string()]));
        let anfrage = &p.anfragen.lock().unwrap()[0];
        assert!(anfrage.nutzer.contains("- [LÜCKE] Einschulung"), "{}", anfrage.nutzer);
        assert!(anfrage.nutzer.contains("<notiz titel=\"GEC 05.10.2026\">"));
        assert!(!anfrage.nutzer.contains("tags:") && !anfrage.nutzer.contains("abgezeichnet"), "keine Frontmatter der Woche");
        assert!(e.quellen.iter().all(|q| q.gesendet));
    }

    #[test]
    fn regel3_ki_antwort_mit_erkaeltung_wird_verworfen_zweimal_fehler() {
        let (dir, ctx) = standard();
        let schlecht = ki_kw41(&DI_GUT.replace("Ich habe Artikel im Shop gepflegt.", "Trotz Erkältung habe ich Artikel im Shop gepflegt."));
        let p = Attrappe::neu(&[&schlecht, &ki_kw41(DI_GUT)]);
        let e = mit_ki(dir.path(), &ctx, P_KW41, &p).unwrap();
        assert_eq!(e.ki.unwrap().versuche, 2);
        assert!(p.anfragen.lock().unwrap()[1].nutzer.contains("verworfen"));
        assert!(!serde_json::to_string(&e.tage).unwrap().contains("Erkältung"));
        let p = Attrappe::neu(&[&schlecht, &schlecht]);
        let r = mit_ki(dir.path(), &ctx, P_KW41, &p);
        assert!(matches!(&r, Err(AppError::Ki(m)) if m.contains("Gesundheitsangabe") && m.contains("Ohne KI formulieren")), "{r:?}");
    }

    #[test]
    fn regel3_krank_tag_und_diagnose_zeile_gehen_nie_an_die_ki() {
        let (dir, ctx) = standard();
        let fr = r#"{"tage":[{"datum":"2026-10-16","abschnitt":"betrieb","ueberschrift":"Retouren und Versand","absaetze":["Ich habe Retouren angenommen, geprüft und im Warenwirtschaftssystem erfasst und Pakete für den Versand gepackt."],"luecken":[]}]}"#;
        let p = Attrappe::neu(&[fr]);
        let e = mit_ki(dir.path(), &ctx, P_KW42, &p).unwrap();
        let n = &p.anfragen.lock().unwrap()[0].nutzer;
        for verboten in ["Grippe", "Arzt", "rbeitsunfähig", "2026-10-13", "2026-10-15", "2026-10-14", "kein Berufsschulbesuch"] {
            assert!(!n.contains(verboten), "„{verboten}“ ging an die KI:\n{n}");
        }
        assert!(n.contains("2026-10-16"));
        // Krank-Tage stehen trotzdem im Entwurf – mit festem Text, ohne KI
        let di = e.tage.iter().find(|t| t.datum == d(2026, 10, 13)).unwrap();
        assert!(!di.ki && di.absaetze == vec!["Ich war arbeitsunfähig erkrankt.".to_string()]);
    }

    #[test]
    fn regel4_satz_ohne_beleg_wird_verworfen() {
        let (dir, ctx) = standard();
        let erfunden = ki_kw41(&DI_GUT.replace(
            "Ich habe Artikel im Shop gepflegt.",
            "Ich habe Artikel im Shop gepflegt. Außerdem habe ich die Kundenhotline betreut.",
        ));
        let p = Attrappe::neu(&[&erfunden, &erfunden]);
        let r = mit_ki(dir.path(), &ctx, P_KW41, &p);
        assert!(matches!(&r, Err(AppError::Ki(m)) if m.contains("Kundenhotline betreut") && m.contains("keinen Beleg")), "{r:?}");
        assert_eq!(p.anfragen.lock().unwrap().len(), 2, "einmal neu angefordert");
    }

    #[test]
    fn regel4_erfundene_zahl_und_fettbegriff_werden_verworfen() {
        let (dir, ctx) = standard();
        let zahl = ki_kw41(&DI_GUT.replace("Ich habe Artikel", "Ich habe 25 Artikel"));
        let p = Attrappe::neu(&[&zahl, &ki_kw41(DI_GUT)]);
        let e = mit_ki(dir.path(), &ctx, P_KW41, &p).unwrap();
        assert_eq!(e.ki.unwrap().versuche, 2);
        assert!(p.anfragen.lock().unwrap()[1].nutzer.contains("Die Zahl „25“"));
        let fett = ki_kw41(&DI_GUT.replace("im Shop", "in **Shopware**"));
        let p = Attrappe::neu(&[&fett, &fett]);
        assert!(matches!(mit_ki(dir.path(), &ctx, P_KW41, &p), Err(AppError::Ki(m)) if m.contains("**Shopware**")));
    }

    #[test]
    fn regel4_nicht_markierte_luecke_wird_verworfen() {
        let (dir, ctx) = standard();
        let ohne = ki_kw41(&DI_GUT.replace(r#""luecken":["Einschulung"]"#, r#""luecken":[]"#));
        let ausgeschmueckt = ki_kw41(&DI_GUT.replace(
            r#""absaetze":["Ich habe Artikel im Shop gepflegt."],"luecken":["Einschulung"]"#,
            r#""absaetze":["Ich habe Artikel im Shop gepflegt. Danach hatte ich eine Einschulung."],"luecken":[]"#,
        ));
        let p = Attrappe::neu(&[&ohne, &ausgeschmueckt]);
        let r = mit_ki(dir.path(), &ctx, P_KW41, &p);
        assert!(matches!(&r, Err(AppError::Ki(m)) if m.contains("Die Lücke „Einschulung“")), "{r:?}");
    }

    #[test]
    fn antwort_mit_falschen_tagen_wird_verworfen() {
        let (_dir, ctx) = standard();
        let roh = roh_von(P_KW41, KW41, &ctx);
        let mut ek = false;
        let tage = ki_tage(&roh, &[], &mut ek);
        assert_eq!(tage.len(), 2);
        let fehlt = format!(r#"{{"tage":[{DI_GUT}]}}"#);
        assert!(pruefe_antwort(&fehlt, &tage).unwrap_err().contains("fehlt"));
        let doppelt = format!(r#"{{"tage":[{KI_MO},{DI_GUT},{DI_GUT}]}}"#);
        assert!(pruefe_antwort(&doppelt, &tage).unwrap_err().contains("doppelt"));
        let fremd = ki_kw41(&DI_GUT.replace("2026-10-06", "2026-10-07"));
        assert!(pruefe_antwort(&fremd, &tage).unwrap_err().contains("gehört nicht zur Anfrage"));
        let feld = ki_kw41(&DI_GUT.replace(r#""luecken""#, r#""extra":1,"luecken""#));
        assert!(pruefe_antwort(&feld, &tage).is_err());
        let ok = pruefe_antwort(&ki_kw41(DI_GUT), &tage).unwrap();
        assert_eq!(ok.len(), 2);
        let _ = testhilfe::KW42;
    }
}
