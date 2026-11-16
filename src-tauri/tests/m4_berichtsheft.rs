//! M4-Integrationstests: Berichtsheft über einer **Kopie** von `fixtures/vault` (Datum fest 2026-11-16).
//!
//! * Übersicht: KW41 mit fehlenden Tagen und Lücke, KW42 mit Gesundheitsangabe, fehlende Wochen bis
//!   KW47 eingereiht, Badge für die Vorwochen.
//! * KW46 anlegen (Vorlage + Unterrichtsnotiz WSP 12.11.), Tag speichern, Nachweis ohne KI schreiben
//!   (Diff vorher, Callouts bleiben), Lernfelder schreiben.
//! * `Betrieb.md`: Vorschau ändert nichts, Schreiben mit Basis fügt genau die Zeilen ein (Regel 9).
//! * `.obsidian/` bleibt byte-gleich (Regel 8).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use chrono::NaiveDate;

use ausbildungs_cockpit_lib::berichtsheft::{self, anlegen, betrieb, lernfelder, nachweis, tag, woche, Kontext};
use ausbildungs_cockpit_lib::error::AppError;
use ausbildungs_cockpit_lib::index::VaultIndex;
use ausbildungs_cockpit_lib::model::{
    BerichtsTagArt, BetriebFortschreiben, CreateResult, LernfelderSchreiben, NachweisAnfrage, NachweisSchreiben,
    NachweisStand, NeueLernfeldZeile, NoteKind, TagAbschnitt, TagSpeichern,
};
use ausbildungs_cockpit_lib::unterricht;
use ausbildungs_cockpit_lib::watcher::SharedIndex;
use ausbildungs_cockpit_lib::write::{sha256_hex, RetryPolicy, SelfWrites};

const KW41: &str = "04 Betrieb/Wochen/2026-KW41.md";
const KW46: &str = "04 Betrieb/Wochen/2026-KW46.md";
const BETRIEB: &str = "04 Betrieb/Betrieb.md";

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

fn vault_copy() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    copy_dir(&fixture(), &root);
    (dir, root)
}

fn d(y: i32, m: u32, t: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, t).unwrap()
}

fn heute() -> NaiveDate {
    d(2026, 11, 16)
}

fn kontext(root: &Path) -> Kontext {
    let idx = VaultIndex::build(root).unwrap();
    let (plan, _) = unterricht::load_stundenplan(root, &RetryPolicy::none());
    Kontext::aus_index(&idx, plan)
}

/// (relativer Pfad, SHA-256) aller Dateien – auch `.obsidian/`.
fn stand(root: &Path) -> Vec<(String, String)> {
    fn rek(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for e in fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                rek(root, &p, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                out.push((rel, sha256_hex(&fs::read(&p).unwrap())));
            }
        }
    }
    let mut out = Vec::new();
    rek(root, root, &mut out);
    out.sort();
    out
}

fn obsidian(root: &Path) -> Vec<(String, String)> {
    let v: Vec<_> = stand(root).into_iter().filter(|(p, _)| p.starts_with(".obsidian/")).collect();
    assert!(v.len() >= 3, "Falle .obsidian/ fehlt in der Kopie: {v:?}");
    v
}

fn lies(root: &Path, rel: &str) -> String {
    String::from_utf8(fs::read(root.join(rel)).unwrap()).unwrap()
}

fn sw() -> SelfWrites {
    SelfWrites::new()
}

#[test]
fn uebersicht_mit_fehlenden_tagen_wochen_und_badge() {
    let (_tmp, root) = vault_copy();
    let vorher = obsidian(&root);
    let ctx = kontext(&root);
    let u = woche::uebersicht(&root, &ctx, heute());
    assert_eq!((u.aktuelle_woche.as_str(), u.aktuelle_vorhanden), ("2026-KW47", false));
    let stems: Vec<(&str, bool, bool)> = u.wochen.iter().map(|w| (w.stem.as_str(), w.path.is_some(), w.aktuell)).collect();
    assert_eq!(
        stems,
        vec![
            ("2026-KW47", false, true),
            ("2026-KW46", false, false),
            ("2026-KW45", false, false),
            ("2026-KW44", false, false),
            ("2026-KW43", false, false),
            ("2026-KW42", true, false),
            ("2026-KW41", true, false),
        ],
        "neueste zuerst, Wochen ohne Notiz eingereiht"
    );
    let kw41 = u.wochen.iter().find(|w| w.kw == 41).unwrap();
    assert_eq!(kw41.fehlende_tage, vec!["Mi 07.10.", "Do 08.10.", "Fr 09.10."]);
    assert_eq!((kw41.luecken, kw41.offene_punkte, kw41.diagnosen), (1, 2, 0), "Lücke „Einschulung“");
    assert_eq!((kw41.montag, kw41.sonntag, kw41.stunden.as_deref()), (d(2026, 10, 5), d(2026, 10, 11), Some("38")));
    let kw42 = u.wochen.iter().find(|w| w.kw == 42).unwrap();
    assert_eq!((kw42.diagnosen, kw42.fehlende_tage.clone()), (1, vec!["Mo 12.10.".to_string()]));
    assert_eq!(u.offen_vorwochen, 2);
    assert_eq!(obsidian(&root), vorher);
}

#[test]
fn kw46_anlegen_tag_nachweis_lernfelder() {
    let (_tmp, root) = vault_copy();
    let vorher_obsidian = obsidian(&root);
    let ctx = kontext(&root);
    let p = RetryPolicy::none();

    // 1. Vorschau über irgendeinen Tag der Woche – schreibt nichts
    let v = anlegen::vorschau(&root, &ctx, d(2026, 11, 12)).unwrap();
    assert_eq!(v.rel_path, KW46);
    assert!(v.existiert.is_none() && !root.join(KW46).exists());
    assert_eq!(v.eingetragen, vec!["03 Unterricht/WSP 12.11.2026.md".to_string()]);
    for zeile in [
        "kw: 46",
        "zeitraum: 2026-11-09 – 2026-11-15",
        "# Berichtsheft KW 46",
        "### Donnerstag, 12.11.",
        "- **WSP** — Kündigung → [[WSP 12.11.2026]]",
        "- [[2026-KW45]] — Vorwoche",
        "- Donnerstag: [[WSP 12.11.2026]]",
    ] {
        assert!(v.text.lines().any(|l| l == zeile), "fehlt: {zeile}\n{}", v.text);
    }

    // 2. Anlegen (nie überschreiben)
    let index: SharedIndex = Arc::new(RwLock::new(Some(VaultIndex::build(&root).unwrap())));
    let r = anlegen::anlegen(&index, &root, &ctx, d(2026, 11, 12), &sw(), &p).unwrap();
    let CreateResult::Created { report } = r else { panic!("{r:?}") };
    assert_eq!(report.rel_path, KW46);
    assert_eq!(lies(&root, KW46), v.text);
    {
        let g = index.read().unwrap();
        assert_eq!(g.as_ref().unwrap().get(KW46).map(|m| m.kind), Some(NoteKind::BerichtsheftWoche));
        assert!(berichtsheft::woche_pfad(g.as_ref().unwrap(), KW46).is_ok());
    }
    let ctx = kontext(&root);
    let r = anlegen::anlegen(&index, &root, &ctx, d(2026, 11, 9), &sw(), &p).unwrap();
    assert_eq!(r, CreateResult::Exists { rel_path: KW46.into() });

    // 3. Tag speichern (Dienstag): nur die Tageszeilen ändern sich
    let w = woche::lesen(&root, KW46, &ctx).unwrap().woche;
    let di = w.tage.iter().find(|t| t.datum == d(2026, 11, 10)).unwrap();
    assert!(di.fehlt && di.bearbeitbar && di.nachweis == NachweisStand::Platzhalter);
    let vorher = lies(&root, KW46);
    let req = TagSpeichern {
        path: KW46.into(),
        abschnitt: TagAbschnitt::Betrieb,
        datum: d(2026, 11, 10),
        art: BerichtsTagArt::Stichpunkte,
        stichpunkte: vec!["Artikel im Shop gepflegt".into(), "Einschulung".into()],
        basis: w.version.clone(),
    };
    let a = tag::speichern(&root, &ctx, &req, &sw(), &p).unwrap();
    assert!(a.report.is_some());
    assert_eq!(
        lies(&root, KW46),
        vorher.replacen("### Dienstag, 10.11.\n- \n", "### Dienstag, 10.11.\n- Artikel im Shop gepflegt\n- Einschulung\n", 1)
    );
    // Regel 3: Diagnose im Stichpunkt → abgelehnt, Datei byte-gleich
    let stand_jetzt = lies(&root, KW46);
    let mut boese = req.clone();
    boese.stichpunkte = vec!["Grippe".into()];
    boese.basis = woche::lesen(&root, KW46, &ctx).unwrap().woche.version;
    assert!(matches!(tag::speichern(&root, &ctx, &boese, &sw(), &p), Err(AppError::InvalidInput(_))));
    assert_eq!(lies(&root, KW46), stand_jetzt);

    // 4. Nachweis ohne KI: Entwurf → Vorschau (Diff) → Schreiben mit Basis; Callouts bleiben
    let e = nachweis::entwurf(&root, &ctx, &NachweisAnfrage { path: KW46.into(), mit_ki: false, privat_erlauben: false }, None).unwrap();
    let mut tage: Vec<_> = e.tage.into_iter().filter(|t| t.uebernehmen).collect();
    assert_eq!(tage.len(), 2, "Dienstag (Betrieb) und Donnerstag (WSP): {tage:?}");
    assert_eq!(tage[0].luecken, vec!["Einschulung"]);
    assert!(e.fehlende_tage.contains(&"Mo 09.11.".to_string()));
    tage.sort_by_key(|t| t.datum);
    let callouts_vorher = stand_jetzt[stand_jetzt.find("> [!warning] Offen vor dem Abzeichnen").unwrap()..].to_string();
    let vs = nachweis::schreiben(&root, &ctx, &NachweisSchreiben { path: KW46.into(), tage: tage.clone(), vorschau: true, basis: None }, &sw(), &p)
        .unwrap();
    assert_eq!(lies(&root, KW46), stand_jetzt, "Vorschau schreibt nichts");
    assert_ne!(vs.vorher, vs.nachher);
    let geschrieben = nachweis::schreiben(
        &root,
        &ctx,
        &NachweisSchreiben { path: KW46.into(), tage, vorschau: false, basis: Some(vs.version.clone()) },
        &sw(),
        &p,
    )
    .unwrap();
    assert!(geschrieben.report.is_some());
    let nach = lies(&root, KW46);
    assert_eq!(nach, vs.nachher);
    assert!(nach.contains(
        "**Dienstag, 10.11.2026 — Tätigkeiten im Betrieb**\n\nArtikel im Shop gepflegt.\n\n⟨Lücke: „Einschulung“ – Gegenstand fehlt⟩\n\n**Mittwoch, 11.11.2026 — ⟨Überschrift⟩**"
    ), "{nach}");
    assert!(nach.contains("**Berufsschule — Donnerstag, 12.11.2026**\n\nIn **WSP**: Kündigung.\n\nAusgearbeitet in [[WSP 12.11.2026]].\n\n> [!warning]"));
    assert_eq!(&nach[nach.find("> [!warning] Offen vor dem Abzeichnen").unwrap()..], callouts_vorher, "Callouts byte-gleich");
    let w = woche::lesen(&root, KW46, &ctx).unwrap().woche;
    assert_eq!(w.tage.iter().find(|t| t.datum == d(2026, 11, 10)).unwrap().nachweis, NachweisStand::Vorhanden);

    // 5. Lernfelder: Platzhalterzeile ersetzt, `lernfelder: [4]`
    let req = LernfelderSchreiben {
        path: KW46.into(),
        zeilen: vec![NeueLernfeldZeile { taetigkeit: "Artikel im Shop gepflegt".into(), lernfelder: vec![4] }],
        vorschau: true,
        basis: None,
    };
    let v = lernfelder::schreiben(&root, &ctx, &req, &sw(), &p).unwrap();
    let req = LernfelderSchreiben { vorschau: false, basis: Some(v.version.clone()), ..req };
    lernfelder::schreiben(&root, &ctx, &req, &sw(), &p).unwrap();
    let nach = lies(&root, KW46);
    assert!(nach.contains("lernfelder: [4]\n"));
    assert!(nach.contains("| Artikel im Shop gepflegt | [[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]] |\n\n> [!tip]-"));
    assert!(!nach.contains("|  | [[]] |"));
    assert_eq!(obsidian(&root), vorher_obsidian);
}

#[test]
fn betrieb_md_nur_nach_vorschau_und_genau_die_zeilen() {
    let (_tmp, root) = vault_copy();
    let ctx = kontext(&root);
    let p = RetryPolicy::none();
    let alles_vorher = stand(&root);
    let betrieb_vorher = lies(&root, BETRIEB);

    let v = betrieb::vorschlag(&root, &ctx, KW41).unwrap();
    assert_eq!(v.zeilen.len(), 1);
    let zeilen: Vec<Vec<String>> = v.zeilen.iter().filter(|z| z.uebernehmen).map(|z| z.zellen.clone()).collect();
    assert_eq!(zeilen.len(), 1);

    // ohne Vorschau: nichts
    let ohne = BetriebFortschreiben { woche_path: KW41.into(), zeilen: zeilen.clone(), vorschau: false, basis: None };
    assert!(matches!(betrieb::fortschreiben(&root, &ohne, &sw(), &p), Err(AppError::InvalidInput(_))));
    // Vorschau: Diff, nichts geschrieben
    let vs = betrieb::fortschreiben(&root, &BetriebFortschreiben { vorschau: true, ..ohne.clone() }, &sw(), &p).unwrap();
    assert_eq!(stand(&root), alles_vorher, "Vorschau ändert nichts");
    let neue_zeile = "| [[GEC 05.10.2026\\|GEC]] | Artikel im Shop gepflegt | [[LF04 – Werteströme erfassen, auswerten und beurteilen\\|LF04]] |\n";
    assert_eq!(vs.nachher, format!("{betrieb_vorher}{neue_zeile}"));
    // mit Basis: genau die Zeile, nur Betrieb.md ändert sich
    let w = betrieb::fortschreiben(&root, &BetriebFortschreiben { basis: Some(vs.version.clone()), ..ohne.clone() }, &sw(), &p).unwrap();
    assert_eq!(w.report.as_ref().map(|r| r.rel_path.as_str()), Some(BETRIEB));
    assert_eq!(lies(&root, BETRIEB), format!("{betrieb_vorher}{neue_zeile}"));
    let geaendert: Vec<String> = stand(&root)
        .into_iter()
        .filter(|x| !alles_vorher.contains(x))
        .map(|(p, _)| p)
        .collect();
    assert_eq!(geaendert, vec![BETRIEB.to_string()]);
    // gleiche Zeile noch einmal: Duplikat, unverändert
    let nochmal = betrieb::fortschreiben(&root, &BetriebFortschreiben { vorschau: true, ..ohne }, &sw(), &p).unwrap();
    assert!(nochmal.unveraendert && nochmal.warnungen.iter().any(|x| x.contains("steht schon")));
    let v2 = betrieb::vorschlag(&root, &ctx, KW41).unwrap();
    assert!(v2.zeilen.is_empty() && v2.schon_vorhanden == vec!["Artikel im Shop gepflegt".to_string()]);
}
