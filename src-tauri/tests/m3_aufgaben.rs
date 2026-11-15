//! M3-Integrationstests: Aufgaben, Lernaufgaben und Fragen über einer **Kopie** von `fixtures/vault`.
//!
//! * Die Übersicht an einem festen Tag entspricht den Dataview-Abfragen in `00 Start/Aufgaben.md`
//!   (Listen unten von Hand aus den Abfragen abgeleitet).
//! * Abhaken ändert in der Datei genau ein Zeichen (`[ ]` → `[x]`), der Index zeigt die Aufgabe danach
//!   als erledigt. „Gelöst“ ändert nur `geloest:`. „Geklärt“ streicht `#frage` durch (oder verschiebt
//!   die Zeile in „## Geklärt“) – sonst bleibt alles byte-gleich.
//! * `.obsidian/` bleibt byte-gleich (Regel 8).

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::NaiveDate;

use ausbildungs_cockpit_lib::error::AppError;
use ausbildungs_cockpit_lib::index::VaultIndex;
use ausbildungs_cockpit_lib::model::{AufgabeAbhaken, Arbeitsauftrag, FaelligeAufgabe, FrageKlaeren, GeklaertModus};
use ausbildungs_cockpit_lib::write::{sha256_hex, RetryPolicy, SelfWrites};
use ausbildungs_cockpit_lib::{aufgaben, fragen, heute};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault")
}

/// Kopie des Testvaults **mit** `.obsidian/` – als Falle: am Ende muss es byte-gleich sein.
fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".gitkeep" {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst.join(&name));
        } else {
            fs::copy(entry.path(), dst.join(&name)).unwrap();
        }
    }
}

const WSP: &str = "03 Unterricht/WSP 12.11.2026.md";
const GEC: &str = "03 Unterricht/GEC 05.10.2026.md";
const GEC2: &str = "03 Unterricht/GEC 2 05.10.2026.md";
const GEC_ALT: &str = "03 Unterricht/Unterrichtsnotiz GEC 07.09.2026.md";
const DDV: &str = "03 Unterricht/D-DV 05.10.2026.md";
const LF04: &str = "02 Lernfelder/LF04 – Werteströme erfassen, auswerten und beurteilen.md";
const KW41: &str = "04 Betrieb/Wochen/2026-KW41.md";
const NOTEN: &str = "10 Privat/Noten/Noten.md";
const HANDELSSPANNE: &str = "07 Material/Aufgabensammlung/Handelsspanne berechnen.md";

fn mtime(root: &Path, rel: &str, sekunden: u64) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000 + sekunden);
    File::options().write(true).open(root.join(rel)).unwrap().set_modified(t).unwrap();
}

/// Temp-Kopie mit festen Änderungszeiten für „Zuletzt erledigt“ (Dataview `SORT file.mtime DESC`).
fn vault_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    copy_dir(&fixture(), &root);
    mtime(&root, GEC_ALT, 1000);
    mtime(&root, GEC2, 2000);
    mtime(&root, WSP, 3000);
    (dir, root)
}

fn d(y: i32, m: u32, t: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, t).unwrap()
}

fn liste(v: &[Arbeitsauftrag]) -> Vec<(&str, usize)> {
    v.iter().map(|a| (a.path.as_str(), a.line)).collect()
}

fn faellig(v: &[FaelligeAufgabe]) -> Vec<(&str, usize)> {
    v.iter().map(|a| (a.path.as_str(), a.line)).collect()
}

/// (Pfad, SHA-256) aller Dateien unter `.obsidian/`.
fn obsidian_stand(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in fs::read_dir(root.join(".obsidian")).unwrap() {
        let p = e.unwrap().path();
        out.push((p.file_name().unwrap().to_string_lossy().into_owned(), sha256_hex(&fs::read(&p).unwrap())));
    }
    out.sort();
    assert!(out.len() >= 3, "Falle .obsidian/ fehlt in der Kopie: {out:?}");
    out
}

/// (Fach, [(Text bzw. Titel, Zeile)]) je Nochmal-Gruppe
type Gruppe<'a> = (Option<&'a str>, Vec<(&'a str, Option<usize>)>);

fn sw() -> SelfWrites {
    SelfWrites::new()
}

/// Zeile `nr` (1-basiert) der Datei, ohne Zeilenende.
fn zeile(root: &Path, rel: &str, nr: usize) -> String {
    fs::read_to_string(root.join(rel)).unwrap().lines().nth(nr - 1).unwrap().to_string()
}

// 1 – Übersicht am Montag, 16.11.2026, Zeile für Zeile aus den Dataview-Abfragen abgeleitet
#[test]
fn uebersicht_wie_die_dataview_seite() {
    let (_d, root) = vault_copy();
    let idx = VaultIndex::build(&root).unwrap();
    let heute = d(2026, 11, 16);
    let u = aufgaben::uebersicht(&idx, heute);
    assert_eq!(u.heute, heute);

    // Quelle: alle Notizen außer `99 Meta/…` und `00 Start/!Entwurf Aufgaben.md` (Dateiname mit „!“).
    // „🔴 Überfällig“: `!completed AND due AND due < date(today)`, `SORT due ASC`
    //   GEC 05.10. Z. 27 (08.10.), Z. 28 (12.10.), D-DV 05.10. Z. 10 (19.10.)
    //   – nicht: `!Entwurf` (01.10.), WSP 12.11. Z. 23 `- [-]` (10.11., abgebrochen – App-Abweichung)
    assert_eq!(liste(&u.ueberfaellig), vec![(GEC, 27), (GEC, 28), (DDV, 10)]);
    // „📌 Diese Woche fällig“: `due >= date(today) AND due <= date(today) + dur(7 days)`
    //   WSP 12.11. Z. 18 (16.11. = heute), Z. 19 (23.11. = heute + 7)
    assert_eq!(liste(&u.diese_woche), vec![(WSP, 18), (WSP, 19)]);
    // App-Extra „Später“: WSP 12.11. Z. 20 (24.11.)
    assert_eq!(liste(&u.spaeter), vec![(WSP, 20)]);
    // „📋 Arbeitsaufträge ohne Frist“: `!completed AND !due AND length(text) > 3`, nach Notiz
    //   LF04-Checkliste Z. 16–19, GEC 05.10. Z. 29 (Unteraufgabe), KW41 Z. 22
    //   – nicht: WSP 12.11. Z. 17 (Vorlagen-Platzhalter `- [ ]  📅 `), Z. 21 (`Kap`, 3 Zeichen),
    //     `!Entwurf` Z. 9, die Vorlagen unter `99 Meta/Templates/`
    assert_eq!(liste(&u.ohne_frist), vec![(LF04, 16), (LF04, 17), (LF04, 18), (LF04, 19), (GEC, 29), (KW41, 22)]);
    assert_eq!(u.ohne_frist[4].indent, 4);
    assert_eq!(u.ohne_frist[0].lernfeld.as_deref(), Some("LF04"));
    // „Zuletzt erledigt“: `completed`, `SORT file.mtime DESC` (Zeiten oben gesetzt), `LIMIT 10`
    //   WSP 12.11. Z. 22 `[X]`, GEC 2 05.10. Z. 14, Unterrichtsnotiz GEC 07.09. Z. 22
    assert_eq!(liste(&u.zuletzt_erledigt), vec![(WSP, 22), (GEC2, 14), (GEC_ALT, 22)]);
    assert_eq!(u.zuletzt_erledigt[0].status_char, 'X');

    let a = &u.diese_woche[0];
    assert_eq!(a.text, "Kündigungsschreiben entwerfen");
    assert_eq!((a.fach.as_deref(), a.lernfeld.as_deref(), a.datum), (Some("WSP"), Some("LF01"), Some(d(2026, 11, 12))));
    assert_eq!(a.due, Some(heute));
    assert!(!a.done && !a.privat);

    // „🧮 Lernaufgaben“: `FROM "07 Material/Aufgabensammlung" WHERE file.name != "Aufgabensammlung"`
    //   (die App liefert auch gelöste – Dataview zeigt nur `geloest != true`: Skonto, Handelsspanne)
    //   Sortierung: Lernfeld (4, 4, ohne), Schwierigkeit (leicht vor 2 = mittel)
    let l: Vec<(&str, Option<&str>, bool, bool)> =
        u.lernaufgaben.iter().map(|x| (x.title.as_str(), x.lernfeld.as_deref(), x.geloest, x.nochmal)).collect();
    assert_eq!(
        l,
        vec![
            ("Skonto berechnen", Some("LF04"), false, true),
            ("Handelsspanne berechnen", Some("LF04"), false, false),
            ("ABC-Analyse durchführen", None, true, false),
        ]
    );
    assert_eq!(u.lernaufgaben[1].typ.as_deref(), Some("Rechnen"), "Altbestand bleibt wie er ist");
    assert_eq!(u.lernaufgaben[1].schwierigkeit.as_deref(), Some("2"));
    assert_eq!(u.lernaufgaben[1].quelle.as_deref(), Some("GEC 05.10.2026"));
    assert_eq!(u.lernaufgaben[2].fach.as_deref(), Some("GEC"));

    // „Heute“ nutzt denselben Quellfilter: `!Entwurf` taucht nicht auf
    let h = heute::faellige_aufgaben(&idx, heute);
    assert_eq!(faellig(&h.ueberfaellig), vec![(GEC, 27), (GEC, 28), (DDV, 10)]);
    assert_eq!(faellig(&h.heute), vec![(WSP, 18)]);
    assert_eq!(faellig(&h.naechste_tage), vec![(WSP, 19)]);
}

// 2 – Fragen und „nochmal“ wie im Testvault
#[test]
fn fragen_und_nochmal_im_testvault() {
    let (_d, root) = vault_copy();
    let idx = VaultIndex::build(&root).unwrap();
    let u = fragen::uebersicht(&idx, GeklaertModus::Durchstreichen);
    // neueste zuerst: WSP 12.11. (Z. 27 `#Frage`, Z. 29 mit `#nochmal`), GEC 05.10., GEC 07.09., Noten ohne Datum.
    // Nicht: WSP Z. 26 (`- #frage ` Platzhalter), Z. 28 (`#fragebogen`), `!Entwurf`, Vorlage, Codeblock.
    let f: Vec<(&str, Option<usize>)> = u.fragen.iter().map(|e| (e.path.as_str(), e.line)).collect();
    assert_eq!(f, vec![(WSP, Some(27)), (WSP, Some(29)), (GEC, Some(32)), (GEC_ALT, Some(25)), (NOTEN, Some(12))]);
    assert_eq!(u.fragen[0].text, "Gilt die Probezeit auch nach einer Verlängerung der Ausbildung?");
    assert_eq!(u.fragen[1].text, "Unterschied zwischen Kündigung und Aufhebungsvertrag");
    assert!(u.fragen[4].privat, "10 Privat wird lokal angezeigt, aber markiert");
    // nochmal: GEC (Zeile + Lernaufgabe mit Tag in der Frontmatter), WSP
    let n: Vec<Gruppe> = u
        .nochmal
        .iter()
        .map(|g| (g.fach.as_deref(), g.eintraege.iter().map(|e| (e.title.as_str(), e.line)).collect()))
        .collect();
    assert_eq!(
        n,
        vec![
            (Some("GEC"), vec![("GEC 05.10.2026", Some(35)), ("Skonto berechnen", None)]),
            (Some("WSP"), vec![("WSP 12.11.2026", Some(29))]),
        ]
    );
}

// 3 – Abhaken: genau ein Zeichen, Index zeigt erledigt, zurück ist byte-gleich
#[test]
fn abhaken_aendert_ein_zeichen_und_der_index_folgt() {
    let (_d, root) = vault_copy();
    let obsidian = obsidian_stand(&root);
    let mut idx = VaultIndex::build(&root).unwrap();
    let a = aufgaben::uebersicht(&idx, d(2026, 11, 16)).diese_woche[0].clone();
    assert_eq!((a.path.as_str(), a.line), (WSP, 18));
    let vorher = fs::read(root.join(WSP)).unwrap();
    let req = AufgabeAbhaken { path: a.path.clone(), line: a.line, text: a.text.clone(), erledigt: true };
    let w = sw();
    let r = aufgaben::abhaken(&root, &req, &w, &RetryPolicy::none()).unwrap();
    assert_eq!((r.line, r.verschoben), (18, false));
    let report = r.report.expect("geschrieben");
    assert_eq!(report.rel_path, WSP);
    assert!(!report.created && report.warnungen.is_empty());

    let nachher = fs::read(root.join(WSP)).unwrap();
    assert_eq!(nachher.len(), vorher.len());
    let diff: Vec<usize> = (0..vorher.len()).filter(|i| vorher[*i] != nachher[*i]).collect();
    assert_eq!(diff.len(), 1, "genau ein Zeichen");
    assert_eq!((vorher[diff[0]], nachher[diff[0]]), (b' ', b'x'));
    assert_eq!(zeile(&root, WSP, 18), "- [x] Kündigungsschreiben entwerfen 📅 2026-11-16");
    assert_eq!(report.version.sha256, sha256_hex(&nachher));
    assert!(w.is_own(&root, WSP), "Wächter meldet den Schreibvorgang als eigenen");

    // Index (wie `index_own_write`): erledigt, nicht mehr fällig, jetzt zuletzt erledigt
    idx.upsert(WSP).unwrap();
    let m = idx.get(WSP).unwrap();
    assert!(m.tasks.iter().find(|t| t.line == 18).unwrap().done);
    let u = aufgaben::uebersicht(&idx, d(2026, 11, 16));
    assert_eq!(liste(&u.diese_woche), vec![(WSP, 19)]);
    assert_eq!(liste(&u.zuletzt_erledigt)[..2], [(WSP, 18), (WSP, 22)], "frisch geändert = neueste");

    // noch einmal → nichts geschrieben
    let r = aufgaben::abhaken(&root, &req, &w, &RetryPolicy::none()).unwrap();
    assert_eq!(r.report, None);
    assert_eq!(fs::read(root.join(WSP)).unwrap(), nachher);
    // wieder öffnen → Datei wie vorher
    let zurueck = AufgabeAbhaken { erledigt: false, ..req };
    assert!(aufgaben::abhaken(&root, &zurueck, &w, &RetryPolicy::none()).unwrap().report.is_some());
    assert_eq!(fs::read(root.join(WSP)).unwrap(), vorher);

    // Regel 8
    let r = aufgaben::abhaken(
        &root,
        &AufgabeAbhaken { path: ".obsidian/falle.md".into(), line: 1, text: "x".into(), erledigt: true },
        &w,
        &RetryPolicy::none(),
    );
    assert!(matches!(r, Err(AppError::Guard(_))), "{r:?}");
    assert_eq!(obsidian_stand(&root), obsidian);
}

// 4 – Obsidian hat dazwischen eine Zeile eingefügt; CRLF + BOM bleiben
#[test]
fn abhaken_nach_aenderung_in_obsidian_mit_crlf_und_bom() {
    let (_d, root) = vault_copy();
    let text = fs::read_to_string(root.join(WSP)).unwrap();
    let crlf = format!("\u{FEFF}{}", text.replace('\n', "\r\n"));
    fs::write(root.join(WSP), &crlf).unwrap();
    let idx = VaultIndex::build(&root).unwrap();
    let a = aufgaben::uebersicht(&idx, d(2026, 11, 16)).diese_woche[1].clone();
    assert_eq!((a.line, a.text.as_str()), (19, "Fallbeispiel Probezeit lösen"));
    // in Obsidian: neue Aufgabe über der Liste
    let geaendert = crlf.replacen("- [ ]  📅 \r\n", "- [ ]  📅 \r\n- [ ] Neu aus Obsidian 📅 2026-11-17\r\n", 1);
    fs::write(root.join(WSP), &geaendert).unwrap();
    let req = AufgabeAbhaken { path: a.path, line: a.line, text: a.text, erledigt: true };
    let r = aufgaben::abhaken(&root, &req, &sw(), &RetryPolicy::none()).unwrap();
    assert_eq!((r.line, r.verschoben), (20, true));
    let nachher = fs::read_to_string(root.join(WSP)).unwrap();
    assert_eq!(nachher, geaendert.replacen("- [ ] Fallbeispiel", "- [x] Fallbeispiel", 1));
    assert!(nachher.starts_with('\u{FEFF}') && !nachher.replace("\r\n", "").contains('\n'), "BOM und CRLF bleiben");
}

// 5 – Lernaufgabe gelöst: nur `geloest:` ändert sich
#[test]
fn lernaufgabe_geloest_setzen() {
    let (_d, root) = vault_copy();
    let obsidian = obsidian_stand(&root);
    let mut idx = VaultIndex::build(&root).unwrap();
    let vorher = fs::read_to_string(root.join(HANDELSSPANNE)).unwrap();
    let w = sw();
    let r = aufgaben::lernaufgabe_geloest(&root, HANDELSSPANNE, true, &w, &RetryPolicy::none()).unwrap();
    assert_eq!(r.expect("geschrieben").rel_path, HANDELSSPANNE);
    let nachher = fs::read_to_string(root.join(HANDELSSPANNE)).unwrap();
    assert_eq!(nachher, vorher.replace("geloest: false", "geloest: true"));
    assert!(nachher.contains("typ: Rechnen") && nachher.contains("schwierigkeit: 2"), "Altbestand nicht umgeschrieben");
    idx.upsert(HANDELSSPANNE).unwrap();
    let l = aufgaben::uebersicht(&idx, d(2026, 11, 16)).lernaufgaben;
    assert!(l.iter().find(|x| x.path == HANDELSSPANNE).unwrap().geloest);
    // schon so → None
    assert_eq!(aufgaben::lernaufgabe_geloest(&root, HANDELSSPANNE, true, &w, &RetryPolicy::none()).unwrap(), None);
    // zurück
    assert!(aufgaben::lernaufgabe_geloest(&root, HANDELSSPANNE, false, &w, &RetryPolicy::none()).unwrap().is_some());
    assert_eq!(fs::read_to_string(root.join(HANDELSSPANNE)).unwrap(), vorher);
    // keine Lernaufgabe
    let gec_vorher = fs::read(root.join(GEC)).unwrap();
    for rel in [GEC, "07 Material/Aufgabensammlung/Aufgabensammlung.md"] {
        let r = aufgaben::lernaufgabe_geloest(&root, rel, true, &w, &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::InvalidInput(_))), "{rel}: {r:?}");
    }
    assert_eq!(fs::read(root.join(GEC)).unwrap(), gec_vorher);
    assert_eq!(obsidian_stand(&root), obsidian);
}

// 6 – Frage geklärt: Zeile bleibt, Tag `~~#frage~~`, nicht mehr in der Übersicht; Modus „Abschnitt“
#[test]
fn frage_geklaert() {
    let (_d, root) = vault_copy();
    let obsidian = obsidian_stand(&root);
    let mut idx = VaultIndex::build(&root).unwrap();
    let u = fragen::uebersicht(&idx, GeklaertModus::Durchstreichen);
    let f = u.fragen.iter().find(|e| e.path == GEC).unwrap().clone();
    assert_eq!(f.line, Some(32));
    let vorher = fs::read_to_string(root.join(GEC)).unwrap();
    let mut req = FrageKlaeren { path: f.path.clone(), line: 32, zeile: f.zeile.clone(), vorschau: true };
    let w = sw();

    // Vorschau schreibt nichts
    let v = fragen::klaeren(&root, &req, GeklaertModus::Durchstreichen, &w, &RetryPolicy::none()).unwrap();
    assert!(v.report.is_none());
    assert_eq!(v.nachher, "- Warum rechnet man Skonto vom Zielverkaufspreis und nicht vom Barverkaufspreis? ~~#frage~~");
    assert_eq!(fs::read_to_string(root.join(GEC)).unwrap(), vorher);

    req.vorschau = false;
    let r = fragen::klaeren(&root, &req, GeklaertModus::Durchstreichen, &w, &RetryPolicy::none()).unwrap();
    assert_eq!((r.line, r.verschoben, r.abschnitt_neu), (32, false, false));
    assert!(r.report.is_some());
    let nachher = fs::read_to_string(root.join(GEC)).unwrap();
    assert_eq!(nachher, vorher.replacen("Barverkaufspreis? #frage", "Barverkaufspreis? ~~#frage~~", 1));
    assert_eq!(zeile(&root, GEC, 32), v.nachher);
    idx.upsert(GEC).unwrap();
    let u = fragen::uebersicht(&idx, GeklaertModus::Durchstreichen);
    assert!(u.fragen.iter().all(|e| e.path != GEC), "geklärt → nicht mehr offen");
    assert!(u.nochmal.iter().flat_map(|g| &g.eintraege).any(|e| e.path == GEC && e.line == Some(35)), "#nochmal bleibt");

    // Modus „Abschnitt“: am Dateiende wird „## Geklärt“ angelegt
    let f = u.fragen.iter().find(|e| e.path == GEC_ALT).unwrap().clone();
    let vorher = fs::read_to_string(root.join(GEC_ALT)).unwrap();
    let req = FrageKlaeren { path: f.path.clone(), line: f.line.unwrap(), zeile: f.zeile.clone(), vorschau: false };
    let r = fragen::klaeren(&root, &req, GeklaertModus::Abschnitt, &w, &RetryPolicy::none()).unwrap();
    assert!(r.verschoben && r.abschnitt_neu);
    let nachher = fs::read_to_string(root.join(GEC_ALT)).unwrap();
    let zeile_alt = "- Ist der Kundenservice ein Kern- oder Unterstützungsprozess? #frage\n";
    assert_eq!(
        nachher,
        format!(
            "{}\n## Geklärt\n- Ist der Kundenservice ein Kern- oder Unterstützungsprozess? ~~#frage~~\n",
            vorher.strip_suffix(zeile_alt).unwrap()
        )
    );
    assert_eq!(zeile(&root, GEC_ALT, r.line), "- Ist der Kundenservice ein Kern- oder Unterstützungsprozess? ~~#frage~~");
    idx.upsert(GEC_ALT).unwrap();
    let u = fragen::uebersicht(&idx, GeklaertModus::Abschnitt);
    let rest: Vec<(&str, Option<usize>)> = u.fragen.iter().map(|e| (e.path.as_str(), e.line)).collect();
    assert_eq!(rest, vec![(WSP, Some(27)), (WSP, Some(29)), (NOTEN, Some(12))]);
    assert_eq!(obsidian_stand(&root), obsidian);
}
