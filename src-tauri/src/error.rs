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
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
