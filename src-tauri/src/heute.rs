//! „Heute“: Stundenplan des Tages, passende Unterrichtsnotizen, fällige Aufgaben.
//! Reine Berechnung – Datum und Uhrzeit werden hineingegeben (testbar, `COCKPIT_NOW`).

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime, Timelike, Weekday};

use crate::aufgaben;
use crate::index::VaultIndex;
use crate::model::{
    Block, FachInfo, FaelligeAufgabe, FaelligeAufgaben, HeuteBlock, HeuteTag, NoteMeta, NoteSummary, Stundenplan,
    StundenplanStatus, TagArt, Wochentag,
};
use crate::unterricht::{self, block_nr, fach_fuer_block, fach_matches, is_unterricht_note, UNTERRICHT_DIR};
use crate::vault::naming;

pub const W_STUNDENPLAN_FEHLT: &str = "Kein Stundenplan unter „00 Start/Stundenplan.md“ gefunden.";
pub const W_STUNDENPLAN_LEER: &str = "Stundenplan nicht lesbar – weder Tabellen unter „## Montag“ … „## Freitag“ noch eine Tabelle mit Wochentagen als Spalten gefunden.";

pub fn wochentag_name(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Montag",
        Weekday::Tue => "Dienstag",
        Weekday::Wed => "Mittwoch",
        Weekday::Thu => "Donnerstag",
        Weekday::Fri => "Freitag",
        Weekday::Sat => "Samstag",
        Weekday::Sun => "Sonntag",
    }
}

/// `08:35` oder `8:35` → Uhrzeit.
fn parse_hm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.trim().split_once(':')?;
    NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

/// Aktueller Block (halboffen `beginn <= jetzt < ende`), sonst der nächste, sonst keiner.
pub fn aktueller_block(bloecke: &[Block], now: NaiveTime) -> Option<usize> {
    let now = NaiveTime::from_hms_opt(now.hour(), now.minute(), 0)?;
    let zeiten: Vec<Option<(NaiveTime, NaiveTime)>> =
        bloecke.iter().map(|b| Some((parse_hm(&b.beginn)?, parse_hm(&b.ende)?))).collect();
    zeiten
        .iter()
        .position(|z| z.is_some_and(|(b, e)| b <= now && now < e))
        .or_else(|| zeiten.iter().position(|z| z.is_some_and(|(b, _)| b >= now)))
}

/// Offene Aufgaben mit Frist, relativ zum **angezeigten** Tag. Quelle wie die Seite „Aufgaben“
/// ([`aufgaben::zaehlt_als_quelle`]: ohne `99 Meta`, ohne `!…`-Dateien).
pub fn faellige_aufgaben(idx: &VaultIndex, datum: NaiveDate) -> FaelligeAufgaben {
    let mut out = FaelligeAufgaben::default();
    let bis = datum + Duration::days(7);
    for m in idx.notes().filter(|m| aufgaben::zaehlt_als_quelle(m)) {
        for t in m.tasks.iter().filter(|t| aufgaben::ist_offen(t)) {
            let Some(due) = t.due else { continue };
            let a = FaelligeAufgabe {
                path: m.path.clone(),
                title: m.title.clone(),
                fach: m.fach.clone(),
                line: t.line,
                text: t.text.clone(),
                due,
            };
            if due < datum {
                out.ueberfaellig.push(a);
            } else if due == datum {
                out.heute.push(a);
            } else if due <= bis {
                out.naechste_tage.push(a);
            }
        }
    }
    for list in [&mut out.ueberfaellig, &mut out.heute, &mut out.naechste_tage] {
        list.sort_by(|a, b| (a.due, &a.path, a.line).cmp(&(b.due, &b.path, b.line)));
    }
    out
}

/// `2` oder `6–7`
fn std_label(b: &Block) -> String {
    if b.von_stunde == b.bis_stunde {
        b.von_stunde.to_string()
    } else {
        format!("{}–{}", b.von_stunde, b.bis_stunde)
    }
}

/// `2 und 6–7`, `2, 5 und 6–7`
fn join_und(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} und {last}", rest.join(", ")),
    }
}

pub fn heute(
    idx: &VaultIndex,
    plan: Option<&Stundenplan>,
    status: StundenplanStatus,
    datum: NaiveDate,
    jetzt: NaiveDateTime,
) -> HeuteTag {
    let weekday = datum.weekday();
    let mut warnungen = Vec::new();
    match status {
        StundenplanStatus::Fehlt => warnungen.push(W_STUNDENPLAN_FEHLT.to_string()),
        StundenplanStatus::Leer => warnungen.push(W_STUNDENPLAN_LEER.to_string()),
        StundenplanStatus::Ok => {}
    }
    let schultag = Wochentag::from_chrono(weekday)
        .and_then(|w| plan.and_then(|p| p.tag(w)))
        .filter(|t| !t.stunden.is_empty());
    let art = if matches!(weekday, Weekday::Sat | Weekday::Sun) {
        TagArt::Wochenende
    } else if schultag.is_some() {
        TagArt::Schultag
    } else {
        TagArt::Betriebstag
    };
    let ist_heute = datum == jetzt.date();

    // Kandidaten: alle Unterrichtsnotizen dieses Tages, in jedem Ordner
    let kandidaten: Vec<&NoteMeta> =
        idx.notes().filter(|m| is_unterricht_note(m) && m.datum == Some(datum)).collect();
    let mut vergeben = vec![false; kandidaten.len()];

    let raw: Vec<Block> = match (art, schultag) {
        (TagArt::Schultag, Some(t)) => t.bloecke(),
        _ => Vec::new(),
    };
    let registry = unterricht::faecher(idx, plan);
    let mut bloecke: Vec<HeuteBlock> = raw
        .iter()
        .enumerate()
        .map(|(position, b)| {
            let gleich = |x: &&Block| x.fach.eq_ignore_ascii_case(&b.fach);
            let fach_nr = 1 + raw[..position].iter().filter(gleich).count() as u8;
            let fach_anzahl = raw.iter().filter(gleich).count() as u8;
            let reg = registry.iter().find(|f| f.kurz.eq_ignore_ascii_case(&b.fach)).cloned().unwrap_or_else(|| {
                FachInfo {
                    kurz: b.fach.clone(),
                    ziel: b.fach.clone(),
                    link: format!("[[{}]]", b.fach),
                    lehrkraft: None,
                    ziel_existiert: false,
                    notiz_pfad: None,
                    im_stundenplan: true,
                }
            });
            let fach = fach_fuer_block(idx, &reg, b);
            let neuer_pfad = format!("{UNTERRICHT_DIR}/{}", naming::unterricht_filename(&fach.kurz, fach_nr, datum));
            HeuteBlock {
                position,
                block: b.clone(),
                fach_nr,
                fach_anzahl,
                fach,
                notiz: None,
                erste_notiz: None,
                neuer_pfad,
                hinweis: None,
            }
        })
        .collect();

    // Notizen den Blöcken zuordnen: Block-Nr. aus dem Dateinamen → n-ter Block des Fachs
    let mut kuerzel: Vec<String> = Vec::new();
    for b in &bloecke {
        if !kuerzel.iter().any(|k| k.eq_ignore_ascii_case(&b.fach.kurz)) {
            kuerzel.push(b.fach.kurz.clone());
        }
    }
    for kurz in &kuerzel {
        let positionen: Vec<usize> =
            bloecke.iter().filter(|b| b.fach.kurz.eq_ignore_ascii_case(kurz)).map(|b| b.position).collect();
        let mut ziele: Vec<String> = positionen.iter().map(|p| bloecke[*p].fach.ziel.clone()).collect();
        if let Some(r) = registry.iter().find(|f| f.kurz.eq_ignore_ascii_case(kurz)) {
            ziele.push(r.ziel.clone());
        }
        let mut passend: Vec<usize> = (0..kandidaten.len())
            .filter(|i| !vergeben[*i] && ziele.iter().any(|z| fach_matches(kandidaten[*i], kurz, z)))
            .collect();
        // Notizen mit echtem Unterrichts-Namen zuerst: Sync-/Konfliktkopien wie „GEC 05.10.2026 (1)“
        // oder „GEC 05.10.2026-LAPTOP“ belegen nie den Platz des Originals (→ „Weitere Notizen“)
        let rang = |i: usize| {
            let m = kandidaten[i];
            (block_nr(m), naming::parse_unterricht(&m.title).is_none(), &m.path)
        };
        passend.sort_by(|a, b| rang(*a).cmp(&rang(*b)));
        for i in passend {
            let n = block_nr(kandidaten[i]);
            if let Some(p) = positionen.iter().find(|p| bloecke[**p].fach_nr == n && bloecke[**p].notiz.is_none()) {
                bloecke[*p].notiz = Some(NoteSummary::from(kandidaten[i]));
                vergeben[i] = true;
            }
        }
    }

    // erste Notiz und Hinweis bei zwei (oder mehr) Blöcken
    for i in 0..bloecke.len() {
        let b = &bloecke[i];
        let erste = (b.fach_nr > 1)
            .then(|| bloecke.iter().find(|x| x.fach.kurz == b.fach.kurz && x.fach_nr == 1).and_then(|x| x.notiz.clone()))
            .flatten();
        let hinweis = if b.fach_anzahl <= 1 || b.notiz.is_some() {
            None
        } else if b.fach_nr == 1 {
            let labels: Vec<String> =
                bloecke.iter().filter(|x| x.fach.kurz == b.fach.kurz).map(|x| std_label(&x.block)).collect();
            Some(format!(
                "{} hat heute {} Blöcke (Std. {}) – eine gemeinsame Notiz oder eine je Block? Die zweite kannst du später im 2. Block anlegen.",
                b.fach.kurz,
                b.fach_anzahl,
                join_und(&labels)
            ))
        } else if erste.is_some() {
            Some(format!(
                "In der Notiz des 1. Blocks weiterschreiben oder eine eigene Notiz für Std. {} anlegen.",
                std_label(&b.block)
            ))
        } else {
            let name = b.neuer_pfad.rsplit('/').next().unwrap_or(&b.neuer_pfad).trim_end_matches(".md");
            Some(format!("Zweiter {}-Block heute – legst du hier an, entsteht „{name}“.", b.fach.kurz))
        };
        bloecke[i].erste_notiz = erste;
        bloecke[i].hinweis = hinweis;
    }

    let mut weitere: Vec<NoteSummary> = kandidaten
        .iter()
        .zip(&vergeben)
        .filter(|(_, v)| !**v)
        .map(|(m, _)| NoteSummary::from(*m))
        .collect();
    weitere.sort_by(|a, b| a.path.cmp(&b.path));

    let aktueller = (ist_heute && art == TagArt::Schultag).then(|| aktueller_block(&raw, jetzt.time())).flatten();

    HeuteTag {
        datum,
        wochentag: wochentag_name(weekday).to_string(),
        art,
        ist_heute,
        stundenplan: status,
        bloecke,
        weitere_notizen: weitere,
        aktueller_block: aktueller,
        aufgaben: faellige_aufgaben(idx, datum),
        warnungen,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{NoteKind, NoteMeta, Task};
    use crate::vault::stundenplan;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    fn t(s: &str) -> NaiveTime {
        NaiveTime::parse_from_str(s, "%H:%M").unwrap()
    }

    #[test]
    fn aktueller_block_montag() {
        let plan = stundenplan::parse_stundenplan(include_str!("../../fixtures/vault/00 Start/Stundenplan.md"));
        let mo = plan.tag(Wochentag::Montag).unwrap().bloecke();
        for (zeit, erwartet) in [
            ("08:00", Some(0)),
            ("08:35", Some(0)),
            ("09:00", Some(0)),
            ("09:20", Some(1)),
            ("09:25", Some(1)),
            ("12:05", Some(3)),
            ("12:55", Some(3)),
            ("13:50", Some(4)),
            ("14:35", None),
            ("14:40", None),
        ] {
            assert_eq!(aktueller_block(&mo, t(zeit)), erwartet, "{zeit}");
        }
        // Sekunden zählen nicht; H:MM wird verstanden; kaputte Zeiten werden übersprungen
        assert_eq!(aktueller_block(&mo, NaiveTime::from_hms_opt(9, 19, 59).unwrap()), Some(0));
        let mut b = mo.clone();
        b[0].beginn = "8:35".into();
        b[1].beginn = "kaputt".into();
        assert_eq!(aktueller_block(&b, t("09:25")), Some(2));
        assert_eq!(aktueller_block(&b, t("08:40")), Some(0));
    }

    fn note_mit_aufgaben(path: &str, kind: NoteKind, tasks: Vec<(usize, char, Option<NaiveDate>)>) -> NoteMeta {
        NoteMeta {
            path: path.into(),
            folder: crate::vault::scan::parent_folder(path),
            title: path.rsplit('/').next().unwrap().trim_end_matches(".md").into(),
            kind,
            fields: vec![],
            tags: vec![],
            inline_tags: vec![],
            tasks: tasks
                .into_iter()
                .map(|(line, c, due)| Task {
                    line,
                    text: format!("Aufgabe {line}"),
                    done: matches!(c, 'x' | 'X'),
                    status_char: c,
                    due,
                    indent: 0,
                })
                .collect(),
            links: vec![],
            datum: None,
            fach: Some("GEC".into()),
            modified_ms: 0,
            size: 0,
            frontmatter_error: None,
            private: path.starts_with("10 Privat/"),
        }
    }

    #[test]
    fn faellige_aufgaben_eimer() {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = VaultIndex::build(dir.path()).unwrap();
        let heute = d(2026, 11, 16);
        idx.insert_meta(note_mit_aufgaben(
            "03 Unterricht/GEC 16.11.2026.md",
            NoteKind::Unterricht,
            vec![
                (10, ' ', Some(d(2026, 11, 15))),
                (11, ' ', Some(heute)),
                (12, ' ', Some(d(2026, 11, 23))), // Tag + 7: dabei
                (13, ' ', Some(d(2026, 11, 24))), // Tag + 8: nicht
                (14, 'x', Some(heute)),
                (15, '-', Some(heute)),
                (16, ' ', None),
                (17, '/', Some(d(2026, 11, 17))),
                (9, ' ', Some(heute)),
            ],
        ));
        idx.insert_meta(note_mit_aufgaben(
            "99 Meta/Templates/Unterrichtsnotiz.md",
            NoteKind::Template,
            vec![(5, ' ', Some(heute))],
        ));
        idx.insert_meta(note_mit_aufgaben("10 Privat/x.md", NoteKind::Sonstige, vec![(3, ' ', Some(d(2026, 1, 1)))]));
        // gleicher Quellfilter wie `00 Start/Aufgaben.md`: kein `!`-Entwurf, nichts unter „99 Meta“
        idx.insert_meta(note_mit_aufgaben("00 Start/!Entwurf.md", NoteKind::Sonstige, vec![(2, ' ', Some(heute))]));
        idx.insert_meta(note_mit_aufgaben("05 Lernen/99 Meta alt/y.md", NoteKind::Sonstige, vec![(2, ' ', Some(heute))]));
        let a = faellige_aufgaben(&idx, heute);
        let lines = |v: &[FaelligeAufgabe]| v.iter().map(|x| (x.path.clone(), x.line)).collect::<Vec<_>>();
        assert_eq!(
            lines(&a.ueberfaellig),
            vec![("10 Privat/x.md".to_string(), 3), ("03 Unterricht/GEC 16.11.2026.md".to_string(), 10)]
        );
        assert_eq!(
            lines(&a.heute),
            vec![("03 Unterricht/GEC 16.11.2026.md".to_string(), 9), ("03 Unterricht/GEC 16.11.2026.md".to_string(), 11)]
        );
        assert_eq!(a.naechste_tage.iter().map(|x| x.line).collect::<Vec<_>>(), vec![17, 12]);
        assert_eq!(a.heute[0].fach.as_deref(), Some("GEC"));
    }

    #[test]
    fn hinweistexte() {
        assert_eq!(join_und(&["2".into(), "5".into(), "6–7".into()]), "2, 5 und 6–7");
        assert_eq!(join_und(&["2".into(), "6–7".into()]), "2 und 6–7");
        assert_eq!(wochentag_name(Weekday::Sun), "Sonntag");
    }
}
