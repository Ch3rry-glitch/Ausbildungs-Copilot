//! `04 Betrieb/Betrieb.md` fortschreiben: neue Überschneidungen Schule ↔ Betrieb in die vorhandene
//! Tabelle – **immer** erst als Vorschau mit Diff (Regel 9), dann mit `basis` schreiben.
//!
//! Ziel-Tabelle = erste Tabelle, deren Kopf eine Spalte mit „Lernfeld“ **und** eine mit
//! „Tätigkeit“ oder „Betrieb“ hat (echt: `| Tätigkeit im Betrieb | Lernfeld | Woche |`).

use std::collections::BTreeSet;
use std::path::Path;

use super::regeln::{self, diagnose_meldung, diagnose_wort, normalisiert};
use super::tabelle::{self, Rolle, Tabelle};
use super::woche::{self, ohne_pfeil_links, WocheRoh, Zeilenart};
use super::{Kontext, BETRIEB_MD};
use crate::aufgaben::lernfeld_label;
use crate::error::{AppError, AppResult};
use crate::model::{Aenderung, BetriebFortschreiben, BetriebVorschlag, BetriebZeile, TagAbschnitt};
use crate::write::{RetryPolicy, SelfWrites};

/// Erste passende Tabelle der Datei.
pub fn ziel_tabelle(zeilen: &[String]) -> Option<Tabelle> {
    tabelle::tabellen(zeilen, 0, zeilen.len()).into_iter().find(|t| {
        t.spalte(Rolle::Lernfeld).is_some() && t.spalte(Rolle::Taetigkeit).is_some()
    })
}

fn keine_tabelle() -> AppError {
    AppError::InvalidInput(format!(
        "In „{BETRIEB_MD}“ gibt es keine Tabelle mit den Spalten „Tätigkeit“ (oder „Betrieb“) und „Lernfeld“ – bitte in Obsidian anlegen."
    ))
}

/// Zelle „Woche“ für eine Wochennotiz: `[[2026-KW46\|KW46]]`.
pub fn woche_zelle(roh: &WocheRoh) -> String {
    format!("[[{}\\|KW{:02}]]", roh.woche.title, roh.woche.kw)
}

/// (Tätigkeit, Woche) normalisiert – Grundlage der Duplikat-Erkennung.
fn schluessel(t: &Tabelle, z: &[String]) -> (String, Option<String>) {
    let tae = t.spalte(Rolle::Taetigkeit).and_then(|i| z.get(i)).map(|s| normalisiert(s)).unwrap_or_default();
    let wo = t.spalte(Rolle::Woche).map(|i| z.get(i).map(|s| normalisiert(s)).unwrap_or_default());
    (tae, wo)
}

fn vorhandene(zeilen: &[String], t: &Tabelle) -> Vec<(String, Option<String>)> {
    t.zeilen.iter().filter_map(|i| tabelle::zellen(&zeilen[*i])).filter(|z| !tabelle::ist_leer(z)).map(|z| schluessel(t, &z)).collect()
}

/// Stämme aller Betriebs-Stichpunkte der Woche.
fn betriebs_staemme(roh: &WocheRoh) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    for t in roh.tage.iter().filter(|t| t.tag.abschnitt == TagAbschnitt::Betrieb) {
        for (i, sp) in t.tag.stichpunkte.iter().enumerate() {
            if sp.diagnose.is_none() && t.arten[i] == Zeilenart::Inhalt {
                s.extend(regeln::staemme(&sp.text));
            }
        }
    }
    s
}

/// Fach-Zeilen der Woche, deren verlinkte Unterrichtsnotiz eines der Lernfelder hat.
fn schul_zelle(roh: &WocheRoh, ctx: &Kontext, lernfelder: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    for t in roh.tage.iter().filter(|t| t.tag.abschnitt == TagAbschnitt::Berufsschule) {
        for u in &t.tag.unterricht {
            let Some(m) = ctx.unterricht.iter().find(|m| m.path == u.path) else { continue };
            let nr = m
                .fields
                .iter()
                .find(|f| f.key.eq_ignore_ascii_case("lernfeld"))
                .and_then(|f| lernfeld_label(&f.value))
                .and_then(|l| l.strip_prefix("LF").and_then(|n| n.parse::<u32>().ok()));
            if nr.is_some_and(|n| lernfelder.contains(&n)) {
                let link = format!("[[{}\\|{}]]", u.title, u.fach.clone().unwrap_or_else(|| u.title.clone()));
                if !out.contains(&link) {
                    out.push(link);
                }
            }
        }
    }
    out.join(" · ")
}

/// Vorschlag aus der Lernfeld-Tabelle der Woche (schreibt nichts).
pub fn vorschlag(root: &Path, ctx: &Kontext, woche_rel: &str) -> AppResult<BetriebVorschlag> {
    let roh = woche::lesen(root, woche_rel, ctx)?;
    let (text, _, betrieb_path) = match super::lesen(root, BETRIEB_MD) {
        Ok(x) => x,
        Err(AppError::NoteNotFound(_)) => {
            return Err(AppError::InvalidInput(format!("„{BETRIEB_MD}“ fehlt im Vault – bitte in Obsidian anlegen.")))
        }
        Err(e) => return Err(e),
    };
    let (zeilen, _) = super::zeilen_von(&text);
    let t = ziel_tabelle(&zeilen).ok_or_else(keine_tabelle)?;
    let da = vorhandene(&zeilen, &t);
    let wz = woche_zelle(&roh);
    let betrieb = betriebs_staemme(&roh);
    let mut zeilen_out = Vec::new();
    let mut schon = Vec::new();
    let mut warnungen = Vec::new();
    if roh.woche.lernfeld_tabelle.is_empty() {
        warnungen.push("Die Lernfeld-Tabelle der Woche ist leer – erst unter „Lernfelder“ zuordnen.".into());
    }
    for lz in &roh.woche.lernfeld_tabelle {
        let zellen: Vec<String> = t
            .spalten
            .iter()
            .map(|s| match tabelle::rolle(s) {
                Rolle::Taetigkeit => tabelle::zelle_escapen(&lz.taetigkeit),
                Rolle::Lernfeld => lz.zelle.clone(),
                Rolle::Woche => wz.clone(),
                Rolle::Schule => schul_zelle(&roh, ctx, &lz.lernfelder),
                Rolle::Sonst => String::new(),
            })
            .collect();
        let k = schluessel(&t, &zellen);
        if da.contains(&k) {
            schon.push(lz.taetigkeit.clone());
            continue;
        }
        let betriebstaetigkeit = regeln::gemeinsamer_stamm(&regeln::staemme(&ohne_pfeil_links(&lz.taetigkeit)), &betrieb);
        zeilen_out.push(BetriebZeile {
            zellen,
            taetigkeit: lz.taetigkeit.clone(),
            uebernehmen: betriebstaetigkeit,
            grund: if betriebstaetigkeit { "Betriebstätigkeit" } else { "nur Schule" }.into(),
        });
    }
    Ok(BetriebVorschlag { betrieb_path, spalten: t.spalten.clone(), zeilen: zeilen_out, schon_vorhanden: schon, warnungen })
}

/// Neue Zeilen nach der letzten nicht-leeren Zeile der Ziel-Tabelle (eine leere Schlusszeile bleibt
/// am Ende). Duplikate werden übersprungen.
pub fn plane_betrieb(text: &str, neue: &[Vec<String>]) -> AppResult<(String, Vec<String>)> {
    let (mut zeilen, endet) = super::zeilen_von(text);
    let t = ziel_tabelle(&zeilen).ok_or_else(keine_tabelle)?;
    let mut da = vorhandene(&zeilen, &t);
    let mut warnungen = Vec::new();
    let mut rein: Vec<String> = Vec::new();
    for (n, z) in neue.iter().enumerate() {
        if z.len() != t.spalten.len() {
            return Err(AppError::InvalidInput(format!(
                "Zeile {} hat {} Zellen, die Tabelle in „{BETRIEB_MD}“ hat {} Spalten.",
                n + 1,
                z.len(),
                t.spalten.len()
            )));
        }
        let zellen: Vec<String> = z.iter().map(|c| tabelle::zelle_escapen(c)).collect();
        if tabelle::ist_leer(&zellen) {
            continue;
        }
        if let Some(w) = zellen.iter().find_map(|c| diagnose_wort(c)) {
            return Err(AppError::InvalidInput(diagnose_meldung(&w)));
        }
        let k = schluessel(&t, &zellen);
        if da.contains(&k) {
            let tae = t.spalte(Rolle::Taetigkeit).map(|i| zellen[i].clone()).unwrap_or_default();
            warnungen.push(format!("„{tae}“ steht schon in „{BETRIEB_MD}“ – übersprungen."));
            continue;
        }
        da.push(k);
        rein.push(tabelle::zeile_bauen(&zellen));
    }
    let nach = t
        .zeilen
        .iter()
        .rev()
        .find(|i| tabelle::zellen(&zeilen[**i]).is_some_and(|z| !tabelle::ist_leer(&z)))
        .copied()
        .unwrap_or(t.kopf + 1);
    zeilen.splice(nach + 1..nach + 1, rein);
    Ok((super::zusammen(&zeilen, endet), warnungen))
}

/// Betrieb.md fortschreiben – Vorschau zuerst, dann mit `basis` (Regel 9). Nur `Betrieb.md` ändert sich.
pub fn fortschreiben(root: &Path, req: &BetriebFortschreiben, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<Aenderung> {
    super::aendern(root, BETRIEB_MD, req.vorschau, req.basis.as_ref(), writes, p, |text| plane_betrieb(text, &req.zeilen)).map_err(
        |e| match e {
            AppError::NoteNotFound(_) => AppError::InvalidInput(format!("„{BETRIEB_MD}“ fehlt im Vault – bitte in Obsidian anlegen.")),
            e => e,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{bytes, standard, version, BETRIEB, P_KW41};
    use super::*;

    /// Aufbau wie im echten Vault: erste Tabelle passt nicht, Ziel mit Woche-Spalte und leerer Schlusszeile.
    const ECHT: &str = "# Betrieb\n\n## Was jede Abteilung liefert\n\n\
        | Abteilung | Passende Lernfelder | Worum es dabei geht |\n|---|---|---|\n| **Logistik** | [[LF02 – X\\|LF02]] | Versand |\n\n\
        ## Praxis ↔ Schule\n\n| Tätigkeit im Betrieb | Lernfeld | Woche |\n| --- | --- | --- |\n\
        | Retouren annehmen | [[LF05 – Y\\|LF05]] | [[2026-KW36\\|KW36]] |\n|  |  |  |\n\n## Weiter\n";

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn echtes_format_mit_leerer_schlusszeile_und_duplikat() {
        let neu = vec![
            s(&["Pakete | Versand", "[[LF02 – X\\|LF02]]", "[[2026-KW41\\|KW41]]"]),
            s(&["retouren  annehmen", "[[LF05 – Y\\|LF05]]", "[[2026-KW36\\|KW36]]"]),
            s(&["Retouren annehmen", "[[LF05 – Y\\|LF05]]", "[[2026-KW41\\|KW41]]"]),
        ];
        let (t, w) = plane_betrieb(ECHT, &neu).unwrap();
        assert_eq!(w.len(), 1, "gleiche Tätigkeit und Woche: {w:?}");
        assert_eq!(
            t,
            ECHT.replace(
                "|  |  |  |\n",
                "| Pakete \\| Versand | [[LF02 – X\\|LF02]] | [[2026-KW41\\|KW41]] |\n| Retouren annehmen | [[LF05 – Y\\|LF05]] | [[2026-KW41\\|KW41]] |\n|  |  |  |\n"
            )
        );
        let e = plane_betrieb(ECHT, &[s(&["nur", "zwei"])]);
        assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("3 Spalten")), "{e:?}");
        assert!(plane_betrieb("# Betrieb\n| Abteilung | Passende Lernfelder |\n|---|---|\n", &[]).is_err());
    }

    #[test]
    fn fixture_format_vorschlag_ohne_woche_spalte() {
        let (dir, ctx) = standard();
        let v = vorschlag(dir.path(), &ctx, P_KW41).unwrap();
        assert_eq!(v.betrieb_path, BETRIEB_MD);
        assert_eq!(v.spalten, vec!["Schule", "Betrieb", "Lernfeld"]);
        assert_eq!(v.zeilen.len(), 1);
        let z = &v.zeilen[0];
        assert_eq!(
            z.zellen,
            s(&["[[GEC 05.10.2026\\|GEC]]", "Artikel im Shop gepflegt", "[[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]]"])
        );
        assert!(z.uebernehmen && z.grund == "Betriebstätigkeit");
        // ohne Woche-Spalte: gleiche Tätigkeit = Duplikat
        let (t, w) = plane_betrieb(BETRIEB, &[s(&["x", "Preisfindung neuer Artikel", "LF04"])]).unwrap();
        assert_eq!((t.as_str(), w.len()), (BETRIEB, 1));
        let (t, _) = plane_betrieb(BETRIEB, std::slice::from_ref(&z.zellen)).unwrap();
        assert_eq!(t, format!("{BETRIEB}{}\n", crate::berichtsheft::tabelle::zeile_bauen(&z.zellen)));
        assert_eq!(bytes(dir.path(), BETRIEB_MD), BETRIEB.as_bytes(), "Vorschlag schreibt nichts");
        let _ = version(dir.path(), BETRIEB_MD);
    }
}
