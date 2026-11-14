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
}

export interface NoteContent {
  meta: NoteMeta;
  body: string;
  frontmatterRaw: string | null;
}

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

export interface VaultChanged {
  changed: string[];
  removed: string[];
  fullRescan: boolean;
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

export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
