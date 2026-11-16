//! Markdown-Tabellen (Lernfeld-Tabelle der Woche, `04 Betrieb/Betrieb.md`). In Wikilinks steht der
//! Trenner als `\|` – der trennt keine Zellen.

use std::sync::LazyLock;

use regex::Regex;

use super::regeln::falten;

/// Zellen einer Tabellenzeile (getrimmt, `\|` bleibt wörtlich). `None`, wenn die Zeile keine
/// Tabellenzeile ist.
pub fn zellen(zeile: &str) -> Option<Vec<String>> {
    let t = zeile.trim();
    if !t.starts_with('|') {
        return None;
    }
    let mut out = Vec::new();
    let mut akt = String::new();
    let mut vorher = '\0';
    for c in t[1..].chars() {
        if c == '|' && vorher != '\\' {
            out.push(akt.trim().to_string());
            akt.clear();
        } else {
            akt.push(c);
        }
        vorher = c;
    }
    // Rest nach dem letzten `|` (Zeile ohne abschließendes `|`)
    if !akt.trim().is_empty() {
        out.push(akt.trim().to_string());
    }
    Some(out)
}

static TRENN_ZELLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:?-+:?$").unwrap());

pub fn ist_trenner(z: &[String]) -> bool {
    !z.is_empty() && z.iter().all(|c| TRENN_ZELLE.is_match(c.trim()))
}

/// Zeile ohne Inhalt: alle Zellen leer oder `[[]]`.
pub fn ist_leer(z: &[String]) -> bool {
    z.iter().all(|c| {
        let c = c.trim();
        c.is_empty() || c == "[[]]"
    })
}

/// Eine Tabelle in den Dateizeilen (Indizes 0-basiert).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tabelle {
    pub kopf: usize,
    pub spalten: Vec<String>,
    /// Datenzeilen (ohne Kopf und Trenner)
    pub zeilen: Vec<usize>,
    /// erste Zeile nach der Tabelle
    pub ende: usize,
}

impl Tabelle {
    /// Letzte Zeile der Tabelle (Trenner, wenn es keine Datenzeile gibt).
    pub fn letzte(&self) -> usize {
        self.zeilen.last().copied().unwrap_or(self.kopf + 1)
    }

    pub fn spalte(&self, r: Rolle) -> Option<usize> {
        self.spalten.iter().position(|s| rolle(s) == r)
    }
}

/// Alle Tabellen in `zeilen[von..bis]`.
pub fn tabellen(zeilen: &[String], von: usize, bis: usize) -> Vec<Tabelle> {
    let bis = bis.min(zeilen.len());
    let mut out = Vec::new();
    let mut i = von;
    while i + 1 < bis {
        let (Some(kopf), Some(trenner)) = (zellen(&zeilen[i]), zellen(&zeilen[i + 1])) else {
            i += 1;
            continue;
        };
        if !ist_trenner(&trenner) || ist_trenner(&kopf) {
            i += 1;
            continue;
        }
        let mut j = i + 2;
        let mut daten = Vec::new();
        while j < bis && zellen(&zeilen[j]).is_some() {
            daten.push(j);
            j += 1;
        }
        out.push(Tabelle { kopf: i, spalten: kopf, zeilen: daten, ende: j });
        i = j;
    }
    out
}

/// Rolle einer Spalte nach ihrem Kopf.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rolle {
    Taetigkeit,
    Lernfeld,
    Woche,
    Schule,
    Sonst,
}

static KW_WORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bkw\b").unwrap());

pub fn rolle(kopf: &str) -> Rolle {
    let k = falten(kopf);
    if k.contains("lernfeld") {
        Rolle::Lernfeld
    } else if k.contains("woche") || KW_WORT.is_match(&k) {
        Rolle::Woche
    } else if k.contains("tatigkeit") || k.contains("betrieb") {
        Rolle::Taetigkeit
    } else if k.contains("schul") || k.contains("fach") {
        Rolle::Schule
    } else {
        Rolle::Sonst
    }
}

/// Zellinhalt zum Schreiben: Zeilenumbrüche → Leerzeichen, `|` → `\|` (außer schon `\|`).
pub fn zelle_escapen(s: &str) -> String {
    let eine: String = s.replace(['\r', '\n'], " ").split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::with_capacity(eine.len() + 4);
    let mut vorher = '\0';
    for c in eine.chars() {
        if c == '|' && vorher != '\\' {
            out.push('\\');
        }
        out.push(c);
        vorher = c;
    }
    out
}

/// `| a | b |` – leere Zellen wie im Bestand als `|  |`.
pub fn zeile_bauen(z: &[String]) -> String {
    format!("| {} |", z.join(" | "))
}

static LF_NR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bLF\s*0*(\d{1,2})\b").unwrap());

/// Lernfeld-Nummern einer Zelle (`[[LF04 – …\|LF04]] · [[LF07 …]]` → [4, 7]), ohne Doppelte.
pub fn lernfeld_nummern(zelle: &str) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for c in LF_NR.captures_iter(zelle) {
        if let Ok(n) = c[1].parse::<u32>() {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn z(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn zellen_mit_wikilink_trenner() {
        assert_eq!(
            zellen("| Retouren | [[LF05 – Rückabwicklung\\|LF05]] · [[LF02 – X\\|LF02]] | [[2026-KW36\\|KW36]] |"),
            Some(z(&["Retouren", "[[LF05 – Rückabwicklung\\|LF05]] · [[LF02 – X\\|LF02]]", "[[2026-KW36\\|KW36]]"]))
        );
        assert_eq!(zellen("|  |  |  |"), Some(z(&["", "", ""])));
        assert_eq!(zellen("kein | Tisch"), None);
        assert!(ist_trenner(&z(&["---", ":--:", "-"])));
        assert!(ist_leer(&z(&["", "[[]]"])));
        assert_eq!(lernfeld_nummern("[[LF04 – Werteströme\\|LF04]] · [[LF10 – X\\|LF10]] · LF4"), vec![4, 10]);
    }

    #[test]
    fn rollen_und_escapen() {
        assert_eq!(rolle("Tätigkeit im Betrieb"), Rolle::Taetigkeit);
        assert_eq!(rolle("Lernfeld"), Rolle::Lernfeld);
        assert_eq!(rolle("Passende Lernfelder"), Rolle::Lernfeld);
        assert_eq!(rolle("Woche"), Rolle::Woche);
        assert_eq!(rolle("Fach · Woche"), Rolle::Woche);
        assert_eq!(rolle("Schule"), Rolle::Schule);
        assert_eq!(rolle("Betrieb"), Rolle::Taetigkeit);
        assert_eq!(rolle("Abteilung"), Rolle::Sonst);
        assert_eq!(zelle_escapen("a | b \\| c\nd"), "a \\| b \\| c d");
        assert_eq!(zeile_bauen(&z(&["a", "", "c"])), "| a |  | c |");
    }

    #[test]
    fn tabellen_finden() {
        let t = z(&["# T", "| A | B |", "|---|---|", "| 1 | 2 |", "|  |  |", "", "| X |", "|---|", "Text"]);
        let tabs = tabellen(&t, 0, t.len());
        assert_eq!(tabs.len(), 2);
        assert_eq!((tabs[0].kopf, tabs[0].zeilen.clone(), tabs[0].ende), (1, vec![3, 4], 5));
        assert_eq!((tabs[1].kopf, tabs[1].zeilen.clone(), tabs[1].letzte()), (6, vec![], 7));
    }
}
