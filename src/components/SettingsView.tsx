import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type AppStatus } from "../api";
import { Icon } from "../icons";

export function SettingsView({
  status,
  onStatus,
  editorDirty,
}: {
  status: AppStatus;
  onStatus: (s: AppStatus) => void;
  /** offene Notiz mit ungespeicherten Änderungen → Vault-Knöpfe gesperrt */
  editorDirty: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  async function run(f: () => Promise<AppStatus>, okText: string) {
    setBusy(true);
    setMsg(null);
    try {
      const s = await f();
      onStatus(s);
      setMsg({ ok: true, text: okText.replace("{n}", String(s.noteCount)) });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  }

  async function changeVault() {
    const dir = await open({ directory: true, title: "Obsidian-Vault auswählen" });
    if (typeof dir === "string") await run(() => api.setVault(dir), "Vault gewechselt – {n} Notizen eingelesen.");
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1 className="page-title">Einstellungen</h1>
          <div className="page-sub">Einstellungen liegen im App-Datenordner, nie im Vault.</div>
        </div>
      </div>
      <section className="settings glass">
        <div>
          <h2>Vault</h2>
          <dl className="kv">
            <dt>Ordner</dt>
            <dd>{status.vaultPath}</dd>
            <dt>Name in Obsidian</dt>
            <dd>{status.vaultName}</dd>
            <dt>Notizen im Index</dt>
            <dd>{status.noteCount}</dd>
            <dt>Dateiwächter</dt>
            <dd>{status.watching ? "aktiv" : "aus"}</dd>
          </dl>
        </div>
        <div className="row">
          <button className="btn" onClick={changeVault} disabled={busy || editorDirty}>
            {Icon.ordnerOeffnen} Vault wechseln
          </button>
          <button className="btn" onClick={() => run(api.reindex, "Neu eingelesen – {n} Notizen.")} disabled={busy || editorDirty}>
            {Icon.refresh} Neu einlesen
          </button>
        </div>
        {editorDirty && <div className="hint">Erst die offene Notiz speichern.</div>}
        {msg && <div className={msg.ok ? "result" : "error"}>{msg.text}</div>}
        <div>
          <h2>App</h2>
          <dl className="kv">
            <dt>Version</dt>
            <dd>{status.version}</dd>
            <dt>App-Datenordner</dt>
            <dd>{status.configDir}</dd>
          </dl>
        </div>
      </section>
    </>
  );
}
