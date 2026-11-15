// Gemeinsame Schreibvorgänge für „Aufgaben“ und „Heute“: Abhaken und „gelöst“ setzen.
// Jeder Schreibvorgang endet mit einem Toast: was wurde wo geschrieben, samt „In Obsidian öffnen“.
// Notiztext erscheint nur im Toast – nie in der Konsole, nie im Browser-Speicher.
import { api, errorText, formatDatum, tageBis } from "./api";
import { klartext } from "./markdown";
import type { ToastInput } from "./toast";

type Notify = (t: ToastInput) => void;

/** Text für Toasts kürzen (eine Zeile reicht zum Wiedererkennen) */
export function kuerzen(s: string, max = 90): string {
  return s.length > max ? s.slice(0, max - 1).trimEnd() + "…" : s;
}

/** Anzeigetext einer Aufgabe */
export function aufgabenText(text: string): string {
  return klartext(text) || "(ohne Text)";
}

/** Fälligkeits-Badge relativ zu `heute`: rot wenn überfällig, „heute“, „morgen“, sonst TT.MM. */
export function faelligkeit(due: string, heute: string): { text: string; cls: string; title: string } {
  const d = tageBis(heute, due);
  const [y, m, t] = due.split("-");
  const kurz = y === heute.slice(0, 4) ? `${t}.${m}.` : formatDatum(due);
  const title = `fällig am ${formatDatum(due)}`;
  if (d < 0) return { text: kurz, cls: "red", title: `überfällig seit ${formatDatum(due)}` };
  if (d === 0) return { text: "heute", cls: "amber", title };
  if (d === 1) return { text: "morgen", cls: "purple", title };
  return { text: kurz, cls: "muted", title };
}

export interface AbhakenZiel {
  path: string;
  title: string;
  line: number;
  /** Task.text genau so, wie das Backend ihn geliefert hat */
  text: string;
}

/** Aufgabe abhaken (`erledigt: true`) oder wieder öffnen. Fehler → Fehler-Toast; Rückgabe: geschrieben/schon so. */
export async function abhaken(notify: Notify, z: AbhakenZiel, erledigt: boolean): Promise<boolean> {
  try {
    const r = await api.aufgabeAbhaken({ path: z.path, line: z.line, text: z.text, erledigt });
    const was = kuerzen(aufgabenText(z.text));
    const wo = `${z.title}, Zeile ${r.line}`;
    if (!r.report) {
      notify({ kind: "info", text: `${erledigt ? "Schon abgehakt" : "Schon offen"}: ${was} · ${wo}`, path: z.path });
      return true;
    }
    notify({
      kind: "saved",
      text: `${erledigt ? "Abgehakt" : "Wieder offen"}: ${was} · ${wo}`,
      path: r.report.relPath,
      detail: [
        `Datei: ${r.report.relPath}`,
        ...(r.verschoben ? [`Die Aufgabe stand inzwischen in Zeile ${r.line} (vorher ${z.line}).`] : []),
        ...r.report.warnungen,
      ],
    });
    return true;
  } catch (e) {
    notify({ kind: "error", text: errorText(e) });
    return false;
  }
}

/** Frontmatter `geloest` einer Lernaufgabe setzen. */
export async function lernaufgabeSetzen(notify: Notify, l: { path: string; title: string }, geloest: boolean): Promise<boolean> {
  try {
    const r = await api.lernaufgabeGeloest(l.path, geloest);
    if (!r) {
      notify({ kind: "info", text: `Unverändert: „${l.title}“ ist schon ${geloest ? "gelöst" : "offen"}.`, path: l.path });
      return true;
    }
    notify({
      kind: "saved",
      text: `${geloest ? "Gelöst" : "Wieder offen"}: ${l.title} · geloest: ${geloest}`,
      path: r.relPath,
      detail: [`Datei: ${r.relPath}`, ...r.warnungen],
    });
    return true;
  } catch (e) {
    notify({ kind: "error", text: errorText(e) });
    return false;
  }
}
