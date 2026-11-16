// Darstellung: Farbthema und Glasstufe der Karten.
// Gespeichert in den App-Einstellungen (Rust, `config.json` im App-Datenordner) – nie im Vault.
// Angewendet über data-theme / data-glas am <html>; die Farben stehen in styles.css und themes.css.
// Eigenständiges Modul (statt api.ts), damit Änderungen anderer Zweige an api.ts nicht kollidieren.
import { invoke } from "@tauri-apps/api/core";

export type Farbthema = "violett" | "ozean" | "wald" | "rose" | "graphit";
export type Glasstufe = "klar" | "mittel" | "satt";
export interface Darstellung { farbe: Farbthema; glas: Glasstufe; }

/** Voreinstellung = das bisherige Aussehen */
export const STANDARD: Darstellung = { farbe: "violett", glas: "mittel" };

export const FARBTHEMEN: { value: Farbthema; label: string; hint: string }[] = [
  { value: "violett", label: "Violett", hint: "Standard – dunkles Violett mit Mint" },
  { value: "ozean", label: "Ozean", hint: "Nachtblau mit Himmelblau" },
  { value: "wald", label: "Wald", hint: "Dunkelgrün mit Smaragd und Limette" },
  { value: "rose", label: "Rosé", hint: "Brombeere mit Rosé" },
  { value: "graphit", label: "Graphit", hint: "Neutrales Anthrazit mit Schiefergrau" },
];

export const GLASSTUFEN: { value: Glasstufe; label: string; hint: string }[] = [
  { value: "klar", label: "Klar", hint: "Durchsichtiger, mehr Hintergrund" },
  { value: "mittel", label: "Mittel", hint: "Standard – Milchglas" },
  { value: "satt", label: "Satt", hint: "Fast deckend, am besten lesbar" },
];

/** Unbekannte Werte (z. B. aus einer neueren Version) → Voreinstellung, wie im Backend. */
function gueltig(d: unknown): Darstellung {
  const o = (d && typeof d === "object" ? d : {}) as Partial<Record<keyof Darstellung, unknown>>;
  const farbe = FARBTHEMEN.find((t) => t.value === o.farbe)?.value ?? STANDARD.farbe;
  const glas = GLASSTUFEN.find((g) => g.value === o.glas)?.value ?? STANDARD.glas;
  return { farbe, glas };
}

// Nur ein Merker gegen kurzes Aufblitzen des Standardthemas beim Start; maßgeblich ist config.json.
const MERKER = "cockpit.darstellung";

export function anwenden(d: Darstellung): void {
  const html = document.documentElement;
  html.dataset.theme = d.farbe;
  html.dataset.glas = d.glas;
  try {
    localStorage.setItem(MERKER, JSON.stringify(d));
  } catch {
    // ohne Speicher geht es auch – dann blitzt beim Start kurz das Standardthema auf
  }
}

export async function darstellungLesen(): Promise<Darstellung> {
  return gueltig(await invoke<Darstellung>("darstellung"));
}

export async function darstellungSpeichern(darstellung: Darstellung): Promise<Darstellung> {
  return gueltig(await invoke<Darstellung>("darstellung_speichern", { darstellung }));
}

/** Beim Start: zuletzt gezeigte Wahl sofort, danach die gespeicherte aus den Einstellungen. */
export function startDarstellung(): void {
  try {
    const t = localStorage.getItem(MERKER);
    if (t) anwenden(gueltig(JSON.parse(t)));
  } catch {
    // kaputter Merker: einfach Standard, gleich kommt der richtige Wert
  }
  darstellungLesen()
    .then(anwenden)
    .catch((e) => console.warn("[darstellung]", e));
}
