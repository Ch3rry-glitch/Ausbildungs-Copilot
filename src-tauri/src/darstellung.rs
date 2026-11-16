//! Darstellung der Oberfläche: Farbthema und Glasstufe der Karten.
//!
//! Liegt in der App-Konfiguration (`config.json` im App-Datenordner), **nie** im Vault.
//! Lesen ist nachsichtig: ein unbekannter oder kaputter Wert (z. B. aus einer neueren App-Version
//! oder von Hand bearbeitet) fällt auf die Voreinstellung zurück, statt die ganze Konfiguration
//! unlesbar zu machen. Voreinstellung = das bisherige Aussehen (Violett, Mittel).

use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::commands::AppState;
use crate::config;
use crate::error::AppError;

/// Farbthema. Alle Themen sind dunkel (M1: „nur dunkel“). Die Farbwerte selbst stehen in
/// `src/themes.css`; hier steht nur, welches Thema gewählt ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Farbthema {
    /// dunkles Violett mit Mint-Akzent – das ursprüngliche Aussehen
    #[default]
    Violett,
    /// Nachtblau mit Himmelblau
    Ozean,
    /// dunkles Grün mit Smaragd
    Wald,
    /// Brombeer-Dunkel mit Rosé
    Rose,
    /// neutrales Anthrazit
    Graphit,
}

/// Wie durchsichtig die Glas-Karten sind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Glasstufe {
    /// durchsichtiger als bisher, der Hintergrund scheint stärker durch
    Klar,
    /// das bisherige Glas
    #[default]
    Mittel,
    /// fast deckend, ohne Weichzeichner – am besten lesbar
    Satt,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Darstellung {
    #[serde(default, deserialize_with = "nachsichtig")]
    pub farbe: Farbthema,
    #[serde(default, deserialize_with = "nachsichtig")]
    pub glas: Glasstufe,
}

impl Farbthema {
    /// Grundfarbe des Fensters (dunkelster Ton des Verlaufs, `--bg-1`) – sichtbar, bevor die
    /// Oberfläche gezeichnet ist und beim Vergrößern des Fensters.
    pub fn fensterfarbe(self) -> (u8, u8, u8) {
        match self {
            Farbthema::Violett => (0x14, 0x0a, 0x2e),
            Farbthema::Ozean => (0x06, 0x16, 0x30),
            Farbthema::Wald => (0x06, 0x1f, 0x18),
            Farbthema::Rose => (0x24, 0x0a, 0x1c),
            Farbthema::Graphit => (0x16, 0x17, 0x1b),
        }
    }
}

/// Feld nachsichtig lesen: passt der Wert nicht (unbekannte Variante, falscher Typ, `null`),
/// gilt die Voreinstellung. Für `#[serde(default, deserialize_with = "…")]`.
pub fn nachsichtig<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let wert = serde_json::Value::deserialize(d)?;
    Ok(T::deserialize(wert).unwrap_or_default())
}

/// Grundfarbe des Hauptfensters an das Thema anpassen. Fehler sind nur kosmetisch → nur protokollieren.
pub fn fensterfarbe_setzen(app: &AppHandle, farbe: Farbthema) {
    let Some(fenster) = app.get_webview_window("main") else { return };
    let (r, g, b) = farbe.fensterfarbe();
    if let Err(e) = fenster.set_background_color(Some(tauri::window::Color(r, g, b, 255))) {
        eprintln!("[darstellung] Fensterfarbe: {e}");
    }
}

// ---------- Tauri-Befehle ----------

#[tauri::command]
pub async fn darstellung(state: State<'_, AppState>) -> Result<Darstellung, AppError> {
    Ok(state.config.lock().unwrap().darstellung)
}

/// Speichert sofort (kein eigener Speichern-Knopf); schlägt das Schreiben fehl, bleibt die alte Wahl.
#[tauri::command]
pub async fn darstellung_speichern(
    app: AppHandle,
    state: State<'_, AppState>,
    darstellung: Darstellung,
) -> Result<Darstellung, AppError> {
    {
        let mut cfg = state.config.lock().unwrap();
        let alt = std::mem::replace(&mut cfg.darstellung, darstellung);
        if let Err(e) = config::save_to(&state.config_dir, &cfg) {
            cfg.darstellung = alt;
            return Err(e);
        }
    }
    fensterfarbe_setzen(&app, darstellung.farbe);
    Ok(darstellung)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_from, save_to};
    use std::fs;
    use std::path::PathBuf;

    const CONFIG: &str = "config.json";

    #[test]
    fn namen_in_der_datei() {
        let d = Darstellung { farbe: Farbthema::Rose, glas: Glasstufe::Satt };
        assert_eq!(serde_json::to_value(d).unwrap(), serde_json::json!({"farbe": "rose", "glas": "satt"}));
        for (f, n) in [
            (Farbthema::Violett, "violett"),
            (Farbthema::Ozean, "ozean"),
            (Farbthema::Wald, "wald"),
            (Farbthema::Rose, "rose"),
            (Farbthema::Graphit, "graphit"),
        ] {
            assert_eq!(serde_json::to_value(f).unwrap(), n);
        }
        for (g, n) in [(Glasstufe::Klar, "klar"), (Glasstufe::Mittel, "mittel"), (Glasstufe::Satt, "satt")] {
            assert_eq!(serde_json::to_value(g).unwrap(), n);
        }
    }

    #[test]
    fn voreinstellung_ist_das_bisherige_aussehen() {
        assert_eq!(Darstellung::default(), Darstellung { farbe: Farbthema::Violett, glas: Glasstufe::Mittel });
        // muss zu `backgroundColor` in tauri.conf.json passen
        assert_eq!(Farbthema::Violett.fensterfarbe(), (0x14, 0x0a, 0x2e));
    }

    #[test]
    fn rundlauf_ueber_die_config() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = load_from(dir.path()).unwrap();
        assert_eq!(cfg.darstellung, Darstellung::default());
        cfg.vault_path = Some(PathBuf::from("C:/V"));
        cfg.darstellung = Darstellung { farbe: Farbthema::Ozean, glas: Glasstufe::Satt };
        save_to(dir.path(), &cfg).unwrap();
        let text = fs::read_to_string(dir.path().join(CONFIG)).unwrap();
        assert!(text.contains(r#""farbe": "ozean""#) && text.contains(r#""glas": "satt""#), "{text}");
        assert_eq!(load_from(dir.path()).unwrap(), cfg);
    }

    #[test]
    fn alte_config_ohne_darstellung_bleibt_beim_bisherigen_aussehen() {
        let dir = tempfile::tempdir().unwrap();
        // Stand Klausur-Trainer: kein Schlüssel `darstellung`
        fs::write(
            dir.path().join(CONFIG),
            r#"{"version":1,"vaultPath":"C:/V","recentVaults":[],"logbuch":[],"ki":{"modell":""},"fragen":{"geklaertModus":"abschnitt"}}"#,
        )
        .unwrap();
        let cfg = load_from(dir.path()).unwrap();
        assert_eq!(cfg.vault_path, Some(PathBuf::from("C:/V")));
        assert_eq!(cfg.darstellung, Darstellung::default());
    }

    #[test]
    fn unbekannte_oder_kaputte_werte_fallen_auf_die_voreinstellung() {
        let dir = tempfile::tempdir().unwrap();
        let faelle = [
            // unbekanntes Thema, bekannte Glasstufe bleibt
            (r#"{"farbe":"neon","glas":"klar"}"#, Darstellung { farbe: Farbthema::Violett, glas: Glasstufe::Klar }),
            // bekanntes Thema, unbekannte Glasstufe
            (r#"{"farbe":"wald","glas":"milchig"}"#, Darstellung { farbe: Farbthema::Wald, glas: Glasstufe::Mittel }),
            // falsche Typen und null
            (r#"{"farbe":5,"glas":null}"#, Darstellung::default()),
            (r#"{"farbe":{"x":1}}"#, Darstellung::default()),
            // ein Feld fehlt
            (r#"{"glas":"satt"}"#, Darstellung { farbe: Farbthema::Violett, glas: Glasstufe::Satt }),
            // Groß-/Kleinschreibung zählt (die App schreibt nur Kleinbuchstaben)
            (r#"{"farbe":"Ozean"}"#, Darstellung::default()),
            // ganzer Block kaputt
            (r#""ozean""#, Darstellung::default()),
            ("null", Darstellung::default()),
            ("[]", Darstellung::default()),
        ];
        for (block, erwartet) in faelle {
            fs::write(dir.path().join(CONFIG), format!(r#"{{"version":1,"vaultPath":"C:/V","darstellung":{block}}}"#)).unwrap();
            let cfg = load_from(dir.path()).unwrap_or_else(|e| panic!("{block}: {e}"));
            assert_eq!(cfg.darstellung, erwartet, "{block}");
            // der Rest der Einstellungen geht dabei nicht verloren
            assert_eq!(cfg.vault_path, Some(PathBuf::from("C:/V")), "{block}");
        }
    }
}
