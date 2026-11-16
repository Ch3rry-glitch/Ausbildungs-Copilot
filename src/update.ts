// Updates: ein gemeinsamer Zustand für den Hinweis in der Seitenleiste und die Karte in den
// Einstellungen. Geprüft und installiert wird in Rust (src-tauri/src/update.rs).
import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { errorText } from "./api";

export interface UpdateInfo {
  version: string;
  notizen: string | null;
  /** RFC 3339 */
  datum: string | null;
}

export interface UpdateStatus {
  version: string;
  /** öffentlicher Update-Schlüssel eingetragen */
  eingerichtet: boolean;
  /** Release-Build: sieht beim Start von selbst nach */
  automatisch: boolean;
  letztePruefung: string | null;
  verfuegbar: UpdateInfo | null;
}

export interface UpdatePruefung {
  geprueft: boolean;
  verfuegbar: UpdateInfo | null;
  fehler: string | null;
  letztePruefung: string | null;
}

interface Fortschritt {
  geladen: number;
  gesamt: number | null;
}

export const updateApi = {
  status: () => invoke<UpdateStatus>("update_status"),
  pruefen: (vonHand: boolean) => invoke<UpdatePruefung>("update_pruefen", { vonHand }),
  installieren: () => invoke<void>("update_installieren"),
};

export interface UpdateZustand {
  verfuegbar: UpdateInfo | null;
  /** „Später“: Hinweis in der Seitenleiste bis zum nächsten Start ausgeblendet */
  spaeter: boolean;
  phase: "ruhe" | "sucht" | "laedt";
  /** Download in Prozent; null = Größe unbekannt */
  prozent: number | null;
  fehler: string | null;
  /** letzte Prüfung von Hand ergab: keine neuere Version */
  aktuell: boolean;
  letztePruefung: string | null;
}

let zustand: UpdateZustand = {
  verfuegbar: null,
  spaeter: false,
  phase: "ruhe",
  prozent: null,
  fehler: null,
  aktuell: false,
  letztePruefung: null,
};
const hoerer = new Set<() => void>();

function setzen(z: Partial<UpdateZustand>) {
  zustand = { ...zustand, ...z };
  hoerer.forEach((h) => h());
}

export function useUpdate(): UpdateZustand {
  return useSyncExternalStore(
    (h) => {
      hoerer.add(h);
      return () => hoerer.delete(h);
    },
    () => zustand,
  );
}

/** Sperre vor dem Installieren (ungespeicherte Arbeit): liefert den Grund oder null */
type Sperre = () => Promise<string | null>;
let sperre: Sperre | null = null;
export function setUpdateSperre(s: Sperre | null) {
  sperre = s;
}

let gestartet = false;

/** Beim Start: Rust entscheidet (nur Release-Build, höchstens einmal am Tag). Fehler bleiben still. */
export function beimStartPruefen() {
  if (gestartet) return;
  gestartet = true;
  updateApi
    .pruefen(false)
    .then((r) => {
      if (r.verfuegbar) setzen({ verfuegbar: r.verfuegbar, spaeter: false });
      setzen({ letztePruefung: r.letztePruefung });
    })
    .catch(() => {});
}

/** „Nach Updates suchen“ in den Einstellungen */
export async function vonHandPruefen() {
  if (zustand.phase !== "ruhe") return;
  setzen({ phase: "sucht", fehler: null, aktuell: false });
  try {
    const r = await updateApi.pruefen(true);
    setzen({
      phase: "ruhe",
      verfuegbar: r.verfuegbar,
      spaeter: false,
      fehler: r.fehler,
      aktuell: r.geprueft && !r.verfuegbar && !r.fehler,
      letztePruefung: r.letztePruefung,
    });
  } catch (e) {
    setzen({ phase: "ruhe", fehler: errorText(e) });
  }
}

/** „Jetzt aktualisieren“: herunterladen, Signatur prüfen, installieren – die App startet neu. */
export async function installieren() {
  if (zustand.phase !== "ruhe" || !zustand.verfuegbar) return;
  const grund = sperre ? await sperre() : null;
  if (grund) {
    setzen({ fehler: grund });
    return;
  }
  setzen({ phase: "laedt", prozent: null, fehler: null });
  const un = await listen<Fortschritt>("update-fortschritt", (e) => {
    const { geladen, gesamt } = e.payload;
    setzen({ prozent: gesamt ? Math.min(100, Math.floor((geladen * 100) / gesamt)) : null });
  });
  try {
    await updateApi.installieren(); // unter Windows beendet der Installer die App vorher
  } catch (e) {
    setzen({ phase: "ruhe", prozent: null, fehler: errorText(e) });
  } finally {
    un();
  }
}

export function spaeter() {
  setzen({ spaeter: true, fehler: null });
}

/** „16.11.2026, 10:42“ */
export function zeitpunkt(iso: string | null): string {
  if (!iso) return "noch nie";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("de-DE", { dateStyle: "medium", timeStyle: "short" });
}
