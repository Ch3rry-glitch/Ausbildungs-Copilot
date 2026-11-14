// Startansicht „Heute“: Tageswechsler, Stundenplan-Blöcke, Betriebstag/Wochenende, fällige Aufgaben.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  formatDatum,
  stdText,
  type FaelligeAufgabe,
  type HeuteBlock,
  type HeuteTag,
  type NeueUnterrichtsnotiz,
  type NoteSummary,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";

/** ISO-Datum ± Tage, in UTC gerechnet (keine Zeitzonen-Sprünge). */
function shiftIso(iso: string, days: number): string {
  const d = new Date(iso + "T00:00:00Z");
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

export function HeuteView({
  change,
  onOpenNote,
  onCreate,
}: {
  change: { seq: number; data: VaultChanged } | null;
  onOpenNote: (path: string, line?: number) => void;
  onCreate: (initial: Partial<NeueUnterrichtsnotiz>) => void;
}) {
  // null = heute (das Datum liefert das Backend, nie new Date() in UTC)
  const [date, setDate] = useState<string | null>(null);
  const [tag, setTag] = useState<HeuteTag | null>(null);
  const [error, setError] = useState<string | null>(null);
  const seq = useRef(0);

  const load = useCallback(async () => {
    seq.current += 1;
    const mine = seq.current;
    try {
      const t = await api.heute(date);
      if (mine !== seq.current) return;
      setTag(t);
      setError(null);
    } catch (e) {
      if (mine !== seq.current) return;
      setError(errorText(e));
    }
  }, [date]);

  useEffect(() => {
    void load();
  }, [load]);

  // Jede Vault-Änderung (auch eigene Schreibvorgänge) → neu laden
  useEffect(() => {
    if (change) void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [change?.seq]);

  // Aktuell bleiben: „jetzt“-Badge und Tageswechsel nach Mitternacht
  const showsToday = date === null || !!tag?.istHeute;
  useEffect(() => {
    if (!showsToday) return;
    const t = setInterval(() => void load(), 60_000);
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => {
      clearInterval(t);
      window.removeEventListener("focus", onFocus);
    };
  }, [showsToday, load]);

  const step = (days: number) => {
    if (tag) setDate(shiftIso(tag.datum, days));
  };

  const a = tag?.aufgaben;
  const aufgabenLeer = !a || a.ueberfaellig.length + a.heute.length + a.naechsteTage.length === 0;

  return (
    <>
      <div className="page-head">
        <div>
          <h1 className="page-title">{tag ? `${tag.wochentag}, ${formatDatum(tag.datum)}` : "Heute"}</h1>
          <div className="page-sub">
            {tag?.istHeute ? "Heute" : tag ? "Anderer Tag" : "Lade …"}
            {tag && tag.art === "schultag" && ` · ${tag.bloecke.length} ${tag.bloecke.length === 1 ? "Block" : "Blöcke"}`}
          </div>
        </div>
        <div className="day-switch">
          <button className="btn small icon-only" onClick={() => step(-1)} disabled={!tag} title="Vortag" aria-label="Vortag">
            {Icon.chevronLeft}
          </button>
          <button className="btn small" onClick={() => setDate(null)} disabled={!tag || tag.istHeute}>
            Heute
          </button>
          <button className="btn small icon-only" onClick={() => step(1)} disabled={!tag} title="Nächster Tag" aria-label="Nächster Tag">
            {Icon.chevronRight}
          </button>
          {/* Wert aus dem lokalen Zustand: sonst setzt React die Tastatureingabe bis zur Antwort zurück */}
          <input
            type="date"
            className="input"
            value={date ?? tag?.datum ?? ""}
            onChange={(e) => setDate(e.target.value || null)}
            aria-label="Datum wählen"
          />
        </div>
      </div>

      {error && <div className="error">{error}</div>}

      <div className="heute-grid">
        <div className="heute-main">
          {tag && tag.warnungen.length > 0 && (
            <div>
              {tag.warnungen.map((w, i) => (
                <div key={i} className="warn">
                  {w}
                </div>
              ))}
            </div>
          )}

          {tag?.art === "schultag" && (
            <section className="panel glass">
              <div className="panel-head">
                <span className="panel-title">Stundenplan</span>
                <button className="btn small" onClick={() => onCreate({ datum: tag.datum })}>
                  {Icon.plus} Notiz anlegen
                </button>
              </div>
              <div className="panel-body">
                {tag.bloecke.map((b) => (
                  <BlockRow
                    key={b.position}
                    b={b}
                    datum={tag.datum}
                    jetzt={b.position === tag.aktuellerBlock}
                    onOpenNote={onOpenNote}
                    onCreate={onCreate}
                  />
                ))}
              </div>
            </section>
          )}

          {tag && tag.art !== "schultag" && (
            <section className="panel glass">
              <div className="empty day-empty">
                <div>
                  <div className="empty-icon">{tag.art === "betriebstag" ? Icon.berichtsheft : Icon.heute}</div>
                  <h2 className="empty-title">{tag.art === "betriebstag" ? "Betriebstag" : "Wochenende"}</h2>
                  <p>
                    {tag.art === "betriebstag"
                      ? "Kein Unterricht laut Stundenplan. Die Eingabe „Was hast du heute gemacht?“ für die Wochennotiz kommt mit M4."
                      : "Kein Unterricht am Wochenende."}
                  </p>
                  <button className="btn small" onClick={() => onCreate({ datum: tag.datum })}>
                    {Icon.plus} Notiz trotzdem anlegen
                  </button>
                </div>
              </div>
            </section>
          )}

          {tag && tag.weitereNotizen.length > 0 && (
            <section className="panel glass">
              <div className="panel-head">
                <span className="panel-title">Weitere Notizen an diesem Tag</span>
                <span className="badge muted">{tag.weitereNotizen.length}</span>
              </div>
              <div className="panel-body">
                {tag.weitereNotizen.map((n) => (
                  <NoteRow key={n.path} n={n} onOpen={() => onOpenNote(n.path)} />
                ))}
              </div>
            </section>
          )}
        </div>

        <section className="panel glass heute-side">
          <div className="panel-head">
            <span className="panel-title">Fällige Aufgaben</span>
            {a && a.ueberfaellig.length > 0 && <span className="badge red">{a.ueberfaellig.length} überfällig</span>}
          </div>
          <div className="panel-body">
            {tag && aufgabenLeer && <div className="hint pad">Keine fälligen Aufgaben.</div>}
            {a && (
              <>
                <TaskGroup title="Überfällig" items={a.ueberfaellig} red onOpenNote={onOpenNote} />
                <TaskGroup title="Heute" items={a.heute} onOpenNote={onOpenNote} />
                <TaskGroup title="Nächste 7 Tage" items={a.naechsteTage} onOpenNote={onOpenNote} />
              </>
            )}
            <div className="hint pad">Abhaken kommt mit M3.</div>
          </div>
        </section>
      </div>
    </>
  );
}

function BlockRow({
  b,
  datum,
  jetzt,
  onOpenNote,
  onCreate,
}: {
  b: HeuteBlock;
  datum: string;
  jetzt: boolean;
  onOpenNote: (path: string) => void;
  onCreate: (initial: Partial<NeueUnterrichtsnotiz>) => void;
}) {
  const create = () => onCreate({ datum, fach: b.fach.kurz, block: b.fachNr });
  return (
    <div className={`block-row ${jetzt ? "now" : ""}`}>
      <div className="block-time">
        <div className="block-std">Std. {stdText(b.block.vonStunde, b.block.bisStunde)}</div>
        <div className="block-clock">
          {b.block.beginn}–{b.block.ende}
        </div>
      </div>
      <div className="block-info">
        <div className="block-badges">
          <span className="badge purple" title={b.fach.ziel}>
            {b.fach.kurz}
          </span>
          {b.fach.lehrkraft && <span className="badge muted">{b.fach.lehrkraft}</span>}
          {!b.fach.zielExistiert && <span className="badge amber">Fachnotiz fehlt</span>}
          {jetzt && <span className="badge mint">{Icon.clock} jetzt</span>}
          {b.fachAnzahl > 1 && <span className="badge muted">{b.fachNr}. Block</span>}
        </div>
        <div className="block-note">
          {b.notiz ? b.notiz.title + (b.notiz.thema ? ` – ${b.notiz.thema}` : "") : b.fach.ziel}
        </div>
      </div>
      <div className="block-actions">
        {b.notiz ? (
          <button className="btn small" onClick={() => onOpenNote(b.notiz!.path)}>
            {Icon.edit} Notiz öffnen
          </button>
        ) : b.ersteNotiz ? (
          <>
            <button className="btn small" onClick={() => onOpenNote(b.ersteNotiz!.path)}>
              In 1. Notiz weiterschreiben
            </button>
            <button className="btn small primary" onClick={create}>
              {Icon.plus} Zweite Notiz anlegen
            </button>
          </>
        ) : (
          <button className="btn small primary" onClick={create}>
            {Icon.plus} Notiz anlegen
          </button>
        )}
      </div>
      {b.hinweis && <div className="hint block-hint">{b.hinweis}</div>}
    </div>
  );
}

function NoteRow({ n, onOpen }: { n: NoteSummary; onOpen: () => void }) {
  return (
    <button className="note-item" onClick={onOpen}>
      <div className="note-title">{n.title}</div>
      <div className="note-meta">
        {n.fach && <span className="badge muted">{n.fach}</span>}
        {n.thema && <span>{n.thema}</span>}
        <span className="mono-path">{n.path}</span>
      </div>
    </button>
  );
}

function TaskGroup({
  title,
  items,
  red,
  onOpenNote,
}: {
  title: string;
  items: FaelligeAufgabe[];
  red?: boolean;
  onOpenNote: (path: string, line?: number) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="task-group">
      <div className="task-group-head">
        {title} <span className={`badge ${red ? "red" : "muted"}`}>{items.length}</span>
      </div>
      {items.map((t) => (
        <button key={`${t.path}:${t.line}`} className="note-item task-item" onClick={() => onOpenNote(t.path, t.line)}>
          <div className="task-text">{t.text || "(ohne Text)"}</div>
          <div className="note-meta">
            <span className={red ? "due red" : "due"}>{formatDatum(t.due)}</span>
            {t.fach && <span className="badge muted">{t.fach}</span>}
            <span className="task-note">{t.title}</span>
          </div>
        </button>
      ))}
    </div>
  );
}
