//! Updates: neue Versionen über GitHub Releases (`latest.json`, Tauri-Updater-Plugin).
//!
//! * **Automatisch** nur im Release-Build, höchstens einmal pro Kalendertag und nie blockierend:
//!   ohne Netz bleibt die App still. Der Zeitpunkt der letzten Prüfung steht in
//!   `%APPDATA%\ausbildungs-cockpit\update.json` – nie im Vault.
//! * **Von Hand** („Nach Updates suchen“ in den Einstellungen) jederzeit, mit Fehlermeldung.
//! * Abgefragt wird nur `latest.json` des öffentlichen Repos; aus dem Vault verlässt nichts den
//!   Rechner, es gibt kein Konto und keinen eigenen Server (SPEC §12).
//! * Installiert wird nur auf Klick und nur ein Paket, dessen Signatur zum öffentlichen Schlüssel in
//!   `tauri.conf.json` passt (`requireSignedVersion`: die signierte Version muss die angekündigte
//!   sein). Steht dort noch der Platzhalter, sind Updates in diesem Build aus.
//! * Unter Windows startet der NSIS-Installer im Passiv-Modus, beendet die App und startet sie
//!   danach selbst neu; auf anderen Systemen startet die App nach der Installation neu.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, FixedOffset, Local, TimeZone};
use semver::{BuildMetadata, Version};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, Updater, UpdaterExt};

use crate::commands::AppState;
use crate::error::{AppError, AppResult};

/// Fortschritt beim Herunterladen ([`Fortschritt`])
pub const EVENT_UPDATE_FORTSCHRITT: &str = "update-fortschritt";
const STAND_DATEI: &str = "update.json";
/// Markierung im öffentlichen Schlüssel von `tauri.conf.json`, solange Levin keinen eingetragen hat
pub const PLATZHALTER: &str = "PLATZHALTER";
/// länger wartet die Prüfung nicht auf GitHub (langsames Schul-WLAN, kein Netz)
const ZEITLIMIT: Duration = Duration::from_secs(20);
/// Nur im Dev-Build: anderer Update-Server zum Ausprobieren der Oberfläche (`http://` erlaubt)
const ENV_DEV_URL: &str = "COCKPIT_UPDATE_URL";

/// Inhalt von `update.json`
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStand {
    /// letzte Prüfung, bei der der Update-Server erreichbar war (Ortszeit mit Versatz)
    #[serde(default)]
    pub letzte_pruefung: Option<DateTime<FixedOffset>>,
}

/// Ein gefundenes, neueres Update
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// Versionshinweise aus `latest.json` (Text der GitHub-Release)
    pub notizen: Option<String>,
    /// Veröffentlichung, RFC 3339
    pub datum: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// installierte Version
    pub version: String,
    /// öffentlicher Update-Schlüssel eingetragen (sonst sind Updates in diesem Build aus)
    pub eingerichtet: bool,
    /// prüft beim Start von selbst (nur Release-Build)
    pub automatisch: bool,
    /// RFC 3339
    pub letzte_pruefung: Option<String>,
    /// in dieser Sitzung schon gefunden
    pub verfuegbar: Option<UpdateInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePruefung {
    /// `false`: nicht nachgesehen (Dev-Build, heute schon geprüft oder Updates nicht eingerichtet)
    pub geprueft: bool,
    pub verfuegbar: Option<UpdateInfo>,
    /// nur bei der Prüfung von Hand – automatisch bleibt die App still
    pub fehler: Option<String>,
    pub letzte_pruefung: Option<String>,
}

/// Ereignis [`EVENT_UPDATE_FORTSCHRITT`]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Fortschritt {
    pub geladen: u64,
    pub gesamt: Option<u64>,
}

/// Laufzeit-Zustand (nur im Speicher)
#[derive(Default)]
pub struct UpdateState {
    gefunden: Mutex<Option<Update>>,
    installiert: AtomicBool,
}

// ---------- reine Logik (getestet) ----------

/// Ist `angeboten` ein Update für `aktuell`? Nur echte, neuere Versionen: nie zurück, nie dieselbe
/// Version mit anderen Build-Metadaten, keine Vorabversionen (`0.3.0-beta.1`) für Mitschüler.
pub fn ist_neuer(aktuell: &Version, angeboten: &Version) -> bool {
    if !angeboten.pre.is_empty() {
        return false;
    }
    ohne_build(angeboten) > ohne_build(aktuell)
}

fn ohne_build(v: &Version) -> Version {
    let mut v = v.clone();
    v.build = BuildMetadata::EMPTY;
    v
}

/// Automatische Prüfung fällig? Höchstens einmal pro Kalendertag (in der Zeitzone von `jetzt`).
/// Liegt die letzte Prüfung an einem anderen Tag – auch in der Zukunft, weil die Uhr verstellt
/// wurde –, ist sie fällig.
pub fn pruefung_faellig<Tz: TimeZone>(letzte: Option<&DateTime<FixedOffset>>, jetzt: &DateTime<Tz>) -> bool {
    match letzte {
        None => true,
        Some(t) => t.with_timezone(&jetzt.timezone()).date_naive() != jetzt.date_naive(),
    }
}

/// Steht in `tauri.conf.json` ein echter öffentlicher Schlüssel (und nicht der Platzhalter)?
pub fn schluessel_eingetragen(pubkey: &str) -> bool {
    let k = pubkey.trim();
    !k.is_empty() && !k.contains(PLATZHALTER)
}

/// Hat der Update-Server geantwortet? Nur dann zählt die Prüfung für „einmal am Tag“ –
/// ohne Netz wird beim nächsten Start wieder nachgesehen.
pub fn server_erreicht(e: &tauri_plugin_updater::Error) -> bool {
    use tauri_plugin_updater::Error as E;
    !matches!(e, E::Reqwest(_) | E::Network(_) | E::Io(_))
}

/// Fehler des Updaters als Satz für die Oberfläche
pub fn fehlertext(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Reqwest(_) | E::Network(_) => {
            "Keine Verbindung zum Update-Server (GitHub). Bist du online? Später noch einmal versuchen.".into()
        }
        E::ReleaseNotFound => "Auf GitHub ist noch keine Version mit Update-Datei (latest.json) veröffentlicht.".into(),
        E::TargetNotFound(_) | E::TargetsNotFound(_) => "Die neue Version enthält kein Paket für Windows.".into(),
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => {
            "Die Signatur des Updates passt nicht – aus Sicherheitsgründen wird nichts installiert.".into()
        }
        E::Semver(_) | E::Serialization(_) => "Die Update-Datei (latest.json) ist fehlerhaft.".into(),
        andere => format!("Update fehlgeschlagen: {andere}"),
    }
}

fn info(u: &Update) -> UpdateInfo {
    UpdateInfo {
        version: u.version.clone(),
        notizen: u.body.as_ref().map(|b| b.trim().to_string()).filter(|b| !b.is_empty()),
        datum: u
            .date
            .and_then(|d| DateTime::from_timestamp(d.unix_timestamp(), 0))
            .map(|d| d.with_timezone(&Local).to_rfc3339()),
    }
}

// ---------- update.json ----------

/// Fehlende oder kaputte Datei → leerer Stand (Prüfung fällig)
pub fn laden(dir: &Path) -> UpdateStand {
    fs::read_to_string(dir.join(STAND_DATEI))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Atomar: temporäre Datei im selben Ordner, dann umbenennen
pub fn speichern(dir: &Path, stand: &UpdateStand) -> AppResult<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(stand).map_err(|e| AppError::Config(e.to_string()))?;
    let tmp = dir.join(format!("{STAND_DATEI}.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(&tmp, dir.join(STAND_DATEI))?;
    Ok(())
}

// ---------- Updater ----------

/// Dev-Build mit `COCKPIT_UPDATE_URL` – im Release-Build nie
fn dev_url() -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var(ENV_DEV_URL).ok().filter(|u| !u.trim().is_empty())
}

fn eingerichtet(app: &AppHandle) -> bool {
    if dev_url().is_some() {
        return true;
    }
    let pubkey = app
        .config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .unwrap_or("");
    schluessel_eingetragen(pubkey)
}

fn updater(app: &AppHandle) -> Result<Updater, tauri_plugin_updater::Error> {
    let mut b = app
        .updater_builder()
        .timeout(ZEITLIMIT)
        .version_comparator(|aktuell, angeboten| ist_neuer(&aktuell, &angeboten.version));
    if let Some(url) = dev_url() {
        eprintln!("[update] Dev-Build: Update-Server {url}");
        b = b.endpoints(vec![url.parse()?])?;
    }
    b.build()
}

fn rfc3339(t: &DateTime<FixedOffset>) -> String {
    t.to_rfc3339()
}

// ---------- Befehle ----------

#[tauri::command]
pub fn update_status(app: AppHandle, state: State<'_, AppState>, upd: State<'_, UpdateState>) -> UpdateStatus {
    let stand = laden(&state.config_dir);
    UpdateStatus {
        version: app.package_info().version.to_string(),
        eingerichtet: eingerichtet(&app),
        automatisch: !cfg!(debug_assertions),
        letzte_pruefung: stand.letzte_pruefung.as_ref().map(rfc3339),
        verfuegbar: upd.gefunden.lock().unwrap().as_ref().map(info),
    }
}

/// Nach einem Update sehen. `von_hand = false` (App-Start): nur im Release-Build, nur einmal am
/// Tag, Fehler bleiben still.
#[tauri::command]
pub async fn update_pruefen(
    app: AppHandle,
    state: State<'_, AppState>,
    upd: State<'_, UpdateState>,
    von_hand: bool,
) -> Result<UpdatePruefung, AppError> {
    let dir = state.config_dir.clone();
    let mut stand = laden(&dir);
    let nicht_geprueft = |stand: &UpdateStand, fehler: Option<String>| UpdatePruefung {
        geprueft: false,
        verfuegbar: None,
        fehler,
        letzte_pruefung: stand.letzte_pruefung.as_ref().map(rfc3339),
    };
    if !eingerichtet(&app) {
        let text = "Updates sind in diesem Build nicht eingerichtet – der öffentliche Update-Schlüssel fehlt in tauri.conf.json.";
        return Ok(nicht_geprueft(&stand, von_hand.then(|| text.to_string())));
    }
    if !von_hand && (cfg!(debug_assertions) || !pruefung_faellig(stand.letzte_pruefung.as_ref(), &Local::now())) {
        return Ok(nicht_geprueft(&stand, None));
    }

    let ergebnis = match updater(&app) {
        Ok(u) => u.check().await,
        Err(e) => Err(e),
    };
    let erreicht = match &ergebnis {
        Ok(_) => true,
        Err(e) => {
            eprintln!("[update] {e}");
            server_erreicht(e)
        }
    };
    if erreicht {
        stand.letzte_pruefung = Some(Local::now().fixed_offset());
        if let Err(e) = speichern(&dir, &stand) {
            eprintln!("[update] {STAND_DATEI}: {e}");
        }
    }
    let letzte_pruefung = stand.letzte_pruefung.as_ref().map(rfc3339);
    match ergebnis {
        Ok(gefunden) => {
            let verfuegbar = gefunden.as_ref().map(info);
            *upd.gefunden.lock().unwrap() = gefunden;
            Ok(UpdatePruefung { geprueft: true, verfuegbar, fehler: None, letzte_pruefung })
        }
        Err(e) => Ok(UpdatePruefung {
            geprueft: true,
            verfuegbar: None,
            fehler: von_hand.then(|| fehlertext(&e)),
            letzte_pruefung,
        }),
    }
}

/// Gefundenes Update herunterladen, Signatur prüfen und installieren. Unter Windows beendet der
/// Installer die App (kehrt nicht zurück) und startet sie danach neu.
#[tauri::command]
pub async fn update_installieren(app: AppHandle, upd: State<'_, UpdateState>) -> Result<(), AppError> {
    let update = upd
        .gefunden
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| AppError::Other("Kein Update gefunden – bitte zuerst nach Updates suchen.".into()))?;
    if upd.installiert.swap(true, Ordering::SeqCst) {
        return Err(AppError::Other("Das Update wird bereits installiert.".into()));
    }
    let handle = app.clone();
    let mut geladen: u64 = 0;
    let mut gemeldet: Option<u64> = None;
    let ergebnis = update
        .download_and_install(
            move |stueck, gesamt| {
                geladen += stueck as u64;
                // höchstens ein Ereignis pro Prozent (ohne Gesamtgröße: alle 256 KiB)
                let stufe = match gesamt {
                    Some(g) if g > 0 => geladen * 100 / g,
                    _ => geladen / (256 * 1024),
                };
                if gemeldet != Some(stufe) {
                    gemeldet = Some(stufe);
                    let _ = handle.emit(EVENT_UPDATE_FORTSCHRITT, Fortschritt { geladen, gesamt });
                }
            },
            || {},
        )
        .await;
    match ergebnis {
        Ok(()) => app.restart(),
        Err(e) => {
            eprintln!("[update] {e}");
            upd.installiert.store(false, Ordering::SeqCst);
            Err(AppError::Other(fehlertext(&e)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn zeit(tz: i32, y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(tz * 3600)
            .unwrap()
            .from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap())
            .unwrap()
    }

    #[test]
    fn neuere_version_wird_angeboten() {
        assert!(ist_neuer(&v("0.1.0"), &v("0.1.1")));
        assert!(ist_neuer(&v("0.1.0"), &v("0.2.0")));
        assert!(ist_neuer(&v("0.9.9"), &v("1.0.0")));
        assert!(ist_neuer(&v("0.1.9"), &v("0.1.10")), "numerisch, nicht als Text vergleichen");
    }

    #[test]
    fn gleiche_oder_aeltere_version_nie() {
        assert!(!ist_neuer(&v("0.2.0"), &v("0.2.0")));
        assert!(!ist_neuer(&v("0.2.0"), &v("0.1.9")), "nie zurück");
        assert!(!ist_neuer(&v("1.0.0"), &v("0.9.9")));
        assert!(!ist_neuer(&v("0.2.0"), &v("0.2.0+ci.7")), "andere Build-Metadaten sind dieselbe Version");
    }

    #[test]
    fn keine_vorabversion_fuer_mitschueler() {
        assert!(!ist_neuer(&v("0.1.0"), &v("0.2.0-beta.1")));
        // wer selbst eine Vorabversion hat, bekommt die fertige Version
        assert!(ist_neuer(&v("0.2.0-beta.1"), &v("0.2.0")));
    }

    #[test]
    fn hoechstens_einmal_am_tag() {
        let jetzt = zeit(1, 2026, 11, 16, 10, 0);
        assert!(pruefung_faellig(None, &jetzt), "noch nie geprüft");
        assert!(!pruefung_faellig(Some(&zeit(1, 2026, 11, 16, 7, 55)), &jetzt), "heute früh schon geprüft");
        assert!(!pruefung_faellig(Some(&zeit(1, 2026, 11, 16, 0, 0)), &jetzt));
        assert!(pruefung_faellig(Some(&zeit(1, 2026, 11, 15, 23, 59)), &jetzt), "gestern");
        assert!(pruefung_faellig(Some(&zeit(1, 2026, 10, 1, 12, 0)), &jetzt));
        // Uhr zurückgestellt: letzte Prüfung „morgen“ → nicht ewig warten
        assert!(pruefung_faellig(Some(&zeit(1, 2026, 11, 17, 8, 0)), &jetzt));
    }

    #[test]
    fn kalendertag_in_der_eigenen_zeitzone() {
        // 23:30 UTC am 15. = 00:30 MEZ am 16. → in Münster derselbe Tag wie 10:00 MEZ am 16.
        let letzte = zeit(0, 2026, 11, 15, 23, 30);
        assert!(!pruefung_faellig(Some(&letzte), &zeit(1, 2026, 11, 16, 10, 0)));
        // aber in UTC gesehen war das gestern
        assert!(pruefung_faellig(Some(&letzte), &zeit(0, 2026, 11, 16, 10, 0)));
    }

    #[test]
    fn platzhalter_schaltet_updates_aus() {
        assert!(!schluessel_eingetragen(""));
        assert!(!schluessel_eingetragen("   "));
        assert!(!schluessel_eingetragen("PLATZHALTER – hier den öffentlichen Update-Schlüssel eintragen"));
        assert!(schluessel_eingetragen("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEFCQ0RFRgpSV1RFU1Q="));
    }

    #[test]
    fn konfiguration_hat_platzhalter_oder_schluessel() {
        // tauri.conf.json: Updater zeigt auf GitHub Releases des öffentlichen Repos, NSIS liefert Update-Pakete
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let up = &conf["plugins"]["updater"];
        assert_eq!(
            up["endpoints"][0],
            "https://github.com/Ch3rry-glitch/Ausbildungs-Copilot/releases/latest/download/latest.json"
        );
        assert_eq!(up["requireSignedVersion"], true);
        assert_eq!(up["windows"]["installMode"], "passive");
        assert!(up["pubkey"].as_str().is_some_and(|k| !k.trim().is_empty()));
        assert_eq!(conf["bundle"]["createUpdaterArtifacts"], true);
    }

    #[test]
    fn ohne_netz_zaehlt_nicht_als_geprueft() {
        use tauri_plugin_updater::Error as E;
        assert!(!server_erreicht(&E::Network("getaddrinfo".into())));
        assert!(!server_erreicht(&E::Io(std::io::Error::from(std::io::ErrorKind::TimedOut))));
        // GitHub hat geantwortet, nur (noch) ohne Release → heute nicht noch einmal fragen
        assert!(server_erreicht(&E::ReleaseNotFound));
        assert!(server_erreicht(&E::TargetsNotFound(vec!["windows-x86_64".into()])));
    }

    #[test]
    fn fehlertexte_auf_deutsch() {
        use tauri_plugin_updater::Error as E;
        assert!(fehlertext(&E::Network("x".into())).contains("Keine Verbindung"));
        assert!(fehlertext(&E::ReleaseNotFound).contains("latest.json"));
        assert!(fehlertext(&E::MissingSignedVersion).contains("Signatur"));
        assert!(fehlertext(&E::SignatureUtf8("x".into())).contains("nichts installiert"));
    }

    #[test]
    fn stand_rundlauf_und_kaputte_datei() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(laden(dir.path()), UpdateStand::default());
        let stand = UpdateStand { letzte_pruefung: Some(zeit(1, 2026, 11, 16, 10, 0)) };
        speichern(dir.path(), &stand).unwrap();
        assert_eq!(laden(dir.path()), stand);
        let text = fs::read_to_string(dir.path().join(STAND_DATEI)).unwrap();
        assert!(text.contains(r#""letztePruefung": "2026-11-16T10:00:00+01:00""#), "{text}");
        // kaputte Datei → wieder fällig statt Absturz
        fs::write(dir.path().join(STAND_DATEI), "{kaputt").unwrap();
        assert_eq!(laden(dir.path()), UpdateStand::default());
    }
}
