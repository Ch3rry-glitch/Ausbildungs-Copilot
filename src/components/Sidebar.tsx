import type { AppStatus } from "../api";
import { Icon } from "../icons";

export type View = "notizen" | "einstellungen";

// Reihenfolge laut SPEC §8. Noch nicht gebaute Bereiche zeigen ihren Meilenstein.
const ITEMS: { key: string; label: string; milestone?: string }[] = [
  { key: "heute", label: "Heute", milestone: "M1" },
  { key: "unterricht", label: "Unterricht", milestone: "M1" },
  { key: "logbuch", label: "Logbuch", milestone: "M2" },
  { key: "aufgaben", label: "Aufgaben", milestone: "M3" },
  { key: "fragen", label: "Fragen", milestone: "M3" },
  { key: "berichtsheft", label: "Berichtsheft", milestone: "M4" },
  { key: "inbox", label: "Inbox", milestone: "M5" },
  { key: "pruefungen", label: "Prüfungen", milestone: "M6" },
];

export function Sidebar({ view, onView, status }: { view: View; onView: (v: View) => void; status: AppStatus }) {
  return (
    <aside className="sidebar glass">
      <div className="brand">
        <div className="brand-logo">{Icon.sparkle}</div>
        <div>
          <div className="brand-name">Ausbildungs-Cockpit</div>
          <div className="brand-sub">Version {status.version}</div>
        </div>
      </div>

      {ITEMS.map((it) => (
        <button key={it.key} className="nav-item" disabled title={`kommt in ${it.milestone}`}>
          {Icon[it.key]}
          {it.label}
          <span className="soon">{it.milestone}</span>
        </button>
      ))}

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
