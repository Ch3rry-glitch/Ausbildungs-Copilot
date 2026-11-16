//! Harte Regeln des Ausbildungsnachweises als reine Funktionen.
//!
//! * **Regel 3** ([`diagnose_wort`]): im Berichtsheft steht nur „arbeitsunfähig erkrankt“. Die
//!   Wortliste enthält nur eindeutig medizinische Wörter – Schulthemen wie „Operationen in Formeln“,
//!   „Symptome schwacher Absatzzahlen“ oder Kleidung („Jeanshose“) treffen nicht.
//! * **Regel 4** ([`ist_luecke`], [`staemme`], [`hat_beleg`]): dünne Stichpunkte werden markiert,
//!   nicht ausgeschmückt; jeder Satz der KI braucht ein Belegwort aus den Quellen des Tages.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::logbuch::ohne_markdown;

/// Kleinbuchstaben, Umlaute gefaltet (ä → a, ö → o, ü → u, ß → ss).
pub fn falten(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push('a'),
            'ö' => out.push('o'),
            'ü' => out.push('u'),
            'ß' => out.push_str("ss"),
            _ => out.push(c),
        }
    }
    out
}

/// Zweite Faltung für umschriebene Umlaute („Erkaeltung“ = „Erkältung“).
fn falten_ae(s: &str) -> String {
    falten(s).replace("ae", "a").replace("oe", "o").replace("ue", "u")
}

// ---------------------------------------------------------------------------------------------
// Regel 3 – keine Diagnose
// ---------------------------------------------------------------------------------------------

/// Eindeutig medizinische Wortanfänge. **Kein** allgemeines „-ose“ (Hose, Dose, Prognose).
/// „krank“, „erkrankt“, „arbeitsunfähig“, „Krankmeldung“, „AU“ sind erlaubt.
const DIAGNOSE_WOERTER: &[&str] = &[
    "diagnose", "grippe", "grippal", "erkältung", "infekt", "fieber", "migräne", "kopfschmerz",
    "bauchschmerz", "rückenschmerz", "zahnschmerz", "halsschmerz", "magen-darm", "durchfall",
    "erbrechen", "übelkeit", "corona", "covid", "angina", "mandelentzündung", "entzündung", "allergie",
    "asthma", "verstauchung", "zerrung", "bänderriss", "knochenbruch", "operiert", "depression",
    "burnout", "panikattacke", "arthrose", "thrombose", "neurose", "tuberkulose", "borreliose",
];

static DIAGNOSE_GEFALTET: LazyLock<Vec<String>> = LazyLock::new(|| DIAGNOSE_WOERTER.iter().map(|w| falten_ae(w)).collect());
static WORT_MIT_BINDESTRICH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}+(?:-\p{L}+)*").unwrap());

fn ist_diagnose_teil(teil: &str) -> bool {
    let f = falten_ae(teil);
    DIAGNOSE_GEFALTET.iter().any(|w| f.starts_with(w.as_str()))
        // Bronchitis, Gastritis … (mindestens 7 Buchstaben)
        || (f.ends_with("itis") && teil.chars().count() >= 7)
        // Blasenentzündung, Mittelohrentzündung …
        || f.ends_with("entzundung")
}

/// Erstes Diagnose-Wort im Text (so geschrieben wie dort), sonst `None`. Ohne Groß/Klein, Umlaute
/// gefaltet, am Wortanfang; Wörter mit Bindestrich werden auch einzeln geprüft („Sommer-Grippe“).
pub fn diagnose_wort(text: &str) -> Option<String> {
    for m in WORT_MIT_BINDESTRICH.find_iter(text) {
        let wort = m.as_str();
        if ist_diagnose_teil(wort) || wort.split('-').any(ist_diagnose_teil) {
            return Some(wort.to_string());
        }
    }
    None
}

/// Meldung, wenn eine Gesundheitsangabe geschrieben werden soll (Regel 3).
pub fn diagnose_meldung(wort: &str) -> String {
    format!("„{wort}“ ist eine Gesundheitsangabe – im Berichtsheft steht nur „arbeitsunfähig erkrankt“ (Tagesart wählen).")
}

// ---------------------------------------------------------------------------------------------
// Sondertage
// ---------------------------------------------------------------------------------------------

/// Art eines Sondertags aus dem Anfang einer Zeile (Markdown entfernt, ohne Groß/Klein).
pub fn sondertag(text: &str) -> Option<crate::model::BerichtsTagArt> {
    use crate::model::BerichtsTagArt as A;
    let t = falten(&ohne_markdown(text));
    let t = t.trim_start_matches(|c: char| !c.is_alphanumeric());
    if t.starts_with("arbeitsunfahig") {
        Some(A::Krank)
    } else if t.starts_with("urlaub") {
        Some(A::Urlaub)
    } else if t.starts_with("feiertag") {
        Some(A::Feiertag)
    } else if t.starts_with("entfallen") {
        Some(A::Entfallen)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------------------------
// Regel 4 – Lücken und Belege
// ---------------------------------------------------------------------------------------------

/// Füllwörter: zählen weder als Gegenstand eines Stichpunkts noch als Beleg eines Satzes.
const STOPPWOERTER: &[&str] = &[
    "ich", "wir", "du", "er", "sie", "es", "man", "mich", "mir", "uns", "ihm", "ihn", "ihnen", "der",
    "die", "das", "den", "dem", "des", "ein", "eine", "einer", "eines", "einem", "einen", "und", "oder",
    "aber", "auch", "noch", "sich", "mit", "von", "vom", "zum", "zur", "zu", "für", "auf", "aus", "bei",
    "beim", "nach", "über", "unter", "vor", "durch", "gegen", "ohne", "um", "bis", "als", "wie", "wenn",
    "dass", "weil", "damit", "dann", "denn", "doch", "schon", "sehr", "mehr", "viel", "viele", "vielen",
    "einige", "einigen", "alle", "allen", "allem", "jede", "jeder", "jedes", "jeden", "dort", "hier",
    "heute", "habe", "hat", "haben", "hatte", "hatten", "wurde", "wurden", "werden", "worden", "wird",
    "war", "waren", "sein", "ist", "sind", "dabei", "sowie", "außerdem", "zudem", "anschließend",
    "danach", "diese", "dieser", "dieses", "diesen", "diesem", "konnte", "konnten", "sollte", "sollten",
    "musste", "mussten", "muss", "kann", "können", "seine", "seiner", "ihre", "ihrer", "unser", "unsere",
    "meine", "meiner", "meinem", "meinen", "mein", "nicht", "kein", "keine", "keinen", "welche",
    "welcher", "welches", "etwa", "etwas", "sowohl", "weitere", "weiteren", "weiterhin", "zunächst",
    "erst", "ebenfalls", "jeweils", "hierbei", "wobei", "sodass", "insbesondere", "besonders",
    "zwischen", "während", "wieder", "immer", "bereits", "dafür", "darüber", "darauf", "daran", "davon",
    "dazu", "hierzu", "im", "in", "am", "an", "ins", "so", "nur", "dies", "denen", "deren", "dessen",
    "einmal", "ganz", "gut", "neue", "neuen", "neuer", "neues", "sowie", "selbst", "eigene", "eigenen",
];

static STOPP: LazyLock<BTreeSet<String>> = LazyLock::new(|| STOPPWOERTER.iter().map(|w| falten(w)).collect());

/// Wörter ohne Gegenstand („Einschulung“ ohne „wovon?“).
const VAGE_WOERTER: &[&str] = &[
    "einschulung", "schulung", "einarbeitung", "einweisung", "unterweisung", "besprechung", "meeting",
    "termin", "orga", "organisatorisches", "sonstiges", "diverses", "verschiedenes", "allgemeines",
    "aufgaben", "arbeit", "arbeiten", "projekt", "projektarbeit", "mithilfe", "hilfe", "unterstützung",
    "aushilfe", "recherche", "vorbereitung", "nacharbeit", "kennenlernen", "rundgang", "büro",
    "bürotätigkeiten",
];

static VAGE: LazyLock<Vec<String>> = LazyLock::new(|| VAGE_WOERTER.iter().map(|w| falten(w)).collect());

fn ist_vage(wort_gefaltet: &str) -> bool {
    VAGE.iter().any(|v| {
        wort_gefaltet == v
            || ["s", "e", "n", "en"].iter().any(|s| wort_gefaltet.strip_suffix(s) == Some(v.as_str()))
    })
}

/// Wörter eines Stichpunkts: Markdown und Links entfernt, Satzzeichen weg (Bindestrich im Wort bleibt).
fn woerter(text: &str) -> Vec<String> {
    let rein = ohne_markdown(text);
    let ohne_zeichen: String =
        rein.chars().map(|c| if c.is_alphanumeric() || c == '-' || c.is_whitespace() { c } else { ' ' }).collect();
    ohne_zeichen.split_whitespace().map(|w| w.trim_matches('-').to_string()).filter(|w| !w.is_empty()).collect()
}

/// „Ohne Substanz“ (Regel 4): ein einziges Wort, oder bis zu drei Wörter, deren Nicht-Füllwörter
/// alle vage sind („Besprechung“, „Projektarbeit“, „Hilfe im Büro“). Sondertag-Zeilen sind nie Lücken.
pub fn ist_luecke(text: &str) -> bool {
    if sondertag(text).is_some() {
        return false;
    }
    let w = woerter(text);
    match w.len() {
        0 | 1 => true,
        2 | 3 => {
            let gegenstand: Vec<String> = w.iter().map(|x| falten(x)).filter(|x| !STOPP.contains(x)).collect();
            gegenstand.iter().all(|x| ist_vage(x))
        }
        _ => false,
    }
}

static BUCHSTABEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\p{L}+").unwrap());

/// Belegwort-Stämme: erste 5 Zeichen (gefaltet) jedes Wortes mit mindestens 4 Buchstaben, ohne Füllwörter.
pub fn staemme(text: &str) -> BTreeSet<String> {
    BUCHSTABEN
        .find_iter(text)
        .map(|m| m.as_str())
        .filter(|w| w.chars().count() >= 4)
        .map(falten)
        .filter(|w| !STOPP.contains(w))
        .map(|w| w.chars().take(5).collect())
        .collect()
}

/// Gleicher Stamm – ein 4-Zeichen-Stamm („shop“) passt auch als Anfang („shops“).
fn stamm_passt(a: &str, b: &str) -> bool {
    a == b || (a.chars().count() == 4 && b.starts_with(a)) || (b.chars().count() == 4 && a.starts_with(b))
}

/// Teilt die Stämme der beiden Mengen mindestens einen?
pub fn gemeinsamer_stamm(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    a.iter().any(|x| b.iter().any(|y| stamm_passt(x, y)))
}

/// Anzahl gemeinsamer Stämme (Lernfeld-Vorschlag aus Betrieb.md: meiste Treffer gewinnen).
pub fn treffer(a: &BTreeSet<String>, b: &BTreeSet<String>) -> usize {
    a.iter().filter(|x| b.iter().any(|y| stamm_passt(x, y))).count()
}

static SATZENDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[.!?]+(?:\s+|$)").unwrap());

/// Sätze: getrennt an `.`/`!`/`?` mit Leerraum oder Textende danach.
pub fn saetze(text: &str) -> Vec<String> {
    SATZENDE.split(text).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Hat der Satz ein Belegwort in den Quellen? Ein Satz ohne ein einziges inhaltliches Wort
/// („z.“) behauptet nichts und gilt als belegt.
pub fn hat_beleg(satz: &str, quellen: &BTreeSet<String>) -> bool {
    let s = staemme(satz);
    s.is_empty() || gemeinsamer_stamm(&s, quellen)
}

static ZAHL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());

/// Alle Ziffernfolgen eines Textes.
pub fn zahlen(text: &str) -> BTreeSet<String> {
    ZAHL.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

static FETT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+?)\*\*").unwrap());

/// `**fett**`-Begriffe eines Textes.
pub fn fette_begriffe(text: &str) -> Vec<String> {
    FETT.captures_iter(text).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Vergleichsform für Tätigkeiten und Lücken: ohne Markdown, gefaltet, nur Buchstaben/Ziffern, Leerraum
/// zusammengefasst.
pub fn normalisiert(text: &str) -> String {
    let f = falten(&ohne_markdown(text));
    let s: String = f.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BerichtsTagArt;

    #[test]
    fn regel3_diagnose_woerter_treffen() {
        for (text, wort) in [
            ("Grippe, beim Arzt", "Grippe"),
            ("starke Kopfschmerzen", "Kopfschmerzen"),
            ("Erkältung", "Erkältung"),
            ("Erkaeltung seit Montag", "Erkaeltung"),
            ("MIGRÄNE", "MIGRÄNE"),
            ("Magen-Darm-Infekt", "Magen-Darm-Infekt"),
            ("Sommer-Grippe", "Sommer-Grippe"),
            ("Bronchitis", "Bronchitis"),
            ("eine Gastritis", "Gastritis"),
            ("Mittelohrentzündung", "Mittelohrentzündung"),
            ("am Knie operiert", "operiert"),
            ("Corona-Test positiv", "Corona-Test"),
            ("Diagnose: Burnout", "Diagnose"),
        ] {
            assert_eq!(diagnose_wort(text).as_deref(), Some(wort), "{text}");
        }
    }

    #[test]
    fn regel3_keine_fehltreffer_bei_schulthemen_und_kleidung() {
        for text in [
            "Operationen in Formeln",
            "Symptome schwacher Absatzzahlen",
            "psychische Belastung am Arbeitsplatz",
            "Verletzung der Sorgfaltspflicht",
            "Prognose der Absatzzahlen",
            "Jeanshose, Hose, Dose, Rose fotografiert",
            "lose Ware verpackt",
            "krank",
            "erkrankt",
            "Arbeitsunfähig erkrankt — ganztägig",
            "Krankmeldung an den Betrieb geschickt, AU eingereicht",
            "Wettbewerbsverzerrung",
            "Kritik",
        ] {
            assert_eq!(diagnose_wort(text), None, "{text}");
        }
    }

    #[test]
    fn regel3_prompts_fragen_nie_nach_krankheit() {
        for (name, text) in crate::ki::eingebaute_prompts().into_iter().filter(|(n, _)| n.starts_with("berichtsheft")) {
            assert_eq!(diagnose_wort(text), None, "{name}: Diagnose-Wort im Prompt");
            for zeile in text.lines().filter(|l| l.contains('?')) {
                let f = falten(zeile);
                for w in ["krank", "gesund", "arzt", "symptom", "beschwerd", "warum gefehlt"] {
                    assert!(!f.contains(w), "{name}: Frage nach Krankheit: {zeile}");
                }
            }
            assert!(falten(text).contains("keine gesundheitsangaben"), "{name}: Verbot fehlt");
        }
        // die Repository-Dateien sind die eingebauten
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../prompts/berichtsheft.md");
        assert_eq!(std::fs::read_to_string(repo).unwrap().replace("\r\n", "\n"), crate::ki::eingebaute_prompts()[1].1.replace("\r\n", "\n"));
    }

    #[test]
    fn sondertage_erkennen() {
        assert_eq!(sondertag("**Arbeitsunfähig erkrankt** — ganztägig"), Some(BerichtsTagArt::Krank));
        assert_eq!(sondertag("arbeitsunfähig erkrankt"), Some(BerichtsTagArt::Krank));
        assert_eq!(sondertag("**Urlaub**"), Some(BerichtsTagArt::Urlaub));
        assert_eq!(sondertag("Feiertag (Tag der Einheit)"), Some(BerichtsTagArt::Feiertag));
        assert_eq!(sondertag(" Entfallen"), Some(BerichtsTagArt::Entfallen));
        assert_eq!(sondertag("Artikel im Shop gepflegt"), None);
    }

    #[test]
    fn regel4_luecken() {
        for t in ["Einschulung", "Meeting", "Besprechung", "Projektarbeit", "Hilfe im Büro", "Aufgaben", "Termine", "**Orga**", ""] {
            assert!(ist_luecke(t), "{t}");
        }
        for t in [
            "Artikel im Shop gepflegt",
            "Besprechung mit dem Chef zur Retourenquote",
            "Retouren bearbeitet",
            "Einschulung LEBK (14:30-16:00)",
            "**Arbeitsunfähig erkrankt** — ganztägig",
            "Entfallen",
            "Urlaub",
        ] {
            assert!(!ist_luecke(t), "{t}");
        }
    }

    #[test]
    fn belege_und_saetze() {
        let quellen = staemme("Artikel im Shop gepflegt");
        assert!(quellen.contains("artik") && quellen.contains("shop") && quellen.contains("gepfl"));
        assert!(hat_beleg("Ich habe die Artikelbeschreibungen im Shopsystem gepflegt", &quellen));
        assert!(!hat_beleg("Außerdem habe ich die Kundenhotline betreut", &quellen));
        assert!(hat_beleg("Ich war da", &quellen), "kein inhaltliches Wort → keine Behauptung");
        assert_eq!(saetze("Erster Satz. Zweiter! Dritter? 10.11.2026 bleibt"), vec!["Erster Satz", "Zweiter", "Dritter", "10.11.2026 bleibt"]);
        assert_eq!(zahlen("38 Stunden, 10.11.2026"), ["10", "11", "2026", "38"].iter().map(|s| s.to_string()).collect());
        assert_eq!(fette_begriffe("In **GEC** und **D-DV**"), vec!["GEC", "D-DV"]);
        assert_eq!(normalisiert("**Artikel** im  Shop gepflegt."), "artikel im shop gepflegt");
        assert_eq!(treffer(&staemme("Preisfindung neuer Artikel"), &quellen), 1);
    }
}
