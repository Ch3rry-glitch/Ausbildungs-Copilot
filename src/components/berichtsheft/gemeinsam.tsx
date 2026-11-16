// Gemeinsame Bausteine der Berichtsheft-Ansicht: Beschriftungen, Datumsformate, kleine Anzeigen
// und der Ablauf „Vorschau → Diff → Schreiben → Toast“ (Regel 9).
import { useRef, useState, type ReactNode } from "react";
import {
  baseName,
  errorText,
  type Aenderung,
  type BerichtsTagArt,
  type KiStatus,
  type LernfeldInfo,
  type NachweisStand,
  type NoteVersion,
  type TagAbschnitt,
} from "../../api";
import { Icon } from "../../icons";
import { useToast, type ToastInput } from "../../toast";
import { DiffDialog } from "../DiffView";

// ---------- Beschriftungen ----------
export const ABSCHNITT_LABEL: Record<TagAbschnitt, string> = { berufsschule: "Berufsschule", betrieb: "Betrieb" };
export const ABSCHNITT_BADGE: Record<TagAbschnitt, string> = { berufsschule: "purple", betrieb: "mint" };

/** Reihenfolge der Tagesarten im Bearbeiten-Feld */
export const ARTEN: BerichtsTagArt[] = ["stichpunkte", "krank", "urlaub", "feiertag", "entfallen"];
export const ART_LABEL: Record<BerichtsTagArt, string> = {
  stichpunkte: "Stichpunkte",
  krank: "Arbeitsunfähig erkrankt",
  urlaub: "Urlaub",
  feiertag: "Feiertag",
  entfallen: "Entfallen",
};

/** Die Zeile, die das Backend für einen Sondertag schreibt (nur zur Anzeige, ohne Markdown). */
export function sondertagZeile(art: BerichtsTagArt, abschnitt: TagAbschnitt): string {
  switch (art) {
    case "krank":
      return abschnitt === "berufsschule"
        ? "Arbeitsunfähig erkrankt — ganztägig, kein Berufsschulbesuch"
        : "Arbeitsunfähig erkrankt — ganztägig";
    case "urlaub":
      return "Urlaub";
    case "feiertag":
      return "Feiertag";
    case "entfallen":
      return "Entfallen";
    default:
      return "";
  }
}

const NACHWEIS_ANZEIGE: Record<NachweisStand, { cls: string; text: string; title: string }> = {
  fehlt: { cls: "muted", text: "Nachweis fehlt", title: "Im „Ausformulierten Nachweis“ steht für diesen Tag noch nichts." },
  platzhalter: { cls: "amber", text: "Nachweis: Platzhalter", title: "Dort steht nur ein Platzhalter (⟨…⟩ oder *noch einzutragen*)." },
  vorhanden: { cls: "mint", text: "Nachweis vorhanden", title: "Für diesen Tag ist schon Fließtext ausformuliert." },
};

export function NachweisBadge({ stand }: { stand: NachweisStand }) {
  const n = NACHWEIS_ANZEIGE[stand];
  return (
    <span className={`badge ${n.cls}`} title={n.title}>
      {n.text}
    </span>
  );
}

/** Status der Wochennotiz: „Abgeschlossen“/„fertig“ mint, „In Arbeit“ amber, sonst neutral */
export function statusBadge(status: string | null): string {
  const s = status?.trim().toLowerCase() ?? "";
  if (s === "abgeschlossen" || s === "fertig") return "mint";
  if (s === "in arbeit") return "amber";
  return "muted";
}

// ---------- Datum und Zahlen ----------
/** 2026-11-10 → „10.11.“ */
export function kurzDatum(iso: string): string {
  const [, m, d] = iso.split("-");
  return `${d}.${m}.`;
}

/** „09.11.–15.11.“ */
export function zeitraumKurz(montag: string, sonntag: string): string {
  return `${kurzDatum(montag)}–${kurzDatum(sonntag)}`;
}

/** „Dienstag, 10.11.“ */
export function tagTitel(wochentag: string, datum: string): string {
  return `${wochentag}, ${kurzDatum(datum)}`;
}

/** 4 → „LF04“ */
export function lfKurz(n: number): string {
  return `LF${String(n).padStart(2, "0")}`;
}

export function lfTitel(n: number, lernfelder: LernfeldInfo[]): string {
  return lernfelder.find((l) => l.nummer === n)?.titel ?? lfKurz(n);
}

/** „1 Tag fehlt“ / „3 Tage fehlen“ */
export function anzahl(n: number, eins: string, mehr: string): string {
  return `${n} ${n === 1 ? eins : mehr}`;
}

let nextId = 0;
/** Fortlaufende Schlüssel für bearbeitbare Listen (Fokus bleibt beim Entfernen am richtigen Feld) */
export function neueId(): number {
  nextId += 1;
  return nextId;
}

// ---------- Hinweise ----------
export function Hinweise({ texte }: { texte: string[] }) {
  if (texte.length === 0) return null;
  return (
    <div className="lb-hinweise">
      {texte.map((w, i) => (
        <div key={i} className="hint lb-hinweis">
          {Icon.warning}
          <span>{w}</span>
        </div>
      ))}
    </div>
  );
}

/** KI-Knöpfe sind ohne Schlüssel gesperrt – mit Weg zu den Einstellungen. */
export function KiFehltHinweis({ kiStatus, onGoSettings, ohne }: { kiStatus: KiStatus | null; onGoSettings: () => void; ohne: string }) {
  if (kiStatus?.schluesselGespeichert) return null;
  return (
    <div className="hint">
      {kiStatus?.fehler ?? `Kein KI-Schlüssel hinterlegt – Einstellungen → KI, oder „${ohne}“.`}{" "}
      <button className="lb-link inline" onClick={onGoSettings}>
        Zu den Einstellungen
      </button>
    </div>
  );
}

// ---------- Rückmeldung nach dem Schreiben ----------
type Notify = (t: ToastInput) => void;

/** Toast nach einem schreibenden Befehl: was wohin geschrieben wurde, mit „In Obsidian öffnen“. */
export function meldeAenderung(notify: Notify, erfolg: string, a: Aenderung) {
  if (a.unveraendert || !a.report) {
    notify({ kind: "info", text: `Keine Änderung nötig · ${baseName(a.path)}`, path: a.path, detail: a.warnungen });
    return;
  }
  const r = a.report;
  notify({
    kind: "saved",
    text: `${erfolg} · ${baseName(r.relPath)}`,
    path: r.relPath,
    detail: [`Datei: ${r.relPath}`, ...new Set([...a.warnungen, ...r.warnungen])],
  });
}

// ---------- Vorschau → Diff → Schreiben ----------
export interface SchreibAuftrag {
  /** Untertitel im Dialog, z. B. „Nachweis in die Wochennotiz schreiben“ */
  aktion: string;
  /** Beschriftung des Schreib-Knopfs */
  knopf: string;
  /** Text des Toasts nach dem Schreiben */
  erfolg: string;
  vorschau: () => Promise<Aenderung>;
  /** schreibt genau mit der Basis der gezeigten Vorschau */
  schreiben: (basis: NoteVersion) => Promise<Aenderung>;
  /** nach dem Schreiben (auch „keine Änderung“) – z. B. Woche neu laden */
  danach?: (a: Aenderung) => void;
}

/**
 * Ablauf für jede Änderung an einer bestehenden Datei: Vorschau holen, Diff zeigen, erst nach der
 * Bestätigung mit `basis` schreiben. Fehler (z. B. Gesundheitsangabe, Datei inzwischen geändert)
 * kommen als Toast; der Entwurf des Aufrufers bleibt stehen.
 */
export function useSchreibFluss(): { laeuft: boolean; starten: (a: SchreibAuftrag) => Promise<void>; dialog: ReactNode } {
  const { notify } = useToast();
  const [laeuft, setLaeuft] = useState(false);
  const [offen, setOffen] = useState<{ auftrag: SchreibAuftrag; aenderung: Aenderung } | null>(null);
  const [schreibt, setSchreibt] = useState(false);
  const belegt = useRef(false);

  async function starten(auftrag: SchreibAuftrag) {
    if (belegt.current) return;
    belegt.current = true;
    setLaeuft(true);
    try {
      const a = await auftrag.vorschau();
      setOffen({ auftrag, aenderung: a });
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
      belegt.current = false;
      setLaeuft(false);
    }
  }

  function schliessen() {
    setOffen(null);
    setLaeuft(false);
    belegt.current = false;
  }

  async function bestaetigen() {
    if (!offen || schreibt) return;
    const { auftrag, aenderung } = offen;
    setSchreibt(true);
    try {
      const r = await auftrag.schreiben(aenderung.version);
      meldeAenderung(notify, auftrag.erfolg, r);
      auftrag.danach?.(r);
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    } finally {
      setSchreibt(false);
      schliessen();
    }
  }

  const dialog = offen ? (
    <DiffDialog
      aenderung={offen.aenderung}
      aktion={offen.auftrag.aktion}
      knopf={offen.auftrag.knopf}
      schreibt={schreibt}
      onConfirm={() => void bestaetigen()}
      onCancel={() => {
        if (!schreibt) schliessen();
      }}
    />
  ) : null;

  return { laeuft, starten, dialog };
}
