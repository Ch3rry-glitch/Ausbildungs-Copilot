import { useCallback, useEffect, useMemo, useState } from "react";
import {
  api,
  errorText,
  formatDatum,
  KIND_LABEL,
  type FolderInfo,
  type NoteContent,
  type NoteSummary,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";

const START_FOLDER = "03 Unterricht";

/** M0: Ordner → Notizliste → Vorschau, rein lesend, live aktualisiert. */
export function NotesView({ change }: { change: { seq: number; data: VaultChanged } | null }) {
  const [folders, setFolders] = useState<FolderInfo[]>([]);
  const [folder, setFolder] = useState<string | null>(null);
  const [recursive, setRecursive] = useState(false);
  const [notes, setNotes] = useState<NoteSummary[]>([]);
  const [all, setAll] = useState<NoteSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [content, setContent] = useState<NoteContent | null>(null);
  const [flash, setFlash] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const loadOverview = useCallback(async () => {
    try {
      const [f, a] = await Promise.all([api.listFolders(), api.listNotes("", true)]);
      setFolders(f);
      setAll(a);
      setFolder((cur) => cur ?? (f.some((x) => x.path === START_FOLDER) ? START_FOLDER : ""));
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  const loadNotes = useCallback(async () => {
    if (folder === null) return;
    try {
      setNotes(await api.listNotes(folder, recursive));
    } catch (e) {
      setError(errorText(e));
    }
  }, [folder, recursive]);

  const loadContent = useCallback(async (path: string | null) => {
    if (!path) return setContent(null);
    try {
      setContent(await api.readNote(path));
      setError(null);
    } catch (e) {
      setContent(null);
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    loadOverview();
  }, [loadOverview]);
  useEffect(() => {
    loadNotes();
  }, [loadNotes]);
  useEffect(() => {
    loadContent(selected);
  }, [selected, loadContent]);

  // Änderungen aus Obsidian
  useEffect(() => {
    if (!change) return;
    const { changed, removed, fullRescan } = change.data;
    loadOverview();
    loadNotes();
    if (selected && removed.includes(selected)) setSelected(null);
    else if (selected && (fullRescan || changed.includes(selected))) loadContent(selected);
    setFlash(new Set(changed));
    const t = setTimeout(() => setFlash(new Set()), 1700);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [change?.seq]);

  const stats = useMemo(() => {
    const unterricht = all.filter((n) => n.kind === "unterricht").length;
    const offen = all.reduce((s, n) => s + n.openTasks, 0);
    return { total: all.length, unterricht, offen, ordner: Math.max(folders.length - 1, 0) };
  }, [all, folders]);

  const fields = content?.meta.fields ?? [];

  return (
    <>
      <div className="page-head">
        <div>
          <h1 className="page-title">Notizen</h1>
          <div className="page-sub">Dein Vault, nur lesend. Änderungen aus Obsidian erscheinen automatisch.</div>
        </div>
        <div className="pills">
          <button className={`pill ${!recursive ? "active" : ""}`} onClick={() => setRecursive(false)}>Nur Ordner</button>
          <button className={`pill ${recursive ? "active" : ""}`} onClick={() => setRecursive(true)}>Mit Unterordnern</button>
        </div>
      </div>

      <div className="stats">
        <Stat label="Notizen" value={stats.total} />
        <Stat label="Unterrichtsnotizen" value={stats.unterricht} />
        <Stat label="Offene Aufgaben" value={stats.offen} badge={stats.offen > 0 ? "red" : "mint"} />
        <Stat label="Ordner" value={stats.ordner} />
      </div>

      <div className="browser">
        <section className="panel glass">
          <div className="panel-head">
            <span className="panel-title">Ordner</span>
          </div>
          <div className="panel-body">
            {folders.map((f) => (
              <button
                key={f.path}
                className={`folder ${folder === f.path ? "active" : ""}`}
                style={{ paddingLeft: 10 + f.depth * 14 }}
                onClick={() => setFolder(f.path)}
                title={f.path || "Vault-Wurzel"}
              >
                {Icon.ordner}
                <span className="name">{f.name}</span>
                <span className="count">{recursive ? f.totalCount : f.noteCount}</span>
              </button>
            ))}
          </div>
        </section>

        <section className="panel glass">
          <div className="panel-head">
            <span className="panel-title">{folder ? folder.split("/").pop() : "Vault"}</span>
            <span className="badge muted">{notes.length}</span>
          </div>
          <div className="panel-body">
            {notes.length === 0 && <div className="empty">Keine Notizen in diesem Ordner.</div>}
            {notes.map((n) => (
              <button
                key={n.path}
                className={`note-item ${selected === n.path ? "active" : ""} ${flash.has(n.path) ? "flash" : ""}`}
                onClick={() => setSelected(n.path)}
              >
                <div className="note-title">{n.title}</div>
                <div className="note-meta">
                  <span className="badge purple">{KIND_LABEL[n.kind]}</span>
                  {n.fach && <span className="badge muted">{n.fach}</span>}
                  {n.datum && <span>{formatDatum(n.datum)}</span>}
                  {n.openTasks > 0 && <span className="badge red">{n.openTasks} offen</span>}
                  {n.tags.includes("frage") && <span className="badge mint">#frage</span>}
                </div>
              </button>
            ))}
          </div>
        </section>

        <section className="panel glass preview">
          {!content ? (
            <div className="empty">
              <div>
                <div className="empty-icon">{Icon.datei}</div>
                {error ?? "Wähle eine Notiz aus."}
              </div>
            </div>
          ) : (
            <>
              <div className="preview-head">
                <h2 className="preview-title">{content.meta.title}</h2>
                <div className="preview-row">
                  <span className="badge purple">{KIND_LABEL[content.meta.kind]}</span>
                  {content.meta.fach && <span className="badge muted">{content.meta.fach}</span>}
                  {content.meta.datum && <span className="badge muted">{formatDatum(content.meta.datum)}</span>}
                  {content.meta.private && <span className="badge red">privat · kein KI-Versand</span>}
                  {content.meta.tasks.length > 0 && (
                    <span className="badge mint">
                      {content.meta.tasks.filter((t) => t.done).length}/{content.meta.tasks.length} Aufgaben erledigt
                    </span>
                  )}
                  <span style={{ flex: 1 }} />
                  <button className="btn small primary" onClick={() => api.openInObsidian(content.meta.path).catch((e) => setError(errorText(e)))}>
                    {Icon.obsidian} In Obsidian öffnen
                  </button>
                </div>
              </div>
              <div className="preview-body">
                {content.meta.frontmatterError && (
                  <div className="warn">Frontmatter fehlerhaft – Obsidian zeigt sie evtl. nicht an: {content.meta.frontmatterError}</div>
                )}
                {fields.length > 0 && (
                  <dl className="fields">
                    {fields.map((f) => (
                      <FieldRow key={f.key} k={f.key} v={f.value} />
                    ))}
                  </dl>
                )}
                <pre className="markdown">{content.body.trim()}</pre>
              </div>
            </>
          )}
        </section>
      </div>
    </>
  );
}

function FieldRow({ k, v }: { k: string; v: unknown }) {
  const text = Array.isArray(v) ? v.join(", ") : v === null ? "—" : typeof v === "object" ? JSON.stringify(v) : String(v);
  return (
    <>
      <dt>{k}</dt>
      <dd>{text}</dd>
    </>
  );
}

function Stat({ label, value, badge }: { label: string; value: number; badge?: "red" | "mint" }) {
  return (
    <div className="stat glass">
      <div className="stat-label">
        {label} {badge && <span className={`badge ${badge}`} style={{ marginLeft: 6 }}>{badge === "red" ? "offen" : "alles erledigt"}</span>}
      </div>
      <div className="stat-value">{value}</div>
    </div>
  );
}
