//! M2-Integrationstests: Logbuch über einer **Kopie** von `fixtures/vault`.
//!
//! * Regel 1: nach „Kopieren“ steht der Eintragstext nirgends im Vault (Volltextsuche über alle
//!   Dateien) und nicht im App-Datenordner; in der Notiz ändert sich nur `logbuch:`.
//! * Regel 2: ohne Zahl kein Kopiertext; GEC hat genau vier Abschnitte, Bewertung leer.
//! * Regel 6 / 10: EK und `10 Privat/` gehen nicht an die KI (Attrappe merkt sich die Anfrage).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use chrono::NaiveDate;

use ausbildungs_cockpit_lib::error::{AppError, AppResult};
use ausbildungs_cockpit_lib::index::VaultIndex;
use ausbildungs_cockpit_lib::ki::{LlmAnfrage, LlmAntwort, LlmProvider};
use ausbildungs_cockpit_lib::logbuch::{self, KiZugang};
use ausbildungs_cockpit_lib::model::{
    CreateResult, LogbuchEntwurf, LogbuchFach, LogbuchStatus, NeueUnterrichtsnotiz, TextAbschnitt,
};
use ausbildungs_cockpit_lib::unterricht;
use ausbildungs_cockpit_lib::watcher::SharedIndex;
use ausbildungs_cockpit_lib::write::{self, RetryPolicy, SaveOutcome, SelfWrites};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("fixtures").join("vault")
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue; // Regel 8
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

fn gec() -> LogbuchFach {
    logbuch::voreinstellung("GEC")
}

const GEC_ZIEL: &str = "GEC – Geschäftsprozesse im E-Commerce";

/// Alle Dateien unter `dir` (rekursiv, jedes Format) als Text.
fn alle_dateien(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if e.file_type().unwrap().is_dir() {
            alle_dateien(&p, out);
        } else {
            out.push((p.clone(), String::from_utf8_lossy(&fs::read(&p).unwrap()).replace("\r\n", "\n")));
        }
    }
}

fn nirgends(dir: &Path, needle: &str) {
    let mut files = Vec::new();
    alle_dateien(dir, &mut files);
    for (p, text) in files {
        assert!(!text.contains(needle), "„{needle}“ steht in {}", p.display());
    }
}

fn entwurf(root: &Path, idx: &VaultIndex, cfg: &LogbuchFach, datum: NaiveDate, ki: Option<KiZugang>, privat: bool) -> AppResult<LogbuchEntwurf> {
    let metas: Vec<_> = logbuch::quellnotizen(idx, &cfg.kurz, GEC_ZIEL, datum).into_iter().cloned().collect();
    let quellen = logbuch::quellen_lesen(root, &metas)?;
    let klausuren = logbuch::klausuren(idx, &cfg.kurz, GEC_ZIEL, datum, ki.is_none() || privat);
    logbuch::entwurf_erstellen(cfg, GEC_ZIEL, datum, &quellen, &klausuren, ki, privat)
}

fn text_abschnitte(e: &LogbuchEntwurf) -> Vec<TextAbschnitt> {
    e.abschnitte
        .iter()
        .map(|a| TextAbschnitt { titel: a.titel.clone(), punkte: a.punkte.iter().map(|p| p.text.clone()).collect() })
        .collect()
}

/// KI-Attrappe: feste Antwort, merkt sich jede Anfrage.
struct Attrappe {
    antwort: String,
    anfragen: Mutex<Vec<LlmAnfrage>>,
}

impl Attrappe {
    fn neu(antwort: &str) -> Self {
        Attrappe { antwort: antwort.into(), anfragen: Mutex::new(vec![]) }
    }
    fn gesendet(&self) -> String {
        self.anfragen.lock().unwrap().iter().map(|a| format!("{}\n{}", a.system, a.nutzer)).collect()
    }
}

impl LlmProvider for Attrappe {
    fn modell(&self) -> &str {
        "claude-opus-5-5"
    }
    fn senden(&self, a: &LlmAnfrage) -> AppResult<LlmAntwort> {
        self.anfragen.lock().unwrap().push(a.clone());
        Ok(LlmAntwort { text: self.antwort.clone(), eingabe_tokens: 1200, ausgabe_tokens: 300, modell: "claude-opus-5-5".into(), stop_reason: "end_turn".into() })
    }
}

const KI_SATZ: &str = "Ich habe heute die Vorwärtskalkulation Schritt für Schritt bis zum Listenverkaufspreis nachvollzogen.";

fn ki_antwort() -> String {
    format!(
        r#"{{"erarbeitet":["{KI_SATZ}","Für mich mitgenommen: Zuschläge bauen aufeinander auf."],"fragen":["Ich frage mich, warum Skonto vom Zielverkaufspreis berechnet wird."],"vorgeschlagene_fragen":[],"naechstes":["Ich fülle bis zum 08.10.2026 das Arbeitsblatt zum Kalkulationsschema aus."]}}"#
    )
}

#[test]
fn gec_eintrag_aus_zwei_bloecken_hat_vier_abschnitte_und_leere_bewertung() {
    let (_tmp, root) = vault_copy();
    let idx = VaultIndex::build(&root).unwrap();
    let e = entwurf(&root, &idx, &gec(), d(2026, 10, 5), None, false).unwrap();
    assert_eq!(e.kopfzeile, "05.10.2026");
    assert_eq!(e.quellen.len(), 2, "beide GEC-Blöcke des Tages zusammengeführt");
    assert_eq!(e.status, LogbuchStatus::Offen, "niedrigster Status (offen + kopiert)");
    assert_eq!(e.abschnitte.len(), 3);
    assert_eq!(e.bewertung.as_ref().map(|b| b.skala), Some(10));
    let erarbeitet: Vec<&str> = e.abschnitte[0].punkte.iter().map(|p| p.text.as_str()).collect();
    assert!(erarbeitet.contains(&"Vorwärtskalkulation vom Listeneinkaufspreis bis zum Listenverkaufspreis"), "{erarbeitet:?}");
    assert!(erarbeitet.iter().any(|p| p.starts_with("Wichtige Begriffe: Deckungsbeitrag")), "{erarbeitet:?}");
    let fragen: Vec<&str> = e.abschnitte[1].punkte.iter().map(|p| p.text.as_str()).collect();
    assert!(fragen.iter().any(|f| f.starts_with("Warum rechnet man Skonto")), "{fragen:?}");
    assert!(fragen.contains(&"Noch nicht verstanden: Rückwärtskalkulation mit Rabatt"), "{fragen:?}");
    assert!(e.abschnitte.iter().flat_map(|a| &a.punkte).all(|p| !p.vorschlag), "ohne KI keine Vorschläge");
    let naechstes: Vec<&str> = e.abschnitte[2].punkte.iter().map(|p| p.text.as_str()).collect();
    assert!(naechstes.contains(&"Arbeitsblatt Kalkulationsschema ausfüllen (bis 08.10.2026)"), "{naechstes:?}");
    assert!(!naechstes.iter().any(|n| n.contains("Übungsaufgabe")), "erledigte Aufgabe zählt nicht");

    let vorschau = logbuch::render_text(&gec(), e.datum, &text_abschnitte(&e), None).unwrap();
    assert!(vorschau.starts_with("05.10.2026\n\nDies habe ich erarbeitet:\n- "));
    assert!(vorschau.ends_with("\n\nBewertung dieser Lerneinheit: __/10"), "{vorschau}");
    let abschnitte = vorschau.lines().filter(|l| l.ends_with(':') || l.starts_with("Bewertung dieser Lerneinheit:")).count();
    assert_eq!(abschnitte, 4, "{vorschau}");
    // Regel 2: ohne Zahl kein Kopiertext
    assert!(matches!(logbuch::kopiertext(&gec(), e.datum, &text_abschnitte(&e), None), Err(AppError::BewertungFehlt(10))));
}

#[test]
fn regel1_e2e_notiz_anlegen_logbuch_kopieren_text_nicht_im_vault() {
    let (tmp, root) = vault_copy();
    let config_dir = tmp.path().join("appdata");
    let writes = SelfWrites::new();
    let index: SharedIndex = Arc::new(RwLock::new(Some(VaultIndex::build(&root).unwrap())));
    // 1. Notiz anlegen (M1-Weg)
    let req = NeueUnterrichtsnotiz { datum: d(2026, 11, 16), fach: "GEC".into(), block: 1, lernfeld: None, thema: Some("Kalkulation".into()) };
    let rel = match unterricht::create_note(&index, &root, &req, &writes, &RetryPolicy::none()).unwrap() {
        CreateResult::Created { report } => report.rel_path,
        other => panic!("{other:?}"),
    };
    // 2. Stoff einfügen und speichern
    let c = write::read_for_edit(&root, &rel, &RetryPolicy::none()).unwrap();
    let text = c
        .text
        .replace("## Inhalt der Stunde\n- \n", "## Inhalt der Stunde\n- Bezugskalkulation mit Liefererrabatt und Skonto\n- Handlungskostenzuschlag auf den Bezugspreis\n")
        .replace("- #frage \n", "- Wie hoch ist ein üblicher Handlungskostenzuschlag? #frage\n")
        .replace("- [ ]  📅 \n", "- [ ] Aufgabe 5 rechnen 📅 2026-11-19\n");
    assert!(matches!(write::save(&root, &rel, &text, &c.version, &RetryPolicy::none(), &writes).unwrap(), SaveOutcome::Saved { .. }));
    index.write().unwrap().as_mut().unwrap().upsert(&rel).unwrap();
    let vorher = fs::read_to_string(root.join(&rel)).unwrap();

    // 3. Logbuch erzeugen: ohne KI und mit KI (Attrappe)
    let g = index.read().unwrap();
    let idx = g.as_ref().unwrap();
    let ohne = entwurf(&root, idx, &gec(), req.datum, None, false).unwrap();
    assert_eq!(ohne.abschnitte[0].punkte[0].text, "Bezugskalkulation mit Liefererrabatt und Skonto");
    let p = Attrappe::neu(&ki_antwort());
    let zugang = KiZugang { provider: &p, system: "Systemprompt", verlauf_dir: Some(&config_dir) };
    let mit = entwurf(&root, idx, &gec(), req.datum, Some(zugang), false).unwrap();
    drop(g);
    assert!(mit.ki.is_some());
    assert_eq!(mit.abschnitte[0].punkte[0].text, KI_SATZ);

    // 4. Kopieren (Text entsteht nur im Speicher) → Status `kopiert`
    let kopie = logbuch::kopiertext(&gec(), mit.datum, &text_abschnitte(&mit), Some(7)).unwrap();
    assert!(kopie.contains(KI_SATZ) && kopie.ends_with("Bewertung dieser Lerneinheit: 7/10"));
    let pfade: Vec<String> = mit.quellen.iter().map(|q| q.path.clone()).collect();
    let r = logbuch::status_setzen(&root, &pfade, LogbuchStatus::Kopiert, true, &writes, Some(&config_dir.join("backups")), &RetryPolicy::none());
    assert_eq!(r.geschrieben.len(), 1, "{r:?}");
    assert!(r.fehler.is_empty() && r.sicherung.is_none(), "eine Datei → keine Sicherung");
    logbuch::verlauf_anhaengen(&config_dir, "GEC", mit.datum, LogbuchStatus::Kopiert).unwrap();

    // 5. In der Notiz hat sich nur die Statuszeile geändert
    let nachher = fs::read_to_string(root.join(&rel)).unwrap();
    assert_eq!(nachher, vorher.replace("lehrkraft: HEIN\n---\n", "lehrkraft: HEIN\nlogbuch: kopiert\n---\n"));

    // 6. Volltextsuche: weder der Eintrag noch seine Formulierungen liegen im Vault oder App-Datenordner
    for wurzel in [root.as_path(), config_dir.as_path()] {
        nirgends(wurzel, &kopie);
        nirgends(wurzel, KI_SATZ);
        nirgends(wurzel, "Bewertung dieser Lerneinheit: 7/10");
        nirgends(wurzel, "16.11.2026\n\nDies habe ich erarbeitet:");
        nirgends(wurzel, &logbuch::render_text(&gec(), ohne.datum, &text_abschnitte(&ohne), None).unwrap());
    }
    // Index enthält nie Text, nur den Status
    let idx = VaultIndex::build(&root).unwrap();
    let meta = idx.get(&rel).unwrap();
    assert_eq!(logbuch::status_von(meta), LogbuchStatus::Kopiert);
    assert!(!serde_json::to_string(meta).unwrap().contains(KI_SATZ));
}

#[test]
fn regel10_privat_und_regel6_ek_gehen_nicht_an_die_ki() {
    let (_tmp, root) = vault_copy();
    let privat = "---\ntags: [unterricht]\ndatum: 2026-10-05\nfach: GEC\n---\n# Privat\n\n## Inhalt der Stunde\n- Geheimer Lernstand Zeile\n";
    fs::create_dir_all(root.join("10 Privat/Lern-Log")).unwrap();
    fs::write(root.join("10 Privat/Lern-Log/GEC 3 05.10.2026.md"), privat).unwrap();
    let rel = "03 Unterricht/GEC 05.10.2026.md";
    let alt = fs::read_to_string(root.join(rel)).unwrap();
    fs::write(root.join(rel), alt.replace("- Handlungskostenzuschlag", "- EK: 12,50 € je Tasche\n- Handlungskostenzuschlag")).unwrap();
    let idx = VaultIndex::build(&root).unwrap();

    // ohne Erlaubnis: private Notiz bleibt draußen, EK-Zeile entfernt
    let p = Attrappe::neu(&ki_antwort());
    let e = entwurf(&root, &idx, &gec(), d(2026, 10, 5), Some(KiZugang { provider: &p, system: "s", verlauf_dir: None }), false).unwrap();
    let gesendet = p.gesendet();
    assert!(!gesendet.contains("Geheimer Lernstand"), "Regel 10");
    assert!(!gesendet.contains("12,50"), "Regel 6");
    assert!(gesendet.contains("Vorwärtskalkulation"));
    let q = e.quellen.iter().find(|q| q.privat).unwrap();
    assert!(!q.gesendet);
    assert!(e.quellen.iter().any(|q| q.ek_entfernt));
    assert!(e.warnungen.iter().any(|w| w.contains("10 Privat")), "{:?}", e.warnungen);

    // ausdrücklich für diesen Vorgang erlaubt → private Notiz geht mit
    let p2 = Attrappe::neu(&ki_antwort());
    entwurf(&root, &idx, &gec(), d(2026, 10, 5), Some(KiZugang { provider: &p2, system: "s", verlauf_dir: None }), true).unwrap();
    assert!(p2.gesendet().contains("Geheimer Lernstand"));
    assert!(!p2.gesendet().contains("12,50"), "EK bleibt trotzdem draußen");

    // auch die Vorlage ohne KI übernimmt keinen EK (Moodle ist außerhalb der App)
    let ohne = entwurf(&root, &idx, &gec(), d(2026, 10, 5), None, false).unwrap();
    assert!(!format!("{:?}", ohne.abschnitte).contains("12,50"));
}

#[test]
fn status_fuer_zwei_notizen_mit_sicherung_und_nur_der_statuszeile() {
    let (tmp, root) = vault_copy();
    let writes = SelfWrites::new();
    let pfade = vec!["03 Unterricht/GEC 05.10.2026.md".to_string(), "03 Unterricht/GEC 2 05.10.2026.md".to_string()];
    let vorher: Vec<String> = pfade.iter().map(|p| fs::read_to_string(root.join(p)).unwrap()).collect();
    let backups = tmp.path().join("backups");
    let r = logbuch::status_setzen(&root, &pfade, LogbuchStatus::Abgegeben, false, &writes, Some(&backups), &RetryPolicy::none());
    assert_eq!(r.geschrieben.len(), 2, "{r:?}");
    let sicherung = PathBuf::from(r.sicherung.expect("Sicherung bei mehr als einer Datei"));
    for (p, alt) in pfade.iter().zip(&vorher) {
        assert_eq!(&fs::read_to_string(sicherung.join(p)).unwrap(), alt, "Sicherung = Original");
        let neu = fs::read_to_string(root.join(p)).unwrap();
        let geaendert: Vec<(&str, &str)> = alt.lines().zip(neu.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(geaendert.len(), 1, "{geaendert:?}");
        assert!(geaendert[0].1 == "logbuch: abgegeben");
        assert_eq!(alt.lines().count(), neu.lines().count());
    }
    // Kopieren stuft `abgegeben` nie zurück
    let r = logbuch::status_setzen(&root, &pfade, LogbuchStatus::Kopiert, true, &writes, Some(&backups), &RetryPolicy::none());
    assert!(r.geschrieben.is_empty() && r.unveraendert.len() == 2, "{r:?}");
    // keine Unterrichtsnotiz / .obsidian → Fehler im Bericht, nichts geschrieben
    let r = logbuch::status_setzen(&root, &[".obsidian/app.json".to_string()], LogbuchStatus::Kopiert, false, &writes, None, &RetryPolicy::none());
    assert_eq!(r.fehler.len(), 1);
}

#[test]
fn uebersicht_je_fach_mit_status_und_offen_ab() {
    let (_tmp, root) = vault_copy();
    let idx = VaultIndex::build(&root).unwrap();
    let (plan, _) = unterricht::load_stundenplan(&root, &RetryPolicy::none());
    let faecher = unterricht::faecher(&idx, plan.as_ref());
    assert!(!faecher.iter().any(|f| f.ziel == "Logbuch GEC"), "Muster-Seite ist kein Fach");
    let u = logbuch::uebersicht(&idx, &[], &faecher);
    assert_eq!(u.faecher.len(), 1, "nur GEC ist voreingestellt aktiv");
    let g = &u.faecher[0];
    assert_eq!(g.kurz, "GEC");
    assert_eq!(g.moodle_url, logbuch::MOODLE_START);
    let daten: Vec<NaiveDate> = g.tage.iter().map(|t| t.datum).collect();
    assert_eq!(daten, vec![d(2026, 10, 5), d(2026, 9, 7)], "neueste zuerst, Altbestand dabei");
    assert_eq!(g.tage[0].notizen.len(), 2);
    assert_eq!(g.tage[0].status, LogbuchStatus::Offen);
    assert_eq!(g.offen, 2);
    let mut cfg = gec();
    cfg.offen_ab = Some(d(2026, 10, 1));
    let u = logbuch::uebersicht(&idx, &[cfg], &faecher);
    assert_eq!(u.faecher[0].offen, 1);
    assert!(!u.faecher[0].tage[1].zaehlt);
    // D-DV einschalten → erscheint
    let mut ddv = logbuch::voreinstellung("D-DV");
    ddv.aktiv = true;
    let u = logbuch::uebersicht(&idx, &[ddv], &faecher);
    assert_eq!(u.faecher.iter().map(|f| f.kurz.as_str()).collect::<Vec<_>>(), vec!["GEC", "D-DV"]);
}

#[test]
fn ohne_notiz_kein_entwurf_und_inaktives_fach() {
    let (_tmp, root) = vault_copy();
    let idx = VaultIndex::build(&root).unwrap();
    let e = entwurf(&root, &idx, &gec(), d(2026, 10, 6), None, false);
    assert!(matches!(e, Err(AppError::InvalidInput(m)) if m.contains("Keine Unterrichtsnotiz für GEC am 06.10.2026")));
}
