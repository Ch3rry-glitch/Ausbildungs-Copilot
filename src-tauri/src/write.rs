//! Der **einzige** Weg, auf dem die App in den Vault schreibt (SPEC §5.2, §9).
//!
//! * atomar: Temporärdatei `.cockpit-XXXXXX.tmp` im Zielordner, dann `ReplaceFileW` (Speichern)
//!   bzw. `MoveFileExW` ohne Überschreiben (Anlegen)
//! * Versionsmarke = SHA-256 der Bytes auf der Platte; der Konfliktvergleich läuft bei **jedem**
//!   Versuch unmittelbar vor dem Ersetzen erneut
//! * gesperrte Dateien (Obsidian, Cloud-Sync, Virenscanner) werden wiederholt, nie überschrieben
//! * „Speichern ohne Änderung“ schreibt nichts – die Datei bleibt byte-gleich
//! * eine verschobene oder gelöschte Datei wird nie neu angelegt; gelöscht wird nur die eigene
//!   Temporärdatei

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use crate::error::{AppError, AppResult};
use crate::guard;
use crate::model::{LineEnding, NoteContent, NoteVersion, RelPath};
use crate::vault::{frontmatter::NoteText, scan};

/// Führender Punkt: Obsidian, `scan_vault` und der Wächter ignorieren die Datei.
pub const TEMP_PREFIX: &str = ".cockpit-";
/// Nie `.md` – eine liegengebliebene Temporärdatei taucht so nie als Notiz auf.
pub const TEMP_SUFFIX: &str = ".tmp";

/// Hinweis beim Speichern einer Datei mit gemischten Zeilenenden.
pub const WARNUNG_GEMISCHT: &str = "Zeilenenden vereinheitlicht (die Datei hatte gemischte Zeilenenden).";

/// SHA-256 als Hex in Kleinbuchstaben (64 Zeichen).
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn modified_ms(md: &fs::Metadata) -> i64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn version_of(bytes: &[u8], md: &fs::Metadata) -> NoteVersion {
    NoteVersion { sha256: sha256_hex(bytes), modified_ms: modified_ms(md), size: md.len() }
}

// ---------------------------------------------------------------------------------------------
// Wiederholen bei gesperrten Dateien
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Pause vor dem 2., 3. … Versuch; Anzahl Versuche = `delays.len() + 1`
    pub delays: Vec<Duration>,
}

fn ms(list: &[u64]) -> Vec<Duration> {
    list.iter().map(|m| Duration::from_millis(*m)).collect()
}

impl RetryPolicy {
    /// ≈0,35 s: Vorschau, Heute, Listen, Vorlage/Stundenplan
    pub fn short() -> Self {
        RetryPolicy { delays: ms(&[50, 100, 200]) }
    }
    /// ≈3,15 s: Notiz öffnen
    pub fn standard() -> Self {
        RetryPolicy { delays: ms(&[50, 100, 200, 400, 800, 1600]) }
    }
    /// ≈15,2 s: Speichern/Anlegen (läuft immer im Hintergrund-Thread)
    pub fn write() -> Self {
        RetryPolicy { delays: ms(&[50, 100, 200, 400, 800, 1600, 3000, 3000, 3000, 3000]) }
    }
    /// genau ein Versuch (Tests)
    pub fn none() -> Self {
        RetryPolicy { delays: Vec::new() }
    }
}

/// Fehler, die „Datei gerade gesperrt“ bedeuten und einen neuen Versuch lohnen.
/// (`ErrorKind::ResourceBusy` braucht Rust 1.83 – rust-version ist 1.80.)
pub fn is_lock_error(e: &io::Error) -> bool {
    #[cfg(windows)]
    {
        // 5 ACCESS_DENIED, 32 SHARING_VIOLATION, 33 LOCK_VIOLATION, 1224 USER_MAPPED_FILE,
        // 1175 UNABLE_TO_REMOVE_REPLACED, 1176 UNABLE_TO_MOVE_REPLACEMENT
        matches!(e.raw_os_error(), Some(5 | 32 | 33 | 1224 | 1175 | 1176))
    }
    #[cfg(not(windows))]
    {
        e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(16 | 26))
    }
}

/// Wiederholt **nur** Sperrfehler. Schläft zwischen den Versuchen, nie nach dem letzten.
pub fn with_retry<T>(p: &RetryPolicy, mut f: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut attempt = 0;
    loop {
        match f() {
            Err(e) if is_lock_error(&e) && attempt < p.delays.len() => {
                std::thread::sleep(p.delays[attempt]);
                attempt += 1;
            }
            other => return other,
        }
    }
}

/// Sperrfehler → `Locked`, alles andere → `Io`.
fn io_err(e: io::Error, rel: &str) -> AppError {
    if is_lock_error(&e) {
        AppError::Locked(rel.to_string())
    } else {
        AppError::Io(e)
    }
}

// ---------------------------------------------------------------------------------------------
// Dekodieren / Kodieren (BOM, Zeilenenden)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// ohne BOM, CRLF → LF; ein einzelnes `\r` bleibt stehen
    pub text: String,
    pub line_ending: LineEnding,
    pub bom: bool,
    /// `false` → Text ist verlustbehaftet dekodiert, Datei wird nicht gespeichert
    pub utf8: bool,
    /// bei gemischten Zeilenenden: CRLF ist mindestens so häufig wie reines LF
    crlf_majority: bool,
}

pub fn decode(bytes: &[u8]) -> Decoded {
    let (bom, rest) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        Some(r) => (true, r),
        None => (false, bytes),
    };
    let (raw, utf8) = match std::str::from_utf8(rest) {
        Ok(s) => (s.to_string(), true),
        Err(_) => (String::from_utf8_lossy(rest).into_owned(), false),
    };
    let crlf = raw.matches("\r\n").count();
    let lf_only = raw.matches('\n').count() - crlf;
    let line_ending = match (crlf > 0, lf_only > 0) {
        (true, false) => LineEnding::Crlf,
        (true, true) => LineEnding::Mixed,
        _ => LineEnding::Lf,
    };
    Decoded { text: raw.replace("\r\n", "\n"), line_ending, bom, utf8, crlf_majority: crlf >= lf_only }
}

/// Editortext (LF) zurück in die Form der Datei: BOM und Zeilenende wie vorher.
/// Gemischte Zeilenenden werden auf das häufigere vereinheitlicht.
pub fn encode_like(editor_text: &str, d: &Decoded) -> String {
    let crlf = match d.line_ending {
        LineEnding::Crlf => true,
        LineEnding::Mixed => d.crlf_majority,
        LineEnding::Lf => false,
    };
    let body = if crlf { editor_text.replace('\n', "\r\n") } else { editor_text.to_string() };
    if d.bom {
        format!("\u{FEFF}{body}")
    } else {
        body
    }
}

// ---------------------------------------------------------------------------------------------
// Lesen
// ---------------------------------------------------------------------------------------------

/// Ein Leseversuch: Datei einmal öffnen, lesen, Metadaten vom **selben** Handle.
fn read_once(abs: &Path) -> io::Result<(Vec<u8>, NoteVersion)> {
    let mut f = File::open(abs)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let md = f.metadata()?;
    let v = version_of(&buf, &md);
    Ok((buf, v))
}

fn read_io(abs: &Path, p: &RetryPolicy) -> io::Result<(Vec<u8>, NoteVersion)> {
    with_retry(p, || read_once(abs))
}

/// Liest Bytes + Version; Sperrfehler werden wiederholt.
pub fn read_versioned(abs: &Path, p: &RetryPolicy) -> AppResult<(Vec<u8>, NoteVersion)> {
    read_io(abs, p).map_err(|e| {
        let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        io_err(e, &name)
    })
}

/// Notiz für den Editor: Text, Version und Metadaten aus **denselben** Bytes.
pub fn read_for_edit(root: &Path, rel: &str, p: &RetryPolicy) -> AppResult<NoteContent> {
    let (abs, rel) = guard::resolve_for_read(root, rel)?;
    let (bytes, version) = match read_io(&abs, p) {
        Ok(x) => x,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(AppError::NoteNotFound(rel)),
        Err(e) => return Err(io_err(e, &rel)),
    };
    let d = decode(&bytes);
    // Metadaten wie im Index (verlustbehaftet dekodiert, BOM/CRLF versteht NoteText selbst)
    let lossy = String::from_utf8_lossy(&bytes);
    let note = NoteText::parse(&lossy);
    let meta = scan::analyze(&rel, &note, version.modified_ms, version.size);
    Ok(NoteContent {
        meta,
        body: note.body().to_string(),
        frontmatter_raw: note.yaml(),
        text: d.text,
        version,
        line_ending: d.line_ending,
        bom: d.bom,
        editable: d.utf8,
        readonly_reason: (!d.utf8).then(|| "Keine gültige UTF-8-Datei – nur Anzeige.".to_string()),
    })
}

// ---------------------------------------------------------------------------------------------
// Schreiben
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateOutcome {
    Created(NoteVersion),
    /// Datei gibt es schon (Schreibweise auf der Platte) – nichts geschrieben
    Exists(RelPath),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved { version: NoteVersion, warnungen: Vec<String> },
    /// Platte enthält schon genau diesen Text – nichts geschrieben
    Unchanged(NoteVersion),
    /// Platte wurde seit `base` geändert – nichts geschrieben
    Conflict { disk_text: String, disk_version: NoteVersion },
    /// Datei verschoben oder gelöscht – wird **nie** neu angelegt
    Missing,
}

/// Wird nach dem Schreiben der Temporärdatei aufgerufen (Tests: Fehler oder fremde Änderung einspielen).
pub(crate) type Hook<'a> = &'a dyn Fn() -> io::Result<()>;

/// Temporärdatei im Zielordner anlegen, schreiben und auf die Platte bringen.
fn write_temp(dir: &Path, bytes: &[u8], p: &RetryPolicy) -> io::Result<NamedTempFile> {
    let mut t = with_retry(p, || {
        tempfile::Builder::new().prefix(TEMP_PREFIX).suffix(TEMP_SUFFIX).tempfile_in(dir)
    })?;
    t.write_all(bytes)?;
    t.as_file().sync_all()?;
    Ok(t)
}

/// Schreibweise einer vorhandenen Datei auf der Platte (NTFS: `gec …` existiert, wenn `GEC …` da ist).
fn on_disk_spelling(abs: &Path, rel: &str) -> RelPath {
    let (Some(parent), Some(name)) = (abs.parent(), abs.file_name()) else { return rel.to_string() };
    let name = name.to_string_lossy();
    let found = fs::read_dir(parent).ok().and_then(|rd| {
        rd.flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.eq_ignore_ascii_case(&name) || n.to_lowercase() == name.to_lowercase())
    });
    match (found, rel.rsplit_once('/')) {
        (Some(n), Some((folder, _))) => format!("{folder}/{n}"),
        (Some(n), None) => n,
        (None, _) => rel.to_string(),
    }
}

/// Neue Datei anlegen – überschreibt **nie** (auch nicht bei gleichzeitigem Anlegen).
pub fn create_new(root: &Path, rel: &str, text: &str, p: &RetryPolicy, writes: &SelfWrites) -> AppResult<CreateOutcome> {
    create_with_hook(root, rel, text, p, writes, &|| Ok(()))
}

pub(crate) fn create_with_hook(
    root: &Path,
    rel: &str,
    text: &str,
    p: &RetryPolicy,
    writes: &SelfWrites,
    before_persist: Hook,
) -> AppResult<CreateOutcome> {
    let (abs, rel) = guard::resolve_for_write(root, rel)?;
    refuse_repo_trees(&abs)?;
    if abs.exists() {
        return Ok(CreateOutcome::Exists(on_disk_spelling(&abs, &rel)));
    }
    let parent = abs.parent().ok_or_else(|| AppError::Other(format!("Kein Ordner für „{rel}“.")))?.to_path_buf();
    with_retry(p, || fs::create_dir_all(&parent)).map_err(|e| io_err(e, &rel))?;

    let bytes = text.as_bytes();
    let sha = sha256_hex(bytes);
    writes.register(&rel, &sha);
    let result = (|| -> AppResult<CreateOutcome> {
        let tmp = write_temp(&parent, bytes, p).map_err(|e| io_err(e, &rel))?;
        before_persist()?;
        let mut slot = Some(tmp);
        let persisted = with_retry(p, || {
            let t = slot.take().expect("Temporärdatei fehlt");
            t.persist_noclobber(&abs).map_err(|e| {
                slot = Some(e.file); // für den nächsten Versuch zurücknehmen
                e.error
            })
        });
        // `slot` (falls noch belegt) wird hier fallen gelassen → eigene Temporärdatei gelöscht
        drop(slot);
        match persisted {
            Ok(file) => {
                let md = file.metadata()?;
                Ok(CreateOutcome::Created(version_of(bytes, &md)))
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(CreateOutcome::Exists(on_disk_spelling(&abs, &rel))),
            Err(e) => Err(io_err(e, &rel)),
        }
    })();
    if !matches!(result, Ok(CreateOutcome::Created(_))) {
        writes.forget(&rel, &sha);
    }
    result
}

/// Speichern mit Konfliktschutz. `base` = Version, die der Editor geladen hat.
pub fn save(
    root: &Path,
    rel: &str,
    editor_text: &str,
    base: &NoteVersion,
    p: &RetryPolicy,
    writes: &SelfWrites,
) -> AppResult<SaveOutcome> {
    save_with_hook(root, rel, editor_text, base, p, writes, &|| Ok(()))
}

pub(crate) fn save_with_hook(
    root: &Path,
    rel: &str,
    editor_text: &str,
    base: &NoteVersion,
    p: &RetryPolicy,
    writes: &SelfWrites,
    before_replace: Hook,
) -> AppResult<SaveOutcome> {
    // 1. Pfad prüfen; ab hier ist `rel` die Schreibweise auf der Platte
    let (abs, rel) = guard::resolve_for_write(root, rel)?;
    refuse_repo_trees(&abs)?;

    // 2. Erst lesen. Fehlt die Datei, wird sie nie neu angelegt.
    let (bytes, disk_ver) = match read_io(&abs, p) {
        Ok(x) => x,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SaveOutcome::Missing),
        Err(e) => return Err(io_err(e, &rel)),
    };
    let d = decode(&bytes);
    if !d.utf8 {
        return Err(AppError::NotEditable(rel));
    }
    // 3. Gleicher Text → kein Schreibvorgang (vor der Konfliktprüfung)
    if editor_text == d.text {
        return Ok(SaveOutcome::Unchanged(disk_ver));
    }
    // 4. Der Hash entscheidet (nur geänderte mtime ist kein Konflikt)
    if disk_ver.sha256 != base.sha256 {
        return Ok(SaveOutcome::Conflict { disk_text: d.text, disk_version: disk_ver });
    }
    // 5. Schreibschutz – nie wiederholen
    match fs::metadata(&abs) {
        Ok(md) if md.permissions().readonly() => return Err(AppError::ReadOnly(rel)),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SaveOutcome::Missing),
        Err(e) => return Err(io_err(e, &rel)),
    }
    // 6. Neuen Inhalt registrieren, bevor irgendetwas das Ziel berührt
    let new = encode_like(editor_text, &d);
    let new_sha = sha256_hex(new.as_bytes());
    writes.register(&rel, &new_sha);

    let result = (|| -> AppResult<SaveOutcome> {
        // 7. Temporärdatei schreiben und schließen
        let parent = abs.parent().ok_or_else(|| AppError::Other(format!("Kein Ordner für „{rel}“.")))?;
        let mut temp = write_temp(parent, new.as_bytes(), p).map_err(|e| io_err(e, &rel))?.into_temp_path();
        before_replace()?;

        // 8. Jeder Versuch: neu lesen → vergleichen → ersetzen
        let outcome = with_retry(p, || -> io::Result<Option<SaveOutcome>> {
            let (b, v) = match read_once(&abs) {
                Ok(x) => x,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(SaveOutcome::Missing)),
                Err(e) => return Err(e),
            };
            let dd = decode(&b);
            if dd.text == editor_text {
                return Ok(Some(SaveOutcome::Unchanged(v)));
            }
            if v.sha256 != base.sha256 {
                return Ok(Some(SaveOutcome::Conflict { disk_text: dd.text, disk_version: v }));
            }
            match replace_file(&abs, &temp) {
                Ok(()) => Ok(None),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Some(SaveOutcome::Missing)),
                Err(e) => Err(e),
            }
        })
        .map_err(|e| io_err(e, &rel))?;

        if let Some(o) = outcome {
            return Ok(o); // 9. `temp` fällt → eigene Temporärdatei gelöscht
        }
        // 10. Ersetzt: der Temporärpfad existiert nicht mehr
        temp.disable_cleanup(true);
        // 11. Version: Hash ist maßgeblich, Zeit/Größe frisch von der Platte
        let md = fs::metadata(&abs)?;
        let mut warnungen = Vec::new();
        if d.line_ending == LineEnding::Mixed {
            warnungen.push(WARNUNG_GEMISCHT.to_string());
        }
        Ok(SaveOutcome::Saved {
            version: NoteVersion { sha256: new_sha.clone(), modified_ms: modified_ms(&md), size: md.len() },
            warnungen,
        })
    })();
    if !matches!(result, Ok(SaveOutcome::Saved { .. })) {
        writes.forget(&rel, &new_sha);
    }
    result
}

/// Ersetzt `target` atomar durch `replacement`. Legt `target` **nie** neu an.
///
/// Windows: `ReplaceFileW` ohne Sicherungskopie. Schlägt mit `ERROR_FILE_NOT_FOUND` fehl, wenn das
/// Ziel fehlt, und behält Erstellzeit, Attribute, ACL, Kurzname und alternative Datenströme.
#[cfg(windows)]
pub(crate) fn replace_file(target: &Path, replacement: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        ReplaceFileW, REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS,
    };
    let wide = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let t = wide(target);
    let r = wide(replacement);
    // SAFETY: beide Zeiger zeigen auf nullterminierte UTF-16-Puffer, die bis nach dem Aufruf leben;
    // Sicherungsdatei und reservierte Parameter sind NULL.
    let ok = unsafe {
        ReplaceFileW(
            t.as_ptr(),
            r.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
pub(crate) fn replace_file(target: &Path, replacement: &Path) -> io::Result<()> {
    if !target.exists() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "Ziel fehlt"));
    }
    fs::rename(replacement, target)
}

/// Nächster vorhandener Vorfahr, kanonisch.
#[cfg(any(test, debug_assertions))]
fn canonical_anchor(p: &Path) -> Option<std::path::PathBuf> {
    p.ancestors().find_map(|a| a.canonicalize().ok())
}

/// Test- und Debug-Builds: nie in `fixtures/` oder `starter-vault/` des Repositories schreiben.
#[cfg(any(test, debug_assertions))]
pub(crate) fn refuse_repo_trees(abs: &Path) -> AppResult<()> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let Some(c) = canonical_anchor(abs) else { return Ok(()) };
    for tree in [manifest.join("..").join("fixtures"), manifest.join("starter-vault")] {
        if tree.canonicalize().is_ok_and(|t| c.starts_with(&t)) {
            return Err(AppError::DevWriteBlocked);
        }
    }
    Ok(())
}

#[cfg(not(any(test, debug_assertions)))]
#[inline]
pub(crate) fn refuse_repo_trees(_abs: &Path) -> AppResult<()> {
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Reste nach einem Absturz
// ---------------------------------------------------------------------------------------------

static TEMP_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\.cockpit-[A-Za-z0-9]{6}\.tmp$").unwrap());
const TEMP_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Löscht liegengebliebene **eigene** Temporärdateien (älter als 1 h) in der Wurzel und den
/// indizierten Ordnern. Nie rekursiv, nie in versteckten Ordnern, nie unter `.obsidian/`.
pub fn sweep_temp_files(root: &Path, folders: &[RelPath]) -> usize {
    let mut removed = 0;
    let dirs = std::iter::once(root.to_path_buf()).chain(
        folders
            .iter()
            .filter(|f| !guard::is_obsidian_config(f.as_str()) && !f.split('/').any(guard::is_hidden_component))
            .filter_map(|f| guard::resolve_in_vault(root, f).ok()),
    );
    let now = SystemTime::now();
    for dir in dirs {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name();
            if !TEMP_NAME.is_match(&name.to_string_lossy()) {
                continue;
            }
            let path = entry.path();
            let Ok(md) = fs::symlink_metadata(&path) else { continue };
            if !md.file_type().is_file() {
                continue;
            }
            let old = md.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age > TEMP_MAX_AGE);
            if old && fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        eprintln!("[vault] {removed} alte Temporärdatei(en) der App entfernt");
    }
    removed
}

// ---------------------------------------------------------------------------------------------
// Eigene Schreibvorgänge erkennen
// ---------------------------------------------------------------------------------------------

/// Merkt sich (Pfad, Hash) jedes eigenen Schreibvorgangs für kurze Zeit. Der Wächter meldet eine
/// Änderung als `own`, wenn die Bytes auf der Platte genau diesen Hash haben – schreibt Obsidian
/// danach, passt der Hash nicht mehr und die Änderung zählt wieder als fremd.
#[derive(Debug)]
pub struct SelfWrites {
    entries: Mutex<HashMap<String, Vec<(String, Instant)>>>,
    ttl: Duration,
}

impl Default for SelfWrites {
    fn default() -> Self {
        Self::new()
    }
}

impl SelfWrites {
    pub const TTL: Duration = Duration::from_secs(10);

    pub fn new() -> Self {
        Self::with_ttl(Self::TTL)
    }

    pub fn with_ttl(ttl: Duration) -> Self {
        SelfWrites { entries: Mutex::new(HashMap::new()), ttl }
    }

    /// NTFS unterscheidet keine Groß-/Kleinschreibung.
    fn key(rel: &str) -> String {
        rel.replace('\\', "/").to_lowercase()
    }

    fn prune(&self, map: &mut HashMap<String, Vec<(String, Instant)>>) {
        map.retain(|_, v| {
            v.retain(|(_, t)| t.elapsed() < self.ttl);
            !v.is_empty()
        });
    }

    pub fn register(&self, rel: &str, sha256: &str) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        self.prune(&mut map);
        map.entry(Self::key(rel)).or_default().push((sha256.to_string(), Instant::now()));
    }

    /// Entfernt **nur** diesen (Pfad, Hash)-Eintrag.
    pub fn forget(&self, rel: &str, sha256: &str) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let key = Self::key(rel);
        if let Some(v) = map.get_mut(&key) {
            if let Some(i) = v.iter().rposition(|(s, _)| s == sha256) {
                v.remove(i);
            }
            if v.is_empty() {
                map.remove(&key);
            }
        }
    }

    /// Stimmt der Hash der aktuellen Bytes mit einem frischen eigenen Eintrag überein?
    pub fn is_own(&self, root: &Path, rel: &str) -> bool {
        let shas: Vec<String> = {
            let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
            self.prune(&mut map);
            match map.get(&Self::key(rel)) {
                Some(v) => v.iter().map(|(s, _)| s.clone()).collect(),
                None => return false,
            }
        };
        let Ok(abs) = guard::resolve_in_vault(root, rel) else { return false };
        let Ok(bytes) = fs::read(abs) else { return false };
        let h = sha256_hex(&bytes);
        shas.contains(&h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    const REL: &str = "03 Unterricht/GEC 05.10.2026.md";

    fn vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("03 Unterricht")).unwrap();
        dir
    }

    fn put(root: &Path, rel: &str, bytes: &[u8]) -> NoteVersion {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, bytes).unwrap();
        read_versioned(&p, &RetryPolicy::none()).unwrap().1
    }

    fn bytes(root: &Path, rel: &str) -> Vec<u8> {
        fs::read(root.join(rel)).unwrap()
    }

    fn mtime(root: &Path, rel: &str) -> SystemTime {
        fs::metadata(root.join(rel)).unwrap().modified().unwrap()
    }

    /// Alle Dateinamen eines Ordners (inkl. versteckter).
    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> =
            fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    fn no_temp_left(dir: &Path) {
        assert!(names(dir).iter().all(|n| !n.starts_with(TEMP_PREFIX)), "{:?}", names(dir));
    }

    fn sw() -> SelfWrites {
        SelfWrites::new()
    }

    #[test]
    fn sha256_bekannter_wert() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b"").len(), 64);
    }

    #[test]
    fn decode_encode_rundlauf() {
        let lf = b"a\nb\n".as_slice();
        let crlf = b"a\r\nb\r\n".as_slice();
        let bom_crlf = b"\xEF\xBB\xBFa\r\nb\r\n".as_slice();
        for (b, le, bom) in [(lf, LineEnding::Lf, false), (crlf, LineEnding::Crlf, false), (bom_crlf, LineEnding::Crlf, true)] {
            let d = decode(b);
            assert_eq!(d.text, "a\nb\n");
            assert_eq!((d.line_ending, d.bom, d.utf8), (le, bom, true));
            assert_eq!(encode_like(&d.text, &d).as_bytes(), b);
        }
        // gemischt: Mehrheit entscheidet
        let d = decode(b"a\r\nb\r\nc\n");
        assert_eq!(d.line_ending, LineEnding::Mixed);
        assert_eq!(encode_like(&d.text, &d), "a\r\nb\r\nc\r\n");
        let d = decode(b"a\nb\nc\r\n");
        assert_eq!(encode_like(&d.text, &d), "a\nb\nc\n");
        let d = decode(b"a\r\nb\n");
        assert_eq!(encode_like(&d.text, &d), "a\r\nb\r\n", "Gleichstand → CRLF");
        // einzelnes \r bleibt
        let d = decode(b"a\rb\n");
        assert_eq!((d.text.as_str(), d.line_ending), ("a\rb\n", LineEnding::Lf));
        assert_eq!(encode_like(&d.text, &d), "a\rb\n");
        // kein UTF-8
        let d = decode(b"Gr\xFC\xDFe\n");
        assert!(!d.utf8);
    }

    #[test]
    fn anlegen_ueberschreibt_nie() {
        let dir = vault();
        let root = dir.path();
        let rel = "03 Unterricht/GEC 16.11.2026.md";
        put(root, rel, b"vorher");
        let t0 = mtime(root, rel);
        let r = create_new(root, rel, "neu", &RetryPolicy::none(), &sw()).unwrap();
        assert_eq!(r, CreateOutcome::Exists(rel.to_string()));
        assert_eq!(bytes(root, rel), b"vorher");
        assert_eq!(mtime(root, rel), t0);
        #[cfg(windows)]
        {
            let r = create_new(root, "03 Unterricht/gec 16.11.2026.md", "neu", &RetryPolicy::none(), &sw()).unwrap();
            assert_eq!(r, CreateOutcome::Exists(rel.to_string()));
            assert_eq!(bytes(root, rel), b"vorher");
        }
        no_temp_left(&root.join("03 Unterricht"));
        // neu anlegen klappt und legt fehlende Ordner an
        let r = create_new(root, "03 Unterricht/Neu/D 16.11.2026.md", "x\n", &RetryPolicy::none(), &sw()).unwrap();
        let CreateOutcome::Created(v) = r else { panic!("{r:?}") };
        assert_eq!(v.sha256, sha256_hex(b"x\n"));
        assert_eq!(v.size, 2);
        assert_eq!(bytes(root, "03 Unterricht/Neu/D 16.11.2026.md"), b"x\n");
        no_temp_left(&root.join("03 Unterricht/Neu"));
    }

    #[test]
    fn fehler_vor_dem_ersetzen_laesst_nichts_zurueck() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let w = sw();
        let fail: Hook = &|| Err(io::Error::other("simulierter Fehler"));
        let r = save_with_hook(root, REL, "neu\n", &base, &RetryPolicy::none(), &w, fail);
        assert!(matches!(r, Err(AppError::Io(_))), "{r:?}");
        assert_eq!(bytes(root, REL), b"alt\n");
        no_temp_left(&root.join("03 Unterricht"));
        assert!(!w.is_own(root, REL));

        let before = names(&root.join("03 Unterricht"));
        let r = create_with_hook(root, "03 Unterricht/WSP 16.11.2026.md", "x", &RetryPolicy::none(), &w, fail);
        assert!(r.is_err());
        assert_eq!(names(&root.join("03 Unterricht")), before, "keine neue .md, keine Temporärdatei");
    }

    #[test]
    fn konflikt_wenn_platte_geaendert() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        fs::write(root.join(REL), b"aus Obsidian\n").unwrap();
        let r = save(root, REL, "meins\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        let SaveOutcome::Conflict { disk_text, disk_version } = r else { panic!("{r:?}") };
        assert_eq!(disk_text, "aus Obsidian\n");
        assert_eq!(disk_version.sha256, sha256_hex(b"aus Obsidian\n"));
        assert_eq!(bytes(root, REL), b"aus Obsidian\n");
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[test]
    fn konflikt_im_letzten_moment() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let target = root.join(REL);
        let hook = move || fs::write(&target, b"dazwischen\n");
        let w = sw();
        let r = save_with_hook(root, REL, "meins\n", &base, &RetryPolicy::none(), &w, &hook).unwrap();
        assert!(matches!(&r, SaveOutcome::Conflict { disk_text, .. } if disk_text == "dazwischen\n"), "{r:?}");
        assert_eq!(bytes(root, REL), b"dazwischen\n");
        no_temp_left(&root.join("03 Unterricht"));
        assert!(!w.is_own(root, REL));
    }

    #[test]
    fn geloescht_vor_dem_ersetzen_wird_nicht_neu_angelegt() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let target = root.join(REL);
        let hook = move || fs::remove_file(&target);
        let r = save_with_hook(root, REL, "meins\n", &base, &RetryPolicy::none(), &sw(), &hook).unwrap();
        assert_eq!(r, SaveOutcome::Missing);
        assert!(!root.join(REL).exists());
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[test]
    fn platte_gleich_editor_ist_unveraendert() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        fs::write(root.join(REL), b"schon so\n").unwrap();
        let t0 = mtime(root, REL);
        let r = save(root, REL, "schon so\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        assert!(matches!(&r, SaveOutcome::Unchanged(v) if v.sha256 == sha256_hex(b"schon so\n")), "{r:?}");
        assert_eq!(mtime(root, REL), t0);
    }

    #[test]
    fn nur_mtime_geaendert_ist_kein_konflikt() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let f = File::options().write(true).open(root.join(REL)).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
        drop(f);
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        assert!(matches!(r, SaveOutcome::Saved { .. }), "{r:?}");
        assert_eq!(bytes(root, REL), b"neu\n");
    }

    #[test]
    fn speichern_ohne_aenderung_ist_byte_gleich() {
        let dir = vault();
        let root = dir.path();
        for content in [b"a\nb\n".as_slice(), b"\xEF\xBB\xBFa\r\nb\r\n", b"a\r\nb\nc\r\n"] {
            let base = put(root, REL, content);
            let t0 = mtime(root, REL);
            let text = decode(content).text;
            let r = save(root, REL, &text, &base, &RetryPolicy::none(), &sw()).unwrap();
            assert_eq!(r, SaveOutcome::Unchanged(base.clone()));
            assert_eq!(bytes(root, REL), content);
            assert_eq!(mtime(root, REL), t0);
        }
    }

    #[test]
    fn crlf_und_bom_bleiben_beim_bearbeiten() {
        let dir = vault();
        let root = dir.path();
        let content = b"\xEF\xBB\xBF---\r\nthema: alt\r\n---\r\nZeile 1\r\nZeile 2\r\n";
        let base = put(root, REL, content);
        let text = decode(content).text.replace("Zeile 2", "Zeile 2 geändert");
        let w = sw();
        let r = save(root, REL, &text, &base, &RetryPolicy::none(), &w).unwrap();
        let SaveOutcome::Saved { version, warnungen } = r else { panic!("{r:?}") };
        assert!(warnungen.is_empty());
        let after = bytes(root, REL);
        assert_eq!(version.sha256, sha256_hex(&after));
        assert_eq!(version.size, after.len() as u64);
        assert_eq!(after, "\u{FEFF}---\r\nthema: alt\r\n---\r\nZeile 1\r\nZeile 2 geändert\r\n".as_bytes());
        assert!(w.is_own(root, REL), "eigener Schreibvorgang registriert");
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[test]
    fn gemischte_zeilenenden_werden_gemeldet() {
        let dir = vault();
        let root = dir.path();
        let content = b"a\r\nb\r\nc\n";
        let base = put(root, REL, content);
        let r = save(root, REL, "a\nb\nc geändert\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        let SaveOutcome::Saved { warnungen, .. } = r else { panic!("{r:?}") };
        assert_eq!(warnungen, vec![WARNUNG_GEMISCHT.to_string()]);
        assert_eq!(bytes(root, REL), b"a\r\nb\r\nc ge\xC3\xA4ndert\r\n");
    }

    #[test]
    fn fehlende_datei_wird_nicht_neu_angelegt() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        fs::remove_file(root.join(REL)).unwrap();
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        assert_eq!(r, SaveOutcome::Missing);
        assert!(!root.join(REL).exists());
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[test]
    fn nicht_utf8_wird_nicht_gespeichert() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"Gr\xFC\xDFe\n");
        let r = save(root, REL, "Grüße\n", &base, &RetryPolicy::none(), &sw());
        assert!(matches!(r, Err(AppError::NotEditable(_))), "{r:?}");
        assert_eq!(bytes(root, REL), b"Gr\xFC\xDFe\n");
        let c = read_for_edit(root, REL, &RetryPolicy::none()).unwrap();
        assert!(!c.editable);
        assert_eq!(c.readonly_reason.as_deref(), Some("Keine gültige UTF-8-Datei – nur Anzeige."));
    }

    #[test]
    fn schreibgeschuetzt() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let p = root.join(REL);
        let mut perm = fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&p, perm.clone()).unwrap();
        let t0 = Instant::now();
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::write(), &sw());
        assert!(matches!(r, Err(AppError::ReadOnly(_))), "{r:?}");
        assert!(t0.elapsed() < Duration::from_millis(500), "kein Warten bei Schreibschutz");
        assert_eq!(bytes(root, REL), b"alt\n");
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
        fs::set_permissions(&p, perm).unwrap();
    }

    #[cfg(windows)]
    fn hold_exclusive(p: &Path) -> File {
        use std::os::windows::fs::OpenOptionsExt;
        File::options().read(true).write(true).share_mode(0).open(p).unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn gesperrte_datei_wird_wiederholt() {
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        // ohne Wiederholen: gesperrt
        let held = hold_exclusive(&root.join(REL));
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::none(), &sw());
        assert!(matches!(r, Err(AppError::Locked(_))), "{r:?}");
        // mit Wiederholen: Sperre löst sich nach 300 ms
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::standard(), &sw()).unwrap();
        h.join().unwrap();
        assert!(matches!(r, SaveOutcome::Saved { .. }), "{r:?}");
        assert_eq!(bytes(root, REL), b"neu\n");
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[cfg(windows)]
    #[test]
    fn fremder_schreiber_waehrend_sperre_ergibt_konflikt() {
        use std::io::Seek;
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let mut held = hold_exclusive(&root.join(REL));
        let h = std::thread::spawn(move || {
            held.set_len(0).unwrap();
            held.seek(io::SeekFrom::Start(0)).unwrap();
            held.write_all(b"vom Sync-Client\n").unwrap();
            held.sync_all().unwrap();
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let r = save(root, REL, "meins\n", &base, &RetryPolicy::standard(), &sw()).unwrap();
        h.join().unwrap();
        assert!(matches!(&r, SaveOutcome::Conflict { disk_text, .. } if disk_text == "vom Sync-Client\n"), "{r:?}");
        assert_eq!(bytes(root, REL), b"vom Sync-Client\n");
        no_temp_left(&root.join("03 Unterricht"));
    }

    #[cfg(windows)]
    #[test]
    fn erstellzeit_bleibt() {
        use std::os::windows::fs::MetadataExt;
        let dir = vault();
        let root = dir.path();
        let base = put(root, REL, b"alt\n");
        let created = fs::metadata(root.join(REL)).unwrap().created().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let r = save(root, REL, "neu\n", &base, &RetryPolicy::none(), &sw()).unwrap();
        assert!(matches!(r, SaveOutcome::Saved { .. }), "{r:?}");
        let md = fs::metadata(root.join(REL)).unwrap();
        assert_eq!(md.created().unwrap(), created);
        const FILE_ATTRIBUTE_TEMPORARY: u32 = 0x100;
        assert_eq!(md.file_attributes() & FILE_ATTRIBUTE_TEMPORARY, 0, "Ziel nicht als temporär markiert");
    }

    #[test]
    fn sperrfehler_erkannt() {
        #[cfg(windows)]
        {
            for code in [5, 32, 33, 1224, 1175, 1176] {
                assert!(is_lock_error(&io::Error::from_raw_os_error(code)), "{code}");
            }
            assert!(!is_lock_error(&io::Error::from_raw_os_error(2)));
        }
        assert!(!is_lock_error(&io::Error::from(io::ErrorKind::NotFound)));
        // with_retry: nur Sperrfehler werden wiederholt, kein Schlaf nach dem letzten Versuch
        let mut n = 0;
        let r: io::Result<()> = with_retry(&RetryPolicy::none(), || {
            n += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert!(r.is_err());
        assert_eq!(n, 1);
        #[cfg(windows)]
        {
            let mut n = 0;
            let t0 = Instant::now();
            let r: io::Result<()> = with_retry(&RetryPolicy::short(), || {
                n += 1;
                Err(io::Error::from_raw_os_error(32))
            });
            assert!(r.is_err());
            assert_eq!(n, 4);
            assert!(t0.elapsed() < Duration::from_millis(1000));
        }
    }

    #[test]
    fn eigene_schreibvorgaenge() {
        let dir = vault();
        let root = dir.path();
        put(root, REL, b"a");
        let w = sw();
        w.register(REL, &sha256_hex(b"a"));
        assert!(w.is_own(root, REL));
        fs::write(root.join(REL), b"extern").unwrap();
        assert!(!w.is_own(root, REL), "nach fremder Änderung nicht mehr eigen");

        let kurz = SelfWrites::with_ttl(Duration::from_millis(0));
        kurz.register(REL, &sha256_hex(b"extern"));
        assert!(!kurz.is_own(root, REL));

        let w = sw();
        let (a, b) = (sha256_hex(b"extern"), sha256_hex(b"b"));
        w.register(REL, &a);
        w.register(REL, &b);
        w.forget(REL, &b);
        assert!(w.is_own(root, REL), "forget(b) lässt a stehen");
        w.forget(REL, &a);
        assert!(!w.is_own(root, REL));
        assert!(!w.is_own(root, "03 Unterricht/unbekannt.md"));
    }

    #[test]
    fn temp_reste_werden_aufgeraeumt() {
        let dir = vault();
        let root = dir.path();
        let unt = root.join("03 Unterricht");
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        let old = SystemTime::now() - Duration::from_secs(2 * 3600);
        let make = |p: PathBuf, alt: bool| {
            fs::write(&p, b"x").unwrap();
            if alt {
                File::options().write(true).open(&p).unwrap().set_modified(old).unwrap();
            }
        };
        make(unt.join(".cockpit-abc123.tmp"), true);
        make(root.join(".cockpit-Zz9Yy8.tmp"), true);
        make(unt.join(".cockpit-def456.tmp"), false);
        make(unt.join(".cockpit-abc123.md"), true);
        make(unt.join("x.tmp"), true);
        make(root.join(".obsidian/.cockpit-abc123.tmp"), true);
        let n = sweep_temp_files(root, &["03 Unterricht".to_string(), ".obsidian".to_string()]);
        assert_eq!(n, 2);
        assert!(!unt.join(".cockpit-abc123.tmp").exists());
        assert!(!root.join(".cockpit-Zz9Yy8.tmp").exists());
        assert!(unt.join(".cockpit-def456.tmp").exists());
        assert!(unt.join(".cockpit-abc123.md").exists());
        assert!(unt.join("x.tmp").exists());
        assert!(root.join(".obsidian/.cockpit-abc123.tmp").exists());
    }

    #[test]
    fn fixtures_sind_tabu() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures/vault");
        let rel = "03 Unterricht/GEC 05.10.2026.md";
        let before = fs::read(root.join(rel)).unwrap();
        let base = read_versioned(&root.join(rel), &RetryPolicy::none()).unwrap().1;
        let r = save(&root, rel, "überschrieben\n", &base, &RetryPolicy::none(), &sw());
        assert!(matches!(r, Err(AppError::DevWriteBlocked)), "{r:?}");
        assert_eq!(fs::read(root.join(rel)).unwrap(), before);
        let neu = "03 Unterricht/GEC 16.11.2099.md";
        let r = create_new(&root, neu, "x", &RetryPolicy::none(), &sw());
        assert!(matches!(r, Err(AppError::DevWriteBlocked)), "{r:?}");
        assert!(!root.join(neu).exists());
        let starter = Path::new(env!("CARGO_MANIFEST_DIR")).join("starter-vault");
        let r = create_new(&starter, "03 Unterricht/GEC 16.11.2099.md", "x", &RetryPolicy::none(), &sw());
        assert!(matches!(r, Err(AppError::DevWriteBlocked)), "{r:?}");
    }

    #[test]
    fn lesen_fuer_den_editor() {
        let dir = vault();
        let root = dir.path();
        put(root, REL, b"\xEF\xBB\xBF---\r\nfach: GEC\r\n---\r\n- [ ] x\r\n");
        let c = read_for_edit(root, REL, &RetryPolicy::none()).unwrap();
        assert_eq!(c.text, "---\nfach: GEC\n---\n- [ ] x\n");
        assert!(c.bom && c.editable);
        assert_eq!(c.line_ending, LineEnding::Crlf);
        assert_eq!(c.meta.fach.as_deref(), Some("GEC"));
        assert_eq!(c.meta.tasks[0].line, 4);
        assert_eq!(c.version.sha256, sha256_hex(&bytes(root, REL)));
        let r = read_for_edit(root, "03 Unterricht/fehlt.md", &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::NoteNotFound(_))), "{r:?}");
        let r = read_for_edit(root, ".obsidian/app.json", &RetryPolicy::none());
        assert!(matches!(r, Err(AppError::Guard(guard::GuardError::ObsidianConfig(_)))), "{r:?}");
    }
}
