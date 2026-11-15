//! M3: Fragen und Lücken (SPEC §6.5).
//!
//! * Alle `#frage`-Zeilen mit Quelle und Datum; alle `#nochmal`-Zeilen (und Notizen mit Tag
//!   `nochmal` in der Frontmatter) gruppiert nach Fach – Grundlage der Prüfungsvorbereitung.
//!   Quelle sind dieselben Notizen wie bei den Aufgaben ([`crate::aufgaben::zaehlt_als_quelle`]).
//! * „Geklärt“ ([`klaeren`]) streicht in **dieser einen Zeile** jedes `#frage` zu `~~#frage~~` durch
//!   (Schreibweise bleibt, `#fragebogen` bleibt). Im Modus „Abschnitt“ wandert die Zeile zusätzlich
//!   (ohne Einrückung) ans Ende des Abschnitts „Geklärt“; fehlt er, wird `## Geklärt` am Dateiende
//!   angelegt. Sonst bleibt die Datei byte-gleich (BOM, CRLF). Keine KI.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::aufgaben;
use crate::error::{AppError, AppResult};
use crate::guard;
use crate::index::VaultIndex;
use crate::model::{FrageEintrag, FrageGeklaert, FrageKlaeren, FragenUebersicht, GeklaertModus, NochmalGruppe, NoteMeta};
use crate::vault::frontmatter::{tags_from_field, NoteText};
use crate::vault::tags::{tag_passt, tag_tokens};
use crate::vault::{mask_inline_code, prose_lines};
use crate::write::{self, RetryPolicy, SaveOutcome, SelfWrites};

pub const TAG_FRAGE: &str = "frage";
pub const TAG_NOCHMAL: &str = "nochmal";
/// Überschrift des Abschnitts für geklärte Fragen (beliebige Ebene, ohne Groß/Klein)
pub const ABSCHNITT_GEKLAERT: &str = "Geklärt";

// ---------------------------------------------------------------------------------------------
// Anzeigetext
// ---------------------------------------------------------------------------------------------

static LISTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]|\d+[.)])[ \t]+(?:\[.\](?:[ \t]+|$))?").unwrap());
static UEBERSCHRIFT_ZEICHEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#{1,6}[ \t]+").unwrap());
static GESTRICHEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)~~#(?:frage|nochmal)(?:[/\-][\p{L}\p{N}_/\-]*)?~~").unwrap());

/// Getrimmt, ohne BOM (die erste Zeile einer Datei ohne Frontmatter trägt es im Index noch).
fn norm(s: &str) -> &str {
    s.trim().trim_start_matches('\u{feff}').trim()
}

/// Anzeigetext einer Zeile: ohne Zitat-, Überschrift- und Listenzeichen, ohne Checkbox, ohne die
/// Tags `#frage`/`#nochmal` (auch durchgestrichene), Leerraum zusammengefasst.
pub fn text_der_zeile(zeile: &str) -> String {
    let mut s = norm(zeile);
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    if let Some(m) = UEBERSCHRIFT_ZEICHEN.find(s) {
        s = &s[m.end()..];
    }
    if let Some(m) = LISTE.find(s) {
        s = &s[m.end()..];
    }
    // gleiche Tag-Erkennung wie `vault/tags.rs`; Inline-Code bleibt, wie er ist
    let masked = mask_inline_code(s);
    let mut out = String::with_capacity(s.len());
    let mut pos = 0;
    for t in tag_tokens(&masked) {
        if tag_passt(&t.tag, TAG_FRAGE) || tag_passt(&t.tag, TAG_NOCHMAL) {
            out.push_str(&s[pos..t.range.start]);
            out.push(' ');
            pos = t.range.end;
        }
    }
    out.push_str(&s[pos..]);
    GESTRICHEN.replace_all(&out, " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------------------------
// Übersicht
// ---------------------------------------------------------------------------------------------

fn eintrag(m: &NoteMeta, line: Option<usize>, text: String, zeile: String) -> FrageEintrag {
    FrageEintrag {
        path: m.path.clone(),
        title: m.title.clone(),
        kind: m.kind,
        fach: m.fach.clone(),
        datum: m.datum,
        line,
        text,
        zeile,
        privat: m.private,
    }
}

/// Nur die `tags` der Frontmatter (nicht die Inline-Tags).
fn frontmatter_tags(m: &NoteMeta) -> Vec<String> {
    m.fields
        .iter()
        .find(|f| f.key.eq_ignore_ascii_case("tags"))
        .map(|f| tags_from_field(&f.value))
        .unwrap_or_default()
}

/// Neueste zuerst (ohne Datum zuletzt), dann Pfad und Zeile.
fn sortieren(v: &mut [FrageEintrag]) {
    v.sort_by(|a, b| {
        (a.datum.is_none(), Reverse(a.datum), &a.path, a.line).cmp(&(b.datum.is_none(), Reverse(b.datum), &b.path, b.line))
    });
}

/// Nach Fach (ohne Groß/Klein, alphabetisch, ohne Fach zuletzt); die Reihenfolge darin bleibt.
fn gruppieren(eintraege: Vec<FrageEintrag>) -> Vec<NochmalGruppe> {
    let schluessel = |f: &Option<String>| f.as_deref().map(str::to_lowercase);
    let mut gruppen: Vec<NochmalGruppe> = Vec::new();
    for e in eintraege {
        match gruppen.iter_mut().find(|g| schluessel(&g.fach) == schluessel(&e.fach)) {
            Some(g) => g.eintraege.push(e),
            None => gruppen.push(NochmalGruppe { fach: e.fach.clone(), eintraege: vec![e] }),
        }
    }
    gruppen.sort_by_cached_key(|g| (g.fach.is_none(), schluessel(&g.fach)));
    gruppen
}

/// Offene Fragen und „nochmal ansehen“. Je Zeile ein Eintrag; leere Platzhalter (`- #frage `)
/// fallen weg; eine Zeile mit beiden Tags steht in beiden Listen.
pub fn uebersicht(idx: &VaultIndex, modus: GeklaertModus) -> FragenUebersicht {
    let mut fragen = Vec::new();
    let mut nochmal = Vec::new();
    for m in idx.notes().filter(|m| aufgaben::zaehlt_als_quelle(m)) {
        let mut frage_zeilen = BTreeSet::new();
        let mut nochmal_zeilen = BTreeSet::new();
        for t in &m.inline_tags {
            let (ist_frage, ist_nochmal) = (tag_passt(&t.tag, TAG_FRAGE), tag_passt(&t.tag, TAG_NOCHMAL));
            if !ist_frage && !ist_nochmal {
                continue;
            }
            let text = text_der_zeile(&t.context);
            if text.is_empty() {
                continue; // Vorlagen-Platzhalter
            }
            if ist_frage && frage_zeilen.insert(t.line) {
                fragen.push(eintrag(m, Some(t.line), text.clone(), t.context.clone()));
            }
            if ist_nochmal && nochmal_zeilen.insert(t.line) {
                nochmal.push(eintrag(m, Some(t.line), text, t.context.clone()));
            }
        }
        // Dataview `FROM #nochmal` findet auch Notizen mit dem Tag in der Frontmatter
        if frontmatter_tags(m).iter().any(|t| tag_passt(t, TAG_NOCHMAL)) {
            nochmal.push(eintrag(m, None, m.title.clone(), String::new()));
        }
    }
    sortieren(&mut fragen);
    sortieren(&mut nochmal);
    FragenUebersicht { fragen, nochmal: gruppieren(nochmal), modus }
}

// ---------------------------------------------------------------------------------------------
// Geklärt
// ---------------------------------------------------------------------------------------------

/// Jedes aktive `#frage` / `#frage/…` der Zeile wird zu `~~#frage~~` (Schreibweise bleibt). `None`,
/// wenn die Zeile keins trägt. `#fragebogen` und `#frage` in Inline-Code bleiben unberührt.
pub fn durchstreichen(zeile: &str) -> Option<String> {
    let masked = mask_inline_code(zeile);
    let treffer: Vec<_> = tag_tokens(&masked).into_iter().filter(|t| tag_passt(&t.tag, TAG_FRAGE)).collect();
    if treffer.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(zeile.len() + 4 * treffer.len());
    let mut pos = 0;
    for t in treffer {
        out.push_str(&zeile[pos..t.range.start]);
        out.push_str("~~");
        out.push_str(&zeile[t.range.clone()]);
        out.push_str("~~");
        pos = t.range.end;
    }
    out.push_str(&zeile[pos..]);
    Some(out)
}

/// Was „Geklärt“ tun würde – reine Berechnung auf dem dekodierten Text (LF).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KlaerPlan {
    pub vorher: String,
    pub nachher: String,
    pub abschnitt_neu: bool,
    pub verschoben: bool,
    /// Zeile der geklärten Frage im neuen Text
    pub line: usize,
    pub neu: String,
}

/// Zeilen ohne `\n`; `true`, wenn der Text mit `\n` endet.
fn zeilen_von(text: &str) -> (Vec<String>, bool) {
    if text.is_empty() {
        return (Vec::new(), false);
    }
    let endet = text.ends_with('\n');
    let kern = if endet { &text[..text.len() - 1] } else { text };
    (kern.split('\n').map(str::to_string).collect(), endet)
}

fn zusammen(zeilen: &[String], endet: bool) -> String {
    let mut s = zeilen.join("\n");
    if endet {
        s.push('\n');
    }
    s
}

/// Zeilen nach der Frontmatter und außerhalb von Codeblöcken: Dateizeile (1-basiert) → maskierte Zeile.
fn prosa(text: &str) -> BTreeMap<usize, String> {
    let note = NoteText::parse(text);
    let off = note.body_line_offset();
    prose_lines(note.body()).into_iter().map(|l| (l.nr + off, l.masked)).collect()
}

static UEBERSCHRIFT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?[ \t]*$").unwrap());

/// `## Geklärt ##` → (2, „Geklärt“)
fn ueberschrift(zeile: &str) -> Option<(usize, String)> {
    let c = UEBERSCHRIFT.captures(zeile)?;
    let text = c.get(2).map(|m| m.as_str()).unwrap_or("");
    Some((c[1].len(), text.trim_end_matches('#').trim().to_string()))
}

/// Einfügestelle (0-basiert) hinter der letzten nicht-leeren Zeile des Abschnitts „Geklärt“. Der
/// Abschnitt endet an der nächsten Überschrift gleicher oder höherer Ebene oder am Dateiende.
fn einfuegestelle(zeilen: &[String], endet: bool) -> Option<usize> {
    let prosa = prosa(&zusammen(zeilen, endet));
    let koepfe: Vec<(usize, usize, String)> = prosa
        .keys()
        .filter_map(|nr| zeilen.get(nr - 1).and_then(|z| ueberschrift(z)).map(|(e, t)| (*nr, e, t)))
        .collect();
    let ziel = ABSCHNITT_GEKLAERT.to_lowercase();
    let (kopf, ebene) = koepfe.iter().find(|(_, _, t)| t.to_lowercase() == ziel).map(|(nr, e, _)| (*nr, *e))?;
    let ende = koepfe.iter().find(|(nr, e, _)| *nr > kopf && *e <= ebene).map(|(nr, _, _)| *nr).unwrap_or(zeilen.len() + 1);
    let letzte = (kopf + 1..ende).rev().find(|nr| !zeilen[nr - 1].trim().is_empty()).unwrap_or(kopf);
    Some(letzte)
}

fn schon_geklaert(zeile: &str) -> AppError {
    AppError::InvalidInput(format!("Die Frage „{}“ ist schon geklärt.", text_der_zeile(zeile)))
}

/// Zeile an `line` (getrimmt gleich `zeile`) nehmen, sonst die eine andere Zeile außerhalb von
/// Codeblöcken mit gleichem Inhalt. Sie muss ein aktives `#frage` tragen.
pub fn plane_klaeren(text: &str, rel: &str, line: usize, zeile: &str, modus: GeklaertModus) -> AppResult<KlaerPlan> {
    let titel = aufgaben::titel_aus_pfad(rel);
    let gesucht = norm(zeile);
    let nicht_mehr_so = || {
        AppError::InvalidInput(format!("Die Zeile „{gesucht}“ steht nicht mehr so in „{titel}“ – bitte Liste neu laden."))
    };
    if gesucht.is_empty() {
        return Err(nicht_mehr_so());
    }
    let (mut zeilen, mut endet) = zeilen_von(text);
    let prosa = prosa(text);
    let passt = |nr: usize| nr >= 1 && zeilen.get(nr - 1).is_some_and(|z| norm(z) == gesucht);
    let ziel = if passt(line) {
        line
    } else {
        match prosa.keys().copied().filter(|nr| passt(*nr)).collect::<Vec<_>>().as_slice() {
            [nr] => *nr,
            _ => {
                // steht die Zeile schon durchgestrichen da?
                let geklaert = durchstreichen(gesucht);
                if geklaert.is_some_and(|g| zeilen.iter().any(|z| norm(z) == norm(&g))) {
                    return Err(schon_geklaert(gesucht));
                }
                return Err(nicht_mehr_so());
            }
        }
    };
    if !prosa.contains_key(&ziel) {
        return Err(AppError::InvalidInput(format!(
            "Die Zeile „{gesucht}“ steht in einem Codeblock oder in der Frontmatter – dort ist #frage kein Tag."
        )));
    }
    let roh = zeilen[ziel - 1].clone();
    let gestrichen = durchstreichen(&roh).ok_or_else(|| schon_geklaert(gesucht))?;
    let vorher = norm(&roh).to_string();
    match modus {
        GeklaertModus::Durchstreichen => {
            let nachher = norm(&gestrichen).to_string();
            zeilen[ziel - 1] = gestrichen;
            Ok(KlaerPlan { vorher, nachher, abschnitt_neu: false, verschoben: false, line: ziel, neu: zusammen(&zeilen, endet) })
        }
        GeklaertModus::Abschnitt => {
            let bewegt = gestrichen.trim_start().to_string();
            zeilen.remove(ziel - 1);
            let (line, abschnitt_neu) = match einfuegestelle(&zeilen, endet) {
                Some(i) => {
                    zeilen.insert(i, bewegt.clone());
                    (i + 1, false)
                }
                None => {
                    // am Dateiende: Leerzeile, `## Geklärt`, die Zeile – die Datei endet danach mit `\n`
                    if zeilen.last().is_some_and(|z| !z.trim().is_empty()) {
                        zeilen.push(String::new());
                    }
                    zeilen.push(format!("## {ABSCHNITT_GEKLAERT}"));
                    zeilen.push(bewegt.clone());
                    endet = true;
                    (zeilen.len(), true)
                }
            };
            Ok(KlaerPlan {
                vorher,
                nachher: norm(&bewegt).to_string(),
                abschnitt_neu,
                verschoben: true,
                line,
                neu: zusammen(&zeilen, endet),
            })
        }
    }
}

/// Frage als geklärt markieren. `vorschau` schreibt nichts.
pub fn klaeren(
    root: &Path,
    req: &FrageKlaeren,
    modus: GeklaertModus,
    writes: &SelfWrites,
    p: &RetryPolicy,
) -> AppResult<FrageGeklaert> {
    let (abs, rel) = guard::resolve_for_write(root, &req.path)?;
    if !aufgaben::pfad_zaehlt_als_quelle(&rel) {
        return Err(AppError::InvalidInput(format!(
            "„{rel}“ zählt nicht als Quelle für Fragen („99 Meta“ oder Dateiname mit „!“) – dort ändert die App nichts."
        )));
    }
    for _ in 0..aufgaben::VERSUCHE {
        let (text, version) = aufgaben::frisch_lesen(&abs, &rel, "klären")?;
        let plan = plane_klaeren(&text, &rel, req.line, &req.zeile, modus)?;
        let mut erg = FrageGeklaert {
            vorher: plan.vorher,
            nachher: plan.nachher,
            abschnitt_neu: plan.abschnitt_neu,
            verschoben: plan.verschoben,
            line: plan.line,
            report: None,
        };
        if req.vorschau {
            return Ok(erg);
        }
        match write::save(root, &rel, &plan.neu, &version, p, writes)? {
            SaveOutcome::Saved { version, warnungen } => {
                erg.report = Some(aufgaben::bericht(&rel, version, warnungen));
                return Ok(erg);
            }
            SaveOutcome::Unchanged(_) => return Ok(erg),
            // Obsidian hat dazwischen gespeichert → frisch lesen und neu planen
            SaveOutcome::Conflict { .. } => continue,
            SaveOutcome::Missing => return Err(AppError::NoteNotFound(rel)),
        }
    }
    Err(aufgaben::zu_oft_geaendert(&rel, "Frage nicht geklärt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NoteKind;
    use crate::vault::scan;
    use chrono::NaiveDate;
    use std::fs;

    /// (Fach, [(Text bzw. Titel, Zeile)]) je Nochmal-Gruppe
    type Gruppe<'a> = (Option<&'a str>, Vec<(&'a str, Option<usize>)>);

    fn index_mit(notizen: &[(&str, &str)]) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        let mut idx = VaultIndex::build(dir.path()).unwrap();
        for (rel, text) in notizen {
            idx.insert_meta(scan::analyze(rel, &NoteText::parse(text), 0, text.len() as u64));
        }
        (dir, idx)
    }

    #[test]
    fn anzeigetext() {
        for (zeile, text) in [
            ("- Warum Skonto? #frage", "Warum Skonto?"),
            ("- #frage ", ""),
            ("- #nochmal", ""),
            ("- [ ] Rechnen üben #nochmal", "Rechnen üben"),
            ("> - Im Callout #Frage/offen bitte", "Im Callout bitte"),
            ("#frage Was ist   Skonto?", "Was ist Skonto?"),
            ("Was ist `#frage` im Code? #frage", "Was ist `#frage` im Code?"),
            ("- ~~#frage~~ geklärt, aber #nochmal", "geklärt, aber"),
            ("## Thema #frage", "Thema"),
            ("1. Was steht im #fragebogen? #frage", "Was steht im #fragebogen?"),
            ("\u{feff}- Erste Zeile #frage", "Erste Zeile"),
        ] {
            assert_eq!(text_der_zeile(zeile), text, "{zeile:?}");
        }
    }

    #[test]
    fn fragen_und_nochmal_mit_platzhaltern_und_gruppen() {
        let gec = "---\ndatum: 2026-10-05\nfach: GEC\n---\n\
                   - Warum Skonto? #frage\n\
                   - #frage \n\
                   - #nochmal \n\
                   - Rückwärtskalkulation #nochmal\n\
                   - Beides #frage #nochmal\n\
                   - Doppelt #frage und #Frage\n\
                   - Fragebogen #fragebogen\n\
                   - ~~#frage~~ erledigt\n\
                   ```\n#frage im Code\n```\n";
        let (_d, idx) = index_mit(&[
            ("03 Unterricht/GEC 05.10.2026.md", gec),
            ("03 Unterricht/WSP 10.09.2026.md", "---\ndatum: 2026-09-10\nfach: WSP\n---\nProbezeit? #Frage/recht\n"),
            ("10 Privat/Noten/Noten.md", "#frage Notenschnitt?\n"),
            ("99 Meta/Templates/Unterrichtsnotiz.md", "- Vorlage #frage\n- Vorlage #nochmal\n"),
            ("00 Start/!Entwurf.md", "- Entwurf #frage\n"),
            ("07 Material/Aufgabensammlung/Skonto.md", "---\ntags: [aufgabe, nochmal]\nfach: gec\n---\n# Skonto\n"),
            ("06 Glossar/Marge.md", "#nochmal Marge vs. Spanne\n"),
        ]);
        let u = uebersicht(&idx, GeklaertModus::Abschnitt);
        assert_eq!(u.modus, GeklaertModus::Abschnitt);
        let f: Vec<(&str, Option<usize>, &str)> = u.fragen.iter().map(|e| (e.title.as_str(), e.line, e.text.as_str())).collect();
        assert_eq!(
            f,
            vec![
                ("GEC 05.10.2026", Some(5), "Warum Skonto?"),
                ("GEC 05.10.2026", Some(9), "Beides"),
                ("GEC 05.10.2026", Some(10), "Doppelt und"),
                ("WSP 10.09.2026", Some(5), "Probezeit?"),
                ("Noten", Some(1), "Notenschnitt?"),
            ]
        );
        assert_eq!(u.fragen[0].zeile, "- Warum Skonto? #frage");
        assert_eq!(u.fragen[0].datum, NaiveDate::from_ymd_opt(2026, 10, 5));
        assert_eq!((u.fragen[0].kind, u.fragen[0].fach.as_deref()), (NoteKind::Unterricht, Some("GEC")));
        assert!(u.fragen[4].privat && !u.fragen[0].privat);
        // nochmal: GEC und gec in einer Gruppe, ohne Fach zuletzt
        let g: Vec<Gruppe> = u
            .nochmal
            .iter()
            .map(|g| (g.fach.as_deref(), g.eintraege.iter().map(|e| (e.text.as_str(), e.line)).collect()))
            .collect();
        assert_eq!(
            g,
            vec![
                (Some("GEC"), vec![("Rückwärtskalkulation", Some(8)), ("Beides", Some(9)), ("Skonto", None)]),
                (None, vec![("Marge vs. Spanne", Some(1))]),
            ]
        );
        assert_eq!(u.nochmal[0].eintraege[2].zeile, "");
    }

    #[test]
    fn durchstreichen_einer_zeile() {
        assert_eq!(durchstreichen("- Gilt das? #Frage").as_deref(), Some("- Gilt das? ~~#Frage~~"));
        assert_eq!(durchstreichen("- #fragebogen und #frage").as_deref(), Some("- #fragebogen und ~~#frage~~"));
        assert_eq!(
            durchstreichen("#frage a #frage/offen b `#frage` (#frage)").as_deref(),
            Some("~~#frage~~ a ~~#frage/offen~~ b `#frage` (~~#frage~~)")
        );
        assert_eq!(durchstreichen("- nur #fragebogen"), None);
        assert_eq!(durchstreichen("- schon ~~#frage~~"), None);
        // durchgestrichen zählt nicht mehr als Tag (vault/tags.rs)
        let neu = durchstreichen("- Was? #frage #nochmal").unwrap();
        let tags: Vec<String> = crate::vault::tags::parse_inline_tags(&neu, 0).into_iter().map(|t| t.tag).collect();
        assert_eq!(tags, vec!["nochmal"]);
    }

    const REL: &str = "03 Unterricht/GEC 05.10.2026.md";

    #[test]
    fn durchstreichen_in_der_datei_auch_mit_crlf_und_bom() {
        let lf = "---\nfach: GEC\n---\n## Offene Fragen\n- Warum Skonto? #frage\n- Und das? #frage\n";
        let crlf = format!("\u{FEFF}{}", lf.replace('\n', "\r\n"));
        for vorher in [lf.to_string(), crlf] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            fs::create_dir_all(root.join("03 Unterricht")).unwrap();
            fs::write(root.join(REL), &vorher).unwrap();
            let mut req = FrageKlaeren { path: REL.into(), line: 5, zeile: "- Warum Skonto? #frage".into(), vorschau: true };
            let w = SelfWrites::new();
            let v = klaeren(root, &req, GeklaertModus::Durchstreichen, &w, &RetryPolicy::none()).unwrap();
            assert_eq!((v.vorher.as_str(), v.nachher.as_str()), ("- Warum Skonto? #frage", "- Warum Skonto? ~~#frage~~"));
            assert!(v.report.is_none() && !v.verschoben && !v.abschnitt_neu);
            assert_eq!(fs::read(root.join(REL)).unwrap(), vorher.as_bytes(), "Vorschau schreibt nichts");
            req.vorschau = false;
            let r = klaeren(root, &req, GeklaertModus::Durchstreichen, &w, &RetryPolicy::none()).unwrap();
            assert!(r.report.is_some());
            assert_eq!(r.line, 5);
            let nachher = fs::read(root.join(REL)).unwrap();
            assert_eq!(nachher, vorher.replacen("#frage", "~~#frage~~", 1).as_bytes());
            // zweites Mal: schon geklärt, nichts geschrieben
            let r = klaeren(root, &req, GeklaertModus::Durchstreichen, &w, &RetryPolicy::none());
            assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m == "Die Frage „Warum Skonto?“ ist schon geklärt."), "{r:?}");
            assert_eq!(fs::read(root.join(REL)).unwrap(), nachher);
        }
    }

    fn plan(text: &str, line: usize, zeile: &str, modus: GeklaertModus) -> AppResult<KlaerPlan> {
        plane_klaeren(text, REL, line, zeile, modus)
    }

    #[test]
    fn verschieben_in_vorhandenen_abschnitt() {
        let text = "# T\n## Offene Fragen\n- Erste? #frage\n\t- Zweite? #frage\n\n## Geklärt\n- Alt ~~#frage~~\n\n\
                    ### Unterpunkt\n```\n## kein Ende im Codeblock\n```\n- im Unterabschnitt\n\n## Verknüpft\n- [[X]]\n";
        let p = plan(text, 4, "- Zweite? #frage", GeklaertModus::Abschnitt).unwrap();
        assert_eq!(
            p.neu,
            "# T\n## Offene Fragen\n- Erste? #frage\n\n## Geklärt\n- Alt ~~#frage~~\n\n\
             ### Unterpunkt\n```\n## kein Ende im Codeblock\n```\n- im Unterabschnitt\n- Zweite? ~~#frage~~\n\n## Verknüpft\n- [[X]]\n"
        );
        assert_eq!((p.line, p.verschoben, p.abschnitt_neu), (13, true, false));
        assert_eq!((p.vorher.as_str(), p.nachher.as_str()), ("- Zweite? #frage", "- Zweite? ~~#frage~~"));
        // leerer Abschnitt am Dateiende ohne Zeilenumbruch, andere Ebene und Schreibweise
        let p = plan("a #frage\n#### geklärt", 1, "a #frage", GeklaertModus::Abschnitt).unwrap();
        assert_eq!((p.neu.as_str(), p.line), ("#### geklärt\na ~~#frage~~", 2));
    }

    #[test]
    fn verschieben_in_neuen_abschnitt() {
        let text = "---\ntags: [x]\n---\n# T\n- Frage? #frage\n```\n## Geklärt\n```\n- Rest";
        let p = plan(text, 5, "- Frage? #frage", GeklaertModus::Abschnitt).unwrap();
        assert_eq!(p.neu, "---\ntags: [x]\n---\n# T\n```\n## Geklärt\n```\n- Rest\n\n## Geklärt\n- Frage? ~~#frage~~\n");
        assert_eq!((p.line, p.abschnitt_neu, p.verschoben), (11, true, true));
        // endet schon mit Leerzeile: keine zweite
        let p = plan("# T\n- F? #frage\n\n", 2, "- F? #frage", GeklaertModus::Abschnitt).unwrap();
        assert_eq!(p.neu, "# T\n\n## Geklärt\n- F? ~~#frage~~\n");
        // nur die Frage in der Datei
        let p = plan("x #frage", 1, "x #frage", GeklaertModus::Abschnitt).unwrap();
        assert_eq!((p.neu.as_str(), p.line), ("## Geklärt\nx ~~#frage~~\n", 2));
    }

    #[test]
    fn zeile_wiederfinden_und_ablehnen() {
        let text = "---\nfach: GEC\n---\nneu\nneu\n- Warum? #frage\n- Doppelt #frage\n- Doppelt #frage\n```\n- Code #frage\n```\n- Bogen #fragebogen\n";
        // verschoben: per Inhalt gefunden
        let p = plan(text, 4, "- Warum? #frage", GeklaertModus::Durchstreichen).unwrap();
        assert_eq!(p.line, 6);
        assert_eq!(p.neu, text.replacen("- Warum? #frage", "- Warum? ~~#frage~~", 1));
        // mehrdeutig
        let r = plan(text, 4, "- Doppelt #frage", GeklaertModus::Durchstreichen);
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("steht nicht mehr so")), "{r:?}");
        assert!(plan(text, 8, "- Doppelt #frage", GeklaertModus::Durchstreichen).is_ok(), "an der Zeile eindeutig");
        // Codeblock
        let r = plan(text, 10, "- Code #frage", GeklaertModus::Durchstreichen);
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("Codeblock")), "{r:?}");
        assert!(plan(text, 4, "- Code #frage", GeklaertModus::Durchstreichen).is_err(), "im Codeblock nicht gesucht");
        // kein aktives #frage
        let r = plan(text, 12, "- Bogen #fragebogen", GeklaertModus::Durchstreichen);
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("schon geklärt")), "{r:?}");
        // Frontmatter
        let r = plan(text, 2, "fach: GEC", GeklaertModus::Durchstreichen);
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        // leer
        assert!(plan(text, 4, "  ", GeklaertModus::Durchstreichen).is_err());
        // schon geklärt und verschoben
        let r = plan("x\n- Alt ~~#frage~~\n", 1, "- Alt #frage", GeklaertModus::Durchstreichen);
        assert!(matches!(&r, Err(AppError::InvalidInput(m)) if m.contains("schon geklärt")), "{r:?}");
    }

    #[test]
    fn klaeren_lehnt_vorlagen_ab() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let rel = "99 Meta/Templates/Unterrichtsnotiz.md";
        fs::create_dir_all(root.join("99 Meta/Templates")).unwrap();
        fs::write(root.join(rel), "- Vorlage #frage\n").unwrap();
        let req = FrageKlaeren { path: rel.into(), line: 1, zeile: "- Vorlage #frage".into(), vorschau: false };
        let r = klaeren(root, &req, GeklaertModus::Durchstreichen, &SelfWrites::new(), &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{r:?}");
        assert_eq!(fs::read(root.join(rel)).unwrap(), b"- Vorlage #frage\n");
    }
}
