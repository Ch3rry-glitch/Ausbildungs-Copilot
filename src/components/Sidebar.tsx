import type { AppStatus } from "../api";
import { Icon } from "../icons";

export type View = "heute" | "unterricht" | "logbuch" | "aufgaben" | "fragen" | "klausur" | "notizen" | "einstellungen";

// Reihenfolge laut SPEC §8. Noch nicht gebaute Bereiche zeigen ihren Meilenstein.
const ACTIVE: { key: View; label: string }[] = [
  { key: "heute", label: "Heute" },
  { key: "unterricht", label: "Unterricht" },
  { key: "logbuch", label: "Logbuch" },
  { key: "aufgaben", label: "Aufgaben" },
  { key: "fragen", label: "Fragen" },
];

const SOON: { key: string; label: string; milestone: string }[] = [
  { key: "berichtsheft", label: "Berichtsheft", milestone: "M4" },
  { key: "inbox", label: "Inbox", milestone: "M5" },
  { key: "pruefungen", label: "Prüfungen", milestone: "M6" },
];

export function Sidebar({
  view,
  onView,
  status,
  editorDirty,
  logbuchOffen,
  aufgabenUeberfaellig,
}: {
  view: View;
  onView: (v: View) => void;
  status: AppStatus;
  editorDirty: boolean;
  /** Zahl offener Logbucheinträge; null = keine Anzeige */
  logbuchOffen: number | null;
  /** Zahl überfälliger Arbeitsaufträge; null = keine Anzeige */
  aufgabenUeberfaellig: number | null;
}) {
  return (
    <aside className="sidebar glass">
      <div className="brand">
        <div className="brand-logo">{Icon.sparkle}</div>
        <div>
          <div className="brand-name">Ausbildungs-Cockpit</div>
          <div className="brand-sub">Version {status.version}</div>
        </div>
      </div>

      {ACTIVE.map((it) => (
        <button key={it.key} className={`nav-item ${view === it.key ? "active" : ""}`} onClick={() => onView(it.key)}>
          {Icon[it.key]}
          {it.label}
          {it.key === "unterricht" && editorDirty && <span className="nav-dot" title="Ungespeicherte Änderungen" />}
          {it.key === "logbuch" && logbuchOffen != null && logbuchOffen > 0 && (
            <span className="nav-count" title={logbuchOffen === 1 ? "1 offener Logbucheintrag" : `${logbuchOffen} offene Logbucheinträge`}>
              {logbuchOffen}
            </span>
          )}
          {it.key === "aufgaben" && aufgabenUeberfaellig != null && aufgabenUeberfaellig > 0 && (
            <span
              className="nav-count red"
              title={aufgabenUeberfaellig === 1 ? "1 überfällige Aufgabe" : `${aufgabenUeberfaellig} überfällige Aufgaben`}
            >
              {aufgabenUeberfaellig}
            </span>
          )}
        </button>
      ))}

      {SOON.map((it) => (
        <button key={it.key} className="nav-item" disabled title={`kommt in ${it.milestone}`}>
          {Icon[it.key]}
          {it.label}
          <span className="soon">{it.milestone}</span>
        </button>
      ))}

      {/* Klausur-Trainer: eigener Eintrag nach „Prüfungen“ */}
      <button className={`nav-item ${view === "klausur" ? "active" : ""}`} onClick={() => onView("klausur")}>
        {Icon.klausur}
        Klausur-Trainer
      </button>

      <div className="nav-sep" />
      <button className={`nav-item ${view === "notizen" ? "active" : ""}`} onClick={() => onView("notizen")}>
        {Icon.notizen}
        Notizen
      </button>
      <button className={`nav-item ${view === "einstellungen" ? "active" : ""}`} onClick={() => onView("einstellungen")}>
        {Icon.einstellungen}
        Einstellungen
      </button>

      <div className="sidebar-foot">
        Vault
        <strong title={status.vaultPath ?? ""}>{status.vaultName}</strong>
        <div className="live">
          <span className={`live-dot ${status.watching ? "" : "off"}`} />
          {status.watching ? `${status.noteCount} Notizen · live` : "Dateiwächter aus"}
        </div>
      </div>
    </aside>
  );
}
