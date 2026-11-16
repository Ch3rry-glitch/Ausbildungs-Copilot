// Typen und Aufrufe des Rust-Backends. Spiegeln src-tauri/src/model.rs.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type NoteKind =
  | "unterricht"
  | "berichtsheftWoche"
  | "aufgabe"
  | "glossar"
  | "fach"
  | "lernfeld"
  | "template"
  | "sonstige";

export interface FrontmatterField {
  key: string;
  value: unknown;
}

export interface Task {
  line: number;
  text: string;
  done: boolean;
  statusChar: string;
  due: string | null;
  indent: number;
}

export interface InlineTag {
  tag: string;
  line: number;
  context: string;
}

export interface WikiLink {
  target: string;
  heading: string | null;
  alias: string | null;
  embed: boolean;
  line: number;
}

export interface NoteMeta {
  path: string;
  folder: string;
  title: string;
  kind: NoteKind;
  fields: FrontmatterField[];
  tags: string[];
  inlineTags: InlineTag[];
  tasks: Task[];
  links: WikiLink[];
  datum: string | null;
  fach: string | null;
  modifiedMs: number;
  size: number;
  frontmatterError: string | null;
  private: boolean;
}

export interface NoteSummary {
  path: string;
  title: string;
  kind: NoteKind;
  datum: string | null;
  fach: string | null;
  modifiedMs: number;
  openTasks: number;
  tags: string[];
  thema: string | null;
}

// ---------- M1: Vertrag laut Design §3.4 ----------
export type LineEnding = "lf" | "crlf" | "mixed";
export interface NoteVersion { sha256: string; modifiedMs: number; size: number; }
export interface NoteContent {
  meta: NoteMeta; body: string; frontmatterRaw: string | null;
  text: string; version: NoteVersion; lineEnding: LineEnding; bom: boolean;
  editable: boolean; readonlyReason: string | null;
}
export interface VaultChanged { changed: string[]; removed: string[]; own: string[]; fullRescan: boolean; }

export interface Block {
  fach: string; fachZiel: string | null; lehrkraft: string | null;
  vonStunde: number; bisStunde: number; beginn: string; ende: string;
}
export interface FachInfo {
  kurz: string; ziel: string; link: string; lehrkraft: string | null;
  zielExistiert: boolean; notizPfad: string | null; imStundenplan: boolean;
}
export interface LernfeldInfo {
  path: string; titel: string; kurz: string | null; nummer: number | null; fach: string | null; link: string;
}
export type TagArt = "schultag" | "betriebstag" | "wochenende";
export type StundenplanStatus = "ok" | "fehlt" | "leer";
export interface HeuteBlock {
  position: number; block: Block; fachNr: number; fachAnzahl: number; fach: FachInfo;
  notiz: NoteSummary | null; ersteNotiz: NoteSummary | null; neuerPfad: string; hinweis: string | null;
}
export interface FaelligeAufgabe { path: string; title: string; fach: string | null; line: number; text: string; due: string; }
export interface FaelligeAufgaben { ueberfaellig: FaelligeAufgabe[]; heute: FaelligeAufgabe[]; naechsteTage: FaelligeAufgabe[]; }
export interface HeuteTag {
  datum: string; wochentag: string; art: TagArt; istHeute: boolean; stundenplan: StundenplanStatus;
  bloecke: HeuteBlock[]; weitereNotizen: NoteSummary[]; aktuellerBlock: number | null;
  aufgaben: FaelligeAufgaben; warnungen: string[];
}
export interface NeueUnterrichtsnotiz { datum: string; fach: string; block: number; lernfeld: string | null; thema: string | null; }
export interface NotizVorschau {
  relPath: string; text: string; fachLink: string; lernfeldLink: string | null;
  lehrkraft: string | null; existiert: string | null; warnungen: string[];
}
export interface WriteReport { relPath: string; created: boolean; version: NoteVersion; createdFolders: string[]; warnungen: string[]; }
export type SaveResult =
  | { status: "saved"; report: WriteReport }
  | { status: "unchanged"; version: NoteVersion }
  | { status: "conflict"; diskText: string; diskVersion: NoteVersion }
  | { status: "missing"; relPath: string };
export type CreateResult =
  | { status: "created"; report: WriteReport }
  | { status: "exists"; relPath: string };

// ---------- M2: Logbuch, Vertrag laut Design §2 ----------
export type LogbuchStatus = "offen" | "kopiert" | "abgegeben";
export type AbschnittQuelle = "erarbeitet" | "fragen" | "naechstes" | "leer";
export type Umlaute = "erhalten" | "umschreiben";

export interface LogbuchAbschnitt { titel: string; quelle: AbschnittQuelle; }
export interface LogbuchFach {
  kurz: string;                 // "GEC"
  aktiv: boolean;
  moodleUrl: string;            // "" = nicht gesetzt → https://www.lebk-moodle.de/
  abschnitte: LogbuchAbschnitt[];
  selbstbewertung: number | null; // Skala-Maximum (10) oder null = keine Bewertung
  bewertungTitel: string;       // "Bewertung dieser Lerneinheit"
  eintraegeProWoche: number;    // 2
  umlaute: Umlaute;             // Standard "erhalten"
  markdownEntfernen: boolean;   // Standard true
  offenAb: string | null;       // "JJJJ-MM-TT": ältere Tage zählen nicht als offen
}

export interface LogbuchNotiz { path: string; title: string; thema: string | null; status: LogbuchStatus; block: number; privat: boolean; }
export interface LogbuchTag { datum: string; status: LogbuchStatus; zaehlt: boolean; notizen: LogbuchNotiz[]; }
export interface LogbuchFachTage {
  kurz: string; ziel: string; moodleUrl: string /* wirksam */; eintraegeProWoche: number; offenAb: string | null;
  tage: LogbuchTag[] /* neueste zuerst */; offen: number /* Tage mit status offen && zaehlt */; kopiert: number;
}
export interface LogbuchUebersicht { faecher: LogbuchFachTage[] /* nur aktive Fächer */; }

export interface LogbuchAnfrage { fach: string; datum: string; mitKi: boolean; privatErlauben: boolean; }
export interface Punkt { text: string; vorschlag: boolean; }
export interface EntwurfAbschnitt { titel: string; quelle: AbschnittQuelle; punkte: Punkt[]; }
export interface BewertungFeld { titel: string; skala: number; }
export interface LogbuchQuelle { path: string; title: string; status: LogbuchStatus; privat: boolean; gesendet: boolean; ekEntfernt: boolean; }
export interface KiAufruf { modell: string; eingabeTokens: number; ausgabeTokens: number; kostenUsd: number; versuche: number; }
export interface LogbuchEntwurf {
  fach: string; datum: string; kopfzeile: string /* "05.10.2026" */;
  abschnitte: EntwurfAbschnitt[]; bewertung: BewertungFeld | null;
  quellen: LogbuchQuelle[]; status: LogbuchStatus; moodleUrl: string;
  mitKi: boolean; ki: KiAufruf | null; warnungen: string[];
}
// quellen[].gesendet: mit KI = was gesendet wurde; ohne KI = was gesendet WÜRDE (UI zeigt das vor dem KI-Klick).

export interface TextAbschnitt { titel: string; punkte: string[]; }
export interface LogbuchTextAnfrage { fach: string; datum: string; abschnitte: TextAbschnitt[]; bewertung: number | null; }
export interface LogbuchKopieren extends LogbuchTextAnfrage { pfade: string[]; }
export interface StatusFehler { path: string; meldung: string; }
export interface LogbuchStatusReport { geschrieben: WriteReport[]; unveraendert: string[]; fehler: StatusFehler[]; sicherung: string | null; }
export interface LogbuchKopiert { moodleUrl: string; moodleGeoeffnet: boolean; moodleFehler: string | null; status: LogbuchStatusReport; }

export interface KiModell { id: string; name: string; eingabeUsdProMio: number; ausgabeUsdProMio: number; }
export interface KiStatus { schluesselGespeichert: boolean; modell: string; modelle: KiModell[]; fehler: string | null; }

// ---------- M3: Aufgaben, Fragen, Lücken (Vertrag laut Design M3) ----------
/** Eine Aufgabe `- [ ]` aus einer Notiz (Arbeitsauftrag der Lehrkraft). `line` ist 1-basiert. */
export interface Arbeitsauftrag {
  path: string; title: string; kind: NoteKind; fach: string | null; lernfeld: string | null;
  datum: string | null; line: number; text: string; done: boolean; statusChar: string; due: string | null;
  indent: number; privat: boolean;
}
/** Eigene Notiz in „07 Material/Aufgabensammlung“ */
export interface Lernaufgabe {
  path: string; title: string; fach: string | null; lernfeld: string | null; typ: string | null;
  schwierigkeit: string | null; geloest: boolean; quelle: string | null /* Linkziel/Alias als Text */; nochmal: boolean;
}
export interface AufgabenUebersicht {
  heute: string;
  ueberfaellig: Arbeitsauftrag[]; dieseWoche: Arbeitsauftrag[]; spaeter: Arbeitsauftrag[];
  ohneFrist: Arbeitsauftrag[]; zuletztErledigt: Arbeitsauftrag[]; lernaufgaben: Lernaufgabe[];
}
export interface AufgabeAbhaken { path: string; line: number; text: string /* Task.text wie geliefert */; erledigt: boolean /* Zielzustand */; }
export interface AufgabeErgebnis { report: WriteReport | null /* null = war schon so */; line: number /* tatsächliche Zeile */; verschoben: boolean; }

export interface FrageEintrag {
  path: string; title: string; kind: NoteKind; fach: string | null; datum: string | null;
  line: number | null /* null = nur Frontmatter-Tag */; text: string; zeile: string /* Originalzeile getrimmt */; privat: boolean;
}
export interface NochmalGruppe { fach: string | null; eintraege: FrageEintrag[]; }
export type GeklaertModus = "durchstreichen" | "abschnitt";
export interface FragenUebersicht { fragen: FrageEintrag[]; nochmal: NochmalGruppe[]; modus: GeklaertModus; }
export interface FrageKlaeren { path: string; line: number; zeile: string; vorschau: boolean; }
export interface FrageGeklaert {
  vorher: string; nachher: string; abschnittNeu: boolean /* „## Geklärt“ wird angelegt */;
  verschoben: boolean /* Modus Abschnitt */; line: number; report: WriteReport | null /* null bei Vorschau */;
}
export interface FragenEinstellungen { geklaertModus: GeklaertModus; }

// ---------- M4: Berichtsheft (Vertrag laut Design M4) ----------
/** Vault-relativer Pfad mit „/“ (Rust `RelPath`) */
export type RelPath = string;
export type TagAbschnitt = "berufsschule" | "betrieb";
/** Rust `TagArt` (M4). In TS umbenannt – `TagArt` ist schon die Tagesart von „Heute“ (M1). */
export type BerichtsTagArt = "stichpunkte" | "krank" | "urlaub" | "feiertag" | "entfallen";
/** `platzhalter` = nur ⟨…⟩ oder *noch einzutragen* */
export type NachweisStand = "fehlt" | "platzhalter" | "vorhanden";

export interface Stichpunkt {
  line: number /* Dateizeile wie Task.line */; text: string /* ohne "- " */; luecke: boolean;
  diagnose: string | null; fach: string | null /* Berufsschule: Kürzel der Zeile */; links: string[];
}
export interface WochenUnterricht {
  path: RelPath; title: string; fach: string | null; thema: string | null; datum: string;
  verlinkt: boolean /* steht schon als [[…]] in der Woche */; privat: boolean;
}
export interface BerichtsTag {
  abschnitt: TagAbschnitt; datum: string; wochentag: string /* "Dienstag" */; ueberschriftZeile: number | null;
  art: BerichtsTagArt; stichpunkte: Stichpunkt[]; fehlt: boolean; bearbeitbar: boolean; hinweis: string | null;
  unterricht: WochenUnterricht[] /* nur Berufsschule, Notizen dieses Tages */; nachweis: NachweisStand;
}
export interface LernfeldZeile { line: number; taetigkeit: string; lernfelder: number[]; zelle: string /* Originalzelle */; }
export interface OffenerPunkt { line: number; text: string; }
export interface BerichtsWoche {
  path: RelPath; title: string; jahr: number; kw: number; montag: string; sonntag: string;
  status: string | null; abgezeichnet: boolean; stunden: string | null; lernfelder: number[];
  tage: BerichtsTag[] /* nach Datum, Berufsschule vor Betrieb am selben Tag */;
  lernfeldTabelle: LernfeldZeile[]; offenePunkte: OffenerPunkt[]; warnungen: string[]; version: NoteVersion;
}
export interface WochenZeile {
  path: RelPath | null /* null = Woche ohne Notiz */; stem: string /* "2026-KW46" */; jahr: number; kw: number;
  montag: string; sonntag: string; status: string | null; abgezeichnet: boolean; stunden: string | null;
  fehlendeTage: string[] /* "Di 10.11." */; offenePunkte: number; luecken: number; diagnosen: number; aktuell: boolean;
}
export interface BerichtsheftUebersicht {
  heute: string; aktuelleWoche: string; aktuelleVorhanden: boolean;
  wochen: WochenZeile[] /* neueste zuerst; fehlende Wochen eingereiht */; offenVorwochen: number /* Badge */;
}
export interface NeueWoche { datum: string /* irgendein Tag der Woche */; }
export interface WochenVorschau {
  relPath: RelPath; text: string; existiert: RelPath | null; eingetragen: RelPath[] /* Unterrichtsnotizen */; warnungen: string[];
}
/** Ergebnis jedes schreibenden M4-Befehls (Vorschau oder geschrieben) */
export interface Aenderung {
  path: RelPath; vorher: string; nachher: string /* ganze Datei, wie geschrieben würde */;
  version: NoteVersion /* Stand beim Lesen = Basis */; unveraendert: boolean; warnungen: string[];
  report: WriteReport | null /* nur nach dem Schreiben */;
}
export interface SchultageUebernehmen { path: RelPath; vorschau: boolean; basis: NoteVersion | null; }
export interface TagSpeichern {
  path: RelPath; abschnitt: TagAbschnitt; datum: string; art: BerichtsTagArt; stichpunkte: string[]; basis: NoteVersion;
}
export interface NachweisAnfrage { path: RelPath; mitKi: boolean; privatErlauben: boolean /* nur dieser Vorgang (Regel 10) */; }
export interface NachweisTag {
  datum: string; abschnitt: TagAbschnitt; wochentag: string; ueberschrift: string; absaetze: string[];
  luecken: string[]; vorhanden: NachweisStand; uebernehmen: boolean /* Vorschlag */; ki: boolean;
}
export interface NachweisQuelle { path: RelPath; title: string; privat: boolean; gesendet: boolean /* mit KI: gesendet; ohne: würde */; ekEntfernt: boolean; }
export interface NachweisEntwurf {
  path: RelPath; tage: NachweisTag[]; quellen: NachweisQuelle[]; mitKi: boolean; ki: KiAufruf | null;
  fehlendeTage: string[]; warnungen: string[];
}
export interface NachweisSchreiben { path: RelPath; tage: NachweisTag[] /* nur die zu schreibenden */; vorschau: boolean; basis: NoteVersion | null; }
export interface LernfeldVorschlag {
  taetigkeit: string; abschnitt: TagAbschnitt; quelle: string /* "Di 10.11." / "GEC 09.11.2026" */; lernfelder: number[];
  grund: string /* "Betrieb.md" | "Lernfeld der Unterrichtsnotiz" | "KI" | "" */; schonInTabelle: boolean;
}
export interface LernfeldAnfrage { path: RelPath; mitKi: boolean; privatErlauben: boolean; }
export interface LernfeldEntwurf {
  path: RelPath; vorschlaege: LernfeldVorschlag[]; lernfelder: LernfeldInfo[]; ki: KiAufruf | null; warnungen: string[];
}
export interface NeueLernfeldZeile { taetigkeit: string; lernfelder: number[]; }
export interface LernfelderSchreiben { path: RelPath; zeilen: NeueLernfeldZeile[]; vorschau: boolean; basis: NoteVersion | null; }
export interface BetriebZeile { zellen: string[] /* Länge = spalten */; taetigkeit: string; uebernehmen: boolean; grund: string; }
export interface BetriebVorschlag {
  betriebPath: RelPath; spalten: string[]; zeilen: BetriebZeile[]; schonVorhanden: string[]; warnungen: string[];
}
export interface BetriebFortschreiben { wochePath: RelPath; zeilen: string[][]; vorschau: boolean; basis: NoteVersion | null; }

export interface FolderInfo {
  path: string;
  name: string;
  depth: number;
  noteCount: number;
  totalCount: number;
}

export interface AppStatus {
  version: string;
  vaultPath: string | null;
  vaultName: string | null;
  vaultOk: boolean;
  noteCount: number;
  watching: boolean;
  defaultStarterPath: string;
  configDir: string;
}

export interface StarterReport {
  target: string;
  createdFiles: number;
  skippedExisting: number;
  createdDirs: number;
}

export const api = {
  appStatus: () => invoke<AppStatus>("app_status"),
  createStarterVault: (target: string | null) => invoke<StarterReport>("create_starter_vault", { target }),
  setVault: (path: string) => invoke<AppStatus>("set_vault", { path }),
  listFolders: () => invoke<FolderInfo[]>("list_folders"),
  listNotes: (folder: string, recursive = false) => invoke<NoteSummary[]>("list_notes", { folder, recursive }),
  readNote: (path: string) => invoke<NoteContent>("read_note", { path }),
  openInObsidian: (path: string) => invoke<string>("open_in_obsidian", { path }),
  reindex: () => invoke<AppStatus>("reindex"),
  // M1
  heute: (datum: string | null) => invoke<HeuteTag>("heute", { datum }),
  listFaecher: () => invoke<FachInfo[]>("list_faecher"),
  listLernfelder: () => invoke<LernfeldInfo[]>("list_lernfelder"),
  listUnterricht: (fach: string | null) => invoke<NoteSummary[]>("list_unterricht", { fach }),
  previewUnterrichtNote: (req: NeueUnterrichtsnotiz) => invoke<NotizVorschau>("preview_unterricht_note", { req }),
  createUnterrichtNote: (req: NeueUnterrichtsnotiz) => invoke<CreateResult>("create_unterricht_note", { req }),
  saveNote: (path: string, text: string, base: NoteVersion) => invoke<SaveResult>("save_note", { path, text, base }),
  // M2
  logbuchEinstellungen: () => invoke<LogbuchFach[]>("logbuch_einstellungen"),        // alle bekannten Fächer (Stundenplan + 01 Fächer + Konfiguration)
  logbuchEinstellungenSpeichern: (faecher: LogbuchFach[]) => invoke<LogbuchFach[]>("logbuch_einstellungen_speichern", { faecher }),
  logbuchUebersicht: () => invoke<LogbuchUebersicht>("logbuch_uebersicht"),
  logbuchEntwurf: (req: LogbuchAnfrage) => invoke<LogbuchEntwurf>("logbuch_entwurf", { req }),
  logbuchText: (req: LogbuchTextAnfrage) => invoke<string>("logbuch_text", { req }),   // Vorschau; ohne Zahl steht "__/10"
  logbuchKopieren: (req: LogbuchKopieren) => invoke<LogbuchKopiert>("logbuch_kopieren", { req }),
  logbuchStatusSetzen: (pfade: string[], status: LogbuchStatus) => invoke<LogbuchStatusReport>("logbuch_status_setzen", { pfade, status }),
  kiStatus: () => invoke<KiStatus>("ki_status"),
  kiSchluesselSpeichern: (schluessel: string) => invoke<KiStatus>("ki_schluessel_speichern", { schluessel }),
  kiSchluesselLoeschen: () => invoke<KiStatus>("ki_schluessel_loeschen"),
  kiModellSetzen: (modell: string) => invoke<KiStatus>("ki_modell_setzen", { modell }),
  // M3
  aufgabenUebersicht: (datum: string | null) => invoke<AufgabenUebersicht>("aufgaben_uebersicht", { datum }),
  aufgabeAbhaken: (req: AufgabeAbhaken) => invoke<AufgabeErgebnis>("aufgabe_abhaken", { req }),
  lernaufgabeGeloest: (path: string, geloest: boolean) => invoke<WriteReport | null>("lernaufgabe_geloest", { path, geloest }),
  fragenUebersicht: () => invoke<FragenUebersicht>("fragen_uebersicht"),
  frageKlaeren: (req: FrageKlaeren) => invoke<FrageGeklaert>("frage_klaeren", { req }),
  fragenEinstellungen: () => invoke<FragenEinstellungen>("fragen_einstellungen"),
  fragenEinstellungenSpeichern: (einstellungen: FragenEinstellungen) =>
    invoke<FragenEinstellungen>("fragen_einstellungen_speichern", { einstellungen }),
  // M4 – schreibende Befehle mit `vorschau` schreiben erst mit `vorschau: false` und `basis` (Regel 9)
  berichtsheftUebersicht: (datum: string | null) => invoke<BerichtsheftUebersicht>("berichtsheft_uebersicht", { datum }),
  berichtsheftWoche: (path: RelPath) => invoke<BerichtsWoche>("berichtsheft_woche", { path }),
  berichtsheftWocheVorschau: (req: NeueWoche) => invoke<WochenVorschau>("berichtsheft_woche_vorschau", { req }),
  berichtsheftWocheAnlegen: (req: NeueWoche) => invoke<CreateResult>("berichtsheft_woche_anlegen", { req }),
  berichtsheftSchultage: (req: SchultageUebernehmen) => invoke<Aenderung>("berichtsheft_schultage", { req }),
  berichtsheftTagSpeichern: (req: TagSpeichern) => invoke<Aenderung>("berichtsheft_tag_speichern", { req }),
  berichtsheftNachweis: (req: NachweisAnfrage) => invoke<NachweisEntwurf>("berichtsheft_nachweis", { req }),
  berichtsheftNachweisSchreiben: (req: NachweisSchreiben) => invoke<Aenderung>("berichtsheft_nachweis_schreiben", { req }),
  berichtsheftLernfelder: (req: LernfeldAnfrage) => invoke<LernfeldEntwurf>("berichtsheft_lernfelder", { req }),
  berichtsheftLernfelderSchreiben: (req: LernfelderSchreiben) => invoke<Aenderung>("berichtsheft_lernfelder_schreiben", { req }),
  betriebVorschlag: (path: RelPath) => invoke<BetriebVorschlag>("betrieb_vorschlag", { path }),
  betriebFortschreiben: (req: BetriebFortschreiben) => invoke<Aenderung>("betrieb_fortschreiben", { req }),
};

export function onVaultChanged(cb: (c: VaultChanged) => void): Promise<UnlistenFn> {
  return listen<VaultChanged>("vault-changed", (e) => cb(e.payload));
}

export const KIND_LABEL: Record<NoteKind, string> = {
  unterricht: "Unterrichtsnotiz",
  berichtsheftWoche: "Berichtsheft",
  aufgabe: "Lernaufgabe",
  glossar: "Begriff",
  fach: "Fach",
  lernfeld: "Lernfeld",
  template: "Vorlage",
  sonstige: "Notiz",
};

/** 2026-10-05 → 05.10.2026 */
export function formatDatum(iso: string | null): string {
  if (!iso) return "";
  const [y, m, d] = iso.split("-");
  return `${d}.${m}.${y}`;
}

/** Tage von `von` bis `bis` (ISO-Daten, in UTC gerechnet; negativ = `bis` liegt davor) */
export function tageBis(von: string, bis: string): number {
  return Math.round((Date.parse(bis + "T00:00:00Z") - Date.parse(von + "T00:00:00Z")) / 86_400_000);
}

const WOCHENTAG_KURZ = ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"];

/** 2026-10-05 → „Mo 05.10.2026“ (in UTC gerechnet, keine Zeitzonen-Sprünge) */
export function formatTagDatum(iso: string): string {
  const d = new Date(iso + "T00:00:00Z");
  const wt = Number.isNaN(d.getTime()) ? "" : WOCHENTAG_KURZ[d.getUTCDay()] + " ";
  return wt + formatDatum(iso);
}

/** ISO-Kalenderwoche (Woche mit dem ersten Donnerstag ist KW 1) */
export function isoWoche(iso: string): { jahr: number; kw: number } {
  const d = new Date(iso + "T00:00:00Z");
  d.setUTCDate(d.getUTCDate() - ((d.getUTCDay() + 6) % 7) + 3); // Donnerstag dieser Woche
  const jahr = d.getUTCFullYear();
  const jan4 = new Date(Date.UTC(jahr, 0, 4));
  const kw = 1 + Math.round(((d.getTime() - jan4.getTime()) / 86_400_000 - 3 + ((jan4.getUTCDay() + 6) % 7)) / 7);
  return { jahr, kw };
}

/** Ältester Unterrichtstag, der noch als offen zählt (Tage kommen neueste zuerst). */
export function aeltesterOffen(f: LogbuchFachTage): LogbuchTag | null {
  for (let i = f.tage.length - 1; i >= 0; i--) {
    const t = f.tage[i];
    if (t.status === "offen" && t.zaehlt) return t;
  }
  return null;
}

/** Statusfarbe: offen = amber, kopiert = lila, abgegeben = mint */
export const LOGBUCH_STATUS_BADGE: Record<LogbuchStatus, string> = {
  offen: "amber",
  kopiert: "lilac",
  abgegeben: "mint",
};

/** Stundenbereich: 2 → „2“, 6–7 → „6–7“ (Halbgeviertstrich) */
export function stdText(von: number, bis: number): string {
  return von === bis ? String(von) : `${von}–${bis}`;
}

/** „Die Notiz „…“ wurde nicht gefunden.“ (AppError::NoteNotFound) */
export function isNoteNotFound(e: unknown): boolean {
  return typeof e === "string" && /^Die Notiz „.*“ wurde nicht gefunden\.$/s.test(e);
}

/** Dateiname ohne Ordner und ohne „.md“ */
export function baseName(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.md$/i, "");
}

/** Pfade im Vault sind unter Windows nicht groß/klein-sensitiv. */
export function samePath(a: string | null | undefined, b: string | null | undefined): boolean {
  const norm = (s: string) => s.replace(/\\/g, "/").toLowerCase();
  return !!a && !!b && norm(a) === norm(b);
}

/** Kostenzeile eines KI-Aufrufs: „KI: Modell · 1.234+567 Tokens · ≈ 0,0042 $“ */
export function kiZeile(k: KiAufruf, modelle: KiModell[] | undefined): string {
  const name = modelle?.find((m) => m.id === k.modell)?.name ?? k.modell;
  const tok = (n: number) => n.toLocaleString("de-DE");
  const kosten = k.kostenUsd.toLocaleString("de-DE", {
    minimumFractionDigits: 2,
    maximumFractionDigits: k.kostenUsd < 0.1 ? 4 : 2,
  });
  const versuche = k.versuche > 1 ? ` · ${k.versuche} Versuche` : "";
  return `KI: ${name} · ${tok(k.eingabeTokens)}+${tok(k.ausgabeTokens)} Tokens · ≈ ${kosten} $${versuche}`;
}

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
