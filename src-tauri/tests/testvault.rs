//! Tests gegen `fixtures/vault/` – den Testvault mit den Unsauberkeiten aus SPEC §5.3.
//! Nie gegen den echten Vault entwickeln (SPEC §10).

use std::path::PathBuf;

use ausbildungs_cockpit_lib::guard::{self, GuardError};
use ausbildungs_cockpit_lib::index::VaultIndex;
use ausbildungs_cockpit_lib::model::{NoteKind, Wochentag};
use ausbildungs_cockpit_lib::vault::{frontmatter::NoteText, scan, stundenplan};

const MARKER: &str = "OBSIDIAN-CONFIG-NIE-LESEN";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault")
}

#[test]
fn regel8_obsidian_wird_weder_indiziert_noch_gelesen() {
    let idx = VaultIndex::build(&root()).unwrap();
    // Die Falle .obsidian/falle.md existiert, darf aber nirgends auftauchen.
    assert!(root().join(".obsidian/falle.md").is_file());
    for m in idx.notes() {
        assert!(!m.path.contains(".obsidian"), "{}", m.path);
        assert!(!m.tags.iter().any(|t| t == "falle"));
        assert!(m.tasks.iter().all(|t| !t.text.contains("nie erscheinen")));
    }
    let serialized = serde_json::to_string(&idx.folders()).unwrap();
    assert!(!serialized.contains(".obsidian"));
    assert!(matches!(
        scan::read_note(&root(), ".obsidian/falle.md"),
        Err(ausbildungs_cockpit_lib::error::AppError::Guard(GuardError::ObsidianConfig(_)))
    ));
    // Volltext über alles, was der Index kennt
    let alles = serde_json::to_string(&idx.notes().collect::<Vec<_>>()).unwrap();
    assert!(!alles.contains(MARKER));
}

#[test]
fn regel10_privat_ist_markiert_und_wird_aussortiert() {
    let idx = VaultIndex::build(&root()).unwrap();
    let privat: Vec<String> = idx.notes().filter(|m| m.private).map(|m| m.path.clone()).collect();
    assert_eq!(privat.len(), 2, "{privat:?}");
    let alle: Vec<String> = idx.notes().map(|m| m.path.clone()).collect();
    let fuer_ki = guard::select_for_ai(&alle, false).unwrap();
    assert!(fuer_ki.iter().all(|p| !p.starts_with("10 Privat/")));
    assert_eq!(fuer_ki.len(), alle.len() - 2);
}

#[test]
fn regel6_ek_aus_privatnotiz_wird_entfernt() {
    let text = std::fs::read_to_string(root().join("10 Privat/Noten/Noten.md")).unwrap();
    assert!(guard::contains_purchase_price(&text));
    let clean = guard::strip_purchase_prices(&text);
    assert!(!guard::contains_purchase_price(&clean));
    assert!(!clean.contains("34,50"));
    assert!(clean.contains("| GEC | 2 |"));
}

#[test]
fn altbestand_und_uneinheitliche_frontmatter_werden_gelesen() {
    let idx = VaultIndex::build(&root()).unwrap();
    let alt = idx.get("03 Unterricht/Unterrichtsnotiz GEC 07.09.2026.md").unwrap();
    assert_eq!(alt.kind, NoteKind::Unterricht);
    assert_eq!(alt.fach.as_deref(), Some("GEC"));
    assert_eq!(alt.tags[..2], ["unterricht".to_string(), "gec".to_string()]);

    let kaputt = idx.get("03 Unterricht/D-DV 05.10.2026.md").unwrap();
    assert!(kaputt.frontmatter_error.is_some());
    assert_eq!(kaputt.fach.as_deref(), Some("D-DV")); // aus dem Dateinamen
    assert_eq!(kaputt.tasks.len(), 1);

    let rechnen = idx.get("07 Material/Aufgabensammlung/Handelsspanne berechnen.md").unwrap();
    assert_eq!(rechnen.kind, NoteKind::Aufgabe);
    let typ = rechnen.fields.iter().find(|f| f.key == "typ").unwrap();
    assert_eq!(typ.value, serde_json::json!("Rechnen"));

    let gec = idx.get("03 Unterricht/GEC 05.10.2026.md").unwrap();
    assert_eq!(gec.tasks.len(), 3, "Aufgabe im Codeblock darf nicht zählen");
    let tags: Vec<&str> = gec.inline_tags.iter().map(|t| t.tag.as_str()).collect();
    assert_eq!(tags, vec!["frage", "nochmal"]);
    assert!(gec.links.iter().any(|l| l.embed));

    let woche = idx.get("04 Betrieb/Wochen/2026-KW41.md").unwrap();
    assert_eq!(woche.kind, NoteKind::BerichtsheftWoche);
}

#[test]
fn testvault_rundlauf_ist_byte_gleich() {
    let res = scan::scan_vault(&root()).unwrap();
    for rel in res.notes {
        let bytes = std::fs::read(root().join(&rel)).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(NoteText::parse(&text).render().as_bytes(), &bytes[..], "{rel}");
    }
}

#[test]
fn stundenplan_im_testvault() {
    let text = std::fs::read_to_string(root().join(stundenplan::STUNDENPLAN_PATH)).unwrap();
    let plan = stundenplan::parse_stundenplan(NoteText::parse(&text).body());
    let mo: Vec<String> = plan.tag(Wochentag::Montag).unwrap().bloecke().into_iter().map(|b| b.fach).collect();
    assert_eq!(mo, ["GEC", "D-DV", "WSP", "GEC", "D"]);
}
