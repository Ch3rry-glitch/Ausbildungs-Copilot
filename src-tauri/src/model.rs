//! Datenmodelle, die zwischen Rust und der Oberfläche ausgetauscht werden.
//!
//! Grundsatz: Der Vault ist die einzige Wahrheit. Alles hier ist eine *Sicht* auf die
//! Markdown-Dateien und kann jederzeit verworfen und neu aufgebaut werden.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Pfad relativ zum Vault-Ordner, immer mit `/` getrennt (z. B. `03 Unterricht/GEC 07.09.2026.md`).
pub type RelPath = String;

/// Grobe Einordnung einer Notiz, abgeleitet aus Ordner, Dateiname und `tags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NoteKind {
    /// `03 Unterricht/<FACH> <TT.MM.JJJJ>.md` (auch Altbestand-Namen)
    Unterricht,
    /// `04 Betrieb/Wochen/<JJJJ>-KW<nn>.md`
    BerichtsheftWoche,
    /// `07 Material/Aufgabensammlung/<Titel>.md`
    Aufgabe,
    /// `06 Glossar/<Begriff>.md`
    Glossar,
    /// `01 Fächer/<Fach>/…`
    Fach,
    /// `02 Lernfelder/LFnn – ….md`
    Lernfeld,
    /// `99 Meta/Templates/…`
    Template,
    /// alles andere
    Sonstige,
}

/// Ein Frontmatter-Feld in der Reihenfolge, in der es in der Datei steht.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrontmatterField {
    pub key: String,
    pub value: serde_json::Value,
}

/// Aufgabe im Tasks-Plugin-Format: `- [ ] Text 📅 2026-11-05`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// 1-basierte Zeilennummer in der Datei (inkl. Frontmatter), damit späteres Abhaken
    /// genau diese Zeile trifft.
    pub line: usize,
    pub text: String,
    pub done: bool,
    /// Zeichen zwischen den Klammern (` `, `x`, `X`, `/`, `-` …) – unverändert aus der Datei.
    pub status_char: char,
    pub due: Option<NaiveDate>,
    /// Einrückung in Leerzeichen (Tabs zählen als 4) – für Unteraufgaben.
    pub indent: usize,
}

/// Inline-Tag im Text, z. B. `#frage` oder `#nochmal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineTag {
    /// ohne `#`, so geschrieben wie in der Datei
    pub tag: String,
    /// 1-basierte Zeilennummer in der Datei
    pub line: usize,
    /// die ganze Zeile (getrimmt) – Kontext für „Fragen und Lücken"
    pub context: String,
}

/// Wikilink `[[Ziel#Überschrift|Alias]]` oder Einbettung `![[…]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WikiLink {
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    pub embed: bool,
    pub line: usize,
}

/// Alles, was der Index über eine Notiz weiß. Enthält **nie** den vollen Text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteMeta {
    pub path: RelPath,
    /// Ordner relativ zum Vault, `""` für die Wurzel
    pub folder: RelPath,
    /// Dateiname ohne `.md`
    pub title: String,
    pub kind: NoteKind,
    pub fields: Vec<FrontmatterField>,
    /// Frontmatter-`tags` und Inline-Tags, ohne `#`, ohne Duplikate
    pub tags: Vec<String>,
    pub inline_tags: Vec<InlineTag>,
    pub tasks: Vec<Task>,
    pub links: Vec<WikiLink>,
    /// `datum` aus der Frontmatter, sonst aus dem Dateinamen
    pub datum: Option<NaiveDate>,
    /// `fach` aus der Frontmatter (Link wird auf den Alias reduziert), sonst aus dem Dateinamen
    pub fach: Option<String>,
    /// Änderungszeit in ms seit 1970 (für Konfliktschutz und Sortierung)
    pub modified_ms: i64,
    pub size: u64,
    /// Fehlermeldung, wenn die Frontmatter kein gültiges YAML ist (Notiz bleibt sichtbar)
    pub frontmatter_error: Option<String>,
    /// liegt unter `10 Privat/` → vom KI-Versand ausgeschlossen (Regel 10)
    pub private: bool,
}

/// Kurzform für Listen in der Oberfläche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub path: RelPath,
    pub title: String,
    pub kind: NoteKind,
    pub datum: Option<NaiveDate>,
    pub fach: Option<String>,
    pub modified_ms: i64,
    pub open_tasks: usize,
    pub tags: Vec<String>,
}

impl From<&NoteMeta> for NoteSummary {
    fn from(m: &NoteMeta) -> Self {
        NoteSummary {
            path: m.path.clone(),
            title: m.title.clone(),
            kind: m.kind,
            datum: m.datum,
            fach: m.fach.clone(),
            modified_ms: m.modified_ms,
            open_tasks: m.tasks.iter().filter(|t| !t.done).count(),
            tags: m.tags.clone(),
        }
    }
}

/// Eine Notiz zum Anzeigen: Metadaten + Text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteContent {
    pub meta: NoteMeta,
    /// Text nach der Frontmatter
    pub body: String,
    /// die Frontmatter wörtlich (ohne `---`), falls vorhanden
    pub frontmatter_raw: Option<String>,
}

/// Ordner im Vault für die Ordnerliste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    /// relativ zum Vault, `""` = Wurzel
    pub path: RelPath,
    pub name: String,
    pub depth: usize,
    /// Notizen direkt in diesem Ordner
    pub note_count: usize,
    /// Notizen in diesem Ordner und allen Unterordnern
    pub total_count: usize,
}

/// Zustand der App für den Startbildschirm.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub version: String,
    pub vault_path: Option<String>,
    pub vault_name: Option<String>,
    /// Vault gesetzt, Ordner existiert und ist indiziert
    pub vault_ok: bool,
    pub note_count: usize,
    pub watching: bool,
    /// Vorschlag für den Starter-Vault: `Dokumente/Ausbildungs-Vault`
    pub default_starter_path: String,
    pub config_dir: String,
}

/// Ergebnis von „Starter-Vault anlegen".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarterReport {
    pub target: String,
    pub created_files: usize,
    /// schon vorhandene Dateien – werden **nie** überschrieben
    pub skipped_existing: usize,
    pub created_dirs: usize,
}

/// Wochentage, an denen Unterricht sein kann.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Wochentag {
    Montag,
    Dienstag,
    Mittwoch,
    Donnerstag,
    Freitag,
}

impl Wochentag {
    pub const ALLE: [Wochentag; 5] = [
        Wochentag::Montag,
        Wochentag::Dienstag,
        Wochentag::Mittwoch,
        Wochentag::Donnerstag,
        Wochentag::Freitag,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Wochentag::Montag => "Montag",
            Wochentag::Dienstag => "Dienstag",
            Wochentag::Mittwoch => "Mittwoch",
            Wochentag::Donnerstag => "Donnerstag",
            Wochentag::Freitag => "Freitag",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALLE.into_iter().find(|w| w.name().eq_ignore_ascii_case(s.trim()))
    }

    pub fn from_chrono(w: chrono::Weekday) -> Option<Self> {
        use chrono::Weekday::*;
        match w {
            Mon => Some(Wochentag::Montag),
            Tue => Some(Wochentag::Dienstag),
            Wed => Some(Wochentag::Mittwoch),
            Thu => Some(Wochentag::Donnerstag),
            Fri => Some(Wochentag::Freitag),
            Sat | Sun => None,
        }
    }
}

/// Eine Zeile im Stundenplan: eine Unterrichtsstunde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stunde {
    pub nr: u8,
    /// z. B. `08:35 – 09:20`, wörtlich aus der Tabelle
    pub zeit: String,
    /// Kürzel, wie es angezeigt wird (Alias des Links), z. B. `GEC`
    pub fach: String,
    /// vollständiges Linkziel, z. B. `GEC – Geschäftsprozesse im E-Commerce`
    pub fach_ziel: Option<String>,
    pub lehrkraft: Option<String>,
}

/// Aufeinanderfolgende Stunden desselben Fachs = ein Block (= eine Unterrichtsnotiz).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub fach: String,
    pub fach_ziel: Option<String>,
    pub lehrkraft: Option<String>,
    pub von_stunde: u8,
    pub bis_stunde: u8,
    pub beginn: String,
    pub ende: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schultag {
    pub tag: Wochentag,
    pub stunden: Vec<Stunde>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stundenplan {
    pub tage: Vec<Schultag>,
}

/// Ein Fach, wie die App es kennt. Kommt später aus `01 Fächer/` + Einstellungen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fach {
    /// `GEC`
    pub kurz: String,
    /// `GEC – Geschäftsprozesse im E-Commerce` (= Dateiname der Fachnotiz ohne `.md`)
    pub ziel: String,
    pub lehrkraft: Option<String>,
}

impl Fach {
    /// Der Link, den die App in Notizen schreibt. **Nie** `[[WSP]]`, wenn die Fachnotiz
    /// anders heißt – immer vollständiges Ziel mit Alias.
    pub fn link(&self) -> String {
        if self.ziel == self.kurz {
            format!("[[{}]]", self.ziel)
        } else {
            format!("[[{}|{}]]", self.ziel, self.kurz)
        }
    }
}

/// Ereignis an die Oberfläche, wenn sich Dateien im Vault geändert haben.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultChanged {
    pub changed: Vec<RelPath>,
    pub removed: Vec<RelPath>,
    /// `true`, wenn der Index komplett neu aufgebaut wurde (z. B. Ordner umbenannt)
    pub full_rescan: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fach_link_hat_immer_vollstaendiges_ziel() {
        let wsp = Fach {
            kurz: "WSP".into(),
            ziel: "WSP – Wirtschafts- und Sozialprozesse".into(),
            lehrkraft: Some("SCMI".into()),
        };
        assert_eq!(wsp.link(), "[[WSP – Wirtschafts- und Sozialprozesse|WSP]]");
        let ddv = Fach { kurz: "D-DV".into(), ziel: "D-DV".into(), lehrkraft: None };
        assert_eq!(ddv.link(), "[[D-DV]]");
    }

    #[test]
    fn wochentag_aus_name_und_chrono() {
        assert_eq!(Wochentag::from_name(" montag "), Some(Wochentag::Montag));
        assert_eq!(Wochentag::from_chrono(chrono::Weekday::Thu), Some(Wochentag::Donnerstag));
        assert_eq!(Wochentag::from_chrono(chrono::Weekday::Sat), None);
    }
}
