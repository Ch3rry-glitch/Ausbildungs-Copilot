//! Lernfeldbezug je Tätigkeit: Vorschlag (ohne KI aus `Betrieb.md` und den Unterrichtsnotizen, mit
//! KI auf Klick) und Schreiben in die Tabelle „Bezug zu Lernfeldern“ + Frontmatter `lernfelder`.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;

use super::betrieb::ziel_tabelle;
use super::regeln::{self, diagnose_meldung, diagnose_wort, normalisiert};
use super::tabelle::{self, Rolle};
use super::woche::{self, ohne_pfeil_links, WocheRoh, Zeilenart};
use super::{lernfeld_nummer, tag_kurz, Gliederung, Kontext, BETRIEB_MD};
use crate::aufgaben::lernfeld_label;
use crate::error::{AppError, AppResult};
use crate::guard;
use crate::ki;
use crate::logbuch::KiZugang;
use crate::model::{
    Aenderung, LernfeldAnfrage, LernfeldEntwurf, LernfeldVorschlag, LernfelderSchreiben, NeueLernfeldZeile, TagAbschnitt,
};
use crate::vault::frontmatter::NoteText;
use crate::write::{RetryPolicy, SelfWrites};

pub const GRUND_BETRIEB: &str = "Betrieb.md";
pub const GRUND_NOTIZ: &str = "Lernfeld der Unterrichtsnotiz";
pub const GRUND_KI: &str = "KI";
pub const ABSCHNITT_LERNFELDER: &str = "Bezug zu Lernfeldern";

/// (Stämme der Tätigkeit, Lernfelder) je Zeile der Ziel-Tabelle in `Betrieb.md`.
fn betrieb_zeilen(root: &Path) -> Vec<(BTreeSet<String>, Vec<u32>)> {
    let Ok((text, _, _)) = super::lesen(root, BETRIEB_MD) else { return vec![] };
    let (zeilen, _) = super::zeilen_von(&text);
    let Some(t) = ziel_tabelle(&zeilen) else { return vec![] };
    let (Some(st), Some(sl)) = (t.spalte(Rolle::Taetigkeit), t.spalte(Rolle::Lernfeld)) else { return vec![] };
    t.zeilen
        .iter()
        .filter_map(|i| tabelle::zellen(&zeilen[*i]))
        .filter(|z| !tabelle::ist_leer(z))
        .map(|z| {
            let tae = z.get(st).map(|s| regeln::staemme(s)).unwrap_or_default();
            let lf = z.get(sl).map(|s| tabelle::lernfeld_nummern(s)).unwrap_or_default();
            (tae, lf)
        })
        .collect()
}

fn notiz_lernfeld(ctx: &Kontext, titel: &str) -> Option<u32> {
    let m = ctx.unterricht.iter().find(|m| m.title.eq_ignore_ascii_case(titel))?;
    let f = m.fields.iter().find(|f| f.key.eq_ignore_ascii_case("lernfeld"))?;
    lernfeld_label(&f.value)?.strip_prefix("LF")?.parse().ok()
}

/// Tätigkeiten der Woche: Stichpunkte der Betriebstage (ohne Lücken, Sondertage, Gesundheitsangaben)
/// und Fach-Zeilen der Schultage (Text ohne Link).
pub fn vorschlaege_ohne_ki(roh: &WocheRoh, ctx: &Kontext, betrieb: &[(BTreeSet<String>, Vec<u32>)]) -> Vec<LernfeldVorschlag> {
    let tabelle_norm: Vec<String> = roh.woche.lernfeld_tabelle.iter().map(|z| normalisiert(&z.taetigkeit)).collect();
    let mut out: Vec<LernfeldVorschlag> = Vec::new();
    for t in roh.tage.iter().filter(|t| !t.tag.fehlt && t.tag.art == crate::model::BerichtsTagArt::Stichpunkte) {
        for (i, s) in t.tag.stichpunkte.iter().enumerate() {
            if s.diagnose.is_some() || s.luecke {
                continue;
            }
            let (taetigkeit, quelle, lernfelder, grund) = match (&t.arten[i], t.tag.abschnitt) {
                (Zeilenart::Inhalt, TagAbschnitt::Betrieb) => {
                    let st = regeln::staemme(&s.text);
                    let best = betrieb.iter().map(|(z, lf)| (regeln::treffer(&st, z), lf)).filter(|(n, _)| *n > 0).max_by_key(|(n, _)| *n);
                    match best {
                        Some((_, lf)) => (s.text.clone(), tag_kurz(t.tag.datum), lf.clone(), GRUND_BETRIEB),
                        None => (s.text.clone(), tag_kurz(t.tag.datum), vec![], ""),
                    }
                }
                (Zeilenart::Fach { inhalt: Some(inhalt), .. }, TagAbschnitt::Berufsschule) => {
                    let text = ohne_pfeil_links(inhalt);
                    if text.is_empty() {
                        continue;
                    }
                    let notiz = s.links.iter().find(|l| t.tag.unterricht.iter().any(|u| u.title.eq_ignore_ascii_case(l)));
                    match notiz {
                        Some(n) => {
                            let lf: Vec<u32> = notiz_lernfeld(ctx, n).into_iter().collect();
                            let grund = if lf.is_empty() { "" } else { GRUND_NOTIZ };
                            (text, n.clone(), lf, grund)
                        }
                        None => (text, tag_kurz(t.tag.datum), vec![], ""),
                    }
                }
                _ => continue,
            };
            let norm = normalisiert(&taetigkeit);
            if norm.is_empty() || out.iter().any(|o| normalisiert(&o.taetigkeit) == norm) {
                continue;
            }
            out.push(LernfeldVorschlag {
                schon_in_tabelle: tabelle_norm.contains(&norm),
                taetigkeit,
                abschnitt: t.tag.abschnitt,
                quelle,
                lernfelder,
                grund: grund.into(),
            });
        }
    }
    out
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ZuordnungRoh {
    zuordnungen: Vec<Zuordnung>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Zuordnung {
    nr: i64,
    lernfelder: Vec<i64>,
}

/// Prüft die KI-Antwort: `nr` gültig und einmalig, Nummern vorhanden, höchstens drei je Tätigkeit.
pub fn pruefe_antwort(text: &str, anzahl: usize, bekannte: &[u32]) -> Result<Vec<(usize, Vec<u32>)>, String> {
    let roh: ZuordnungRoh =
        serde_json::from_str(text.trim()).map_err(|e| format!("Die Antwort ist kein gültiges JSON nach dem Schema ({e})."))?;
    let mut out: Vec<(usize, Vec<u32>)> = Vec::new();
    for z in roh.zuordnungen {
        let nr = usize::try_from(z.nr).ok().filter(|n| (1..=anzahl).contains(n)).ok_or_else(|| format!("Ungültige Nummer {}.", z.nr))?;
        if out.iter().any(|(n, _)| *n == nr) {
            return Err(format!("Tätigkeit {nr} ist doppelt zugeordnet."));
        }
        if z.lernfelder.len() > 3 {
            return Err(format!("Tätigkeit {nr}: höchstens drei Lernfelder."));
        }
        let mut lf = Vec::new();
        for n in z.lernfelder {
            let n = u32::try_from(n).ok().filter(|n| bekannte.contains(n)).ok_or_else(|| format!("Lernfeld {n} gibt es nicht."))?;
            if !lf.contains(&n) {
                lf.push(n);
            }
        }
        out.push((nr, lf));
    }
    Ok(out)
}

/// Lernfeld-Vorschläge der Woche (ohne KI; mit KI nur auf Klick). Schreibt nichts.
pub fn entwurf(root: &Path, ctx: &Kontext, req: &LernfeldAnfrage, ki: Option<KiZugang>) -> AppResult<LernfeldEntwurf> {
    let roh = woche::lesen(root, &req.path, ctx)?;
    let betrieb = betrieb_zeilen(root);
    let mut warnungen = Vec::new();
    let mut vorschlaege = vorschlaege_ohne_ki(&roh, ctx, &betrieb);
    let bekannte: Vec<u32> = ctx.lernfelder.iter().filter_map(lernfeld_nummer).collect();
    for v in &mut vorschlaege {
        let unbekannt: Vec<u32> = v.lernfelder.iter().copied().filter(|n| !bekannte.contains(n)).collect();
        if !unbekannt.is_empty() {
            v.lernfelder.retain(|n| bekannte.contains(n));
            for n in unbekannt {
                let w = format!("LF{n:02} fehlt in „02 Lernfelder“ – nicht vorgeschlagen.");
                if !warnungen.contains(&w) {
                    warnungen.push(w);
                }
            }
            if v.lernfelder.is_empty() {
                v.grund.clear();
            }
        }
    }
    let mut aufruf = None;
    if let Some(z) = ki {
        // Regel 6: Tätigkeiten mit EK gehen nicht mit
        let sendbar: Vec<usize> = (0..vorschlaege.len()).filter(|i| !guard::contains_purchase_price(&vorschlaege[*i].taetigkeit)).collect();
        if sendbar.len() < vorschlaege.len() {
            warnungen.push("Tätigkeiten mit EK-Angabe gehen nicht an die KI (Regel 6).".into());
        }
        if sendbar.is_empty() || bekannte.is_empty() {
            warnungen.push("Keine Tätigkeit oder kein Lernfeld – nichts an die KI gesendet.".into());
        } else {
            let mut nutzer = String::from("Tätigkeiten:\n");
            for (k, i) in sendbar.iter().enumerate() {
                let v = &vorschlaege[*i];
                let wo = match v.abschnitt {
                    TagAbschnitt::Betrieb => "Betrieb",
                    TagAbschnitt::Berufsschule => "Berufsschule",
                };
                nutzer.push_str(&format!("{}. ({wo}) {}\n", k + 1, v.taetigkeit));
            }
            nutzer.push_str("\nLernfelder:\n");
            for l in &ctx.lernfelder {
                if let Some(n) = lernfeld_nummer(l) {
                    nutzer.push_str(&format!("- {n}: {}\n", l.titel));
                }
            }
            let anzahl = sendbar.len();
            let (zuordnung, a) =
                ki::lernfelder_ki(z.provider, z.system, &nutzer, z.verlauf_dir, |t| pruefe_antwort(t, anzahl, &bekannte))?;
            for (nr, lf) in zuordnung {
                if lf.is_empty() {
                    continue;
                }
                let v = &mut vorschlaege[sendbar[nr - 1]];
                v.lernfelder = lf;
                v.grund = GRUND_KI.into();
            }
            aufruf = Some(a);
        }
    }
    let _ = req.privat_erlauben; // es werden nur Zeilen der Wochennotiz gesendet, keine Notizen
    Ok(LernfeldEntwurf { path: roh.woche.path.clone(), vorschlaege, lernfelder: ctx.lernfelder.clone(), ki: aufruf, warnungen })
}

// ---------------------------------------------------------------------------------------------
// Schreiben
// ---------------------------------------------------------------------------------------------

/// `[[LF04 – Werteströme …\|LF04]] · [[…]]`
fn lernfeld_zelle(ctx: &Kontext, nummern: &[u32]) -> AppResult<String> {
    let mut teile = Vec::new();
    for n in nummern {
        let l = ctx
            .lernfeld(*n)
            .ok_or_else(|| AppError::InvalidInput(format!("Lernfeld {n} gibt es nicht in „02 Lernfelder“.")))?;
        let kurz = l.kurz.clone().unwrap_or_else(|| format!("LF{n:02}"));
        teile.push(format!("[[{}\\|{kurz}]]", l.titel));
    }
    Ok(teile.join(" · "))
}

/// Neuer Text: Zeilen an die Lernfeld-Tabelle (Platzhalterzeile wird ersetzt) und `lernfelder` in
/// der Frontmatter als sortierte Vereinigung (Stil bleibt: Blockliste oder Inline).
pub fn plane_lernfelder(text: &str, ctx: &Kontext, neue: &[NeueLernfeldZeile]) -> AppResult<(String, Vec<String>)> {
    let (mut zeilen, endet) = super::zeilen_von(text);
    let gl = Gliederung::von(text, &zeilen);
    let (tab, bestand) = woche::lernfeld_tabelle(&zeilen, &gl);
    let mut da: Vec<String> = bestand.iter().map(|z| normalisiert(&z.taetigkeit)).collect();
    let mut warnungen = Vec::new();
    let mut zeilen_neu: Vec<(String, String)> = Vec::new();
    let mut nummern: Vec<u32> = Vec::new();
    for z in neue {
        let tae = z.taetigkeit.replace(['\r', '\n'], " ").trim().to_string();
        if let Some(w) = diagnose_wort(&tae) {
            return Err(AppError::InvalidInput(diagnose_meldung(&w)));
        }
        let zelle = lernfeld_zelle(ctx, &z.lernfelder)?;
        if tae.is_empty() || z.lernfelder.is_empty() {
            warnungen.push(format!("„{tae}“ ohne Tätigkeit oder Lernfeld – übersprungen."));
            continue;
        }
        let norm = normalisiert(&tae);
        if da.contains(&norm) {
            warnungen.push(format!("„{tae}“ steht schon in der Lernfeld-Tabelle – übersprungen."));
            continue;
        }
        da.push(norm);
        nummern.extend(z.lernfelder.iter().copied());
        zeilen_neu.push((tabelle::zelle_escapen(&tae), zelle));
    }
    if !zeilen_neu.is_empty() {
        match tab {
            Some(t) => {
                let n = t.spalten.len().max(2);
                let (st, sl) = (t.spalte(Rolle::Taetigkeit).unwrap_or(0), t.spalte(Rolle::Lernfeld).unwrap_or(1));
                let bauen = |(tae, lf): &(String, String)| {
                    let mut c = vec![String::new(); n];
                    c[st] = tae.clone();
                    c[sl] = lf.clone();
                    tabelle::zeile_bauen(&c)
                };
                let mut rest: Vec<String> = zeilen_neu.iter().map(bauen).collect();
                let platz = t.zeilen.iter().copied().find(|i| tabelle::zellen(&zeilen[*i]).is_some_and(|z| tabelle::ist_leer(&z)));
                if let Some(i) = platz {
                    zeilen[i] = rest.remove(0);
                }
                let nach = t.letzte();
                zeilen.splice(nach + 1..nach + 1, rest);
            }
            None => {
                let mut tabelle_neu = vec!["| Tätigkeit | Lernfeld |".to_string(), "|---|---|".to_string()];
                tabelle_neu.extend(zeilen_neu.iter().map(|(t, l)| tabelle::zeile_bauen(&[t.clone(), l.clone()])));
                match gl.abschnitt_beginnt("Bezug zu Lernfeld", zeilen.len()) {
                    Some((k, _, _)) => {
                        let mut neu = vec![String::new()];
                        neu.extend(tabelle_neu);
                        if zeilen.get(k + 1).is_some_and(|z| !z.trim().is_empty()) {
                            neu.push(String::new());
                        }
                        zeilen.splice(k + 1..k + 1, neu);
                    }
                    None => {
                        let pos = gl
                            .koepfe
                            .iter()
                            .find(|k| k.ebene == 2 && super::titel_norm(&k.text).starts_with("ausformulierter nachweis"))
                            .map(|k| k.idx)
                            .unwrap_or(zeilen.len());
                        let mut neu = Vec::new();
                        if pos > 0 && !zeilen[pos - 1].trim().is_empty() {
                            neu.push(String::new());
                        }
                        neu.push(format!("## {ABSCHNITT_LERNFELDER}"));
                        neu.push(String::new());
                        neu.extend(tabelle_neu);
                        if pos < zeilen.len() {
                            neu.push(String::new());
                        }
                        zeilen.splice(pos..pos, neu);
                    }
                }
                warnungen.push("Tabelle „Bezug zu Lernfeldern“ angelegt.".into());
            }
        }
    }
    let mut neu_text = super::zusammen(&zeilen, endet);
    // Frontmatter `lernfelder`
    if !nummern.is_empty() {
        let mut note = NoteText::parse(&neu_text);
        let felder = note.fields().unwrap_or_default();
        let bestand = felder.iter().find(|f| f.key == "lernfelder").map(|f| woche::lernfelder_aus(&f.value));
        match bestand {
            Some(None) => warnungen.push("„lernfelder“ in der Frontmatter enthält Einträge ohne Nummer – dort bitte in Obsidian ergänzen.".into()),
            b => {
                let alt: BTreeSet<u32> = b.flatten().unwrap_or_default().into_iter().collect();
                let mut alle = alt.clone();
                alle.extend(nummern);
                if alle != alt {
                    let liste: Vec<String> = alle.iter().map(|n| n.to_string()).collect();
                    note.set_list("lernfelder", &liste);
                    neu_text = note.render();
                }
            }
        }
    }
    Ok((neu_text, warnungen))
}

/// Lernfeld-Zeilen in die Woche schreiben (Vorschau zuerst, Regel 9).
pub fn schreiben(root: &Path, ctx: &Kontext, req: &LernfelderSchreiben, writes: &SelfWrites, p: &RetryPolicy) -> AppResult<Aenderung> {
    for z in &req.zeilen {
        if let Some(w) = diagnose_wort(&z.taetigkeit) {
            return Err(AppError::InvalidInput(diagnose_meldung(&w)));
        }
        lernfeld_zelle(ctx, &z.lernfelder)?;
    }
    super::aendern(root, &req.path, req.vorschau, req.basis.as_ref(), writes, p, |text| plane_lernfelder(text, ctx, &req.zeilen))
}

#[cfg(test)]
mod tests {
    use super::super::testhilfe::{self, standard, KW41, KW42, P_KW41, P_KW42, P_LF04, LF04};
    use super::*;
    use crate::ki::attrappe::Attrappe;

    const LF10: &str = "---\ntags: [lernfeld]\nlernfeld: 10\n---\n# LF10 – Den Online-Vertrieb kennzahlengestützt optimieren\n";
    const P_LF10: &str = "02 Lernfelder/LF10 – Den Online-Vertrieb kennzahlengestützt optimieren.md";

    fn z(t: &str, lf: &[u32]) -> NeueLernfeldZeile {
        NeueLernfeldZeile { taetigkeit: t.into(), lernfelder: lf.to_vec() }
    }

    #[test]
    fn platzhalterzeile_ersetzt_und_blockliste_bleibt() {
        let (_dir, ctx) = testhilfe::vault(&[(P_LF04, LF04), (P_LF10, LF10)]);
        let (t, w) = plane_lernfelder(KW42, &ctx, &[z("Retouren im Warenwirtschaftssystem erfasst", &[4, 10])]).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let zeile = "| Retouren im Warenwirtschaftssystem erfasst | [[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]] · [[LF10 – Den Online-Vertrieb kennzahlengestützt optimieren\\|LF10]] |";
        let erwartet = KW42.replace("|  | [[]] |", zeile).replace("lernfelder:\n  - 4\n", "lernfelder:\n  - 4\n  - 10\n");
        assert_eq!(t, erwartet);
        // nichts Neues an Nummern → Frontmatter bleibt
        let (t, _) = plane_lernfelder(KW42, &ctx, &[z("Pakete gepackt", &[4])]).unwrap();
        assert!(t.contains("lernfelder:\n  - 4\nstatus"));
    }

    #[test]
    fn inline_liste_duplikat_und_unbekanntes_lernfeld() {
        let (_dir, ctx) = testhilfe::vault(&[(P_LF04, LF04), (P_LF10, LF10)]);
        let (t, w) = plane_lernfelder(KW41, &ctx, &[z("artikel im SHOP gepflegt", &[4]), z("Kennzahlen | Umsatz", &[10])]).unwrap();
        assert_eq!(w.len(), 1, "Duplikat übersprungen: {w:?}");
        assert!(t.contains("lernfelder: [2, 4, 10]\n"));
        assert!(t.contains(
            "| Artikel im Shop gepflegt | [[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]] |\n\
             | Kennzahlen \\| Umsatz | [[LF10 – Den Online-Vertrieb kennzahlengestützt optimieren\\|LF10]] |\n\n## Ausformulierter Nachweis"
        ), "{t}");
        let e = plane_lernfelder(KW41, &ctx, &[z("x", &[13])]);
        assert!(matches!(&e, Err(AppError::InvalidInput(m)) if m.contains("Lernfeld 13")), "{e:?}");
        let e = plane_lernfelder(KW41, &ctx, &[z("Fieber gemessen", &[4])]);
        assert!(matches!(e, Err(AppError::InvalidInput(_))));
        // leere Liste in der Frontmatter → Inline
        let leer = "---\nlernfelder: \n---\n## Ausformulierter Nachweis\n";
        let (t, w) = plane_lernfelder(leer, &ctx, &[z("Retouren", &[4])]).unwrap();
        assert_eq!(
            t,
            "---\nlernfelder: [4]\n---\n\n## Bezug zu Lernfeldern\n\n| Tätigkeit | Lernfeld |\n|---|---|\n\
             | Retouren | [[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]] |\n\n## Ausformulierter Nachweis\n"
        );
        assert!(w.iter().any(|x| x.contains("angelegt")));
    }

    #[test]
    fn vorschlaege_aus_betrieb_md_und_unterrichtsnotiz() {
        let (dir, ctx) = standard();
        let req = LernfeldAnfrage { path: P_KW41.into(), mit_ki: false, privat_erlauben: false };
        let e = entwurf(dir.path(), &ctx, &req, None).unwrap();
        let v: Vec<(&str, &str, Vec<u32>, &str, bool)> = e
            .vorschlaege
            .iter()
            .map(|v| (v.taetigkeit.as_str(), v.quelle.as_str(), v.lernfelder.clone(), v.grund.as_str(), v.schon_in_tabelle))
            .collect();
        assert_eq!(
            v,
            vec![
                ("Handelskalkulation", "GEC 05.10.2026", vec![4], GRUND_NOTIZ, false),
                ("Artikel im Shop gepflegt", "Di 06.10.", vec![4], GRUND_BETRIEB, true),
            ],
            "„Einschulung“ ist eine Lücke und wird nicht vorgeschlagen"
        );
        assert_eq!(e.lernfelder.len(), 1);
        // KW42: Gesundheitsangabe und Krank-Tage sind keine Tätigkeiten
        let req = LernfeldAnfrage { path: P_KW42.into(), mit_ki: false, privat_erlauben: false };
        let e = entwurf(dir.path(), &ctx, &req, None).unwrap();
        assert!(e.vorschlaege.iter().all(|v| !v.taetigkeit.contains("Grippe") && !v.taetigkeit.contains("rbeitsunfähig")));
        assert_eq!(e.vorschlaege.len(), 2);
    }

    #[test]
    fn mit_ki_nur_gueltige_zuordnungen() {
        let (dir, ctx) = standard();
        let req = LernfeldAnfrage { path: P_KW41.into(), mit_ki: true, privat_erlauben: false };
        let p = Attrappe::neu(&[r#"{"zuordnungen":[{"nr":3,"lernfelder":[4]}]}"#, r#"{"zuordnungen":[{"nr":2,"lernfelder":[4]}]}"#]);
        let e = entwurf(dir.path(), &ctx, &req, Some(KiZugang { provider: &p, system: "sys", verlauf_dir: None })).unwrap();
        assert_eq!(e.ki.unwrap().versuche, 2, "Nummer 3 gibt es nicht");
        assert_eq!(e.vorschlaege[1].grund, GRUND_KI);
        let n = &p.anfragen.lock().unwrap()[0].nutzer;
        assert!(n.contains("1. (Berufsschule) Handelskalkulation") && n.contains("- 4: LF04 – Werteströme"), "{n}");
        assert!(!n.contains("Einschulung"));
        assert!(pruefe_antwort(r#"{"zuordnungen":[{"nr":1,"lernfelder":[99]}]}"#, 2, &[4]).is_err());
        assert!(pruefe_antwort(r#"{"zuordnungen":[{"nr":1,"lernfelder":[4,4,4,4]}]}"#, 2, &[4]).is_err());
        assert!(pruefe_antwort(r#"{"zuordnungen":[{"nr":1,"lernfelder":[4]},{"nr":1,"lernfelder":[]}]}"#, 2, &[4]).is_err());
        assert_eq!(pruefe_antwort(r#"{"zuordnungen":[]}"#, 2, &[4]).unwrap(), vec![]);
    }
}
