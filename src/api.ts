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

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
