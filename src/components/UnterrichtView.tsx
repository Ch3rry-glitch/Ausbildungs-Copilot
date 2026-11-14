// Ansicht „Unterricht“: Liste der Unterrichtsnotizen + Editor. Bleibt nach dem ersten Besuch
// eingehängt (App blendet sie nur aus), damit der Puffer beim Bereichswechsel nie verloren geht.
import { useCallback, useEffect, useMemo, useRef, useState, type RefObject } from "react";
import {
  api,
  baseName,
  errorText,
  formatDatum,
  isNoteNotFound,
  samePath,
  type FachInfo,
  type LineEnding,
  type NeueUnterrichtsnotiz,
  type NoteContent,
  type NoteSummary,
  type NoteVersion,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";
import { useToast } from "../toast";
import { askUnsaved } from "./Modal";
import { ConflictDialog } from "./ConflictDialog";
import { MarkdownPreview } from "./MarkdownPreview";
import { NoteEditor, type NoteEditorHandle } from "./NoteEditor";

/** Was die App (Tastenkürzel, Fenster schließen) vom Editor braucht. */
export type EditorHandle = {
  dirty: boolean;
  canSave: boolean;
  currentPath: string | null;
  save(): Promise<boolean>;
  whenIdle(): Promise<void>;
};

interface Loaded {
  path: string;
  title: string;
  base: { text: string; version: NoteVersion };
  editable: boolean;
  readonlyReason: string | null;
  lineEnding: LineEnding;
  bom: boolean;
}

type External = null | "changed" | "removed";

interface Conflict {
  mine: string;
  diskText: string;
  diskVersion: NoteVersion;
  error: string | null;
}

const MOVE_WINDOW_MS = 3000;

function fromContent(c: NoteContent): Loaded {
  return {
    path: c.meta.path,
    title: c.meta.title,
    base: { text: c.text, version: c.version },
    editable: c.editable,
    readonlyReason: c.readonlyReason,
    lineEnding: c.lineEnding,
    bom: c.bom,
  };
}

/** Zustand, der zugleich synchron (Ref, für asynchrone Abläufe) und für das Rendern (State) gilt. */
function useRefState<T>(init: T): [T, { readonly current: T }, (v: T) => void] {
  const [value, setValue] = useState<T>(init);
  const ref = useRef<T>(init);
  const set = useCallback((v: T) => {
    ref.current = v;
    setValue(v);
  }, []);
  return [value, ref, set];
}

export function UnterrichtView({
  active,
  vaultPath,
  change,
  openRequest,
  editorRef,
  onDirtyChange,
  onCreate,
}: {
  active: boolean;
  vaultPath: string | null;
  change: { seq: number; data: VaultChanged } | null;
  openRequest: { path: string; line?: number; seq: number } | null;
  editorRef: RefObject<EditorHandle | null>;
  onDirtyChange: (dirty: boolean) => void;
  onCreate: (initial: Partial<NeueUnterrichtsnotiz>) => void;
}) {
  const { notify } = useToast();
  const ed = useRef<NoteEditorHandle>(null);

  // ---------- Liste ----------
  const [list, setList] = useState<NoteSummary[]>([]);
  const [faecher, setFaecher] = useState<FachInfo[]>([]);
  const [fachFilter, setFachFilter] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const listSeq = useRef(0);

  // ---------- Editor ----------
  const [loaded, loadedRef, setLoaded] = useRefState<Loaded | null>(null);
  const [dirty, dirtyRef, setDirty] = useRefState(false);
  const [external, externalRef, setExternal] = useRefState<External>(null);
  const [moved, movedRef, setMoved] = useRefState<string | null>(null);
  const [saving, savingRef, setSaving] = useRefState(false);
  const [slow, setSlow] = useState(false);
  const [mode, modeRef, setMode] = useRefState<"edit" | "preview">("edit");
  const [conflict, setConflict] = useState<Conflict | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [previewTick, setPreviewTick] = useState(0);
  const pendingSave = useRef<Promise<boolean> | null>(null);
  const recheckAfterSave = useRef(false);
  const removedInfo = useRef<{ path: string; at: number; sha: string } | null>(null);

  const readonly = !loaded || !loaded.editable || external === "removed";
  const canSaveNow = () => {
    const l = loadedRef.current;
    return !!l && l.editable && externalRef.current !== "removed";
  };
  const bumpPreview = () => setPreviewTick((t) => t + 1);

  function recomputeDirty() {
    const l = loadedRef.current;
    const e = ed.current;
    const d = !!l && !!e && e.getText() !== l.base.text;
    if (d !== dirtyRef.current) setDirty(d);
  }

  function onEditorChange() {
    recomputeDirty();
    if (modeRef.current === "preview") bumpPreview();
  }

  // ---------- Liste laden ----------
  async function refreshList() {
    listSeq.current += 1;
    const seq = listSeq.current;
    try {
      const l = await api.listUnterricht(fachFilter);
      if (seq !== listSeq.current) return;
      setList(l);
      setListError(null);
    } catch (e) {
      if (seq === listSeq.current) setListError(errorText(e));
    }
  }

  async function refreshFaecher() {
    try {
      setFaecher(await api.listFaecher());
    } catch {
      // Filter-Pillen sind Komfort; Fehler zeigt die Liste
    }
  }

  // ---------- Speichern ----------
  function withSaving(fn: () => Promise<boolean>): Promise<boolean> {
    setSaving(true);
    const slowTimer = setTimeout(() => setSlow(true), 1000);
    const p = fn()
      .catch((e) => {
        notify({ kind: "error", text: errorText(e) });
        return false;
      })
      .finally(() => {
        clearTimeout(slowTimer);
        setSlow(false);
        setSaving(false);
        pendingSave.current = null;
        if (recheckAfterSave.current) {
          recheckAfterSave.current = false;
          void checkDisk();
        }
      });
    pendingSave.current = p;
    return p;
  }

  function save(): Promise<boolean> {
    if (pendingSave.current) return pendingSave.current; // nie ein zweiter Speichervorgang
    const l = loadedRef.current;
    const e = ed.current;
    if (!l || !e || !canSaveNow()) return Promise.resolve(false);
    if (!dirtyRef.current) return Promise.resolve(true);
    const text = e.getText();
    const path = l.path;
    const baseVersion = l.base.version;
    return withSaving(async () => {
      let r;
      try {
        r = await api.saveNote(path, text, baseVersion);
      } catch (err) {
        const msg = errorText(err);
        setSaveError(msg);
        notify({ kind: "error", text: msg });
        return false;
      }
      const cur = loadedRef.current;
      if (!cur || !samePath(cur.path, path)) return false;
      switch (r.status) {
        case "saved":
          setLoaded({ ...cur, path: r.report.relPath, base: { text, version: r.report.version } });
          clearExternal();
          setSaveError(null);
          recomputeDirty(); // während des Speicherns kann weitergetippt worden sein
          notify({ kind: "saved", text: "Gespeichert: " + r.report.relPath, path: r.report.relPath, detail: r.report.warnungen });
          return true;
        case "unchanged":
          setLoaded({ ...cur, base: { text, version: r.version } });
          clearExternal();
          setSaveError(null);
          recomputeDirty();
          notify({ kind: "info", text: "Keine Änderung – Datei unverändert." });
          return true;
        case "conflict":
          setSaveError(null);
          setConflict({ mine: text, diskText: r.diskText, diskVersion: r.diskVersion, error: null });
          return false;
        case "missing":
          setExternal("removed");
          notify({ kind: "error", text: `Datei nicht mehr vorhanden: ${r.relPath}` });
          return false;
      }
    });
  }

  async function whenIdle(): Promise<void> {
    while (pendingSave.current) await pendingSave.current;
  }

  function clearExternal() {
    setExternal(null);
    setMoved(null);
    removedInfo.current = null;
  }

  // ---------- Konfliktdialog ----------
  async function saveMerged(merged: string) {
    const c = conflict;
    const l = loadedRef.current;
    if (!c || !l) return;
    await withSaving(async () => {
      let r;
      try {
        r = await api.saveNote(l.path, merged, c.diskVersion);
      } catch (err) {
        const msg = errorText(err);
        setConflict({ ...c, mine: merged, error: msg });
        notify({ kind: "error", text: msg });
        return false;
      }
      const cur = loadedRef.current;
      const e = ed.current;
      if (!cur || !e) return false;
      switch (r.status) {
        case "saved":
        case "unchanged": {
          const version = r.status === "saved" ? r.report.version : r.version;
          const path = r.status === "saved" ? r.report.relPath : cur.path;
          // Der Haupteditor hält danach genau die Fassung auf der Platte (frischer Verlauf).
          setLoaded({ ...cur, path, base: { text: merged, version } });
          e.load(merged, { readonly: !cur.editable });
          setDirty(false);
          clearExternal();
          setSaveError(null);
          setConflict(null);
          bumpPreview();
          if (r.status === "saved") {
            notify({ kind: "saved", text: "Gespeichert: " + path, path, detail: r.report.warnungen });
          } else {
            notify({ kind: "info", text: "Keine Änderung – Datei unverändert." });
          }
          return true;
        }
        case "conflict":
          setConflict({ mine: merged, diskText: r.diskText, diskVersion: r.diskVersion, error: null });
          return false;
        case "missing":
          // Zusammengeführten Text behalten, aber nicht speichern.
          e.load(merged, { readonly: true });
          setConflict(null);
          setExternal("removed");
          recomputeDirty();
          bumpPreview();
          notify({ kind: "error", text: `Datei nicht mehr vorhanden: ${r.relPath}` });
          return false;
      }
    });
  }

  function takeDisk() {
    const c = conflict;
    const l = loadedRef.current;
    const e = ed.current;
    if (!c || !l || !e) return;
    setLoaded({ ...l, base: { text: c.diskText, version: c.diskVersion } });
    e.load(c.diskText, { readonly: !l.editable });
    setDirty(false);
    clearExternal();
    setConflict(null);
    bumpPreview();
  }

  // ---------- Änderungen von außen ----------
  /** Ablauf „Datei geändert“: ausgewertet, wenn readNote zurückkommt, nicht beim Ereignis. */
  async function checkDisk() {
    const l0 = loadedRef.current;
    if (!l0) return;
    const path = l0.path;
    if (savingRef.current) {
      recheckAfterSave.current = true;
      return;
    }
    let disk: NoteContent;
    try {
      disk = await api.readNote(path);
    } catch (e) {
      if (!samePath(loadedRef.current?.path, path)) return;
      if (isNoteNotFound(e)) markRemoved(path, []);
      else notify({ kind: "error", text: errorText(e) });
      return;
    }
    if (savingRef.current) {
      recheckAfterSave.current = true;
      return;
    }
    const l = loadedRef.current;
    const e = ed.current;
    if (!l || !e || !samePath(l.path, path)) return;
    if (externalRef.current === "removed") clearExternal(); // wieder da

    if (disk.version.sha256 === l.base.version.sha256) {
      setLoaded({ ...l, base: { ...l.base, version: disk.version } });
      if (externalRef.current === "changed") setExternal(null);
      return;
    }
    if (disk.text === e.getText()) {
      setLoaded({ ...l, base: { text: disk.text, version: disk.version } });
      setExternal(null);
      recomputeDirty();
      return;
    }
    if (!dirtyRef.current) {
      // still nachladen, nicht im Rückgängig-Verlauf
      setLoaded({ ...fromContent(disk), title: l.title });
      e.replaceDoc(disk.text);
      setExternal(null);
      recomputeDirty();
      bumpPreview();
      return;
    }
    setExternal("changed");
  }

  function markRemoved(path: string, changedInSameEvent: string[]) {
    const l = loadedRef.current;
    if (!l) return;
    setExternal("removed");
    setMoved(null);
    removedInfo.current = { path, at: Date.now(), sha: l.base.version.sha256 };
    void findMove(changedInSameEvent);
  }

  /** „Der Notiz folgen“: gleicher Dateiname oder gleicher Inhalt (Hash), im selben oder nächsten Ereignis (3 s). */
  async function findMove(candidates: string[]) {
    const r = removedInfo.current;
    if (!r || Date.now() - r.at > MOVE_WINDOW_MS) return;
    const others = candidates.filter((p) => !samePath(p, r.path) && /\.md$/i.test(p));
    if (others.length === 0) return;
    const still = () => removedInfo.current === r && externalRef.current === "removed" && !movedRef.current;
    const name = baseName(r.path).toLowerCase();
    const byName = others.find((p) => baseName(p).toLowerCase() === name);
    if (byName) {
      if (still()) setMoved(byName);
      return;
    }
    for (const p of others.slice(0, 20)) {
      try {
        const d = await api.readNote(p);
        if (d.version.sha256 === r.sha) {
          if (still()) setMoved(d.meta.path);
          return;
        }
      } catch {
        // nicht lesbar → kein Kandidat
      }
    }
  }

  async function follow() {
    const target = movedRef.current;
    const l = loadedRef.current;
    if (!target || !l) return;
    let disk: NoteContent | null = null;
    try {
      disk = await api.readNote(target);
    } catch {
      disk = null;
    }
    const cur = loadedRef.current;
    if (!cur || !samePath(cur.path, l.path)) return;
    const sameContent = !!disk && disk.version.sha256 === cur.base.version.sha256;
    setLoaded({
      ...cur,
      path: disk?.meta.path ?? target,
      title: disk?.meta.title ?? baseName(target),
      // gleicher Inhalt → neue Version übernehmen; sonst bleibt die Basis, das nächste Speichern geht in den Konfliktablauf
      base: sameContent && disk ? { ...cur.base, version: disk.version } : cur.base,
    });
    clearExternal();
  }

  function onVaultEvent(data: VaultChanged) {
    void refreshList();
    if (data.fullRescan) void refreshFaecher();
    const l = loadedRef.current;
    if (!l) return;
    const path = l.path;
    const own = data.own.some((p) => samePath(p, path));
    const inChanged = data.changed.some((p) => samePath(p, path));
    const inRemoved = data.removed.some((p) => samePath(p, path));
    if (inChanged) {
      if (externalRef.current === "removed") clearExternal(); // Rückbenennung oder Sync-Client
      void checkDisk();
      return;
    }
    if (inRemoved) {
      if (savingRef.current) recheckAfterSave.current = true;
      markRemoved(path, data.changed);
      return;
    }
    if (externalRef.current === "removed" && !movedRef.current) void findMove(data.changed);
    if (data.fullRescan && !own) void checkDisk();
  }

  // ---------- Öffnen ----------
  async function loadNote(path: string, line?: number) {
    let c: NoteContent;
    try {
      c = await api.readNote(path);
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
      return;
    }
    const e = ed.current;
    if (!e) return;
    setLoaded(fromContent(c));
    e.load(c.text, { readonly: !c.editable });
    setDirty(false);
    clearExternal();
    setConflict(null);
    setSaveError(null);
    setMode("edit");
    bumpPreview();
    if (line) revealSoon(line);
  }

  function revealSoon(line: number) {
    // erst nach dem Einblenden messen, sonst scrollt CodeMirror ins Leere
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        ed.current?.requestMeasure();
        ed.current?.revealLine(line);
      }),
    );
  }

  async function handleOpen(path: string, line?: number) {
    await whenIdle();
    const l = loadedRef.current;
    if (l && samePath(l.path, path)) {
      // gleiche Notiz: nur umschalten, nie neu laden (ungespeicherter Puffer bleibt)
      if (modeRef.current !== "edit" && line) setMode("edit");
      if (line) revealSoon(line);
      return;
    }
    if (l && dirtyRef.current) {
      const choice = await askUnsaved(l.title, { canSave: canSaveNow() });
      if (choice === "cancel") return;
      if (choice === "save" && !(await save())) return;
    }
    await loadNote(path, line);
  }

  async function loadDiskVersion() {
    const l = loadedRef.current;
    if (!l) return;
    const choice = await askUnsaved(l.title, {
      canSave: false,
      text: `Deine Änderungen an „${l.title}“ in der App gehen verloren, wenn du die Obsidian-Fassung lädst.`,
    });
    if (choice !== "discard") return;
    const path = l.path;
    let disk: NoteContent;
    try {
      disk = await api.readNote(path);
    } catch (e) {
      if (isNoteNotFound(e)) markRemoved(path, []);
      else notify({ kind: "error", text: errorText(e) });
      return;
    }
    const e = ed.current;
    if (!e || !samePath(loadedRef.current?.path, path)) return;
    setLoaded(fromContent(disk));
    e.load(disk.text, { readonly: !disk.editable });
    setDirty(false);
    clearExternal();
    bumpPreview();
  }

  async function compare() {
    const l = loadedRef.current;
    const e = ed.current;
    if (!l || !e) return;
    try {
      const disk = await api.readNote(l.path);
      setConflict({ mine: e.getText(), diskText: disk.text, diskVersion: disk.version, error: null });
    } catch (err) {
      if (isNoteNotFound(err)) markRemoved(l.path, []);
      else notify({ kind: "error", text: errorText(err) });
    }
  }

  // Aktuelle Funktionen für Effekte und den App-Griff (keine veralteten Closures)
  const actions = useRef({ save, whenIdle, handleOpen, onVaultEvent, refreshList, refreshFaecher });
  actions.current = { save, whenIdle, handleOpen, onVaultEvent, refreshList, refreshFaecher };

  // ---------- Effekte ----------
  useEffect(() => {
    editorRef.current = {
      get dirty() {
        return dirtyRef.current;
      },
      get canSave() {
        const l = loadedRef.current;
        return !!l && l.editable && externalRef.current !== "removed";
      },
      get currentPath() {
        return loadedRef.current?.path ?? null;
      },
      save: () => actions.current.save(),
      whenIdle: () => actions.current.whenIdle(),
    };
    return () => {
      editorRef.current = null;
    };
  }, [editorRef, dirtyRef, loadedRef, externalRef]);

  useEffect(() => {
    onDirtyChange(dirty);
  }, [dirty, onDirtyChange]);

  useEffect(() => {
    ed.current?.setReadonly(readonly);
  }, [readonly, loaded?.path]);

  useEffect(() => {
    void actions.current.refreshList();
  }, [fachFilter]);

  useEffect(() => {
    void actions.current.refreshFaecher();
  }, []);

  // Öffnen-Anfragen der App (StrictMode-sicher: jede seq nur einmal)
  const handledOpen = useRef(0);
  useEffect(() => {
    if (!openRequest || openRequest.seq <= handledOpen.current) return;
    handledOpen.current = openRequest.seq;
    void actions.current.handleOpen(openRequest.path, openRequest.line);
  }, [openRequest]);

  // Vault-Ereignisse (jede seq nur einmal; Ereignisse vor dem Einhängen zählen nicht)
  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    actions.current.onVaultEvent(change.data);
  }, [change]);

  // Vault gewechselt → Notiz ausladen
  const prevVault = useRef(vaultPath);
  useEffect(() => {
    if (prevVault.current === vaultPath) return;
    prevVault.current = vaultPath;
    setLoaded(null);
    ed.current?.load("", { readonly: true });
    setDirty(false);
    clearExternal();
    setConflict(null);
    setSaveError(null);
    setFachFilter(null);
    void actions.current.refreshList();
    void actions.current.refreshFaecher();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [vaultPath]);

  // Wieder sichtbar → CodeMirror neu vermessen
  useEffect(() => {
    if (!active) return;
    const id = requestAnimationFrame(() => ed.current?.requestMeasure());
    return () => cancelAnimationFrame(id);
  }, [active]);

  const previewText = useMemo(
    () => (mode === "preview" ? (ed.current?.getText() ?? "") : ""),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [mode, previewTick],
  );

  function switchMode(m: "edit" | "preview") {
    setMode(m);
    if (m === "preview") bumpPreview();
    else
      requestAnimationFrame(() => {
        ed.current?.requestMeasure();
        ed.current?.focus();
      });
  }

  const canSave = !readonly;

  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Unterricht</h1>
          <div className="page-sub">Unterrichtsnotizen, neueste zuerst. Speichern mit Strg+S, neue Notiz mit Strg+N.</div>
        </div>
        <button className="btn primary" onClick={() => onCreate(fachFilter ? { fach: fachFilter } : {})}>
          {Icon.plus} Neue Notiz
        </button>
      </div>

      {faecher.length > 0 && (
        <div className="pills fach-pills">
          <button className={`pill ${fachFilter === null ? "active" : ""}`} onClick={() => setFachFilter(null)}>
            Alle
          </button>
          {faecher.map((f) => (
            <button
              key={f.kurz}
              className={`pill ${fachFilter === f.kurz ? "active" : ""}`}
              onClick={() => setFachFilter(f.kurz)}
              title={f.ziel}
            >
              {f.kurz}
            </button>
          ))}
        </div>
      )}

      <div className="unterricht-grid">
        <section className="panel glass">
          <div className="panel-head">
            <span className="panel-title">{fachFilter ?? "Alle Fächer"}</span>
            <span className="badge muted">{list.length}</span>
          </div>
          <div className="panel-body">
            {listError && <div className="error">{listError}</div>}
            {!listError && list.length === 0 && <div className="empty">Noch keine Unterrichtsnotizen.</div>}
            {list.map((n) => {
              const isOpen = samePath(n.path, loaded?.path);
              return (
                <button
                  key={n.path}
                  className={`note-item ${isOpen ? "active" : ""}`}
                  onClick={() => void actions.current.handleOpen(n.path)}
                  title={n.path}
                >
                  <div className="note-title">
                    {isOpen && dirty && <span className="dirty-dot" title="ungespeichert" />}
                    {n.title}
                  </div>
                  <div className="note-meta">
                    {n.fach && <span className="badge purple">{n.fach}</span>}
                    {n.datum && <span>{formatDatum(n.datum)}</span>}
                    {n.thema && <span className="note-thema">{n.thema}</span>}
                    {n.openTasks > 0 && <span className="badge red">{n.openTasks} offen</span>}
                  </div>
                </button>
              );
            })}
          </div>
        </section>

        <section className="panel glass editor-panel">
          {!loaded && (
            <div className="empty">
              <div>
                <div className="empty-icon">{Icon.datei}</div>
                Wähle links eine Notiz aus – oder lege mit Strg+N eine neue an.
              </div>
            </div>
          )}
          <div className="editor-wrap" hidden={!loaded}>
            {loaded && (
              <div className="editor-head">
                <div className="editor-title-row">
                  <h2 className="preview-title">{loaded.title}</h2>
                  {dirty && <span className="badge amber">● ungespeichert</span>}
                </div>
                <div className="preview-row">
                  <code className="mono-path" title={loaded.path}>
                    {loaded.path}
                  </code>
                  {loaded.lineEnding === "crlf" && <span className="badge muted">CRLF</span>}
                  {loaded.lineEnding === "mixed" && <span className="badge amber">gemischte Zeilenenden</span>}
                  {loaded.bom && <span className="badge muted">BOM</span>}
                  <span style={{ flex: 1 }} />
                  <div className="pills">
                    <button className={`pill ${mode === "edit" ? "active" : ""}`} onClick={() => switchMode("edit")}>
                      {Icon.edit} Bearbeiten
                    </button>
                    <button className={`pill ${mode === "preview" ? "active" : ""}`} onClick={() => switchMode("preview")}>
                      {Icon.eye} Vorschau
                    </button>
                  </div>
                  <button
                    className="btn small primary"
                    onClick={() => void save()}
                    disabled={!dirty || !canSave || saving}
                    title="Strg+S"
                  >
                    {Icon.save} Speichern
                  </button>
                  <button
                    className="btn small"
                    onClick={() => api.openInObsidian(loaded.path).catch((e) => notify({ kind: "error", text: errorText(e) }))}
                  >
                    {Icon.obsidian} In Obsidian öffnen
                  </button>
                </div>
                {saving && (
                  <div className="save-status">
                    {Icon.clock}
                    {slow ? "Speichert … – die Datei ist evtl. gesperrt, neuer Versuch läuft (bis zu 15 s)." : "Speichert …"}
                  </div>
                )}
              </div>
            )}

            {loaded && !loaded.editable && (
              <div className="banner">
                {Icon.warning}
                <span>{loaded.readonlyReason ?? "Nur Anzeige."}</span>
              </div>
            )}
            {external === "removed" && (
              <div className="banner red">
                {Icon.warning}
                <span>Die Datei wurde in Obsidian verschoben oder gelöscht. Dein Text bleibt hier, wird aber nicht gespeichert.</span>
                {moved && (
                  <button className="btn small" onClick={() => void follow()} title={moved}>
                    Der Notiz folgen
                  </button>
                )}
              </div>
            )}
            {external === "changed" && (
              <div className="banner">
                {Icon.warning}
                <span>Diese Notiz wurde außerhalb der App geändert (z. B. in Obsidian).</span>
                <button className="btn small" onClick={() => void compare()}>
                  {Icon.diff} Vergleichen
                </button>
                <button className="btn small" onClick={() => void loadDiskVersion()}>
                  Obsidian-Fassung laden
                </button>
                <button className="btn small" onClick={() => setExternal(null)}>
                  Meine behalten
                </button>
              </div>
            )}
            {saveError && (
              <div className="banner red">
                {Icon.warning}
                <span>{saveError}</span>
                <button className="btn small" onClick={() => void save()} disabled={saving || !canSave}>
                  {Icon.save} Erneut speichern
                </button>
              </div>
            )}

            <div className="editor-body">
              <div className="editor-host" hidden={mode !== "edit"}>
                <NoteEditor ref={ed} onSave={() => void save()} onChange={onEditorChange} />
              </div>
              {mode === "preview" && <MarkdownPreview text={previewText} />}
            </div>
          </div>
        </section>
      </div>

      {conflict && loaded && (
        <ConflictDialog
          name={loaded.title}
          mine={conflict.mine}
          diskText={conflict.diskText}
          error={conflict.error}
          onSaveMerged={saveMerged}
          onTakeDisk={takeDisk}
          onCancel={() => setConflict(null)}
        />
      )}
    </div>
  );
}
