//! Woche anlegen und Schultage aus den Unterrichtsnotizen übernehmen.
//!
//! Die Vorlage `99 Meta/Templates/Berichtsheft-Woche.md` wird per Zeilenchirurgie gefüllt: nur
//! `kw`, `zeitraum`, die KW-/Datums-Platzhalter, die Verknüpft-Zeilen der Schultage und die
//! Fach-Zeilen ändern sich. Alles andere bleibt wörtlich (auch der „So füllst du“-Kasten und
//! `⟨Überschrift⟩`). Eigener Text in der Woche wird nie geändert.

use std::path::Path;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use regex::{NoExpand, Regex};

use super::regeln::diagnose_wort;
use super::tag::tag_einfuegen;
use super::woche::{self, bekannte_faecher, fach_zeile, listenzeile, notiz_kurz, thema_von, Zeilenart};
use super::{stem, tag_der_woche, ueberschrift, wochentag_am_anfang, wochentag_im_text, Gliederung, Kontext};
use super::{TEMPLATE_WOCHE, WOCHEN_DIR};
use crate::error::{AppError, AppResult};
use crate::guard;
use crate::logbuch::{ohne_markdown, quellinhalt};
use crate::model::{Aenderung, CreateResult, NoteMeta, RelPath, SchultageUebernehmen, TagAbschnitt, WochenVorschau};
use crate::unterricht::{self, normalize_thema, W_FREMDE_PLATZHALTER};
use crate::watcher::SharedIndex;
use crate::write::{self, RetryPolicy, SelfWrites};

pub const THEMA_FEHLT: &str = "*(Thema noch eintragen)*";

// ---------------------------------------------------------------------------------------------
// Inhalt einer Unterrichtsnotiz für die Fach-Zeile
// ---------------------------------------------------------------------------------------------

/// Eine Unterrichtsnotiz der Woche mit dem Text für ihre Fach-Zeile.
#[derive(Debug, Clone)]
pub struct NotizInhalt {
    pub meta: NoteMeta,
    /// `thema`, sonst erster Punkt aus „Inhalt der Stunde“; `None` → `*(Thema noch eintragen)*`
    pub inhalt: Option<String>,
}

/// Höchstens `max` Zeichen, an einer Wortgrenze gekürzt.
fn kuerzen(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let teil: String = s.chars().take(max - 1).collect();
    let schnitt = teil.rfind(char::is_whitespace).map(|i| &teil[..i]).unwrap_or(&teil);
    format!("{}…", schnitt.trim_end_matches([',', ';', ':', ' ']))
}

/// Unterrichtsnotizen (ohne `10 Privat`) frisch lesen und ihren Zeilentext bestimmen.
pub fn notiz_inhalte(root: &Path, notizen: &[&NoteMeta], warnungen: &mut Vec<String>) -> Vec<NotizInhalt> {
    let mut out = Vec::new();
    for m in notizen.iter().filter(|m| !m.private) {
        let inhalt = thema_von(m).and_then(|t| normalize_thema(&t)).or_else(|| {
            let body = write::read_for_edit(root, &m.path, &RetryPolicy::short()).ok()?.body;
            let erster = quellinhalt(&body).inhalt.into_iter().next()?;
            normalize_thema(&ohne_markdown(&erster)).map(|t| kuerzen(&t, 120))
        });
        // Regel 3: nichts mit Gesundheitsangabe ins Berichtsheft übernehmen
        let inhalt = match inhalt {
            Some(t) if diagnose_wort(&t).is_some() => {
                warnungen.push(format!("Das Thema von „{}“ wurde nicht übernommen (Gesundheitsangabe).", m.title));
                None
            }
            x => x,
        };
        out.push(NotizInhalt { meta: (*m).clone(), inhalt });
    }
    out
}

/// `- **GEC** — Thema 1 · Thema 2 → [[GEC 09.11.2026]] · [[GEC 2 09.11.2026]]`
fn fach_neu(kurz: &str, ns: &[&NotizInhalt]) -> String {
    let mut themen: Vec<String> = Vec::new();
    for n in ns {
        let t = n.inhalt.clone().unwrap_or_else(|| THEMA_FEHLT.to_string());
        if !themen.contains(&t) {
            themen.push(t);
        }
    }
    let links: Vec<String> = ns.iter().map(|n| format!("[[{}]]", n.meta.title)).collect();
    format!("- **{kurz}** — {} → {}", themen.join(" · "), links.join(" · "))
}

// ---------------------------------------------------------------------------------------------
// Schultage übernehmen
// ---------------------------------------------------------------------------------------------

/// Fach-Zeilen für die noch nicht verlinkten Unterrichtsnotizen eintragen. Liefert den neuen Text,
/// die eingetragenen Notizen und Warnungen. Ohne `## Berufsschule` → Fehler.
pub fn plane_schultage(
    text: &str,
    montag: NaiveDate,
    notizen: &[NotizInhalt],
    bekannte: &[String],
) -> AppResult<(String, Vec<RelPath>, Vec<String>)> {
    let (mut zeilen, endet) = super::zeilen_von(text);
    let titel = woche::ABSCHNITT_BERUFSSCHULE;
    if Gliederung::von(text, &zeilen).abschnitt(titel, zeilen.len()).is_none() {
        return Err(AppError::InvalidInput(format!(
            "Die Woche hat keinen Abschnitt „## {titel}“ – bitte in Obsidian anlegen."
        )));
    }
    // je Tag und Fach (in Stundenplan-Reihenfolge der Blöcke)
    let mut gruppen: Vec<(NaiveDate, String, Vec<&NotizInhalt>)> = Vec::new();
    for n in notizen.iter().filter(|n| !n.meta.private) {
        let (Some(d), Some(kurz)) = (n.meta.datum, notiz_kurz(&n.meta)) else { continue };
        match gruppen.iter_mut().find(|(gd, gk, _)| *gd == d && gk.eq_ignore_ascii_case(&kurz)) {
            Some(g) => g.2.push(n),
            None => gruppen.push((d, kurz, vec![n])),
        }
    }
    gruppen.sort_by_key(|(d, _, _)| *d);
    let mut eingetragen = Vec::new();
    let mut warnungen = Vec::new();
    for (d, kurz, ns) in gruppen {
        let jetzt = super::zusammen(&zeilen, endet);
        let gl = Gliederung::von(&jetzt, &zeilen);
        let Some(bereich) = gl.abschnitt(titel, zeilen.len()) else { break };
        let w = d.weekday();
        let kopf = gl
            .koepfe
            .iter()
            .find(|k| k.idx > bereich.0 && k.idx < bereich.1 && k.ebene >= 3 && wochentag_am_anfang(&k.text) == Some(w));
        let Some(kopf) = kopf else {
            let zeile = fach_neu(&kurz, &ns);
            tag_einfuegen(&mut zeilen, &gl, bereich, montag, w, vec![zeile]);
            warnungen.push(format!("Überschrift „{}“ unter „## {titel}“ angelegt.", super::tag::tag_kopf(d, w)));
            eingetragen.extend(ns.iter().map(|n| n.meta.path.clone()));
            continue;
        };
        let k = kopf.idx;
        let ende = gl.naechster_kopf(k).map(|n| n.idx).unwrap_or(zeilen.len()).min(bereich.1);
        let verlinkt = |n: &NotizInhalt| {
            zeilen[k + 1..ende]
                .iter()
                .any(|z| crate::vault::links::parse_links(z, 0).iter().any(|l| l.target.eq_ignore_ascii_case(&n.meta.title)))
        };
        let offen: Vec<&NotizInhalt> = ns.iter().copied().filter(|n| !verlinkt(n)).collect();
        if offen.is_empty() {
            continue;
        }
        let mut platzhalter: Option<(usize, String)> = None;
        let mut letzte_fach = None;
        let mut letzte_liste = None;
        let mut nur_platzhalter: Option<usize> = None;
        let mut anderes = false;
        for (i, z) in zeilen.iter().enumerate().take(ende).skip(k + 1) {
            if z.trim().is_empty() {
                continue;
            }
            let liste = z.trim_start().starts_with('-');
            match listenzeile(z) {
                None => nur_platzhalter = nur_platzhalter.or(Some(i)), // bloßes `- `
                Some((t, _)) if liste => {
                    letzte_liste = Some(i);
                    if let Some((x, inhalt)) = fach_zeile(&t, bekannte) {
                        letzte_fach = Some(i);
                        anderes = true;
                        if x.eq_ignore_ascii_case(&kurz) && inhalt.is_none() && platzhalter.is_none() {
                            platzhalter = Some((i, x));
                        }
                    } else if woche::zeilenart(&t, TagAbschnitt::Berufsschule, bekannte) == Zeilenart::Platzhalter {
                        nur_platzhalter = nur_platzhalter.or(Some(i));
                    } else {
                        anderes = true;
                    }
                }
                Some(_) => anderes = true,
            }
        }
        match (platzhalter, nur_platzhalter) {
            (Some((i, x)), _) => zeilen[i] = fach_neu(&x, &offen),
            (None, Some(i)) if !anderes => zeilen[i] = fach_neu(&kurz, &offen),
            _ => {
                let nach = letzte_fach.or(letzte_liste).unwrap_or(k);
                zeilen.insert(nach + 1, fach_neu(&kurz, &offen));
            }
        }
        eingetragen.extend(offen.iter().map(|n| n.meta.path.clone()));
    }
    Ok((super::zusammen(&zeilen, endet), eingetragen, warnungen))
}

/// „Aus Unterrichtsnotizen übernehmen“ (Vorschau zuerst, Regel 9).
pub fn schultage(root: &Path, ctx: &Kontext, req: &SchultageUebernehmen, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<Aenderung> {
    let (_, _, montag) = woche::woche_aus_pfad(&req.path)?;
    let mut warnungen = Vec::new();
    let notizen = notiz_inhalte(root, &ctx.unterricht_der_woche(montag), &mut warnungen);
    let bekannte = bekannte_faecher(ctx);
    let mut a = super::aendern(root, &req.path, req.vorschau, req.basis.as_ref(), writes, p, |text| {
        let (neu, eingetragen, w) = plane_schultage(text, montag, &notizen, &bekannte)?;
        let mut alle = w;
        if eingetragen.is_empty() {
            alle.push("Alle Unterrichtsnotizen der Woche sind schon eingetragen.".into());
        }
        Ok((neu, alle))
    })?;
    warnungen.append(&mut a.warnungen);
    a.warnungen = warnungen;
    Ok(a)
}

// ---------------------------------------------------------------------------------------------
// Vorlage füllen
// ---------------------------------------------------------------------------------------------

static VOR_STEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}-KW⟨NN-1⟩").unwrap());
static EIGEN_STEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}-KW⟨NN⟩").unwrap());
static DATUM_PLATZ: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"⟨TT\.MM\.(?:JJJJ)?⟩").unwrap());
static VERKNUEPFT_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^- (Montag|Dienstag|Mittwoch|Donnerstag|Freitag|Samstag|Sonntag): \[\[\]\][ \t]*$").unwrap()
});

fn datum_fuer(platzhalter: &str, d: NaiveDate) -> String {
    if platzhalter.contains("JJJJ") {
        d.format("%d.%m.%Y").to_string()
    } else {
        d.format("%d.%m.").to_string()
    }
}

/// Füllt die Vorlage (dekodierter Text, `\n`). `links` = Unterrichtsnotizen je Wochentag.
pub fn vorlage_fuellen(vorlage: &str, jahr: i32, kw: u32, montag: NaiveDate, links: &[(Weekday, Vec<String>)]) -> (String, Vec<String>) {
    let sonntag = montag + Duration::days(6);
    let (vj, vkw, _) = super::woche_von(montag - Duration::days(7));
    let mut note = crate::vault::frontmatter::NoteText::parse(vorlage);
    note.set_scalar("kw", &kw.to_string());
    note.set_scalar("zeitraum", &format!("{} – {}", montag.format("%Y-%m-%d"), sonntag.format("%Y-%m-%d")));
    let (vor, eigen) = (stem(vj, vkw), stem(jahr, kw));
    let mut abschnitt = String::new();
    let mut zeilen = Vec::new();
    for zeile in note.body().split('\n') {
        if let Some((2, t)) = ueberschrift(zeile) {
            abschnitt = super::titel_norm(&t);
        }
        let mut z = VOR_STEM.replace_all(zeile, NoExpand(&vor)).into_owned();
        z = EIGEN_STEM.replace_all(&z, NoExpand(&eigen)).into_owned();
        z = z.replace("⟨NN-1⟩", &format!("{vkw:02}")).replace("⟨NN⟩", &format!("{kw:02}"));
        if z.contains("Zeitraum") {
            let mut n = 0;
            z = DATUM_PLATZ
                .replace_all(&z, |c: &regex::Captures| {
                    n += 1;
                    match n {
                        1 => datum_fuer(&c[0], montag),
                        2 => datum_fuer(&c[0], sonntag),
                        _ => c[0].to_string(),
                    }
                })
                .into_owned();
        } else if let Some(w) = wochentag_im_text(&z) {
            let d = tag_der_woche(montag, w);
            z = DATUM_PLATZ.replace_all(&z, |c: &regex::Captures| datum_fuer(&c[0], d)).into_owned();
        }
        if abschnitt.starts_with("verknupft") {
            if let Some(c) = VERKNUEPFT_TAG.captures(&z) {
                let w = wochentag_am_anfang(&c[1]);
                if let Some((_, l)) = links.iter().find(|(lw, l)| Some(*lw) == w && !l.is_empty()) {
                    z = format!("- {}: {}", &c[1], l.iter().map(|t| format!("[[{t}]]")).collect::<Vec<_>>().join(" · "));
                }
            }
        }
        zeilen.push(z);
    }
    note.set_body(zeilen.join("\n"));
    let out = note.render();
    let mut warnungen = Vec::new();
    if out.contains("{{") || out.contains("<%") {
        warnungen.push(W_FREMDE_PLATZHALTER.to_string());
    }
    (out, warnungen)
}

/// Liest die Vorlage im Vault (fehlt → Fehler). Liefert die Bytes dekodiert.
fn vorlage_lesen(root: &Path) -> AppResult<write::Decoded> {
    let (abs, _) = guard::resolve_for_read(root, TEMPLATE_WOCHE)?;
    let bytes = match write::read_versioned(&abs, &RetryPolicy::short()) {
        Ok((b, _)) => b,
        Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(AppError::InvalidInput(format!(
                "Die Vorlage „{TEMPLATE_WOCHE}“ fehlt im Vault – ohne Vorlage legt die App keine Woche an."
            )))
        }
        Err(e) => return Err(e),
    };
    let d = write::decode(&bytes);
    if !d.utf8 {
        return Err(AppError::NotEditable(TEMPLATE_WOCHE.into()));
    }
    Ok(d)
}

/// Trockenlauf „Diese Woche anlegen“ – schreibt nichts.
pub fn vorschau(root: &Path, ctx: &Kontext, datum: NaiveDate) -> AppResult<WochenVorschau> {
    let (jahr, kw, montag) = super::woche_von(datum);
    let s = stem(jahr, kw);
    let rel_path = format!("{WOCHEN_DIR}/{s}.md");
    let existiert = ctx.wochen.iter().find(|m| m.title.eq_ignore_ascii_case(&s)).map(|m| m.path.clone());
    let d = vorlage_lesen(root)?;
    let mut warnungen = Vec::new();
    let woche_notizen = ctx.unterricht_der_woche(montag);
    let notizen = notiz_inhalte(root, &woche_notizen, &mut warnungen);
    let mut links: Vec<(Weekday, Vec<String>)> = Vec::new();
    for n in &notizen {
        let Some(t) = n.meta.datum else { continue };
        match links.iter_mut().find(|(w, _)| *w == t.weekday()) {
            Some((_, l)) => l.push(n.meta.title.clone()),
            None => links.push((t.weekday(), vec![n.meta.title.clone()])),
        }
    }
    let (gefuellt, w1) = vorlage_fuellen(&d.text, jahr, kw, montag, &links);
    warnungen.extend(w1);
    let (text, eingetragen) = match plane_schultage(&gefuellt, montag, &notizen, &bekannte_faecher(ctx)) {
        Ok((t, e, w)) => {
            warnungen.extend(w);
            (t, e)
        }
        Err(AppError::InvalidInput(m)) => {
            warnungen.push(m);
            (gefuellt, vec![])
        }
        Err(e) => return Err(e),
    };
    if ctx.plan.is_none() {
        warnungen.push("Kein Stundenplan gefunden – Schul- und Betriebstage stehen nur so in der Woche, wie die Vorlage sie vorgibt.".into());
    }
    for m in woche_notizen.iter().filter(|m| m.private) {
        warnungen.push(format!("„{}“ liegt in „10 Privat/“ und wird nicht eingetragen.", m.title));
    }
    Ok(WochenVorschau { rel_path, text: write::encode_like(&text, &d), existiert, eingetragen, warnungen })
}

/// Woche anlegen (nie überschreiben → `Exists`) und sofort in den Index übernehmen.
pub fn anlegen(
    index: &SharedIndex,
    root: &Path,
    ctx: &Kontext,
    datum: NaiveDate,
    writes: &SelfWrites,
    p: &RetryPolicy,
) -> AppResult<CreateResult> {
    let v = vorschau(root, ctx, datum)?;
    if let Some(existing) = v.existiert {
        return Ok(CreateResult::Exists { rel_path: existing });
    }
    unterricht::anlegen_mit_index(index, root, &v.rel_path, &v.text, v.warnungen, writes, p)
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{self, standard, KW41, P_KW41, VORLAGE, WSP};
    use super::*;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    #[test]
    fn vorlage_kw46_nur_platzhalter_aendern_sich() {
        let links = vec![(Weekday::Thu, vec!["WSP 12.11.2026".to_string()])];
        let (t, w) = vorlage_fuellen(VORLAGE, 2026, 46, d(2026, 11, 9), &links);
        assert!(w.is_empty(), "{w:?}");
        for zeile in [
            "kw: 46",
            "zeitraum: 2026-11-09 – 2026-11-15",
            "# Berichtsheft KW 46",
            "**Zeitraum:** 09.11. – 15.11.2026 · **Ausbildungsjahr:** 1",
            "### Dienstag, 10.11.",
            "### Freitag, 13.11.",
            "### Donnerstag, 12.11.",
            "**Dienstag, 10.11.2026 — ⟨Überschrift⟩**",
            "**Berufsschule — Montag, 09.11.2026**",
            "- [[2026-KW45]] — Vorwoche",
            "- Montag: [[]]",
            "- Donnerstag: [[WSP 12.11.2026]]",
            "- [[2026-KW46]] — Wochenrückblick in [[Reflexion]]",
        ] {
            assert!(t.lines().any(|l| l == zeile), "fehlt: {zeile}\n{t}");
        }
        // alles andere byte-gleich
        let (a, b): (Vec<&str>, Vec<&str>) = (VORLAGE.lines().collect(), t.lines().collect());
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            if x != y {
                assert!(x.contains('⟨') || x.starts_with("kw:") || x.starts_with("zeitraum:") || *x == "- Donnerstag: [[]]", "{x} → {y}");
            }
        }
        assert!(t.contains("⟨Fließtext, 3–5 Sätze."), "Inhalts-Platzhalter bleiben wörtlich");
        assert!(t.ends_with('\n') == VORLAGE.ends_with('\n'));
    }

    #[test]
    fn vorlage_kw01_vorwoche_ueber_den_jahreswechsel() {
        let (t, _) = vorlage_fuellen(VORLAGE, 2026, 1, d(2025, 12, 29), &[]);
        assert!(t.contains("- [[2025-KW52]] — Vorwoche"), "{t}");
        assert!(t.contains("- [[2026-KW01]] — Wochenrückblick"));
        assert!(t.contains("zeitraum: 2025-12-29 – 2026-01-04"));
        assert!(t.contains("# Berichtsheft KW 01"));
        assert!(t.contains("**Zeitraum:** 29.12. – 04.01.2026"));
        assert!(t.contains("### Freitag, 02.01."));
        let (t, _) = vorlage_fuellen(VORLAGE, 2027, 1, d(2027, 1, 4), &[]);
        assert!(t.contains("- [[2026-KW53]] — Vorwoche"), "2026 hat 53 ISO-Wochen");
    }

    #[test]
    fn schultage_leere_zeile_ersetzt_eigener_text_bleibt() {
        let (dir, ctx) = testhilfe::vault(&[("03 Unterricht/WSP 12.11.2026.md", WSP)]);
        let mut w = Vec::new();
        let notizen = notiz_inhalte(dir.path(), &ctx.unterricht_der_woche(d(2026, 11, 9)), &mut w);
        let b = bekannte_faecher(&ctx);
        let (vorlage, _) = vorlage_fuellen(VORLAGE, 2026, 46, d(2026, 11, 9), &[]);
        let (t, e, _) = plane_schultage(&vorlage, d(2026, 11, 9), &notizen, &b).unwrap();
        assert_eq!(e, vec!["03 Unterricht/WSP 12.11.2026.md".to_string()]);
        let neu: Vec<&str> = t.lines().filter(|l| !vorlage.lines().any(|v| v == *l)).collect();
        assert_eq!(neu, vec!["- **WSP** — Kündigung → [[WSP 12.11.2026]]"]);
        assert_eq!(t.lines().count(), vorlage.lines().count(), "ersetzt, nicht eingefügt");
        assert!(t.contains("### Montag, 09.11.\n- GEC · D-DV · WSP · D\n- **GEC** — \n- **D-DV** — \n- **WSP** — \n"), "Montag unberührt");
        // schon verlinkt → nichts
        let (t2, e2, _) = plane_schultage(&t, d(2026, 11, 9), &notizen, &b).unwrap();
        assert_eq!((t2, e2.len()), (t.clone(), 0));
        // eigener Text bleibt, neue Zeile dahinter
        let eigen = "## Berufsschule\n\n### Donnerstag, 12.11.\n- **WSP** — Eigener Text\n- Sonstiges\n\n## Betrieb\n";
        let (t3, _, _) = plane_schultage(eigen, d(2026, 11, 9), &notizen, &b).unwrap();
        assert_eq!(
            t3,
            "## Berufsschule\n\n### Donnerstag, 12.11.\n- **WSP** — Eigener Text\n- **WSP** — Kündigung → [[WSP 12.11.2026]]\n- Sonstiges\n\n## Betrieb\n"
        );
        // Tag ohne Überschrift → anlegen (chronologisch, mit Warnung); ohne Abschnitt → Fehler
        let (t4, _, w4) = plane_schultage("## Berufsschule\n\n### Montag, 09.11.\n- x\n\n## Betrieb\n", d(2026, 11, 9), &notizen, &b).unwrap();
        assert_eq!(t4, "## Berufsschule\n\n### Montag, 09.11.\n- x\n\n### Donnerstag, 12.11.\n- **WSP** — Kündigung → [[WSP 12.11.2026]]\n\n## Betrieb\n");
        assert_eq!(w4.len(), 1);
        assert!(matches!(plane_schultage("## Betrieb\n", d(2026, 11, 9), &notizen, &b), Err(AppError::InvalidInput(_))));
    }

    #[test]
    fn schultage_altformat_kw41() {
        let (dir, ctx) = standard();
        let mut w = Vec::new();
        let notizen = notiz_inhalte(dir.path(), &ctx.unterricht_der_woche(d(2026, 10, 5)), &mut w);
        let (t, e, _) = plane_schultage(KW41, d(2026, 10, 5), &notizen, &bekannte_faecher(&ctx)).unwrap();
        assert_eq!(e.len(), 2, "GEC 05.10. ist schon verlinkt: {e:?}");
        assert!(t.contains(
            "- [[GEC – Geschäftsprozesse im E-Commerce|GEC]]: Handelskalkulation → [[GEC 05.10.2026]]\n\
             - **D-DV** — *(Thema noch eintragen)* → [[D-DV 05.10.2026]]\n\
             - **GEC** — Kalkulation – Übung → [[GEC 2 05.10.2026]]\n\n## Betrieb"
        ), "{t}");
        // Rest byte-gleich
        assert_eq!(t.replace("- **D-DV** — *(Thema noch eintragen)* → [[D-DV 05.10.2026]]\n", "").replace("- **GEC** — Kalkulation – Übung → [[GEC 2 05.10.2026]]\n", ""), KW41);
        // über die Datei: Vorschau schreibt nichts
        let req = SchultageUebernehmen { path: P_KW41.into(), vorschau: true, basis: None };
        let vorher = testhilfe::bytes(dir.path(), P_KW41);
        let a = schultage(dir.path(), &ctx, &req, &SelfWrites::new(), &RetryPolicy::none()).unwrap();
        assert_eq!((a.nachher.as_str(), a.report.is_none()), (t.as_str(), true));
        assert_eq!(testhilfe::bytes(dir.path(), P_KW41), vorher);
    }
}
