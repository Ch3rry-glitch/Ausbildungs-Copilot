//! Fehler, die bis in die Oberfläche gereicht werden. Meldungen auf Deutsch.

use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Kein Vault ausgewählt.")]
    NoVault,
    #[error("Der Ordner „{0}“ existiert nicht.")]
    NotFound(String),
    #[error("Zugriff verweigert: {0}")]
    Guard(#[from] crate::guard::GuardError),
    #[error("Dateifehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("Einstellungen fehlerhaft: {0}")]
    Config(String),
    #[error("Dateiwächter: {0}")]
    Watch(String),
    #[error("{0}")]
    Other(String),
    /// `read_note`: Datei nicht (mehr) vorhanden
    #[error("Die Notiz „{0}“ wurde nicht gefunden.")]
    NoteNotFound(String),
    #[error("Die Vorlage „{0}“ fehlt im Vault. Ohne Vorlage legt die App keine Unterrichtsnotiz an.")]
    TemplateMissing(String),
    #[error("„{0}“ ließ sich nicht speichern – die Datei ist gerade gesperrt (Obsidian oder Cloud-Sync) oder nicht beschreibbar. Dein Text ist nicht verloren – bitte gleich noch einmal speichern.")]
    Locked(String),
    #[error("„{0}“ ist schreibgeschützt. Entferne den Schreibschutz (Datei-Eigenschaften) und speichere dann erneut. Dein Text ist nicht verloren.")]
    ReadOnly(String),
    #[error("„{0}“ ist keine gültige UTF-8-Datei und wird nur angezeigt, nicht gespeichert.")]
    NotEditable(String),
    #[error("Das Fach „{0}“ steht weder im Stundenplan noch in „01 Fächer/“.")]
    UnknownFach(String),
    #[error("{0}")]
    InvalidInput(String),
    #[error("Entwicklungs-Build: Schreiben ist nur in einer Vault-Kopie unter %TEMP% erlaubt (oder COCKPIT_DEV_WRITE=1 setzen).")]
    DevWriteBlocked,
    /// Regel 2: ohne Zahl kein Kopieren
    #[error("Bitte zuerst die Bewertung (1–{0}) eintragen – ohne Zahl wird nicht kopiert.")]
    BewertungFehlt(u8),
    #[error("Kein KI-Schlüssel hinterlegt – Einstellungen → KI, oder „Ohne KI befüllen“.")]
    KiKeinSchluessel,
    #[error("KI: {0}")]
    Ki(String),
    #[error("Zwischenablage: {0}")]
    Zwischenablage(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
