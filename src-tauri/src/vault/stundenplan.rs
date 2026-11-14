//! Stundenplan aus `00 Start/Stundenplan.md` lesen – die App dupliziert ihn nie.
//!
//! Zwei Formate, beide überall in der Datei:
//!
//! * **Tagesabschnitte:** je Schultag eine Überschrift `## Montag` … `## Freitag`, darunter eine
//!   Tabelle mit den Spalten `Std.`, `Zeit`, `Fach`, `Lehrkraft` (Reihenfolge egal).
//! * **Matrix** (SPEC §2): eine Tabelle `| Std. | Zeit | Montag | Donnerstag |` – Spalte `Std.`,
//!   **keine** Spalte `Fach`, mindestens eine Spalte mit einem Wochentag als Kopf.
//!
//! Hat ein Wochentag Stunden aus einem Tagesabschnitt, gilt nur der Abschnitt. Zeilen ohne
//! Stunde (z. B. `*Pause*`) und ohne Fach werden übersprungen. Tage ohne Stunden sind Betriebstage.

use super::links::{parse_link_inner, parse_links};
use crate::model::{Block, Fach, Schultag, Stunde, Stundenplan, Wochentag};

pub const STUNDENPLAN_PATH: &str = "00 Start/Stundenplan.md";

/// Welche Art Tabelle gerade gelesen wird.
enum Table {
    /// Kopfzeile noch nicht gesehen
    Start,
    Tag(Wochentag, Columns),
    Matrix(MatrixColumns),
    /// irgendeine andere Tabelle – wird ignoriert
    Andere,
}

pub fn parse_stundenplan(body: &str) -> Stundenplan {
    let mut tage: Vec<Schultag> = Vec::new();
    let mut matrix: Vec<Schultag> = Vec::new();
    let mut current: Option<Wochentag> = None;
    let mut table = Table::Start;

    for line in body.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix('#') {
            let title = h.trim_start_matches('#').trim();
            current = Wochentag::from_name(title);
            table = Table::Start;
            continue;
        }
        if !t.starts_with('|') {
            if !t.is_empty() {
                table = Table::Start; // Tabelle zu Ende
            }
            continue;
        }
        let cells = split_row(t);
        if cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue; // Trennzeile |---|---|
        }
        match &table {
            Table::Start => {
                table = if let Some(m) = MatrixColumns::from_header(&cells) {
                    Table::Matrix(m)
                } else {
                    match (current, Columns::from_header(&cells)) {
                        (Some(tag), Some(cols)) => Table::Tag(tag, cols),
                        // M0-Verhalten: unter einer Tagesüberschrift zählt die erste passende Kopfzeile
                        (Some(_), None) => Table::Start,
                        (None, _) => Table::Andere,
                    }
                };
            }
            Table::Tag(tag, cols) => {
                if let Some(stunde) = cols.row(&cells) {
                    push_stunde(&mut tage, *tag, stunde, false);
                }
            }
            Table::Matrix(m) => {
                for (tag, stunde) in m.row(&cells) {
                    push_stunde(&mut matrix, tag, stunde, true);
                }
            }
            Table::Andere => {}
        }
    }
    // Vorrang: ein Tag mit Stunden aus „## <Tag>“ ignoriert die Matrix
    for day in matrix {
        if !tage.iter().any(|d| d.tag == day.tag && !d.stunden.is_empty()) {
            tage.push(day);
        }
    }
    for d in &mut tage {
        d.stunden.sort_by_key(|s| s.nr);
    }
    tage.sort_by_key(|d| d.tag);
    Stundenplan { tage, fach_links: fach_links(body) }
}

/// Fügt eine Stunde an. `dedupe`: bei doppeltem (Tag, Nr.) gewinnt die erste (mehrere Matrizen).
fn push_stunde(tage: &mut Vec<Schultag>, tag: Wochentag, stunde: Stunde, dedupe: bool) {
    match tage.iter_mut().find(|d| d.tag == tag) {
        Some(day) => {
            if !(dedupe && day.stunden.iter().any(|s| s.nr == stunde.nr)) {
                day.stunden.push(stunde);
            }
        }
        None => tage.push(Schultag { tag, stunden: vec![stunde] }),
    }
}

/// Alle Wikilinks mit Alias in der Datei (auch `\|` in Tabellen): Alias → volles Ziel.
/// Der erste Link je Kürzel gewinnt.
pub fn fach_links(body: &str) -> Vec<Fach> {
    let mut out: Vec<Fach> = Vec::new();
    for l in parse_links(body, 0) {
        let Some(alias) = l.alias else { continue };
        if l.embed || l.target.is_empty() || out.iter().any(|f| f.kurz.eq_ignore_ascii_case(&alias)) {
            continue;
        }
        out.push(Fach { kurz: alias, ziel: l.target, lehrkraft: None });
    }
    out
}

/// `—`, `–`, `-` und leer (nach Trimmen und ohne `*`) bedeuten: keine Lehrkraft / frei.
pub fn ist_platzhalter(s: &str) -> bool {
    matches!(s.trim().trim_matches('*').trim(), "" | "—" | "–" | "-")
}

impl Stundenplan {
    pub fn tag(&self, tag: Wochentag) -> Option<&Schultag> {
        self.tage.iter().find(|d| d.tag == tag)
    }

    pub fn ist_schultag(&self, tag: Wochentag) -> bool {
        self.tag(tag).is_some_and(|d| !d.stunden.is_empty())
    }
}

impl Schultag {
    /// Aufeinanderfolgende Stunden desselben Fachs werden zu einem Block zusammengefasst.
    /// Montag: GEC (2) · D-DV (3–4) · WSP (5) · GEC (6–7) · D (8).
    pub fn bloecke(&self) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::new();
        for s in &self.stunden {
            if let Some(last) = out.last_mut() {
                if last.fach == s.fach && last.bis_stunde + 1 == s.nr {
                    last.bis_stunde = s.nr;
                    last.ende = zeit_ende(&s.zeit);
                    continue;
                }
            }
            out.push(Block {
                fach: s.fach.clone(),
                fach_ziel: s.fach_ziel.clone(),
                lehrkraft: s.lehrkraft.clone(),
                von_stunde: s.nr,
                bis_stunde: s.nr,
                beginn: zeit_beginn(&s.zeit),
                ende: zeit_ende(&s.zeit),
            });
        }
        out
    }
}

fn zeit_teile(z: &str) -> Vec<String> {
    z.split(['–', '-', '—']).map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}
fn zeit_beginn(z: &str) -> String {
    zeit_teile(z).first().cloned().unwrap_or_default()
}
fn zeit_ende(z: &str) -> String {
    zeit_teile(z).last().cloned().unwrap_or_default()
}

struct Columns {
    nr: usize,
    zeit: Option<usize>,
    fach: usize,
    lehrkraft: Option<usize>,
}

impl Columns {
    fn from_header(cells: &[String]) -> Option<Self> {
        let find = |names: &[&str]| {
            cells.iter().position(|c| {
                let c = c.to_lowercase();
                names.iter().any(|n| c.trim_end_matches('.') == *n)
            })
        };
        Some(Columns {
            nr: find(&["std", "stunde"])?,
            zeit: find(&["zeit"]),
            fach: find(&["fach"])?,
            lehrkraft: find(&["lehrkraft", "lehrer"]),
        })
    }

    fn row(&self, cells: &[String]) -> Option<Stunde> {
        let get = |i: usize| cells.get(i).map(|s| s.trim()).unwrap_or("");
        let nr: u8 = get(self.nr).parse().ok()?;
        let (fach, fach_ziel) = fach_aus_zelle(get(self.fach))?;
        let opt = |i: Option<usize>| i.map(get).filter(|s| !s.is_empty()).map(str::to_string);
        Some(Stunde {
            nr,
            zeit: opt(self.zeit).unwrap_or_default(),
            fach,
            fach_ziel,
            lehrkraft: opt(self.lehrkraft).filter(|l| !ist_platzhalter(l)),
        })
    }
}

/// Spalten einer Matrix-Tabelle: `Std.`, optional `Zeit`, je Wochentag eine Spalte.
struct MatrixColumns {
    nr: usize,
    zeit: Option<usize>,
    tage: Vec<(usize, Wochentag)>,
}

impl MatrixColumns {
    fn from_header(cells: &[String]) -> Option<Self> {
        let norm = |c: &str| c.trim().trim_matches('*').trim().to_lowercase();
        let find = |names: &[&str]| cells.iter().position(|c| names.iter().any(|n| norm(c).trim_end_matches('.') == *n));
        let nr = find(&["std", "stunde"])?;
        if find(&["fach"]).is_some() {
            return None;
        }
        let tage: Vec<(usize, Wochentag)> = cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Wochentag::from_name(c.trim().trim_matches('*')).map(|w| (i, w)))
            .collect();
        if tage.is_empty() {
            return None;
        }
        Some(MatrixColumns { nr, zeit: find(&["zeit"]), tage })
    }

    fn row(&self, cells: &[String]) -> Vec<(Wochentag, Stunde)> {
        let get = |i: usize| cells.get(i).map(|s| s.trim()).unwrap_or("");
        let Ok(nr) = get(self.nr).trim_matches('*').trim().parse::<u8>() else { return Vec::new() };
        let zeit = self.zeit.map(get).unwrap_or("").to_string();
        self.tage
            .iter()
            .filter_map(|(i, tag)| {
                let (fach, fach_ziel) = fach_aus_zelle(get(*i))?;
                Some((*tag, Stunde { nr, zeit: zeit.clone(), fach, fach_ziel, lehrkraft: None }))
            })
            .collect()
    }
}

/// `[[GEC – Geschäftsprozesse im E-Commerce\|GEC]]` → (`GEC`, Some(Ziel)); `[[D-DV]]` → (`D-DV`, Some(`D-DV`));
/// `GEC` → (`GEC`, None); leer / `—` → None.
fn fach_aus_zelle(cell: &str) -> Option<(String, Option<String>)> {
    let cell = cell.trim().trim_matches('*').trim();
    if cell.is_empty() || matches!(cell, "—" | "-" | "–") {
        return None;
    }
    if let Some(inner) = cell.strip_prefix("[[").and_then(|r| r.split_once("]]")).map(|(i, _)| i) {
        let (target, _, alias) = parse_link_inner(inner);
        let kurz = alias.unwrap_or_else(|| target.clone());
        return Some((kurz, Some(target)));
    }
    Some((cell.to_string(), None))
}

/// Teilt eine Tabellenzeile an `|`, aber nicht an `\|` (Alias-Trenner in Tabellen).
fn split_row(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|');
    let inner = inner.strip_suffix('|').filter(|s| !s.ends_with('\\')).unwrap_or(inner);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut prev_backslash = false;
    for ch in inner.chars() {
        if ch == '|' && !prev_backslash {
            cells.push(cur.trim().to_string());
            cur.clear();
        } else {
            cur.push(ch);
        }
        prev_backslash = ch == '\\';
    }
    cells.push(cur.trim().to_string());
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    const STARTER: &str = include_str!("../../starter-vault/00 Start/Stundenplan.md");

    #[test]
    fn montag_bloecke_gec_ddv_wsp_gec_d() {
        let plan = parse_stundenplan(STARTER);
        let mo = plan.tag(Wochentag::Montag).expect("Montag fehlt");
        assert_eq!(mo.stunden.len(), 7);
        let b = mo.bloecke();
        let faecher: Vec<&str> = b.iter().map(|b| b.fach.as_str()).collect();
        assert_eq!(faecher, vec!["GEC", "D-DV", "WSP", "GEC", "D"]);
        assert_eq!((b[1].von_stunde, b[1].bis_stunde), (3, 4));
        assert_eq!((b[3].von_stunde, b[3].bis_stunde), (6, 7));
        assert_eq!(b[3].beginn, "12:05");
        assert_eq!(b[3].ende, "13:50");
        assert_eq!(b[0].fach_ziel.as_deref(), Some("GEC – Geschäftsprozesse im E-Commerce"));
        assert_eq!(b[0].lehrkraft.as_deref(), Some("HEIN"));
        assert_eq!(b[4].lehrkraft, None);
    }

    #[test]
    fn donnerstag_und_betriebstage() {
        let plan = parse_stundenplan(STARTER);
        let faecher: Vec<String> =
            plan.tag(Wochentag::Donnerstag).unwrap().bloecke().into_iter().map(|b| b.fach).collect();
        assert_eq!(faecher, vec!["KSK", "WSP", "E", "GEC", "PG"]);
        assert!(plan.ist_schultag(Wochentag::Montag));
        assert!(!plan.ist_schultag(Wochentag::Dienstag));
        assert!(!plan.ist_schultag(Wochentag::Freitag));
    }

    #[test]
    fn pausen_und_leere_zeilen_werden_uebersprungen() {
        let md = "## Montag\n| Std. | Zeit | Fach | Lehrkraft |\n|---|---|---|---|\n\
                  | 2 | 08:35 – 09:20 | GEC | HEIN |\n|  | *Pause* | | |\n\
                  | 3 | 09:35 – 10:20 | — | |\n| 4 | 10:20 – 11:05 | GEC | HEIN |\n\n## Zeiten\n| Std. | Zeit |\n|---|---|\n| 1 | x |\n";
        let plan = parse_stundenplan(md);
        let mo = plan.tag(Wochentag::Montag).unwrap();
        assert_eq!(mo.stunden.len(), 2);
        // Stunde 3 ist frei → zwei getrennte GEC-Blöcke
        assert_eq!(mo.bloecke().len(), 2);
        assert_eq!(plan.tage.len(), 1);
    }

    #[test]
    fn spaltenreihenfolge_ist_egal() {
        let md = "## Donnerstag\n| Fach | Std | Lehrkraft |\n|--|--|--|\n| [[PG – Politik\\|PG]] | 7 | Rothaus |\n";
        let s = &parse_stundenplan(md).tage[0].stunden[0];
        assert_eq!((s.nr, s.fach.as_str(), s.lehrkraft.as_deref()), (7, "PG", Some("Rothaus")));
    }

    /// Die Tabelle aus SPEC §2, wörtlich.
    const SPEC_MATRIX: &str = "| Std. | Zeit | Montag | Donnerstag |\n\
                                          |---:|---|---|---|\n\
                                          | 2 | 08:35–09:20 | GEC | KSK |\n\
                                          | 3 | 09:35–10:20 | D-DV | WSP |\n\
                                          | 4 | 10:20–11:05 | D-DV | E |\n\
                                          | 5 | 11:20–12:05 | WSP | GEC |\n\
                                          | 6 | 12:05–12:50 | GEC | GEC |\n\
                                          | 7 | 13:05–13:50 | GEC | PG |\n\
                                          | 8 | 13:50–14:35 | D | — |\n";

    #[test]
    fn matrix_wie_spec() {
        let plan = parse_stundenplan(SPEC_MATRIX);
        let mo = plan.tag(Wochentag::Montag).unwrap().bloecke();
        let got: Vec<(&str, u8, u8, &str, &str)> = mo
            .iter()
            .map(|b| (b.fach.as_str(), b.von_stunde, b.bis_stunde, b.beginn.as_str(), b.ende.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("GEC", 2, 2, "08:35", "09:20"),
                ("D-DV", 3, 4, "09:35", "11:05"),
                ("WSP", 5, 5, "11:20", "12:05"),
                ("GEC", 6, 7, "12:05", "13:50"),
                ("D", 8, 8, "13:50", "14:35"),
            ]
        );
        assert!(mo.iter().all(|b| b.fach_ziel.is_none() && b.lehrkraft.is_none()));
        let don = plan.tag(Wochentag::Donnerstag).unwrap();
        let faecher: Vec<String> = don.bloecke().into_iter().map(|b| b.fach).collect();
        assert_eq!(faecher, vec!["KSK", "WSP", "E", "GEC", "PG"]);
        assert!(don.stunden.iter().all(|s| s.nr != 8), "— in Std. 8 ist frei");
        assert_eq!(don.bloecke()[3].von_stunde, 5);
        assert_eq!(don.bloecke()[3].bis_stunde, 6);
        assert!(!plan.ist_schultag(Wochentag::Dienstag));
    }

    #[test]
    fn tagesabschnitt_schlaegt_matrix() {
        let md = format!(
            "## Montag\n| Std. | Zeit | Fach | Lehrkraft |\n|---|---|---|---|\n| 1 | 07:50 – 08:35 | KSK | BERN |\n\n## Plan\n{SPEC_MATRIX}"
        );
        let plan = parse_stundenplan(&md);
        let mo: Vec<String> = plan.tag(Wochentag::Montag).unwrap().bloecke().into_iter().map(|b| b.fach).collect();
        assert_eq!(mo, vec!["KSK"]);
        // Donnerstag kommt weiter aus der Matrix
        assert_eq!(plan.tag(Wochentag::Donnerstag).unwrap().bloecke().len(), 5);
        // mehrere Matrizen: bei doppeltem (Tag, Nr.) gewinnt die erste
        let zwei = format!("{SPEC_MATRIX}\ntext\n| Std. | Montag |\n|---|---|\n| 2 | PG |\n| 9 | PG |\n");
        let mo = parse_stundenplan(&zwei).tag(Wochentag::Montag).unwrap().stunden.clone();
        assert_eq!(mo[0].fach, "GEC");
        assert_eq!(mo.last().unwrap().nr, 9);
    }

    #[test]
    fn lehrkraft_platzhalter() {
        let md = "## Montag\n| Std. | Fach | Lehrkraft |\n|---|---|---|\n| 1 | D | — |\n| 2 | E | – |\n| 3 | PG | - |\n| 4 | WSP | *—* |\n| 5 | GEC | HEIN |\n";
        let plan = parse_stundenplan(md);
        let l: Vec<Option<&str>> =
            plan.tag(Wochentag::Montag).unwrap().stunden.iter().map(|s| s.lehrkraft.as_deref()).collect();
        assert_eq!(l, vec![None, None, None, None, Some("HEIN")]);
        assert!(ist_platzhalter(" — ") && ist_platzhalter("") && !ist_platzhalter("HEIN"));
    }

    #[test]
    fn fach_links_aus_lehrkraefte_tabelle() {
        let text = include_str!("../../../fixtures/vault/00 Start/Stundenplan.md");
        let plan = parse_stundenplan(text);
        let ziel = |k: &str| plan.fach_links.iter().find(|f| f.kurz == k).map(|f| f.ziel.as_str());
        assert_eq!(ziel("D"), Some("D – Deutsch"));
        assert_eq!(ziel("KSK"), Some("KSK – Kaufmännische Steuerung und Kontrolle"));
        assert_eq!(ziel("GEC"), Some("GEC – Geschäftsprozesse im E-Commerce"));
        assert_eq!(ziel("D-DV"), None, "Link ohne Alias zählt nicht");
        // je Kürzel nur ein Eintrag
        assert_eq!(plan.fach_links.iter().filter(|f| f.kurz == "GEC").count(), 1);
    }

    #[test]
    fn zeile_mit_escaped_pipe_wird_richtig_geteilt() {
        let cells = split_row("| 2 | 08:35 | [[GEC – X\\|GEC]] | HEIN |");
        assert_eq!(cells, vec!["2", "08:35", "[[GEC – X\\|GEC]]", "HEIN"]);
    }
}
