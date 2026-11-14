//! Stundenplan aus `00 Start/Stundenplan.md` lesen – die App dupliziert ihn nie.
//!
//! Format: je Schultag eine Überschrift `## Montag` … `## Freitag`, darunter eine Tabelle mit
//! den Spalten `Std.`, `Zeit`, `Fach`, `Lehrkraft` (Reihenfolge egal). Zeilen ohne Stunde
//! (z. B. `*Pause*`) und ohne Fach werden übersprungen. Tage ohne Tabelle sind Betriebstage.

use super::links::parse_link_inner;
use crate::model::{Block, Schultag, Stunde, Stundenplan, Wochentag};

pub const STUNDENPLAN_PATH: &str = "00 Start/Stundenplan.md";

pub fn parse_stundenplan(body: &str) -> Stundenplan {
    let mut tage: Vec<Schultag> = Vec::new();
    let mut current: Option<Wochentag> = None;
    let mut header: Option<Columns> = None;

    for line in body.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix('#') {
            let title = h.trim_start_matches('#').trim();
            current = Wochentag::from_name(title);
            header = None;
            continue;
        }
        let Some(tag) = current else { continue };
        if !t.starts_with('|') {
            if !t.is_empty() {
                header = None; // Tabelle zu Ende
            }
            continue;
        }
        let cells = split_row(t);
        if cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue; // Trennzeile |---|---|
        }
        let Some(cols) = &header else {
            header = Columns::from_header(&cells);
            continue;
        };
        if let Some(stunde) = cols.row(&cells) {
            match tage.iter_mut().find(|d| d.tag == tag) {
                Some(day) => day.stunden.push(stunde),
                None => tage.push(Schultag { tag, stunden: vec![stunde] }),
            }
        }
    }
    for d in &mut tage {
        d.stunden.sort_by_key(|s| s.nr);
    }
    tage.sort_by_key(|d| d.tag);
    Stundenplan { tage }
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
            lehrkraft: opt(self.lehrkraft),
        })
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

    #[test]
    fn zeile_mit_escaped_pipe_wird_richtig_geteilt() {
        let cells = split_row("| 2 | 08:35 | [[GEC – X\\|GEC]] | HEIN |");
        assert_eq!(cells, vec!["2", "08:35", "[[GEC – X\\|GEC]]", "HEIN"]);
    }
}
