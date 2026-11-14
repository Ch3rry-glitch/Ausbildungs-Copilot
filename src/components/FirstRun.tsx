import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type AppStatus, type StarterReport } from "../api";
import { Icon } from "../icons";

/** Erststart: Starter-Vault anlegen oder bestehenden Vault öffnen. */
export function FirstRun({ status, onDone }: { status: AppStatus; onDone: (s: AppStatus) => void }) {
  const [target, setTarget] = useState(status.defaultStarterPath);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<StarterReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function pickTargetParent() {
    const dir = await open({ directory: true, title: "Wo soll der Vault-Ordner angelegt werden?" });
    if (typeof dir === "string") setTarget(`${dir.replace(/[\\/]$/, "")}\\Ausbildungs-Vault`);
  }

  async function createStarter() {
    setBusy(true);
    setError(null);
    try {
      setReport(await api.createStarterVault(target));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function openExisting() {
    const dir = await open({ directory: true, title: "Obsidian-Vault auswählen" });
    if (typeof dir !== "string") return;
    setBusy(true);
    setError(null);
    try {
      onDone(await api.setVault(dir));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="firstrun">
      <div className="firstrun-inner">
        <div className="brand-logo" style={{ width: 56, height: 56, borderRadius: 18 }}>{Icon.sparkle}</div>
        <h1>Willkommen im Ausbildungs-Cockpit</h1>
        <p className="lead">
          Das Cockpit ist eine zweite Oberfläche auf deinen Obsidian-Vault. Deine Notizen bleiben ganz normale
          Markdown-Dateien – Obsidian funktioniert parallel weiter. Womit möchtest du starten?
        </p>

        <div className="choices">
          <section className="choice glass">
            <div className="choice-icon purple">{Icon.plus}</div>
            <h2>Neuen Vault anlegen</h2>
            <p>Ein leerer Starter-Vault mit Ordnern, Fächern, Lernfeldern, Stundenplan und Vorlagen für die Klasse.</p>
            <div className="path-input">
              <input value={target} onChange={(e) => setTarget(e.target.value)} spellCheck={false} />
              <button className="btn small" onClick={pickTargetParent} disabled={busy}>
                {Icon.ordnerOeffnen} Ort wählen
              </button>
            </div>
            <div className="hint">Vorhandene Dateien werden nie überschrieben.</div>
            {report ? (
              <>
                <div className="result">
                  Fertig: {report.createdFiles} Dateien und {report.createdDirs} Ordner angelegt
                  {report.skippedExisting > 0 && `, ${report.skippedExisting} vorhandene Dateien unverändert gelassen`}.
                  <br />
                  Öffne den Ordner jetzt einmal in Obsidian („Ordner als Vault öffnen“) und installiere die
                  Community-Plugins <b>Dataview</b> und <b>Tasks</b>.
                </div>
                <button className="btn mint" onClick={async () => onDone(await api.appStatus())}>
                  Zum Cockpit
                </button>
              </>
            ) : (
              <button className="btn primary" onClick={createStarter} disabled={busy || !target.trim()}>
                {busy ? "Lege an …" : "Starter-Vault anlegen"}
              </button>
            )}
          </section>

          <section className="choice glass">
            <div className="choice-icon mint">{Icon.ordnerOeffnen}</div>
            <h2>Bestehenden Vault öffnen</h2>
            <p>Du hast schon einen Obsidian-Vault für die Schule? Wähle seinen Ordner aus.</p>
            <ul>
              <li>Die App liest nur – in diesem Stand wird nichts geschrieben.</li>
              <li>Der Ordner <code>.obsidian</code> wird nie angefasst.</li>
              <li>Änderungen aus Obsidian erscheinen sofort.</li>
            </ul>
            <button className="btn" onClick={openExisting} disabled={busy} style={{ marginTop: "auto" }}>
              {Icon.ordnerOeffnen} Vault-Ordner wählen
            </button>
          </section>
        </div>
        {error && <div className="error" style={{ marginTop: 20 }}>{error}</div>}
      </div>
    </div>
  );
}
