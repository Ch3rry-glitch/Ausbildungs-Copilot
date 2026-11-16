//! App-Einstellungen in `%APPDATA%\ausbildungs-cockpit\config.json` – **nie** im Vault.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::darstellung::Darstellung;
use crate::error::{AppError, AppResult};
use crate::model::{FragenEinstellungen, LogbuchFach};

pub const APP_DIR_NAME: &str = "ausbildungs-cockpit";
const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// Format-Version der Datei, für spätere Migrationen
    #[serde(default = "default_version")]
    pub version: u32,
    pub vault_path: Option<PathBuf>,
    #[serde(default)]
    pub recent_vaults: Vec<PathBuf>,
    /// M2: Logbuch je Fach. Fehlt ein Fach hier, gilt die Voreinstellung
    /// ([`crate::logbuch::voreinstellung`]: GEC aktiv, alle anderen aus).
    #[serde(default)]
    pub logbuch: Vec<LogbuchFach>,
    /// M2: KI-Einstellungen (der Schlüssel liegt **nicht** hier, sondern im Anmeldeinformationsspeicher)
    #[serde(default)]
    pub ki: KiEinstellungen,
    /// M3: was „geklärt“ mit einer `#frage`-Zeile macht (Standard: Tag durchstreichen)
    #[serde(default)]
    pub fragen: FragenEinstellungen,
    /// Farbthema und Glasstufe der Oberfläche; fehlt oder unbekannt → bisheriges Aussehen
    #[serde(default, deserialize_with = "crate::darstellung::nachsichtig")]
    pub darstellung: Darstellung,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KiEinstellungen {
    /// Modell-ID; leer = Standardmodell
    #[serde(default)]
    pub modell: String,
}

fn default_version() -> u32 {
    1
}

impl AppConfig {
    pub fn set_vault(&mut self, path: PathBuf) {
        self.recent_vaults.retain(|p| p != &path);
        self.recent_vaults.insert(0, path.clone());
        self.recent_vaults.truncate(5);
        self.vault_path = Some(path);
    }
}

/// `%APPDATA%\ausbildungs-cockpit` (Roaming). Fällt auf das Arbeitsverzeichnis zurück, falls
/// Windows keinen Ordner liefert (sollte nie passieren).
pub fn app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR_NAME)
}

pub fn load_from(dir: &Path) -> AppResult<AppConfig> {
    let file = dir.join(CONFIG_FILE);
    match fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| AppError::Config(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig {
            version: default_version(),
            ..Default::default()
        }),
        Err(e) => Err(e.into()),
    }
}

/// Atomar schreiben: temporäre Datei im selben Ordner, dann umbenennen.
pub fn save_to(dir: &Path, cfg: &AppConfig) -> AppResult<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(cfg).map_err(|e| AppError::Config(e.to_string()))?;
    let tmp = dir.join(format!("{CONFIG_FILE}.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(&tmp, dir.join(CONFIG_FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn einstellungen_rundlauf_und_standard() {
        let dir = tempfile::tempdir().unwrap();
        let leer = load_from(dir.path()).unwrap();
        assert_eq!(leer.vault_path, None);
        assert_eq!(leer.version, 1);

        let mut cfg = leer.clone();
        cfg.set_vault(PathBuf::from("C:/A"));
        cfg.set_vault(PathBuf::from("C:/B"));
        cfg.set_vault(PathBuf::from("C:/A"));
        save_to(dir.path(), &cfg).unwrap();
        let wieder = load_from(dir.path()).unwrap();
        assert_eq!(wieder, cfg);
        assert_eq!(wieder.recent_vaults, vec![PathBuf::from("C:/A"), PathBuf::from("C:/B")]);
    }

    #[test]
    fn alte_config_ohne_fragen_bleibt_lesbar() {
        use crate::model::GeklaertModus;
        let dir = tempfile::tempdir().unwrap();
        // Stand M2: kein Schlüssel `fragen`
        fs::write(dir.path().join(CONFIG_FILE), r#"{"version":1,"vaultPath":"C:/V","recentVaults":[],"logbuch":[],"ki":{"modell":""}}"#)
            .unwrap();
        let cfg = load_from(dir.path()).unwrap();
        assert_eq!(cfg.vault_path, Some(PathBuf::from("C:/V")));
        assert_eq!(cfg.fragen.geklaert_modus, GeklaertModus::Durchstreichen);
        let mut cfg = cfg;
        cfg.fragen.geklaert_modus = GeklaertModus::Abschnitt;
        save_to(dir.path(), &cfg).unwrap();
        let text = fs::read_to_string(dir.path().join(CONFIG_FILE)).unwrap();
        assert!(text.contains(r#""geklaertModus": "abschnitt""#), "{text}");
        assert_eq!(load_from(dir.path()).unwrap(), cfg);
    }
}
