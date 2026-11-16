//! M4: Berichtsheft (SPEC §5.3 „Berichtsheft-Woche“, §6.6).
//!
//! * **Woche lesen** ([`woche`]): Tage unter `## Berufsschule` / `## Betrieb`, Stichpunkte, Lücken,
//!   Gesundheitsangaben, Lernfeld-Tabelle, offene Punkte; Übersicht aller Wochen mit fehlenden Wochen.
//! * **Anlegen** ([`anlegen`]): Vorlage `99 Meta/Templates/Berichtsheft-Woche.md` füllen (nur KW,
//!   Zeitraum, Datums-Platzhalter, Links), Schultage aus den Unterrichtsnotizen der Woche eintragen.
//! * **Tag speichern** ([`tag`]), **Nachweis** ([`nachweis`]), **Lernfelder** ([`lernfelder`]),
//!   **Betrieb.md fortschreiben** ([`betrieb`]).
//!
//! Harte Regeln:
//! * **Regel 3** – kein Feld, kein Freitext, kein Prompt für eine Diagnose; nur die Tagesart
//!   „Arbeitsunfähig erkrankt“. Gesundheitsangaben aus Obsidian werden angezeigt, aber nie an die KI
//!   gesendet und nie in den Nachweis übernommen ([`regeln::diagnose_wort`]).
//! * **Regel 4** – dünne Stichpunkte werden als `⟨Lücke: „…“ – Gegenstand fehlt⟩` markiert
//!   ([`regeln::ist_luecke`]); KI-Sätze ohne Beleg im Stichpunkt werden verworfen.
//! * **Regel 9** – jede Änderung an einer bestehenden Datei braucht vorher eine Vorschau: ohne
//!   `basis` aus einer Vorschau schreibt kein M4-Befehl ([`aendern`]).
//!
//! Geschrieben wird nur über [`crate::write`] (Zeilenchirurgie auf dem dekodierten Text, BOM und
//! CRLF bleiben).

pub mod anlegen;
pub mod betrieb;
pub mod lernfelder;
pub mod nachweis;
pub mod regeln;
pub mod tabelle;
pub mod tag;
pub mod woche;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use regex::Regex;

pub use regeln::{diagnose_wort, ist_luecke};

use crate::aufgaben::{bericht, frisch_lesen, titel_aus_pfad};
use crate::error::{AppError, AppResult};
use crate::guard;
use crate::index::VaultIndex;
use crate::model::{Aenderung, LernfeldInfo, NoteKind, NoteMeta, NoteVersion, RelPath, Stundenplan};
use crate::unterricht;
use crate::vault::frontmatter::NoteText;
use crate::vault::prose_lines;
use crate::write::{self, RetryPolicy, SaveOutcome, SelfWrites};

pub const WOCHEN_DIR: &str = "04 Betrieb/Wochen";
pub const TEMPLATE_WOCHE: &str = "99 Meta/Templates/Berichtsheft-Woche.md";
pub const BETRIEB_MD: &str = "04 Betrieb/Betrieb.md";

// ---------------------------------------------------------------------------------------------
// Kontext aus Index und Platte
// ---------------------------------------------------------------------------------------------

/// Alles, was die Berichtsheft-Funktionen außer der Wochendatei brauchen. Unter der Index-Sperre
/// geklont ([`Kontext::aus_index`]); gelesen wird danach ohne Sperre.
#[derive(Debug, Clone, Default)]
pub struct Kontext {
    /// Stundenplan (`None` = fehlt → nur die Tage aus der Datei)
    pub plan: Option<Stundenplan>,
    /// alle Unterrichtsnotizen des Vaults (Metadaten)
    pub unterricht: Vec<NoteMeta>,
    /// alle Wochennotizen (`NoteKind::BerichtsheftWoche`)
    pub wochen: Vec<NoteMeta>,
    pub lernfelder: Vec<LernfeldInfo>,
}

impl Kontext {
    pub fn aus_index(idx: &VaultIndex, plan: Option<Stundenplan>) -> Self {
        Kontext {
            plan,
            unterricht: idx.notes().filter(|m| unterricht::is_unterricht_note(m)).cloned().collect(),
            wochen: idx.notes().filter(|m| m.kind == NoteKind::BerichtsheftWoche).cloned().collect(),
            lernfelder: unterricht::lernfelder(idx),
        }
    }

    /// Unterrichtsnotizen mit Datum in `montag..=montag+6`, nach Datum, Block, Pfad.
    pub fn unterricht_der_woche(&self, montag: NaiveDate) -> Vec<&NoteMeta> {
        let sonntag = montag + Duration::days(6);
        let mut v: Vec<&NoteMeta> =
            self.unterricht.iter().filter(|m| m.datum.is_some_and(|d| d >= montag && d <= sonntag)).collect();
        v.sort_by(|a, b| (a.datum, unterricht::block_nr(a), &a.path).cmp(&(b.datum, unterricht::block_nr(b), &b.path)));
        v
    }

    /// Lernfeld mit dieser Nummer (`lernfeld:` der Lernfeld-Notiz oder `LFnn` im Titel).
    pub fn lernfeld(&self, nr: u32) -> Option<&LernfeldInfo> {
        self.lernfelder.iter().find(|l| lernfeld_nummer(l) == Some(nr))
    }
}

/// Nummer eines Lernfelds: Frontmatter `lernfeld`, sonst aus `LF04`.
pub fn lernfeld_nummer(l: &LernfeldInfo) -> Option<u32> {
    l.nummer.or_else(|| l.kurz.as_deref().and_then(|k| k.strip_prefix("LF")).and_then(|n| n.parse().ok()))
}

/// Der Pfad muss eine Berichtsheft-Woche im Index sein. Liefert die Schreibweise des Index.
pub fn woche_pfad(idx: &VaultIndex, path: &str) -> AppResult<RelPath> {
    let p = path.replace('\\', "/");
    idx.get(&p)
        .or_else(|| idx.notes().find(|m| m.path.eq_ignore_ascii_case(&p)))
        .filter(|m| m.kind == NoteKind::BerichtsheftWoche)
        .map(|m| m.path.clone())
        .ok_or_else(|| AppError::InvalidInput(format!("„{path}“ ist keine Berichtsheft-Woche in „{WOCHEN_DIR}/“.")))
}

// ---------------------------------------------------------------------------------------------
// Wochen und Wochentage
// ---------------------------------------------------------------------------------------------

/// Montag der ISO-Woche.
pub fn montag_von(jahr: i32, kw: u32) -> Option<NaiveDate> {
    NaiveDate::from_isoywd_opt(jahr, kw, Weekday::Mon)
}

/// (ISO-Jahr, KW, Montag) des Tages.
pub fn woche_von(datum: NaiveDate) -> (i32, u32, NaiveDate) {
    let w = datum.iso_week();
    (w.year(), w.week(), datum - Duration::days(i64::from(datum.weekday().num_days_from_monday())))
}

/// `2026-KW46`
pub fn stem(jahr: i32, kw: u32) -> String {
    format!("{jahr}-KW{kw:02}")
}

pub const WOCHENTAGE: [Weekday; 7] =
    [Weekday::Mon, Weekday::Tue, Weekday::Wed, Weekday::Thu, Weekday::Fri, Weekday::Sat, Weekday::Sun];

pub fn wochentag_name(w: Weekday) -> &'static str {
    crate::heute::wochentag_name(w)
}

/// `Mo` … `So`
pub fn wochentag_kurz(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Mo",
        Weekday::Tue => "Di",
        Weekday::Wed => "Mi",
        Weekday::Thu => "Do",
        Weekday::Fri => "Fr",
        Weekday::Sat => "Sa",
        Weekday::Sun => "So",
    }
}

/// `Di 10.11.`
pub fn tag_kurz(d: NaiveDate) -> String {
    format!("{} {}", wochentag_kurz(d.weekday()), d.format("%d.%m."))
}

static WOCHENTAG_WORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(montag|dienstag|mittwoch|donnerstag|freitag|samstag|sonntag)\b").unwrap());

/// Erster Wochentagsname im Text.
pub fn wochentag_im_text(s: &str) -> Option<Weekday> {
    let m = WOCHENTAG_WORT.find(s)?;
    let w = m.as_str().to_lowercase();
    WOCHENTAGE.into_iter().find(|t| wochentag_name(*t).to_lowercase() == w)
}

/// Wochentag, wenn der Text damit **beginnt** (Tages-Überschrift `### Dienstag, 10.11.`).
pub fn wochentag_am_anfang(s: &str) -> Option<Weekday> {
    let t = s.trim_start_matches(|c: char| !c.is_alphanumeric());
    let wort: String = t.chars().take_while(|c| c.is_alphabetic()).collect::<String>().to_lowercase();
    WOCHENTAGE.into_iter().find(|w| wochentag_name(*w).to_lowercase() == wort)
}

/// Tag der Woche mit diesem Wochentag.
pub fn tag_der_woche(montag: NaiveDate, w: Weekday) -> NaiveDate {
    montag + Duration::days(i64::from(w.num_days_from_monday()))
}

// ---------------------------------------------------------------------------------------------
// Zeilen und Überschriften
// ---------------------------------------------------------------------------------------------

/// Zeilen ohne `\n`; `true`, wenn der Text mit `\n` endet.
pub fn zeilen_von(text: &str) -> (Vec<String>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    let endet = text.ends_with('\n');
    let kern = if endet { &text[..text.len() - 1] } else { text };
    (kern.split('\n').map(str::to_string).collect(), endet)
}

pub fn zusammen(zeilen: &[String], endet: bool) -> String {
    let mut s = zeilen.join("\n");
    if endet {
        s.push('\n');
    }
    s
}

static UEBERSCHRIFT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$").unwrap());

/// `## Betrieb ##` → (2, „Betrieb“)
pub(crate) fn ueberschrift(zeile: &str) -> Option<(usize, String)> {
    let c = UEBERSCHRIFT.captures(zeile)?;
    let text = c.get(2).map(|m| m.as_str()).unwrap_or("");
    Some((c[1].len(), text.trim_end_matches('#').trim().to_string()))
}

/// Text einer Überschrift zum Vergleichen: ohne führende Zeichen (Emoji), gefaltet.
pub(crate) fn titel_norm(s: &str) -> String {
    regeln::falten(s.trim_start_matches(|c: char| !c.is_alphanumeric()).trim())
}

/// Eine Überschrift im Body (außerhalb von Codeblöcken); `idx` ist 0-basiert in den Dateizeilen.
#[derive(Debug, Clone)]
pub struct Kopf {
    pub idx: usize,
    pub ebene: usize,
    pub text: String,
}

/// Gliederung einer Datei: Überschriften und welche Zeilen Prosa sind (nicht Frontmatter/Code).
#[derive(Debug, Clone)]
pub struct Gliederung {
    pub koepfe: Vec<Kopf>,
    /// 0-basierte Zeilen außerhalb von Frontmatter und Codeblöcken
    pub prosa: BTreeSet<usize>,
}

impl Gliederung {
    pub fn von(text: &str, zeilen: &[String]) -> Self {
        let note = NoteText::parse(text);
        let off = note.body_line_offset();
        let prosa: BTreeSet<usize> = prose_lines(note.body()).into_iter().map(|l| l.nr - 1 + off).collect();
        let koepfe = prosa
            .iter()
            .filter_map(|i| zeilen.get(*i).and_then(|z| ueberschrift(z)).map(|(ebene, text)| Kopf { idx: *i, ebene, text }))
            .collect();
        Gliederung { koepfe, prosa }
    }

    /// Abschnitt `## <titel>` (Ebene 2, ohne Groß/Klein): (Index der Überschrift, Ende exklusiv) –
    /// endet an der nächsten Überschrift der Ebene ≤ 2 oder am Dateiende.
    pub fn abschnitt(&self, titel: &str, n_zeilen: usize) -> Option<(usize, usize)> {
        let ziel = regeln::falten(titel);
        let k = self.koepfe.iter().find(|k| k.ebene == 2 && titel_norm(&k.text) == ziel)?;
        Some((k.idx, self.ende_nach(k.idx, 2, n_zeilen)))
    }

    /// Wie [`Self::abschnitt`], aber Titel beginnt mit `anfang` und beliebige Ebene.
    pub fn abschnitt_beginnt(&self, anfang: &str, n_zeilen: usize) -> Option<(usize, usize, usize)> {
        let ziel = regeln::falten(anfang);
        let k = self.koepfe.iter().find(|k| titel_norm(&k.text).starts_with(&ziel))?;
        Some((k.idx, self.ende_nach(k.idx, k.ebene, n_zeilen), k.ebene))
    }

    /// Erste Überschrift nach `idx` mit Ebene ≤ `ebene` (sonst `n_zeilen`).
    pub fn ende_nach(&self, idx: usize, ebene: usize, n_zeilen: usize) -> usize {
        self.koepfe.iter().find(|k| k.idx > idx && k.ebene <= ebene).map(|k| k.idx).unwrap_or(n_zeilen)
    }

    /// Nächste Überschrift (beliebige Ebene) nach `idx`.
    pub fn naechster_kopf(&self, idx: usize) -> Option<&Kopf> {
        self.koepfe.iter().find(|k| k.idx > idx)
    }
}

// ---------------------------------------------------------------------------------------------
// Schreiben: Vorschau zuerst (Regel 9)
// ---------------------------------------------------------------------------------------------

pub const MELDUNG_ERST_VORSCHAU: &str =
    "Erst die Vorschau ansehen – ohne Vorschau (Diff) schreibt die App nichts in die Datei (Regel 9).";

pub(crate) fn geaendert_meldung(rel: &str) -> AppError {
    AppError::InvalidInput(format!("„{}“ wurde inzwischen geändert – bitte Vorschau neu laden.", titel_aus_pfad(rel)))
}

/// Gemeinsamer Ablauf aller schreibenden M4-Befehle.
///
/// * `vorschau: true` → frisch lesen, `nachher` berechnen, nichts schreiben; `version` = gelesener Stand
/// * `vorschau: false` ohne `basis` → Fehler (Regel 9)
/// * mit `basis`: frisch lesen; Inhalt ≠ `basis` → Fehler, nichts geschrieben; sonst genau das neu
///   berechnete `nachher` über [`write::save`] mit `basis` schreiben
pub(crate) fn aendern(
    root: &Path,
    rel: &str,
    vorschau: bool,
    basis: Option<&NoteVersion>,
    writes: &SelfWrites,
    p: &RetryPolicy,
    plane: impl FnOnce(&str) -> AppResult<(String, Vec<String>)>,
) -> AppResult<Aenderung> {
    if !vorschau && basis.is_none() {
        return Err(AppError::InvalidInput(MELDUNG_ERST_VORSCHAU.into()));
    }
    let (abs, rel) = guard::resolve_for_write(root, rel)?;
    let (vorher, version) = frisch_lesen(&abs, &rel, "ändern")?;
    if let (false, Some(b)) = (vorschau, basis) {
        if b.sha256 != version.sha256 {
            return Err(geaendert_meldung(&rel));
        }
    }
    let (nachher, warnungen) = plane(&vorher)?;
    let unveraendert = nachher == vorher;
    let mut erg = Aenderung { path: rel.clone(), vorher, nachher, version: version.clone(), unveraendert, warnungen, report: None };
    if vorschau || unveraendert {
        return Ok(erg);
    }
    let basis = basis.unwrap_or(&version);
    match write::save(root, &rel, &erg.nachher, basis, p, writes)? {
        SaveOutcome::Saved { version, warnungen } => {
            erg.warnungen.extend(warnungen.iter().cloned());
            erg.report = Some(bericht(&rel, version, warnungen));
            Ok(erg)
        }
        SaveOutcome::Unchanged(_) => {
            erg.unveraendert = true;
            Ok(erg)
        }
        SaveOutcome::Conflict { .. } => Err(geaendert_meldung(&rel)),
        SaveOutcome::Missing => Err(AppError::NoteNotFound(rel)),
    }
}

/// Liest eine Datei frisch (dekodiert, LF) – für Lesebefehle.
pub(crate) fn lesen(root: &Path, rel: &str) -> AppResult<(String, NoteVersion, RelPath)> {
    let (abs, rel) = guard::resolve_for_read(root, rel)?;
    let (bytes, version) = match write::read_versioned(&abs, &RetryPolicy::short()) {
        Ok(x) => x,
        Err(AppError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(AppError::NoteNotFound(rel)),
        Err(e) => return Err(e),
    };
    let d = write::decode(&bytes);
    Ok((d.text, version, rel))
}

/// Testhilfen: Mini-Vault in einem Temp-Ordner (nie die Fixtures selbst), Texte aus `fixtures/vault`.
#[cfg(test)]
pub(crate) mod testhilfe {
    use super::*;
    use crate::vault::stundenplan;

    pub const STUNDENPLAN: &str = include_str!("../../../fixtures/vault/00 Start/Stundenplan.md");
    pub const VORLAGE: &str = include_str!("../../../fixtures/vault/99 Meta/Templates/Berichtsheft-Woche.md");
    pub const KW41: &str = include_str!("../../../fixtures/vault/04 Betrieb/Wochen/2026-KW41.md");
    pub const KW42: &str = include_str!("../../../fixtures/vault/04 Betrieb/Wochen/2026-KW42.md");
    pub const GEC: &str = include_str!("../../../fixtures/vault/03 Unterricht/GEC 05.10.2026.md");
    pub const GEC2: &str = include_str!("../../../fixtures/vault/03 Unterricht/GEC 2 05.10.2026.md");
    pub const DDV: &str = include_str!("../../../fixtures/vault/03 Unterricht/D-DV 05.10.2026.md");
    pub const WSP: &str = include_str!("../../../fixtures/vault/03 Unterricht/WSP 12.11.2026.md");
    pub const LF04: &str = include_str!("../../../fixtures/vault/02 Lernfelder/LF04 – Werteströme erfassen, auswerten und beurteilen.md");
    pub const BETRIEB: &str = include_str!("../../../fixtures/vault/04 Betrieb/Betrieb.md");

    pub const P_KW41: &str = "04 Betrieb/Wochen/2026-KW41.md";
    pub const P_KW42: &str = "04 Betrieb/Wochen/2026-KW42.md";
    pub const P_LF04: &str = "02 Lernfelder/LF04 – Werteströme erfassen, auswerten und beurteilen.md";

    pub fn plan() -> Stundenplan {
        stundenplan::parse_stundenplan(NoteText::parse(STUNDENPLAN).body())
    }

    /// Die Dateien in einen Temp-Ordner schreiben, Index bauen, Kontext mit Stundenplan.
    pub fn vault(dateien: &[(&str, &str)]) -> (tempfile::TempDir, Kontext) {
        let dir = tempfile::tempdir().unwrap();
        for (rel, text) in dateien {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        let idx = VaultIndex::build(dir.path()).unwrap();
        let ctx = Kontext::aus_index(&idx, Some(plan()));
        (dir, ctx)
    }

    /// Der Fixture-Vault für KW41/KW42 (Wochen, GEC-/D-DV-Notizen, LF04, Betrieb.md).
    pub fn standard() -> (tempfile::TempDir, Kontext) {
        vault(&[
            (P_KW41, KW41),
            (P_KW42, KW42),
            ("03 Unterricht/GEC 05.10.2026.md", GEC),
            ("03 Unterricht/GEC 2 05.10.2026.md", GEC2),
            ("03 Unterricht/D-DV 05.10.2026.md", DDV),
            (P_LF04, LF04),
            (BETRIEB_MD, BETRIEB),
        ])
    }

    pub fn version(root: &Path, rel: &str) -> NoteVersion {
        write::read_versioned(&root.join(rel), &RetryPolicy::none()).unwrap().1
    }

    pub fn bytes(root: &Path, rel: &str) -> Vec<u8> {
        std::fs::read(root.join(rel)).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, t: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, t).unwrap()
    }

    #[test]
    fn wochen_nach_iso() {
        assert_eq!(woche_von(d(2026, 11, 12)), (2026, 46, d(2026, 11, 9)));
        assert_eq!(woche_von(d(2027, 1, 1)), (2026, 53, d(2026, 12, 28)));
        assert_eq!(montag_von(2026, 1), Some(d(2025, 12, 29)));
        assert_eq!(stem(2026, 7), "2026-KW07");
        assert_eq!(tag_kurz(d(2026, 11, 10)), "Di 10.11.");
        assert_eq!(wochentag_am_anfang("Dienstag, 10.11."), Some(Weekday::Tue));
        assert_eq!(wochentag_am_anfang("Berufsschule — Montag"), None);
        assert_eq!(wochentag_im_text("**Berufsschule — Montag, ⟨TT.MM.JJJJ⟩**"), Some(Weekday::Mon));
        assert_eq!(wochentag_im_text("Wochenende"), None);
    }

    #[test]
    fn gliederung_ohne_code_und_frontmatter() {
        let text = "---\nx: 1\n---\n# T\n## Betrieb\n### Dienstag\n```\n## kein Kopf\n```\n## Berufsschule\n";
        let (z, _) = zeilen_von(text);
        let g = Gliederung::von(text, &z);
        let k: Vec<(usize, usize)> = g.koepfe.iter().map(|k| (k.idx, k.ebene)).collect();
        assert_eq!(k, vec![(3, 1), (4, 2), (5, 3), (9, 2)]);
        assert_eq!(g.abschnitt("betrieb", z.len()), Some((4, 9)));
        assert_eq!(g.abschnitt("Berufsschule", z.len()), Some((9, 10)));
    }
}

/// Regel 9: jede Änderung an einer bestehenden Datei erst als Vorschau (Diff), geschrieben wird nur
/// mit der `basis` aus dieser Vorschau – und nur, wenn die Datei seitdem gleich geblieben ist.
#[cfg(test)]
mod regel9 {
    use super::testhilfe::{bytes, standard, version, BETRIEB, KW41, P_KW41};
    use super::*;
    use crate::model::{
        BetriebFortschreiben, LernfelderSchreiben, NachweisSchreiben, NachweisTag, NeueLernfeldZeile, SchultageUebernehmen,
        TagAbschnitt,
    };

    fn sw() -> SelfWrites {
        SelfWrites::new()
    }

    fn betrieb_req(vorschau: bool, basis: Option<NoteVersion>) -> BetriebFortschreiben {
        BetriebFortschreiben {
            woche_path: P_KW41.into(),
            zeilen: vec![vec!["".into(), "Artikel im Shop gepflegt".into(), "LF04".into()]],
            vorschau,
            basis,
        }
    }

    fn nachweis_req(vorschau: bool, basis: Option<NoteVersion>) -> NachweisSchreiben {
        let t = NachweisTag {
            datum: NaiveDate::from_ymd_opt(2026, 10, 6).unwrap(),
            abschnitt: TagAbschnitt::Betrieb,
            wochentag: "Dienstag".into(),
            ueberschrift: "Tätigkeiten im Betrieb".into(),
            absaetze: vec!["Artikel im Shop gepflegt.".into()],
            luecken: vec!["Einschulung".into()],
            vorhanden: crate::model::NachweisStand::Vorhanden,
            uebernehmen: true,
            ki: false,
        };
        NachweisSchreiben { path: P_KW41.into(), tage: vec![t], vorschau, basis }
    }

    fn lernfelder_req(vorschau: bool, basis: Option<NoteVersion>) -> LernfelderSchreiben {
        LernfelderSchreiben {
            path: P_KW41.into(),
            zeilen: vec![NeueLernfeldZeile { taetigkeit: "Preise kalkuliert".into(), lernfelder: vec![4] }],
            vorschau,
            basis,
        }
    }

    #[test]
    fn regel9_ohne_basis_wird_nichts_geschrieben() {
        let (dir, ctx) = standard();
        let root = dir.path();
        let p = RetryPolicy::none();
        let ohne = [
            betrieb::fortschreiben(root, &betrieb_req(false, None), &sw(), &p),
            nachweis::schreiben(root, &ctx, &nachweis_req(false, None), &sw(), &p),
            lernfelder::schreiben(root, &ctx, &lernfelder_req(false, None), &sw(), &p),
            anlegen::schultage(root, &ctx, &SchultageUebernehmen { path: P_KW41.into(), vorschau: false, basis: None }, &sw(), &p),
        ];
        for r in ohne {
            assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m == MELDUNG_ERST_VORSCHAU), "{r:?}");
        }
        assert_eq!(bytes(root, P_KW41), KW41.as_bytes());
        assert_eq!(bytes(root, BETRIEB_MD), BETRIEB.as_bytes());
    }

    #[test]
    fn regel9_vorschau_zeigt_den_diff_und_schreibt_nichts_dann_mit_basis() {
        let (dir, ctx) = standard();
        let root = dir.path();
        let p = RetryPolicy::none();
        let v = betrieb::fortschreiben(root, &betrieb_req(true, None), &sw(), &p).unwrap();
        assert_eq!(bytes(root, BETRIEB_MD), BETRIEB.as_bytes(), "Vorschau schreibt nichts");
        assert_eq!(v.vorher, BETRIEB);
        assert!(v.nachher.ends_with("|  | Artikel im Shop gepflegt | LF04 |\n") && v.report.is_none() && !v.unveraendert);
        let w = betrieb::fortschreiben(root, &betrieb_req(false, Some(v.version.clone())), &sw(), &p).unwrap();
        assert_eq!(bytes(root, BETRIEB_MD), v.nachher.as_bytes(), "genau das Gezeigte");
        assert!(w.report.is_some());
        // Nachweis und Lernfelder: Vorschau, dann Schreiben mit Basis
        for vorschau_und_schreiben in [0, 1] {
            let (vor, nach): (Aenderung, Aenderung) = if vorschau_und_schreiben == 0 {
                let v = nachweis::schreiben(root, &ctx, &nachweis_req(true, None), &sw(), &p).unwrap();
                let b = Some(v.version.clone());
                (v, nachweis::schreiben(root, &ctx, &nachweis_req(false, b), &sw(), &p).unwrap())
            } else {
                let v = lernfelder::schreiben(root, &ctx, &lernfelder_req(true, None), &sw(), &p).unwrap();
                let b = Some(v.version.clone());
                (v, lernfelder::schreiben(root, &ctx, &lernfelder_req(false, b), &sw(), &p).unwrap())
            };
            assert_eq!(nach.nachher, vor.nachher);
            assert_eq!(String::from_utf8(bytes(root, P_KW41)).unwrap(), vor.nachher);
        }
    }

    #[test]
    fn regel9_datei_zwischen_vorschau_und_schreiben_geaendert() {
        let (dir, ctx) = standard();
        let root = dir.path();
        let p = RetryPolicy::none();
        let vb = betrieb::fortschreiben(root, &betrieb_req(true, None), &sw(), &p).unwrap();
        let vn = nachweis::schreiben(root, &ctx, &nachweis_req(true, None), &sw(), &p).unwrap();
        let vl = lernfelder::schreiben(root, &ctx, &lernfelder_req(true, None), &sw(), &p).unwrap();
        let vs = anlegen::schultage(root, &ctx, &SchultageUebernehmen { path: P_KW41.into(), vorschau: true, basis: None }, &sw(), &p).unwrap();
        let obsidian_b = format!("{BETRIEB}\nIn Obsidian ergänzt.\n");
        let obsidian_w = format!("{KW41}\nIn Obsidian ergänzt.\n");
        std::fs::write(root.join(BETRIEB_MD), &obsidian_b).unwrap();
        std::fs::write(root.join(P_KW41), &obsidian_w).unwrap();
        let rs = [
            betrieb::fortschreiben(root, &betrieb_req(false, Some(vb.version)), &sw(), &p),
            nachweis::schreiben(root, &ctx, &nachweis_req(false, Some(vn.version)), &sw(), &p),
            lernfelder::schreiben(root, &ctx, &lernfelder_req(false, Some(vl.version)), &sw(), &p),
            anlegen::schultage(root, &ctx, &SchultageUebernehmen { path: P_KW41.into(), vorschau: false, basis: Some(vs.version) }, &sw(), &p),
        ];
        for r in rs {
            assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("inzwischen geändert – bitte Vorschau neu laden")), "{r:?}");
        }
        assert_eq!(bytes(root, BETRIEB_MD), obsidian_b.as_bytes());
        assert_eq!(bytes(root, P_KW41), obsidian_w.as_bytes());
        let _ = version(root, P_KW41);
    }
}
