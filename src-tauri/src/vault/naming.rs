//! Dateinamens-Konventionen (SPEC §5.1, §5.3).
//!
//! * Unterricht: `<FACH> <TT.MM.JJJJ>.md`, zweiter Block: `<FACH> 2 <TT.MM.JJJJ>.md`
//! * Altbestand: `Unterrichtsnotiz GEC 07.09.2026.md` – **lesen ja, umbenennen nein**
//! * Berichtsheft: `<JJJJ>-KW<nn>.md` (ISO-Kalenderwoche)
//! * Material: `JJJJ-MM-TT Fach – Thema.ext`

use std::sync::LazyLock;

use chrono::{Datelike, NaiveDate};
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnterrichtName {
    pub fach: String,
    /// 1 = erster Block des Tages, 2 = zweiter Block …
    pub block: u8,
    pub datum: NaiveDate,
    /// alter Name (`Unterrichtsnotiz GEC 07.09.2026`) – wird nie umbenannt
    pub altbestand: bool,
}

static UNTERRICHT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<legacy>Unterrichtsnotiz\s+)?(?P<fach>[\p{L}][\p{L}\p{N}\-]*)(?:\s+(?P<block>[2-9]))?\s+(?P<d>\d{1,2})\.(?P<m>\d{1,2})\.(?P<y>\d{4})$",
    )
    .unwrap()
});
static WOCHE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<y>\d{4})-KW(?P<kw>\d{1,2})$").unwrap());
static MATERIAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<date>\d{4}-\d{2}-\d{2}) (?P<fach>\S+) – (?P<thema>.+)\.(?P<ext>[A-Za-z0-9]+)$").unwrap()
});

/// Erkennt eine Unterrichtsnotiz am Dateinamen **ohne** `.md`.
pub fn parse_unterricht(stem: &str) -> Option<UnterrichtName> {
    let c = UNTERRICHT.captures(stem.trim())?;
    let datum = NaiveDate::from_ymd_opt(c["y"].parse().ok()?, c["m"].parse().ok()?, c["d"].parse().ok()?)?;
    Some(UnterrichtName {
        fach: c["fach"].to_string(),
        block: c.name("block").map(|b| b.as_str().parse().unwrap_or(1)).unwrap_or(1),
        datum,
        altbestand: c.name("legacy").is_some(),
    })
}

/// Dateiname für eine **neue** Unterrichtsnotiz (immer neue Konvention).
pub fn unterricht_filename(fach: &str, block: u8, datum: NaiveDate) -> String {
    let d = datum.format("%d.%m.%Y");
    if block <= 1 {
        format!("{fach} {d}.md")
    } else {
        format!("{fach} {block} {d}.md")
    }
}

/// `(ISO-Jahr, KW)` einer Berichtsheft-Woche, Dateiname ohne `.md`.
pub fn parse_woche(stem: &str) -> Option<(i32, u32)> {
    let c = WOCHE.captures(stem.trim())?;
    let kw: u32 = c["kw"].parse().ok()?;
    let y: i32 = c["y"].parse().ok()?;
    (1..=53).contains(&kw).then_some((y, kw))
}

/// `2026-KW41.md` – nach ISO 8601 (Woche mit dem ersten Donnerstag ist KW 1).
pub fn woche_filename(datum: NaiveDate) -> String {
    let w = datum.iso_week();
    format!("{}-KW{:02}.md", w.year(), w.week())
}

/// `JJJJ-MM-TT Fach – Thema.ext`
pub fn material_filename(datum: NaiveDate, fach: &str, thema: &str, ext: &str) -> String {
    let thema = sanitize(thema);
    format!("{} {fach} – {thema}.{}", datum.format("%Y-%m-%d"), ext.trim_start_matches('.').to_lowercase())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialName {
    pub datum: NaiveDate,
    pub fach: String,
    pub thema: String,
    pub ext: String,
}

pub fn parse_material(file_name: &str) -> Option<MaterialName> {
    let c = MATERIAL.captures(file_name)?;
    Some(MaterialName {
        datum: NaiveDate::parse_from_str(&c["date"], "%Y-%m-%d").ok()?,
        fach: c["fach"].to_string(),
        thema: c["thema"].to_string(),
        ext: c["ext"].to_string(),
    })
}

/// Entfernt Zeichen, die Windows in Dateinamen nicht erlaubt.
pub fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    #[test]
    fn unterricht_neu_und_zweiter_block() {
        let n = parse_unterricht("GEC 07.09.2026").unwrap();
        assert_eq!((n.fach.as_str(), n.block, n.datum, n.altbestand), ("GEC", 1, d(2026, 9, 7), false));
        let n = parse_unterricht("GEC 2 12.10.2026").unwrap();
        assert_eq!(n.block, 2);
        let n = parse_unterricht("D-DV 05.10.2026").unwrap();
        assert_eq!(n.fach, "D-DV");
        assert_eq!(parse_unterricht("D 09.11.2026").unwrap().fach, "D");
    }

    #[test]
    fn altbestand_wird_erkannt() {
        let n = parse_unterricht("Unterrichtsnotiz GEC 07.09.2026").unwrap();
        assert!(n.altbestand);
        assert_eq!(n.fach, "GEC");
    }

    #[test]
    fn keine_unterrichtsnotizen() {
        for s in ["Unterricht", "!Unterricht", "1. Unterrichtsnotiz Template", "GEC 31.02.2026", "GEC 2026-09-07"] {
            assert_eq!(parse_unterricht(s), None, "{s}");
        }
    }

    #[test]
    fn dateinamen_erzeugen() {
        assert_eq!(unterricht_filename("GEC", 1, d(2026, 9, 7)), "GEC 07.09.2026.md");
        assert_eq!(unterricht_filename("GEC", 2, d(2026, 9, 7)), "GEC 2 07.09.2026.md");
        let name = unterricht_filename("D-DV", 1, d(2026, 11, 2));
        assert_eq!(parse_unterricht(name.trim_end_matches(".md")).unwrap().datum, d(2026, 11, 2));
    }

    #[test]
    fn kalenderwoche_nach_iso() {
        assert_eq!(woche_filename(d(2026, 10, 5)), "2026-KW41.md");
        // 01.01.2027 ist ein Freitag → gehört zu KW 53 von 2026
        assert_eq!(woche_filename(d(2027, 1, 1)), "2026-KW53.md");
        assert_eq!(parse_woche("2026-KW41"), Some((2026, 41)));
        assert_eq!(parse_woche("2026-KW60"), None);
    }

    #[test]
    fn material_namen() {
        let n = material_filename(d(2026, 9, 7), "GEC", "Kalkulation: Teil 1?", ".PDF");
        assert_eq!(n, "2026-09-07 GEC – Kalkulation Teil 1.pdf");
        let p = parse_material(&n).unwrap();
        assert_eq!(p.thema, "Kalkulation Teil 1");
        assert_eq!(p.ext, "pdf");
    }
}
