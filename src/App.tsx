import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api, baseName, errorText, onVaultChanged, type AppStatus, type NeueUnterrichtsnotiz, type VaultChanged } from "./api";
import { Sidebar, type View } from "./components/Sidebar";
import { FirstRun } from "./components/FirstRun";
import { NotesView } from "./components/NotesView";
import { SettingsView } from "./components/SettingsView";
import { HeuteView } from "./components/HeuteView";
import { UnterrichtView, type EditorHandle } from "./components/UnterrichtView";
import { LogbuchView, type LogbuchRequest } from "./components/LogbuchView";
import { AufgabenView } from "./components/AufgabenView";
import { FragenView } from "./components/FragenView";
import { BerichtsheftView } from "./components/BerichtsheftView";
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
  // Logbuch bleibt nach dem ersten Besuch eingehängt: der Entwurf (nur im Speicher) überlebt
  // einen Abstecher in die Quellnotiz.
  const [logbuchMounted, setLogbuchMounted] = useState(false);
  const [logbuchRequest, setLogbuchRequest] = useState<LogbuchRequest | null>(null);
  const [logbuchOffen, setLogbuchOffen] = useState<number | null>(null);
  // Aufgaben und Fragen bleiben nach dem ersten Besuch eingehängt: Reiter und Filter überleben
  // einen Abstecher in die Quellnotiz.
  const [aufgabenMounted, setAufgabenMounted] = useState(false);
  const [fragenMounted, setFragenMounted] = useState(false);
  // Berichtsheft bleibt eingehängt: Entwürfe (Nachweis, Lernfelder, Betrieb.md) überleben „Im Editor öffnen“.
  const [berichtsheftMounted, setBerichtsheftMounted] = useState(false);
  const [berichtsheftOffen, setBerichtsheftOffen] = useState<number | null>(null);
  const [aufgabenUeberfaellig, setAufgabenUeberfaellig] = useState<number | null>(null);
  const [change, setChange] = useState<{ seq: number; data: VaultChanged } | null>(null);
  const [openRequest, setOpenRequest] = useState<{ path: string; line?: number; seq: number } | null>(null);
  const [createInitial, setCreateInitial] = useState<Partial<NeueUnterrichtsnotiz> | null>(null);
  const [editorDirty, setEditorDirty] = useState(false);
  const editorRef = useRef<EditorHandle | null>(null);
  const changeSeq = useRef(0);
  const openSeq = useRef(0);
  const logbuchSeq = useRef(0);
  const viewRef = useRef(view);
  viewRef.current = view;
  // bearbeiteter, noch nicht kopierter Logbuch-Entwurf (lebt nur im Speicher, Regel 1)
  const logbuchUngesichert = useRef(false);
  // ungespeicherte Logbuch-Einstellungen (SettingsView ist nur in „Einstellungen“ eingehängt)
  const einstellungenDirty = useRef(false);
  const verlassenFragt = useRef(false);

  /** Vor dem Verlassen der Einstellungen: ungespeicherte Logbuch-Einstellungen nicht still verwerfen. */
  const einstellungenVerlassenOk = useCallback(async (): Promise<boolean> => {
    if (viewRef.current !== "einstellungen" || !einstellungenDirty.current) return true;
    if (verlassenFragt.current) return false;
    verlassenFragt.current = true;
    try {
      const c = await askUnsaved("Logbuch je Fach", {
        canSave: false,
        text: "Die Logbuch-Einstellungen sind noch nicht gespeichert – beim Verlassen gehen die Änderungen verloren.",
      });
      if (c !== "discard") return false;
      einstellungenDirty.current = false;
      return true;
    } finally {
      verlassenFragt.current = false;
    }
  }, []);

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

  const go = useCallback(
    async (v: View) => {
      if (v !== viewRef.current && !(await einstellungenVerlassenOk())) return;
      if (v === "unterricht") setUnterrichtMounted(true);
      if (v === "logbuch") setLogbuchMounted(true);
      if (v === "aufgaben") setAufgabenMounted(true);
      if (v === "fragen") setFragenMounted(true);
      if (v === "berichtsheft") setBerichtsheftMounted(true);
      setView(v);
    },
    [einstellungenVerlassenOk],
  );

  const openLogbuch = useCallback(
    async (req: Omit<LogbuchRequest, "seq">) => {
      if (!(await einstellungenVerlassenOk())) return;
      logbuchSeq.current += 1;
      setLogbuchMounted(true);
      setView("logbuch");
      setLogbuchRequest({ ...req, seq: logbuchSeq.current });
    },
    [einstellungenVerlassenOk],
  );

  // Zahl offener Logbucheinträge für die Seitenleiste (Fehler → keine Anzeige)
  const zaehlSeq = useRef(0);
  const refreshLogbuchOffen = useCallback(async () => {
    zaehlSeq.current += 1;
    const mine = zaehlSeq.current;
    try {
      const u = await api.logbuchUebersicht();
      if (mine === zaehlSeq.current) setLogbuchOffen(u.faecher.length > 0 ? u.faecher.reduce((n, f) => n + f.offen, 0) : null);
    } catch {
      if (mine === zaehlSeq.current) setLogbuchOffen(null);
    }
  }, []);

  // Zahl überfälliger Arbeitsaufträge für die Seitenleiste (Fehler → keine Anzeige)
  const ueberfaelligSeq = useRef(0);
  const refreshAufgabenUeberfaellig = useCallback(async () => {
    ueberfaelligSeq.current += 1;
    const mine = ueberfaelligSeq.current;
    try {
      const u = await api.aufgabenUebersicht(null);
      if (mine === ueberfaelligSeq.current) setAufgabenUeberfaellig(u.ueberfaellig.length);
    } catch {
      if (mine === ueberfaelligSeq.current) setAufgabenUeberfaellig(null);
    }
  }, []);

  // Vorwochen mit Offenem für die Seitenleiste (Fehler → keine Anzeige)
  const berichtsheftSeq = useRef(0);
  const refreshBerichtsheftOffen = useCallback(async () => {
    berichtsheftSeq.current += 1;
    const mine = berichtsheftSeq.current;
    try {
      const u = await api.berichtsheftUebersicht(null);
      if (mine === berichtsheftSeq.current) setBerichtsheftOffen(u.offenVorwochen);
    } catch {
      if (mine === berichtsheftSeq.current) setBerichtsheftOffen(null);
    }
  }, []);

  const openNote = useCallback(
    async (path: string, line?: number) => {
      if (!(await einstellungenVerlassenOk())) return;
      openSeq.current += 1;
      setUnterrichtMounted(true);
      setView("unterricht");
      setOpenRequest({ path, line, seq: openSeq.current });
    },
    [einstellungenVerlassenOk],
  );

  const openCreate = useCallback((initial: Partial<NeueUnterrichtsnotiz>) => {
    setCreateInitial(initial);
  }, []);

  // Live-Aktualisierung aus dem Dateiwächter. Eigene Schreibvorgänge (`own`) lösen keinen Toast aus.
  useEffect(() => {
    const un = onVaultChanged((data) => {
      changeSeq.current += 1;
      setChange({ seq: changeSeq.current, data });
      void refreshStatus();
      void refreshLogbuchOffen();
      void refreshAufgabenUeberfaellig();
      void refreshBerichtsheftOffen();
      const n = data.changed.length + data.removed.length;
      if (data.fullRescan) notify({ kind: "live", text: "Vault neu eingelesen" });
      else if (n > 0) notify({ kind: "live", text: `${n} ${n === 1 ? "Änderung" : "Änderungen"} aus Obsidian übernommen` });
    });
    return () => {
      void un.then((f) => f());
    };
  }, [refreshStatus, refreshLogbuchOffen, refreshAufgabenUeberfaellig, refreshBerichtsheftOffen, notify]);

  // Bereichswechsel (z. B. nach den Einstellungen) → Zähler frisch
  useEffect(() => {
    if (!status?.vaultOk) return;
    void refreshLogbuchOffen();
    void refreshAufgabenUeberfaellig();
    void refreshBerichtsheftOffen();
  }, [view, status?.vaultOk, status?.vaultPath, refreshLogbuchOffen, refreshAufgabenUeberfaellig, refreshBerichtsheftOffen]);

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

  // Strg+L: in der Unterrichtsansicht mit geöffneter Notiz → Logbuch für diese Notiz, sonst nur die Übersicht
  const strgL = useCallback(() => {
    const ed = editorRef.current;
    const path = ed?.currentPath ?? null;
    if (viewRef.current === "unterricht" && ed && path) {
      if (ed.dirty) notify({ kind: "info", text: "Erst speichern – dann Logbuch erzeugen." });
      else void openLogbuch({ path });
      return;
    }
    void go("logbuch");
  }, [go, openLogbuch, notify]);

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
      if (mod && !e.altKey && !e.shiftKey && key === "l") {
        e.preventDefault();
        if (!isModalOpen()) strgL();
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
        } else if (logbuchUngesichert.current) {
          e.preventDefault();
          notify({ kind: "info", text: "Der Logbuch-Entwurf ginge verloren – erst kopieren." });
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [strgN, strgL, notify]);

  // Fenster schließen: einmal registrieren; liest editorRef.current zur Aufrufzeit.
  useEffect(() => {
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (editorRef.current?.dirty || logbuchUngesichert.current) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", onBeforeUnload);
    let asking = false;
    const un = getCurrentWindow().onCloseRequested(async (e) => {
      if (asking) {
        e.preventDefault(); // zweites Schließen, während noch gewartet oder gefragt wird
        return;
      }
      asking = true; // schon vor dem Warten auf das Speichern, sonst stapeln sich zwei Dialoge
      try {
        const ed = editorRef.current;
        if (ed) await ed.whenIdle(); // nie einen zweiten Speichervorgang auf der alten Basis starten
        if (!ed?.dirty && !logbuchUngesichert.current) return; // kein preventDefault → das Fenster schließt
        e.preventDefault();
        if (ed?.dirty) {
          const name = ed.currentPath ? baseName(ed.currentPath) : "Notiz";
          const c = await askUnsaved(name, { canSave: ed.canSave });
          if (c === "cancel") return;
          if (c === "save" && !(await ed.save())) return;
        }
        if (logbuchUngesichert.current) {
          const c = await askUnsaved("Logbuch-Entwurf", {
            canSave: false,
            text: "Der Logbuch-Entwurf ist bearbeitet und noch nicht kopiert – beim Schließen geht er verloren.",
          });
          if (c === "cancel") return;
        }
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

  // Pfad der Editor-Notiz mit ungespeicherten Änderungen (sperrt „Kopieren“ im Logbuch)
  const dirtyPath = editorDirty ? (editorRef.current?.currentPath ?? null) : null;

  if (error) return <div className="firstrun"><div className="error">{error}</div></div>;
  if (!status) return null;
  if (!status.vaultOk) return <FirstRun status={status} onDone={setStatus} />;

  return (
    <div className="shell">
      <Sidebar
        view={view}
        onView={go}
        status={status}
        editorDirty={editorDirty}
        logbuchOffen={logbuchOffen}
        aufgabenUeberfaellig={aufgabenUeberfaellig}
        berichtsheftOffen={berichtsheftOffen}
      />
      <main className="main">
        {view === "heute" && (
          <HeuteView
            change={change}
            onOpenNote={openNote}
            onCreate={openCreate}
            onOpenLogbuch={(fach) => void openLogbuch({ fach })}
            onOpenAufgaben={() => void go("aufgaben")}
            dirtyPath={dirtyPath}
          />
        )}
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
              onLogbuch={(path) => void openLogbuch({ path })}
            />
          </div>
        )}
        {logbuchMounted && (
          <div className="view-host" hidden={view !== "logbuch"}>
            <LogbuchView
              active={view === "logbuch"}
              vaultPath={status.vaultPath}
              change={change}
              request={logbuchRequest}
              onOpenNote={openNote}
              dirtyPath={dirtyPath}
              onGoSettings={() => void go("einstellungen")}
              onUngesichert={(b) => {
                logbuchUngesichert.current = b;
              }}
            />
          </div>
        )}
        {aufgabenMounted && (
          <div className="view-host" hidden={view !== "aufgaben"}>
            <AufgabenView active={view === "aufgaben"} change={change} onOpenNote={openNote} dirtyPath={dirtyPath} />
          </div>
        )}
        {fragenMounted && (
          <div className="view-host" hidden={view !== "fragen"}>
            <FragenView
              active={view === "fragen"}
              change={change}
              onOpenNote={openNote}
              dirtyPath={dirtyPath}
              onGoSettings={() => void go("einstellungen")}
            />
          </div>
        )}
        {berichtsheftMounted && (
          <div className="view-host" hidden={view !== "berichtsheft"}>
            <BerichtsheftView
              active={view === "berichtsheft"}
              change={change}
              onOpenNote={openNote}
              dirtyPath={dirtyPath}
              onGoSettings={() => void go("einstellungen")}
            />
          </div>
        )}
        {view === "notizen" && <NotesView change={change} />}
        {view === "einstellungen" && (
          <SettingsView
            status={status}
            onStatus={setStatus}
            editorDirty={editorDirty}
            onLogbuchDirty={(b) => {
              einstellungenDirty.current = b;
            }}
          />
        )}
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
