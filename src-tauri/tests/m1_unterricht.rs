//! M1-Integrationstests: Heute, Unterrichtsnotiz anlegen, Speichern, Wächter.
//!
//! Jeder Test arbeitet auf einer **Kopie** von `fixtures/vault` in einem Temp-Ordner
//! ([`vault_copy`]) – ohne alles, was mit `.` beginnt (Regel 8). Die Fixtures selbst werden nie
//! verändert (zusätzlich erzwungen durch `write::refuse_repo_trees`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use chrono::{NaiveDate, NaiveDateTime};

use ausbildungs_cockpit_lib::error::AppError;
use ausbildungs_cockpit_lib::guard;
use ausbildungs_cockpit_lib::heute::{self, W_STUNDENPLAN_FEHLT, W_STUNDENPLAN_LEER};
use ausbildungs_cockpit_lib::index::VaultIndex;
use ausbildungs_cockpit_lib::model::{
    CreateResult, HeuteTag, NeueUnterrichtsnotiz, NoteKind, NoteSummary, StundenplanStatus, TagArt, VaultChanged,
};
use ausbildungs_cockpit_lib::unterricht::{self, PlanInputs};
use ausbildungs_cockpit_lib::vault::{naming, scan};
use ausbildungs_cockpit_lib::watcher::{self, SharedIndex};
use ausbildungs_cockpit_lib::write::{self, sha256_hex, RetryPolicy, SaveOutcome, SelfWrites};

const LF04: &str = "02 Lernfelder/LF04 – Werteströme erfassen, auswerten und beurteilen.md";
const GEC_LINK: &str = "[[GEC – Geschäftsprozesse im E-Commerce|GEC]]";
const LF04_LINK: &str = "[[LF04 – Werteströme erfassen, auswerten und beurteilen|LF04]]";
const SHA_A: &str = "cfa66253fd023c8898dd7a4e958935984b30b8335696881440dc6f36d732e535";
const SHA_C: &str = "e80a252535dd6e1609217c0856a1e6001e0d8d6eddaa772a32d22640da35f742";

const REST: &str = "\n## Status\n\
    - [ ] **Notiz abgeschlossen** · danach beim Wochenabschluss nach `Abgeschlossene Notizen` verschieben\n\n\
    ## Inhalt der Stunde\n- \n\n## Wichtige Begriffe\n- [[]]\n\n## Merksätze / Formeln\n> \n\n\
    ## Aufgaben / Hausaufgaben\n- [ ]  📅 \n\n## Offene Fragen\n- #frage \n\n\
    ## Noch nicht verstanden\n- #nochmal \n\n## Verknüpft\n- [[Unterricht]]\n";

/// Erwartete Ausgabe A (SPEC-Beispiel: Montag 16.11.2026, GEC Block 1, LF04, „Kalkulation“).
fn golden_a() -> String {
    format!(
        "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"{GEC_LINK}\"\nlernfeld: \"{LF04_LINK}\"\n\
         thema: Kalkulation\nlehrkraft: HEIN\n---\n# 2026-11-16 GEC – Kalkulation\n\n\
         **Fach:** {GEC_LINK} · **Lernfeld:** {LF04_LINK} · **Lehrkraft:** HEIN\n{REST}"
    )
}

/// Erwartete Ausgabe C (D, Std. 8, ohne Lehrkraft, Thema und Lernfeld).
fn golden_c() -> String {
    "---\ntags: [unterricht]\ndatum: 2026-11-16\nfach: \"[[D – Deutsch|D]]\"\nlernfeld: \nthema: \n\
     lehrkraft: \n---\n# 2026-11-16 D\n\n**Fach:** [[D – Deutsch|D]] · **Lernfeld:** [[]] · **Lehrkraft:** \n"
        .to_string()
        + REST
}

/// SPEC §2, wörtlich.
const SPEC_MATRIX: &str = "| Std. | Zeit | Montag | Donnerstag |\n|---:|---|---|---|\n\
    | 2 | 08:35–09:20 | GEC | KSK |\n| 3 | 09:35–10:20 | D-DV | WSP |\n| 4 | 10:20–11:05 | D-DV | E |\n\
    | 5 | 11:20–12:05 | WSP | GEC |\n| 6 | 12:05–12:50 | GEC | GEC |\n| 7 | 13:05–13:50 | GEC | PG |\n\
    | 8 | 13:50–14:35 | D | — |\n";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault")
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue; // .obsidian, .gitkeep … werden nie gelesen
        }
        let ft = entry.file_type().unwrap();
        if ft.is_dir() {
            copy_dir(&entry.path(), &dst.join(&name));
        } else if ft.is_file() {
            fs::copy(entry.path(), dst.join(&name)).unwrap();
        }
    }
}

/// Kopie des Testvaults in einem Temp-Ordner (Fixtures bleiben unberührt).
fn vault_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    copy_dir(&fixture(), &root);
    (dir, root)
}

fn d(y: i32, m: u32, t: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, t).unwrap()
}

/// Samstag – kein „heute“ für die getesteten Tage.
fn samstag() -> NaiveDateTime {
    d(2026, 11, 14).and_hms_opt(10, 0, 0).unwrap()
}

fn shared(root: &Path) -> SharedIndex {
    Arc::new(RwLock::new(Some(VaultIndex::build(root).unwrap())))
}

fn tag(root: &Path, index: &SharedIndex, datum: NaiveDate, jetzt: NaiveDateTime) -> HeuteTag {
    let (plan, status) = unterricht::load_stundenplan(root, &RetryPolicy::short());
    let g = index.read().unwrap();
    heute::heute(g.as_ref().unwrap(), plan.as_ref(), status, datum, jetzt)
}

fn req(datum: NaiveDate, fach: &str, block: u8, lernfeld: Option<&str>, thema: Option<&str>) -> NeueUnterrichtsnotiz {
    NeueUnterrichtsnotiz {
        datum,
        fach: fach.into(),
        block,
        lernfeld: lernfeld.map(str::to_string),
        thema: thema.map(str::to_string),
    }
}

fn create(index: &SharedIndex, root: &Path, r: &NeueUnterrichtsnotiz, writes: &SelfWrites) -> CreateResult {
    unterricht::create_note(index, root, r, writes, &RetryPolicy::none()).unwrap()
}

fn created_path(r: &CreateResult) -> String {
    match r {
        CreateResult::Created { report } => report.rel_path.clone(),
        other => panic!("erwartet Created, bekam {other:?}"),
    }
}

fn notiz(b: &Option<NoteSummary>) -> Option<&str> {
    b.as_ref().map(|n| n.path.as_str())
}

/// Alle `.md` der Kopie (ohne versteckte Ordner).
fn md_files(root: &Path) -> BTreeSet<String> {
    scan::scan_vault(root).unwrap().notes.into_iter().collect()
}

/// Montag-Prüfungen aus Test 1 (auch für den Matrix-Stundenplan, Test 16).
fn pruefe_montag(t: &HeuteTag) {
    assert_eq!(t.art, TagArt::Schultag);
    assert_eq!(t.wochentag, "Montag");
    let faecher: Vec<&str> = t.bloecke.iter().map(|b| b.block.fach.as_str()).collect();
    assert_eq!(faecher, vec!["GEC", "D-DV", "WSP", "GEC", "D"]);
    let stunden: Vec<(u8, u8)> = t.bloecke.iter().map(|b| (b.block.von_stunde, b.block.bis_stunde)).collect();
    assert_eq!(stunden, vec![(2, 2), (3, 4), (5, 5), (6, 7), (8, 8)]);
    let zeiten: Vec<(&str, &str)> = t.bloecke.iter().map(|b| (b.block.beginn.as_str(), b.block.ende.as_str())).collect();
    assert_eq!(
        zeiten,
        vec![("08:35", "09:20"), ("09:35", "11:05"), ("11:20", "12:05"), ("12:05", "13:50"), ("13:50", "14:35")]
    );
    let lehrer: Vec<Option<&str>> = t.bloecke.iter().map(|b| b.fach.lehrkraft.as_deref()).collect();
    assert_eq!(lehrer, vec![Some("HEIN"), Some("HUGO"), Some("SCMI"), Some("HEIN"), None]);
    let links: Vec<&str> = t.bloecke.iter().map(|b| b.fach.link.as_str()).collect();
    assert_eq!(
        links,
        vec![GEC_LINK, "[[D-DV]]", "[[WSP – Wirtschafts- und Sozialprozesse|WSP]]", GEC_LINK, "[[D – Deutsch|D]]"]
    );
    for b in &t.bloecke {
        let zwei = b.position == 0 || b.position == 3;
        assert_eq!(b.fach_anzahl == 2, zwei, "fach_anzahl an {}", b.position);
        assert_eq!(b.hinweis.is_some(), zwei, "hinweis an {}: {:?}", b.position, b.hinweis);
        assert!(b.notiz.is_none());
    }
    assert_eq!((t.bloecke[0].fach_nr, t.bloecke[3].fach_nr), (1, 2));
    assert_eq!(t.bloecke[3].neuer_pfad, "03 Unterricht/GEC 2 16.11.2026.md");
    assert_eq!(t.bloecke[0].neuer_pfad, "03 Unterricht/GEC 16.11.2026.md");
    assert_eq!(
        t.bloecke[0].hinweis.as_deref(),
        Some("GEC hat heute 2 Blöcke (Std. 2 und 6–7) – eine gemeinsame Notiz oder eine je Block? Die zweite kannst du später im 2. Block anlegen.")
    );
    assert_eq!(
        t.bloecke[3].hinweis.as_deref(),
        Some("Zweiter GEC-Block heute – legst du hier an, entsteht „GEC 2 16.11.2026“.")
    );
}

// 1
#[test]
fn montag_zeigt_gec_ddv_wsp_gec_d() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 11, 16), samstag());
    pruefe_montag(&t);
    assert!(!t.ist_heute);
    assert_eq!(t.aktueller_block, None);
    assert_eq!(t.stundenplan, StundenplanStatus::Ok);
    assert!(t.warnungen.is_empty());
    // heute um 12:05 → zweiter GEC-Block
    let jetzt = d(2026, 11, 16).and_hms_opt(12, 5, 30).unwrap();
    let t = tag(&root, &index, d(2026, 11, 16), jetzt);
    assert!(t.ist_heute);
    assert_eq!(t.aktueller_block, Some(3));
}

// 2
#[test]
fn donnerstag_betriebstag_wochenende() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 11, 19), samstag());
    assert_eq!(t.art, TagArt::Schultag);
    let faecher: Vec<&str> = t.bloecke.iter().map(|b| b.block.fach.as_str()).collect();
    assert_eq!(faecher, vec!["KSK", "WSP", "E", "GEC", "PG"]);
    assert!(t.bloecke.iter().all(|b| b.hinweis.is_none() && b.fach_anzahl == 1));
    assert_eq!((t.bloecke[3].block.von_stunde, t.bloecke[3].block.bis_stunde), (5, 6));

    let t = tag(&root, &index, d(2026, 11, 17), samstag());
    assert_eq!((t.art, t.wochentag.as_str()), (TagArt::Betriebstag, "Dienstag"));
    assert!(t.bloecke.is_empty());
    let t = tag(&root, &index, d(2026, 11, 14), samstag());
    assert_eq!((t.art, t.wochentag.as_str()), (TagArt::Wochenende, "Samstag"));
    assert!(t.ist_heute && t.aktueller_block.is_none());
}

// 3
#[test]
fn vorhandene_notizen_werden_zugeordnet() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 10, 5), samstag());
    let n: Vec<Option<&str>> = t.bloecke.iter().map(|b| notiz(&b.notiz)).collect();
    assert_eq!(
        n,
        vec![
            Some("03 Unterricht/GEC 05.10.2026.md"),
            Some("03 Unterricht/D-DV 05.10.2026.md"), // kaputtes YAML → aus dem Dateinamen
            None,
            Some("03 Unterricht/GEC 2 05.10.2026.md"),
            None,
        ]
    );
    assert!(t.weitere_notizen.is_empty());
    assert!(t.bloecke.iter().all(|b| b.hinweis.is_none() || b.notiz.is_none()));
    assert!(t.bloecke[0].hinweis.is_none() && t.bloecke[3].hinweis.is_none());
    assert_eq!(notiz(&t.bloecke[3].erste_notiz), Some("03 Unterricht/GEC 05.10.2026.md"));
}

// 4
#[test]
fn altbestand_wird_zugeordnet() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let w = SelfWrites::new();
    let t = tag(&root, &index, d(2026, 9, 7), samstag());
    let legacy = "03 Unterricht/Unterrichtsnotiz GEC 07.09.2026.md";
    assert_eq!(notiz(&t.bloecke[0].notiz), Some(legacy));
    assert_eq!(notiz(&t.bloecke[3].notiz), None);
    assert_eq!(notiz(&t.bloecke[3].erste_notiz), Some(legacy));
    assert_eq!(t.bloecke[3].neuer_pfad, "03 Unterricht/GEC 2 07.09.2026.md");
    assert_eq!(
        t.bloecke[3].hinweis.as_deref(),
        Some("In der Notiz des 1. Blocks weiterschreiben oder eine eigene Notiz für Std. 6–7 anlegen.")
    );

    let vorher = md_files(&root);
    let r = create(&index, &root, &req(d(2026, 9, 7), "GEC", 1, None, None), &w);
    assert_eq!(r, CreateResult::Exists { rel_path: legacy.into() });
    assert_eq!(md_files(&root), vorher, "keine neue Datei");
    let r = create(&index, &root, &req(d(2026, 9, 7), "GEC", 2, None, None), &w);
    assert_eq!(created_path(&r), "03 Unterricht/GEC 2 07.09.2026.md");
    assert!(root.join("03 Unterricht/GEC 2 07.09.2026.md").is_file());
    // jetzt hat Block 2 seine Notiz
    let t = tag(&root, &index, d(2026, 9, 7), samstag());
    assert_eq!(notiz(&t.bloecke[3].notiz), Some("03 Unterricht/GEC 2 07.09.2026.md"));
}

// 5
#[test]
fn unterordner_wird_gefunden() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 9, 10), samstag());
    assert_eq!(t.wochentag, "Donnerstag");
    assert_eq!(notiz(&t.bloecke[1].notiz), Some("03 Unterricht/Abgeschlossene Notizen/WSP 10.09.2026.md"));
    assert!(t.bloecke.iter().all(|b| b.hinweis.is_none()));
    let r = create(&index, &root, &req(d(2026, 9, 10), "WSP", 1, None, None), &SelfWrites::new());
    assert_eq!(r, CreateResult::Exists { rel_path: "03 Unterricht/Abgeschlossene Notizen/WSP 10.09.2026.md".into() });
    assert!(!root.join("03 Unterricht/WSP 10.09.2026.md").exists());
}

// 6
#[test]
fn neue_notiz_golden_und_im_index() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let r = create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, Some(LF04), Some("Kalkulation")), &SelfWrites::new());
    let rel = created_path(&r);
    assert_eq!(rel, "03 Unterricht/GEC 16.11.2026.md");
    let CreateResult::Created { report } = &r else { unreachable!() };
    assert!(report.created && report.created_folders.is_empty() && report.warnungen.is_empty(), "{report:?}");
    let bytes = fs::read(root.join(&rel)).unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), golden_a());
    assert_eq!(sha256_hex(&bytes), SHA_A);
    assert_eq!(report.version.sha256, SHA_A);
    assert_eq!(report.version.size, 749);

    // sofort im Index, ohne Wächter
    {
        let g = index.read().unwrap();
        let idx = g.as_ref().unwrap();
        let m = idx.get(&rel).expect("nicht im Index");
        assert_eq!(m.kind, NoteKind::Unterricht);
        assert_eq!(m.fach.as_deref(), Some("GEC"));
        assert_eq!(m.datum, Some(d(2026, 11, 16)));
        assert_eq!(NoteSummary::from(m).thema.as_deref(), Some("Kalkulation"));
        let liste = unterricht::list_unterricht(idx, None);
        assert_eq!(liste[0].path, rel, "neueste zuerst");
        assert!(liste.iter().all(|n| n.path != "03 Unterricht/Unterricht.md"), "Übersicht nicht in der Liste");
    }
    let t = tag(&root, &index, d(2026, 11, 16), samstag());
    assert_eq!(notiz(&t.bloecke[0].notiz), Some(rel.as_str()));
    assert!(t.bloecke[0].hinweis.is_none());
    assert!(t.weitere_notizen.iter().all(|n| n.path != "03 Unterricht/Unterricht.md"));
    // zweiter Block bietet jetzt „In 1. Notiz weiterschreiben“
    assert_eq!(notiz(&t.bloecke[3].erste_notiz), Some(rel.as_str()));
}

// 7
#[test]
fn links_der_neuen_notiz_loesen_auf() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let rel = created_path(&create(
        &index,
        &root,
        &req(d(2026, 11, 16), "GEC", 1, Some(LF04), Some("Kalkulation")),
        &SelfWrites::new(),
    ));
    let (meta, note) = scan::read_note(&root, &rel).unwrap();
    let alle = md_files(&root);
    let mut gesehen = 0;
    for l in meta.links.iter().filter(|l| [4, 5, 11].contains(&l.line)) {
        let treffer = alle.iter().filter(|p| p.rsplit('/').next().unwrap().trim_end_matches(".md") == l.target).count();
        assert_eq!(treffer, 1, "Link {} (Zeile {}) löst nicht eindeutig auf", l.target, l.line);
        gesehen += 1;
    }
    assert_eq!(gesehen, 4, "fach + lernfeld in Frontmatter und Fachzeile: {:#?}", meta.links);
    assert!(!fs::read_to_string(root.join(&rel)).unwrap().contains("\\|"));
    let yaml: serde_yaml::Value = serde_yaml::from_str(&note.yaml().unwrap()).unwrap();
    for k in ["fach", "lernfeld"] {
        assert!(yaml[k].as_str().is_some_and(|s| s.starts_with("[[")), "{k}: {:?}", yaml[k]);
    }
    // auch [[Unterricht]] löst jetzt auf (Übersicht im Testvault)
    assert!(alle.contains("03 Unterricht/Unterricht.md"));
}

// 8
#[test]
fn zweiter_block_dateiname() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let rel = created_path(&create(&index, &root, &req(d(2026, 11, 16), "GEC", 2, None, None), &SelfWrites::new()));
    assert_eq!(rel, "03 Unterricht/GEC 2 16.11.2026.md");
    let stem = rel.rsplit('/').next().unwrap().trim_end_matches(".md");
    let n = naming::parse_unterricht(stem).unwrap();
    assert_eq!((n.block, n.fach.as_str(), n.datum), (2, "GEC", d(2026, 11, 16)));
    let text = fs::read_to_string(root.join(&rel)).unwrap();
    assert!(text.contains(&format!("\nfach: \"{GEC_LINK}\"\n")));
    assert!(text.contains("\n# 2026-11-16 GEC\n"));
}

// 9
#[test]
fn anlegen_ueberschreibt_nie() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    // nach dem Indexaufbau angelegt → erst die Schreibschicht bemerkt es
    let target = root.join("03 Unterricht/GEC 16.11.2026.md");
    fs::write(&target, b"schon da").unwrap();
    let r = create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &SelfWrites::new());
    assert_eq!(r, CreateResult::Exists { rel_path: "03 Unterricht/GEC 16.11.2026.md".into() });
    assert_eq!(fs::read(&target).unwrap(), b"schon da");
    // im Index bekannt → schon der Trockenlauf meldet es
    let index = shared(&root);
    let r = create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &SelfWrites::new());
    assert_eq!(r, CreateResult::Exists { rel_path: "03 Unterricht/GEC 16.11.2026.md".into() });
    assert_eq!(fs::read(&target).unwrap(), b"schon da");
}

/// Wartet bis `deadline` auf Ereignisse, bis `fertig` sie akzeptiert.
fn sammeln(rx: &mpsc::Receiver<VaultChanged>, deadline: Duration, fertig: impl Fn(&[VaultChanged]) -> bool) -> Vec<VaultChanged> {
    let t0 = Instant::now();
    let mut out = Vec::new();
    while t0.elapsed() < deadline {
        if let Ok(ch) = rx.recv_timeout(Duration::from_millis(50)) {
            out.push(ch);
            if fertig(&out) {
                break;
            }
        }
    }
    out
}

fn start_watcher(root: &Path, index: &SharedIndex, writes: &Arc<SelfWrites>) -> (watcher::VaultWatcher, mpsc::Receiver<VaultChanged>) {
    let (tx, rx) = mpsc::channel();
    let w = watcher::start(root.to_path_buf(), index.clone(), writes.clone(), move |ch| {
        let _ = tx.send(ch);
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300)); // Wächter läuft unter Windows kurz an
    (w, rx)
}

// 10
#[test]
fn waechter_meldet_eigene_schreibvorgaenge_als_own() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let writes = Arc::new(SelfWrites::new());
    let (_w, rx) = start_watcher(&root, &index, &writes);

    let rel = created_path(&create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &writes));
    let ev = sammeln(&rx, Duration::from_secs(2), |e| e.iter().any(|c| c.own.contains(&rel)));
    assert!(ev.iter().any(|c| c.own.contains(&rel)), "kein own-Ereignis: {ev:?}");
    assert!(ev.iter().all(|c| !c.changed.contains(&rel) && !c.full_rescan), "{ev:?}");

    // eigener Speichervorgang ebenfalls own
    let c = write::read_for_edit(&root, &rel, &RetryPolicy::none()).unwrap();
    let r = write::save(&root, &rel, &format!("{}- Notiz\n", c.text), &c.version, &RetryPolicy::none(), &writes).unwrap();
    assert!(matches!(r, SaveOutcome::Saved { .. }));
    let ev = sammeln(&rx, Duration::from_secs(2), |e| e.iter().any(|c| c.own.contains(&rel)));
    assert!(ev.iter().any(|c| c.own.contains(&rel)) && ev.iter().all(|c| !c.changed.contains(&rel)), "{ev:?}");

    // fremde Änderung (Obsidian) → changed
    let fremd = "03 Unterricht/GEC 05.10.2026.md";
    fs::write(root.join(fremd), "---\nfach: GEC\n---\nextern\n").unwrap();
    let ev = sammeln(&rx, Duration::from_secs(2), |e| e.iter().any(|c| c.changed.iter().any(|p| p == fremd)));
    assert!(ev.iter().any(|c| c.changed.iter().any(|p| p == fremd)), "{ev:?}");
    assert!(ev.iter().all(|c| !c.own.iter().any(|p| p == fremd)));
}

// 11
#[test]
fn regel8_beim_anlegen_und_speichern() {
    let (_d, root) = vault_copy();
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    let app_json = b"{\"falle\": true}".to_vec();
    let falle = b"---\ntags: [falle]\n---\n- [ ] darf nie erscheinen OBSIDIAN-CONFIG-NIE-LESEN\n".to_vec();
    fs::write(root.join(".obsidian/app.json"), &app_json).unwrap();
    fs::write(root.join(".obsidian/falle.md"), &falle).unwrap();
    let mt = |p: &str| fs::metadata(root.join(p)).unwrap().modified().unwrap();
    let (m1, m2) = (mt(".obsidian/app.json"), mt(".obsidian/falle.md"));
    std::thread::sleep(Duration::from_millis(20));

    let index = shared(&root);
    let w = SelfWrites::new();
    let rel = created_path(&create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, Some(LF04), Some("Regel 8")), &w));
    let c = write::read_for_edit(&root, &rel, &RetryPolicy::none()).unwrap();
    let r = write::save(&root, &rel, &c.text.replace("- \n", "- Inhalt\n"), &c.version, &RetryPolicy::none(), &w).unwrap();
    assert!(matches!(r, SaveOutcome::Saved { .. }), "{r:?}");

    assert_eq!(fs::read(root.join(".obsidian/app.json")).unwrap(), app_json);
    assert_eq!(fs::read(root.join(".obsidian/falle.md")).unwrap(), falle);
    assert_eq!((mt(".obsidian/app.json"), mt(".obsidian/falle.md")), (m1, m2));
    let g = index.read().unwrap();
    assert!(g.as_ref().unwrap().notes().all(|m| !m.path.to_lowercase().contains(".obsidian")));
    assert!(guard::resolve_for_write(&root, ".obsidian/x.md").is_err());
    assert!(guard::resolve_for_read(&root, "OBSIDI~1/falle.md").is_err());
    assert!(write::read_for_edit(&root, ".obsidian/falle.md", &RetryPolicy::none()).is_err());
    let base = c.version.clone();
    assert!(write::save(&root, ".obsidian/falle.md", "x", &base, &RetryPolicy::none(), &w).is_err());
    assert_eq!(fs::read(root.join(".obsidian/falle.md")).unwrap(), falle);
}

// 12
#[test]
fn nie_loeschen() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let w = SelfWrites::new();
    let vorher = md_files(&root);
    // wie Test 4
    create(&index, &root, &req(d(2026, 9, 7), "GEC", 1, None, None), &w);
    create(&index, &root, &req(d(2026, 9, 7), "GEC", 2, None, None), &w);
    // wie Test 6 und 8
    let rel = created_path(&create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, Some(LF04), Some("Kalkulation")), &w));
    create(&index, &root, &req(d(2026, 11, 16), "GEC", 2, None, None), &w);
    // wie Test 9
    fs::write(root.join("03 Unterricht/WSP 16.11.2026.md"), "schon da").unwrap();
    create(&index, &root, &req(d(2026, 11, 16), "WSP", 1, None, None), &w);
    // speichern
    let c = write::read_for_edit(&root, &rel, &RetryPolicy::none()).unwrap();
    write::save(&root, &rel, "nur noch das\n", &c.version, &RetryPolicy::none(), &w).unwrap();
    let nachher = md_files(&root);
    assert!(nachher.is_superset(&vorher), "fehlt: {:?}", vorher.difference(&nachher).collect::<Vec<_>>());
    assert_eq!(nachher.len(), vorher.len() + 4);
}

// 13
#[test]
fn vorlage_fehlt() {
    let (_d, root) = vault_copy();
    fs::remove_file(root.join(unterricht::TEMPLATE_UNTERRICHT)).unwrap(); // nur in der Kopie
    let index = shared(&root);
    let vorher = md_files(&root);
    let r = unterricht::create_note(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &SelfWrites::new(), &RetryPolicy::none());
    assert!(matches!(r, Err(AppError::TemplateMissing(_))), "{r:?}");
    assert_eq!(
        r.unwrap_err().to_string(),
        "Die Vorlage „99 Meta/Templates/Unterrichtsnotiz.md“ fehlt im Vault. Ohne Vorlage legt die App keine Unterrichtsnotiz an."
    );
    assert_eq!(md_files(&root), vorher);
}

// 14
#[test]
fn stundenplan_fehlt_oder_leer() {
    let (_d, root) = vault_copy();
    let plan_pfad = root.join("00 Start/Stundenplan.md");
    fs::remove_file(&plan_pfad).unwrap();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 11, 16), samstag());
    assert_eq!(t.stundenplan, StundenplanStatus::Fehlt);
    assert_eq!(t.warnungen, vec![W_STUNDENPLAN_FEHLT.to_string()]);
    assert_eq!(t.art, TagArt::Betriebstag);

    fs::write(&plan_pfad, "---\ntags: [stundenplan]\n---\n# Stundenplan\n\nKommt noch.\n").unwrap();
    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 11, 16), samstag());
    assert_eq!(t.stundenplan, StundenplanStatus::Leer);
    assert_eq!(t.warnungen, vec![W_STUNDENPLAN_LEER.to_string()]);
    assert_eq!(t.art, TagArt::Betriebstag);

    // Fach aus 01 Fächer/ funktioniert trotzdem – mit Lehrkraft aus der Fachnotiz
    let inputs = PlanInputs::read(&root, &RetryPolicy::none()).unwrap();
    let v = unterricht::plan_note(index.read().unwrap().as_ref().unwrap(), &inputs, &req(d(2026, 11, 16), "GEC", 1, None, None))
        .unwrap();
    assert_eq!(v.fach_link, GEC_LINK);
    assert_eq!(v.lehrkraft.as_deref(), Some("HEIN"));
    assert_eq!(v.warnungen, vec!["Stundenplan nicht gefunden – Lehrkraft und Fachlink aus „01 Fächer/“.".to_string()]);
    let rel = created_path(&create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &SelfWrites::new()));
    assert_eq!(rel, "03 Unterricht/GEC 16.11.2026.md");
    // ohne Stundenplan ist D unbekannt
    let r = unterricht::create_note(&index, &root, &req(d(2026, 11, 16), "D", 1, None, None), &SelfWrites::new(), &RetryPolicy::none());
    assert!(matches!(r, Err(AppError::UnknownFach(_))), "{r:?}");
}

// 15
#[test]
fn fach_ohne_fachnotiz() {
    let (_d, root) = vault_copy();
    let index = shared(&root);
    let inputs = PlanInputs::read(&root, &RetryPolicy::none()).unwrap();
    let g = index.read().unwrap();
    let v = unterricht::plan_note(g.as_ref().unwrap(), &inputs, &req(d(2026, 11, 16), "D", 1, None, None)).unwrap();
    assert_eq!(v.fach_link, "[[D – Deutsch|D]]");
    assert_eq!(v.lehrkraft, None);
    assert_eq!(v.warnungen, vec!["Fachnotiz „D – Deutsch“ fehlt im Vault – der Link zeigt noch ins Leere.".to_string()]);
    assert_eq!(v.rel_path, "03 Unterricht/D 16.11.2026.md");
    assert_eq!(v.existiert, None);
    assert_eq!(v.text, golden_c());
    assert_eq!(v.text.len(), 526);
    assert_eq!(sha256_hex(v.text.as_bytes()), SHA_C);
}

// 16
#[test]
fn matrix_stundenplan() {
    let (_d, root) = vault_copy();
    let plan_pfad = root.join("00 Start/Stundenplan.md");
    let alt = fs::read_to_string(&plan_pfad).unwrap();
    // Überschrift als eigene Zeile suchen – der Hinweis-Kasten erwähnt „**## Montag**“ auch
    let start = alt.find("\n## Montag\n").unwrap() + 1;
    let ende = alt.find("> Dienstag, Mittwoch und Freitag").unwrap();
    let neu = format!("{}## Plan\n{SPEC_MATRIX}\n{}", &alt[..start], &alt[ende..]);
    assert!(neu.contains("## Lehrkräfte") && !neu.contains("\n## Montag\n") && !neu.contains("\n## Donnerstag\n"));
    fs::write(&plan_pfad, neu).unwrap();

    let index = shared(&root);
    let t = tag(&root, &index, d(2026, 11, 16), samstag());
    assert_eq!(t.stundenplan, StundenplanStatus::Ok);
    pruefe_montag(&t);
    let don = tag(&root, &index, d(2026, 11, 19), samstag());
    let faecher: Vec<&str> = don.bloecke.iter().map(|b| b.block.fach.as_str()).collect();
    assert_eq!(faecher, vec!["KSK", "WSP", "E", "GEC", "PG"]);
    assert_eq!(don.bloecke[0].fach.link, "[[KSK – Kaufmännische Steuerung und Kontrolle|KSK]]");

    let r = create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, Some(LF04), Some("Kalkulation")), &SelfWrites::new());
    let bytes = fs::read(root.join(created_path(&r))).unwrap();
    assert_eq!(String::from_utf8(bytes.clone()).unwrap(), golden_a());
    assert_eq!(sha256_hex(&bytes), SHA_A);
}

// 17
#[test]
fn neuer_ordner_ohne_vollscan() {
    let (_d, root) = vault_copy();
    fs::remove_dir_all(root.join("03 Unterricht")).unwrap(); // nur in der Kopie
    let index = shared(&root);
    let writes = Arc::new(SelfWrites::new());
    let (_w, rx) = start_watcher(&root, &index, &writes);

    let r = create(&index, &root, &req(d(2026, 11, 16), "GEC", 1, None, None), &writes);
    let CreateResult::Created { report } = &r else { panic!("{r:?}") };
    assert_eq!(report.created_folders, vec!["03 Unterricht".to_string()]);
    let rel = report.rel_path.clone();
    let mut ev = sammeln(&rx, Duration::from_secs(2), |e| e.iter().any(|c| c.own.contains(&rel)));
    // noch kurz nachlauschen, ob ein Vollscan hinterherkommt
    ev.extend(sammeln(&rx, Duration::from_millis(700), |_| false));
    assert!(ev.iter().any(|c| c.own.contains(&rel)), "kein own-Ereignis: {ev:?}");
    assert!(ev.iter().all(|c| !c.full_rescan && !c.changed.contains(&rel)), "{ev:?}");
    let g = index.read().unwrap();
    assert!(g.as_ref().unwrap().folders().iter().any(|f| f.path == "03 Unterricht"));
    assert!(g.as_ref().unwrap().get(&rel).is_some());
}
