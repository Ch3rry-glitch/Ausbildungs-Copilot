//! Einen Tag speichern: die Zeilen zwischen Tages-Überschrift und nächster Überschrift ersetzen.
//!
//! **Regel 3:** „Arbeitsunfähig erkrankt“ ist nur eine Tagesart – mit Stichpunkten (Freitext) wird
//! sie abgelehnt, und kein Stichpunkt darf ein Diagnose-Wort enthalten.

use std::path::Path;

use chrono::{NaiveDate, Weekday};

use super::regeln::{diagnose_meldung, diagnose_wort};
use super::woche::{self, abschnitt_titel, listenzeile, WocheRoh, HINWEIS_NICHT_BEARBEITBAR};
use super::{tag_der_woche, wochentag_am_anfang, wochentag_name, Gliederung, Kontext};
use crate::error::{AppError, AppResult};
use crate::model::{Aenderung, BerichtsTagArt, NoteVersion, TagAbschnitt, TagSpeichern};
use crate::write::{RetryPolicy, SelfWrites};

pub const ZEILE_KRANK_BETRIEB: &str = "- **Arbeitsunfähig erkrankt** — ganztägig";
pub const ZEILE_KRANK_SCHULE: &str = "- **Arbeitsunfähig erkrankt** — ganztägig, kein Berufsschulbesuch";

/// `### Dienstag, 10.11.`
pub fn tag_kopf(d: NaiveDate, w: Weekday) -> String {
    format!("### {}, {}", wochentag_name(w), d.format("%d.%m."))
}

/// Legt `### <Wochentag>, TT.MM.` mit `inhalt` an der chronologischen Stelle im Abschnitt
/// `bereich` (Index der `##`-Überschrift, Ende exklusiv) an. Liefert den Index der neuen Überschrift.
pub(crate) fn tag_einfuegen(
    zeilen: &mut Vec<String>,
    gl: &Gliederung,
    bereich: (usize, usize),
    montag: NaiveDate,
    w: Weekday,
    inhalt: Vec<String>,
) -> usize {
    let (start, ende) = bereich;
    let kopf = tag_kopf(tag_der_woche(montag, w), w);
    let spaeter = gl
        .koepfe
        .iter()
        .filter(|k| k.idx > start && k.idx < ende && k.ebene >= 3)
        .find(|k| wochentag_am_anfang(&k.text).is_some_and(|x| x.num_days_from_monday() > w.num_days_from_monday()));
    match spaeter {
        Some(k) => {
            let pos = k.idx;
            let mut neu = vec![kopf];
            neu.extend(inhalt);
            neu.push(String::new());
            zeilen.splice(pos..pos, neu);
            pos
        }
        None => {
            let letzte = (start..ende).rev().find(|i| !zeilen[*i].trim().is_empty()).unwrap_or(start);
            let mut neu = vec![String::new(), kopf];
            neu.extend(inhalt);
            if zeilen.get(letzte + 1).is_some_and(|z| !z.trim().is_empty()) {
                neu.push(String::new());
            }
            zeilen.splice(letzte + 1..letzte + 1, neu);
            letzte + 2
        }
    }
}

/// Neue Zeilen eines Tages nach Tagesart. Sondertag mit Stichpunkten → Fehler (Regel 3).
pub fn neue_zeilen(abschnitt: TagAbschnitt, art: BerichtsTagArt, stichpunkte: &[String]) -> AppResult<Vec<String>> {
    let eintraege: Vec<String> = stichpunkte
        .iter()
        .map(|s| {
            let eine = s.replace(['\r', '\n'], " ");
            let t = eine.trim();
            t.strip_prefix("- ").unwrap_or(t).trim().to_string()
        })
        .filter(|s| !s.is_empty() && s != "-")
        .collect();
    if let Some(w) = eintraege.iter().find_map(|e| diagnose_wort(e)) {
        return Err(AppError::InvalidInput(diagnose_meldung(&w)));
    }
    let sondertag = |zeile: &str| -> AppResult<Vec<String>> {
        if eintraege.is_empty() {
            Ok(vec![zeile.to_string()])
        } else {
            Err(AppError::InvalidInput(
                "Bei einem Sondertag (arbeitsunfähig erkrankt, Urlaub, Feiertag, entfallen) gibt es keine Stichpunkte – nur die Tagesart (Regel 3)."
                    .into(),
            ))
        }
    };
    match art {
        BerichtsTagArt::Stichpunkte if eintraege.is_empty() => Ok(vec!["- ".into()]),
        BerichtsTagArt::Stichpunkte => Ok(eintraege.into_iter().map(|e| format!("- {e}")).collect()),
        BerichtsTagArt::Krank => sondertag(match abschnitt {
            TagAbschnitt::Berufsschule => ZEILE_KRANK_SCHULE,
            TagAbschnitt::Betrieb => ZEILE_KRANK_BETRIEB,
        }),
        BerichtsTagArt::Urlaub => sondertag("- **Urlaub**"),
        BerichtsTagArt::Feiertag => sondertag("- **Feiertag**"),
        BerichtsTagArt::Entfallen => sondertag("- Entfallen"),
    }
}

/// Neuer Text mit den Zeilen des Tages. Unveränderte Stichpunkte behalten ihre Originalzeile.
pub fn plane_tag(text: &str, roh: &WocheRoh, req: &TagSpeichern) -> AppResult<(String, Vec<String>)> {
    let w = &roh.woche;
    if req.datum < w.montag || req.datum > w.sonntag {
        return Err(AppError::InvalidInput(format!("{} gehört nicht zu KW {}.", req.datum.format("%d.%m.%Y"), w.kw)));
    }
    let mut neu = neue_zeilen(req.abschnitt, req.art, &req.stichpunkte)?;
    let (mut zeilen, endet) = super::zeilen_von(text);
    let gl = Gliederung::von(text, &zeilen);
    let titel = abschnitt_titel(req.abschnitt);
    let bereich = gl.abschnitt(titel, zeilen.len()).ok_or_else(|| {
        AppError::InvalidInput(format!("Die Woche hat keinen Abschnitt „## {titel}“ – bitte in Obsidian anlegen."))
    })?;
    let mut warnungen = Vec::new();
    match roh.tag(req.abschnitt, req.datum).and_then(|t| t.bereich.map(|b| (t, b))) {
        Some((t, (kopf, ende))) => {
            if !t.tag.bearbeitbar {
                return Err(AppError::InvalidInput(format!("„{}“ {HINWEIS_NICHT_BEARBEITBAR}.", zeilen[kopf].trim())));
            }
            let belegt: Vec<usize> = (kopf + 1..ende).filter(|i| !zeilen[*i].trim().is_empty()).collect();
            // gleiche Stichpunkte → gleiche Zeile (Leerzeichen am Ende bleiben)
            let original: Vec<(String, String)> =
                belegt.iter().filter_map(|i| listenzeile(&zeilen[*i]).map(|(t, _)| (t, zeilen[*i].clone()))).collect();
            for z in &mut neu {
                if let Some((_, o)) = z.strip_prefix("- ").and_then(|t| original.iter().find(|(ot, _)| ot == t.trim())) {
                    *z = o.clone();
                }
            }
            match (belegt.first(), belegt.last()) {
                (Some(&f), Some(&l)) => {
                    zeilen.splice(f..=l, neu);
                }
                _ => {
                    if ende == kopf + 1 && ende < zeilen.len() {
                        neu.push(String::new());
                    }
                    zeilen.splice(kopf + 1..kopf + 1, neu);
                }
            }
        }
        None => {
            let wt = chrono::Datelike::weekday(&req.datum);
            tag_einfuegen(&mut zeilen, &gl, bereich, w.montag, wt, neu);
            warnungen.push(format!("Überschrift „{}“ unter „## {titel}“ angelegt.", tag_kopf(req.datum, wt)));
        }
    }
    Ok((super::zusammen(&zeilen, endet), warnungen))
}

/// Tag speichern – reine Nutzereingabe, kein eigener Vorschau-Schritt, aber `basis` ist Pflicht.
pub fn speichern(root: &Path, ctx: &Kontext, req: &TagSpeichern, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<Aenderung> {
    // Regel 3 vor jedem Lesen: Diagnose-Wort oder Sondertag mit Text → nichts geschrieben
    neue_zeilen(req.abschnitt, req.art, &req.stichpunkte)?;
    super::aendern(root, &req.path, false, Some(&req.basis), writes, p, |text| {
        let leer = NoteVersion { sha256: String::new(), modified_ms: 0, size: 0 };
        let roh = woche::parsen(&req.path, text, leer, ctx)?;
        plane_tag(text, &roh, req)
    })
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{self, bytes, version, KW41, KW42, P_KW41, P_KW42};
    use super::*;
    use crate::model::TagAbschnitt::{Berufsschule, Betrieb};

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    fn req(path: &str, a: TagAbschnitt, datum: NaiveDate, art: BerichtsTagArt, sp: &[&str], basis: NoteVersion) -> TagSpeichern {
        TagSpeichern { path: path.into(), abschnitt: a, datum, art, stichpunkte: sp.iter().map(|s| s.to_string()).collect(), basis }
    }

    fn speichern_t(root: &Path, ctx: &Kontext, r: &TagSpeichern) -> AppResult<Aenderung> {
        speichern(root, ctx, r, &SelfWrites::new(), &RetryPolicy::none())
    }

    #[test]
    fn nur_die_tageszeilen_aendern_sich_crlf_und_bom_bleiben() {
        let crlf = format!("\u{FEFF}{}", KW41.replace('\n', "\r\n"));
        let (dir, ctx) = testhilfe::vault(&[(P_KW41, &crlf)]);
        let root = dir.path();
        let r = req(P_KW41, Betrieb, d(2026, 10, 6), BerichtsTagArt::Stichpunkte, &["Artikel im Shop gepflegt", " Produktfotos\nfür 12 Artikel bearbeitet "], version(root, P_KW41));
        let a = speichern_t(root, &ctx, &r).unwrap();
        assert!(a.report.is_some() && !a.unveraendert);
        let erwartet = crlf.replace("- Einschulung\r\n", "- Produktfotos für 12 Artikel bearbeitet\r\n");
        assert_eq!(bytes(root, P_KW41), erwartet.as_bytes());
        assert_eq!(a.nachher, erwartet.trim_start_matches('\u{FEFF}').replace("\r\n", "\n"));
        // gleiche Stichpunkte noch einmal: nichts geschrieben
        let r = req(P_KW41, Betrieb, d(2026, 10, 6), BerichtsTagArt::Stichpunkte, &["Artikel im Shop gepflegt", "Produktfotos für 12 Artikel bearbeitet"], version(root, P_KW41));
        let a = speichern_t(root, &ctx, &r).unwrap();
        assert!(a.unveraendert && a.report.is_none());
    }

    #[test]
    fn unveraenderte_platzhalter_bleiben_byte_gleich() {
        let (dir, ctx) = testhilfe::vault(&[(P_KW42, KW42)]);
        let root = dir.path();
        // die Oberfläche füllt das Textfeld mit allen Stichpunkten (auch Übersicht und Platzhalter)
        let sp = ["GEC · D-DV · WSP · D", "**GEC** — *(noch einzutragen)*", "**D-DV** —"];
        let r = req(P_KW42, Berufsschule, d(2026, 10, 12), BerichtsTagArt::Stichpunkte, &sp, version(root, P_KW42));
        let a = speichern_t(root, &ctx, &r).unwrap();
        assert!(a.unveraendert, "`- **D-DV** — ` behält sein Leerzeichen");
        assert_eq!(bytes(root, P_KW42), KW42.as_bytes());
    }

    #[test]
    fn krank_legt_fehlenden_tag_chronologisch_an() {
        let (dir, ctx) = testhilfe::vault(&[(P_KW41, KW41)]);
        let root = dir.path();
        let r = req(P_KW41, Betrieb, d(2026, 10, 7), BerichtsTagArt::Krank, &[], version(root, P_KW41));
        let a = speichern_t(root, &ctx, &r).unwrap();
        let erwartet = KW41.replace(
            "- Einschulung\n\n## Offen",
            "- Einschulung\n\n### Mittwoch, 07.10.\n- **Arbeitsunfähig erkrankt** — ganztägig\n\n## Offen",
        );
        assert_eq!(String::from_utf8(bytes(root, P_KW41)).unwrap(), erwartet);
        assert_eq!(a.warnungen.len(), 1);
        // Berufsschule: „kein Berufsschulbesuch“
        let r = req(P_KW41, Berufsschule, d(2026, 10, 5), BerichtsTagArt::Krank, &[], version(root, P_KW41));
        speichern_t(root, &ctx, &r).unwrap();
        assert!(String::from_utf8(bytes(root, P_KW41)).unwrap().contains("### Montag, 05.10.2026\n- **Arbeitsunfähig erkrankt** — ganztägig, kein Berufsschulbesuch\n\n## Betrieb"));
    }

    #[test]
    fn regel3_stichpunkt_grippe_speichern_blockiert() {
        let (dir, ctx) = testhilfe::vault(&[(P_KW41, KW41)]);
        let root = dir.path();
        for text in ["Grippe", "beim Arzt wegen Migräne", "Magen-Darm-Infekt"] {
            let r = req(P_KW41, Betrieb, d(2026, 10, 6), BerichtsTagArt::Stichpunkte, &[text], version(root, P_KW41));
            let e = speichern_t(root, &ctx, &r);
            assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("ist eine Gesundheitsangabe") && m.contains("arbeitsunfähig erkrankt")), "{e:?}");
            assert_eq!(bytes(root, P_KW41), KW41.as_bytes(), "Datei byte-gleich");
        }
    }

    #[test]
    fn regel3_sondertag_mit_text_blockiert() {
        let (dir, ctx) = testhilfe::vault(&[(P_KW41, KW41)]);
        let root = dir.path();
        for art in [BerichtsTagArt::Krank, BerichtsTagArt::Urlaub, BerichtsTagArt::Feiertag, BerichtsTagArt::Entfallen] {
            let r = req(P_KW41, Betrieb, d(2026, 10, 6), art, &["war zu Hause"], version(root, P_KW41));
            let e = speichern_t(root, &ctx, &r);
            assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("Regel 3")), "{e:?}");
            assert_eq!(bytes(root, P_KW41), KW41.as_bytes());
        }
        // leere Einträge sind kein Text
        assert_eq!(neue_zeilen(Betrieb, BerichtsTagArt::Urlaub, &["  ".into()]).unwrap(), vec!["- **Urlaub**".to_string()]);
        assert_eq!(neue_zeilen(Betrieb, BerichtsTagArt::Entfallen, &[]).unwrap(), vec!["- Entfallen".to_string()]);
        assert_eq!(neue_zeilen(Betrieb, BerichtsTagArt::Stichpunkte, &[]).unwrap(), vec!["- ".to_string()]);
    }

    #[test]
    fn veraltete_basis_und_nicht_bearbeitbar() {
        let (dir, ctx) = testhilfe::vault(&[(P_KW41, KW41)]);
        let root = dir.path();
        let alt = version(root, P_KW41);
        std::fs::write(root.join(P_KW41), format!("{KW41}\nin Obsidian ergänzt\n")).unwrap();
        let r = req(P_KW41, Betrieb, d(2026, 10, 6), BerichtsTagArt::Stichpunkte, &["Neu"], alt);
        let e = speichern_t(root, &ctx, &r);
        assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("inzwischen geändert")), "{e:?}");
        let verschachtelt = KW41.replace("- Einschulung\n", "- Einschulung\n\t- Unterpunkt\n");
        std::fs::write(root.join(P_KW41), &verschachtelt).unwrap();
        let r = req(P_KW41, Betrieb, d(2026, 10, 6), BerichtsTagArt::Stichpunkte, &["Neu"], version(root, P_KW41));
        let e = speichern_t(root, &ctx, &r);
        assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("bitte in Obsidian bearbeiten")), "{e:?}");
        assert_eq!(bytes(root, P_KW41), verschachtelt.as_bytes());
    }
}
