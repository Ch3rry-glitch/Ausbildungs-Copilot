import { useCallback, useEffect, useState } from "react";
import { api, errorText, onVaultChanged, type AppStatus, type VaultChanged } from "./api";
import { Sidebar, type View } from "./components/Sidebar";
import { FirstRun } from "./components/FirstRun";
import { NotesView } from "./components/NotesView";
import { SettingsView } from "./components/SettingsView";
import { Icon } from "./icons";

export default function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("notizen");
  const [change, setChange] = useState<{ seq: number; data: VaultChanged } | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.appStatus());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    refreshStatus();
  }, [refreshStatus]);

  // Live-Aktualisierung aus dem Dateiwächter
  useEffect(() => {
    let seq = 0;
    const un = onVaultChanged((data) => {
      seq += 1;
      setChange({ seq, data });
      refreshStatus();
      const n = data.changed.length + data.removed.length;
      setToast(data.fullRescan ? "Vault neu eingelesen" : `${n} ${n === 1 ? "Änderung" : "Änderungen"} aus Obsidian übernommen`);
    });
    return () => {
      un.then((f) => f());
    };
  }, [refreshStatus]);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 3200);
    return () => clearTimeout(t);
  }, [toast]);

  if (error) return <div className="firstrun"><div className="error">{error}</div></div>;
  if (!status) return null;
  if (!status.vaultOk) return <FirstRun status={status} onDone={setStatus} />;

  return (
    <div className="shell">
      <Sidebar view={view} onView={setView} status={status} />
      <main className="main">
        {view === "notizen" && <NotesView change={change} />}
        {view === "einstellungen" && <SettingsView status={status} onStatus={setStatus} />}
      </main>
      {toast && (
        <div className="toast glass">
          <span className="badge mint">{Icon.refresh} live</span>
          {toast}
        </div>
      )}
    </div>
  );
}
