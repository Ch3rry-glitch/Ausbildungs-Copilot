import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, baseName, errorText, onVaultChanged, type AppStatus, type NeueUnterrichtsnotiz, type VaultChanged } from "./api";
import { Sidebar, type View } from "./components/Sidebar";
import { FirstRun } from "./components/FirstRun";
import { NotesView } from "./components/NotesView";
import { SettingsView } from "./components/SettingsView";
import { HeuteView } from "./components/HeuteView";
import { UnterrichtView, type EditorHandle } from "./components/UnterrichtView";
import { CreateNoteDialog } from "./components/CreateNoteDialog";
import { askUnsaved, isModalOpen } from "./components/Modal";
import { ToastProvider, useToast } from "./toast";

export default function App() {
  return (
    <ToastProvider>
      <Cockpit />
    </ToastProvider>
  );
}

function Cockpit() {
  const { notify } = useToast();
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("heute");
  const [unterrichtMounted, setUnterrichtMounted] = useState(false);
  const [change, setChange] = useState<{ seq: number; data: VaultChanged } | null>(null);
  const [openRequest, setOpenRequest] = useState<{ path: string; line?: number; seq: number } | null>(null);
  const [createInitial, setCreateInitial] = useState<Partial<NeueUnterrichtsnotiz> | null>(null);
  const [editorDirty, setEditorDirty] = useState(false);
  const editorRef = useRef<EditorHandle | null>(null);
  const changeSeq = useRef(0);
  const openSeq = useRef(0);
  const viewRef = useRef(view);
  viewRef.current = view;

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.appStatus());
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);

  const go = useCallback((v: View) => {
    if (v === "unterricht") setUnterrichtMounted(true);
    setView(v);
  }, []);

  const openNote = useCallback((path: string, line?: number) => {
    openSeq.current += 1;
    setUnterrichtMounted(true);
    setView("unterricht");
    setOpenRequest({ path, line, seq: openSeq.current });
  }, []);

  const openCreate = useCallback((initial: Partial<NeueUnterrichtsnotiz>) => {
    setCreateInitial(initial);
  }, []);

  // Live-Aktualisierung aus dem Dateiwächter. Eigene Schreibvorgänge (`own`) lösen keinen Toast aus.
  useEffect(() => {
    const un = onVaultChanged((data) => {
      changeSeq.current += 1;
      setChange({ seq: changeSeq.current, data });
      void refreshStatus();
      const n = data.changed.length + data.removed.length;
      if (data.fullRescan) notify({ kind: "live", text: "Vault neu eingelesen" });
      else if (n > 0) notify({ kind: "live", text: `${n} ${n === 1 ? "Änderung" : "Änderungen"} aus Obsidian übernommen` });
    });
    return () => {
      void un.then((f) => f());
    };
  }, [refreshStatus, notify]);

  // Strg+N: aktuelle Stunde → Notiz öffnen oder anlegen
  const strgNBusy = useRef(false);
  const strgN = useCallback(async () => {
    if (strgNBusy.current) return;
    strgNBusy.current = true;
    try {
      const h = await api.heute(null);
      const b = h.aktuellerBlock != null ? h.bloecke.find((x) => x.position === h.aktuellerBlock) : undefined;
      if (b?.notiz) openNote(b.notiz.path);
      else if (b) openCreate({ datum: h.datum, fach: b.fach.kurz, block: b.fachNr });
      else openCreate({ datum: h.datum });
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    } finally {
      strgNBusy.current = false;
    }
  }, [openNote, openCreate, notify]);

  // Globale Tastenkürzel (Bubble-Phase; CodeMirrors Mod-s hat dann schon preventDefault gesetzt)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const mod = e.ctrlKey || e.metaKey;
      const key = e.key.toLowerCase();
      if (mod && !e.altKey && !e.shiftKey && key === "n") {
        e.preventDefault();
        if (!isModalOpen()) void strgN();
        return;
      }
      if (mod && !e.altKey && key === "s") {
        e.preventDefault();
        if (!isModalOpen() && viewRef.current === "unterricht") void editorRef.current?.save();
        return;
      }
      if (e.key === "F5" || (mod && key === "r")) {
        if (editorRef.current?.dirty) {
          e.preventDefault();
          notify({ kind: "info", text: "Ungespeicherte Änderungen – bitte zuerst speichern." });
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [strgN, notify]);

  // Fenster schließen: einmal registrieren; liest editorRef.current zur Aufrufzeit.
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (editorRef.current?.dirty) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    let asking = false;
    const un = getCurrentWindow().onCloseRequested(async (e) => {
      if (asking) {
        e.preventDefault();
        return;
      }
      const ed = editorRef.current;
      if (ed) await ed.whenIdle(); // nie einen zweiten Speichervorgang auf der alten Basis starten
      if (!ed?.dirty) return; // kein preventDefault → das Fenster schließt
      e.preventDefault();
      asking = true;
      try {
        const name = ed.currentPath ? baseName(ed.currentPath) : "Notiz";
        const c = await askUnsaved(name, { canSave: ed.canSave });
        if (c === "cancel") return;
        if (c === "save" && !(await ed.save())) return;
        window.removeEventListener("beforeunload", onBeforeUnload);
        await getCurrentWindow().destroy();
      } finally {
        asking = false;
      }
    });
    return () => {
      window.removeEventListener("beforeunload", onBeforeUnload);
      void un.then((f) => f());
    };
  }, []);

  if (error) return <div className="firstrun"><div className="error">{error}</div></div>;
  if (!status) return null;
  if (!status.vaultOk) return <FirstRun status={status} onDone={setStatus} />;

  return (
    <div className="shell">
      <Sidebar view={view} onView={go} status={status} editorDirty={editorDirty} />
      <main className="main">
        {view === "heute" && <HeuteView change={change} onOpenNote={openNote} onCreate={openCreate} />}
        {unterrichtMounted && (
          <div className="view-host" hidden={view !== "unterricht"}>
            <UnterrichtView
              active={view === "unterricht"}
              vaultPath={status.vaultPath}
              change={change}
              openRequest={openRequest}
              editorRef={editorRef}
              onDirtyChange={setEditorDirty}
              onCreate={openCreate}
            />
          </div>
        )}
        {view === "notizen" && <NotesView change={change} />}
        {view === "einstellungen" && <SettingsView status={status} onStatus={setStatus} editorDirty={editorDirty} />}
      </main>
      {createInitial && (
        <CreateNoteDialog
          initial={createInitial}
          onClose={() => setCreateInitial(null)}
          onCreated={(relPath) => {
            setCreateInitial(null);
            openNote(relPath);
          }}
          onOpenExisting={(relPath) => {
            setCreateInitial(null);
            openNote(relPath);
          }}
        />
      )}
    </div>
  );
}
