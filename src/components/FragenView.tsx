// Ansicht „Fragen“: offene #frage-Zeilen aus allen Notizen und #nochmal-Einträge je Fach.
// „Geklärt“ zeigt erst eine Vorschau (vorher/nachher) und schreibt erst nach der Bestätigung –
// das Backend ändert nur die eine Zeile (Tag durchstreichen oder in „## Geklärt“ verschieben).
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  api,
  errorText,
  formatDatum,
  samePath,
  type FrageEintrag,
  type FrageGeklaert,
  type FragenUebersicht,
  type VaultChanged,
} from "../api";
import { kuerzen } from "../aufgaben";
import { klartext } from "../markdown";
import { Icon } from "../icons";
import { Modal } from "./Modal";
import { useToast } from "../toast";

type Tab = "fragen" | "nochmal";

const ALLE = "";
const OHNE = "\u0000ohne";

const eintragKey = (e: FrageEintrag) => `${e.path}\n${e.line ?? "fm"}`;

interface Bestaetigung {
  eintrag: FrageEintrag;
  vorschau: FrageGeklaert;
}

export function FragenView({
  active,
  change,
  onOpenNote,
  dirtyPath,
  onGoSettings,
}: {
  active: boolean;
  change: { seq: number; data: VaultChanged } | null;
  onOpenNote: (path: string, line?: number) => void;
  /** Pfad der Notiz im Editor, wenn sie ungespeicherte Änderungen hat */
  dirtyPath: string | null;
  onGoSettings: () => void;
}) {
  const { notify } = useToast();
  const [u, setU] = useState<FragenUebersicht | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("fragen");
  const [fach, setFach] = useState(ALLE);
  // Eintrag, dessen Vorschau oder Schreibvorgang läuft
  const [laeuft, setLaeuft] = useState<string | null>(null);
  const [bestaetigung, setBestaetigung] = useState<Bestaetigung | null>(null);
  const [schreibt, setSchreibt] = useState(false);
  const seq = useRef(0);

  const load = useCallback(async () => {
    seq.current += 1;
    const mine = seq.current;
    try {
      const r = await api.fragenUebersicht();
      if (mine !== seq.current) return;
      setU(r);
      setError(null);
    } catch (e) {
      if (mine !== seq.current) return;
      setError(errorText(e));
    }
  }, []);

  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    if (active) void load();
  }, [active, load]);

  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    if (activeRef.current) void load();
  }, [change, load]);

  const obsidian = (path: string) => api.openInObsidian(path).catch((e) => notify({ kind: "error", text: errorText(e) }));

  // ---------- Geklärt: Vorschau → Bestätigen → Schreiben ----------
  async function vorschau(e: FrageEintrag) {
    if (e.line == null || laeuft) return;
    const k = eintragKey(e);
    setLaeuft(k);
    try {
      const v = await api.frageKlaeren({ path: e.path, line: e.line, zeile: e.zeile, vorschau: true });
      setBestaetigung({ eintrag: e, vorschau: v });
    } catch (err) {
      notify({ kind: "error", text: errorText(err) });
      setLaeuft(null);
      void load();
    }
  }

  function abbrechen() {
    if (schreibt) return;
    setBestaetigung(null);
    setLaeuft(null);
  }

  async function bestaetigen() {
    const b = bestaetigung;
    if (!b || schreibt) return;
    const e = b.eintrag;
    if (e.line == null) return;
    setSchreibt(true);
    try {
      // `line` der Vorschau ist die Zeile NACH der Änderung (Modus Abschnitt: neue Stelle) –
      // geschrieben wird an der angezeigten Zeile; das Backend findet sie über `zeile` wieder.
      const r = await api.frageKlaeren({ path: e.path, line: e.line, zeile: e.zeile, vorschau: false });
      const was = kuerzen(klartext(e.text) || e.zeile);
      const wo = `${e.title}, Zeile ${r.line}`;
      const wie = r.verschoben ? (r.abschnittNeu ? "Abschnitt „Geklärt“ angelegt" : "in „Geklärt“ verschoben") : "Tag durchgestrichen";
      if (r.report) {
        notify({
          kind: "saved",
          text: `Geklärt: ${was} · ${wo} · ${wie}`,
          path: r.report.relPath,
          detail: [`Datei: ${r.report.relPath}`, ...r.report.warnungen],
        });
      } else {
        notify({ kind: "info", text: `Unverändert: ${was} · ${e.title}`, path: e.path });
      }
    } catch (err) {
      notify({ kind: "error", text: errorText(err) });
    } finally {
      setBestaetigung(null);
      setSchreibt(false);
      await load();
      setLaeuft(null);
    }
  }

  // ---------- Abgeleitete Werte ----------
  const fragen = u?.fragen ?? [];
  const fachWerte = useMemo(() => {
    const m = new Map<string, string>();
    let ohne = fach === OHNE;
    for (const f of fragen) {
      if (f.fach == null) ohne = true;
      else if (!m.has(f.fach.toLowerCase())) m.set(f.fach.toLowerCase(), f.fach);
    }
    if (fach !== ALLE && fach !== OHNE && !m.has(fach.toLowerCase())) m.set(fach.toLowerCase(), fach);
    return { werte: [...m.values()].sort((a, b) => a.localeCompare(b, "de")), ohne };
  }, [fragen, fach]);
  const fragenSichtbar = fragen.filter((f) =>
    fach === ALLE ? true : fach === OHNE ? f.fach == null : f.fach?.toLowerCase() === fach.toLowerCase(),
  );
  const nochmalAnzahl = (u?.nochmal ?? []).reduce((n, g) => n + g.eintraege.length, 0);

  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Fragen</h1>
          <div className="page-sub">
            {tab === "fragen"
              ? "Zeilen mit #frage aus deinen Notizen – „Geklärt“ ändert nur diese eine Zeile."
              : "Mit #nochmal markiert: Stoff, den du dir noch einmal ansehen willst."}
          </div>
        </div>
      </div>

      <div className="filter-bar">
        <div className="pills" role="tablist" aria-label="Fragen">
          <button className={`pill ${tab === "fragen" ? "active" : ""}`} role="tab" aria-selected={tab === "fragen"} onClick={() => setTab("fragen")}>
            Offene Fragen
            {u && <span className="pill-count muted">{fragen.length}</span>}
          </button>
          <button
            className={`pill ${tab === "nochmal" ? "active" : ""}`}
            role="tab"
            aria-selected={tab === "nochmal"}
            onClick={() => setTab("nochmal")}
          >
            Nochmal ansehen
            {u && <span className="pill-count muted">{nochmalAnzahl}</span>}
          </button>
        </div>
        <span className="filter-spacer" />
        {tab === "fragen" && (
          <label className="filter">
            <span>Fach</span>
            <select className="input" value={fach} onChange={(e) => setFach(e.target.value)}>
              <option value={ALLE}>Alle</option>
              {fachWerte.werte.map((w) => (
                <option key={w} value={w}>
                  {w}
                </option>
              ))}
              {fachWerte.ohne && <option value={OHNE}>ohne Fach</option>}
            </select>
          </label>
        )}
      </div>

      {error && <div className="error">{error}</div>}
      {!u && !error && <div className="hint pad">Lade …</div>}

      {u && tab === "fragen" && (
        <div className="aufgaben-scroll">
          <section className="panel glass">
            <div className="panel-head">
              <span className="panel-title">Offene Fragen</span>
              <span className="badge muted">{fragenSichtbar.length}</span>
            </div>
            <div className="hint fragen-modus">
              „Geklärt“ {u.modus === "abschnitt" ? "verschiebt die Zeile in den Abschnitt „Geklärt“ der Notiz" : "streicht das Tag durch (~~#frage~~), die Zeile bleibt stehen"}
              {" · "}
              <button className="lb-link inline" onClick={onGoSettings}>
                ändern
              </button>
            </div>
            <div className="panel-body">
              {fragenSichtbar.length === 0 && (
                <div className="hint pad">
                  {fragen.length === 0 ? "Keine offenen Fragen. Markiere eine Zeile in einer Notiz mit #frage." : "Keine offenen Fragen für diesen Filter."}
                </div>
              )}
              {fragenSichtbar.map((e) => {
                const k = eintragKey(e);
                const busy = laeuft === k;
                const dirty = samePath(e.path, dirtyPath);
                return (
                  <div key={k} className={`auftrag ${busy ? "busy" : ""}`} aria-busy={busy}>
                    <span className="frage-icon">{Icon.fragen}</span>
                    <div className="auftrag-main">
                      <div className="auftrag-text">{klartext(e.text) || e.zeile}</div>
                      <div className="note-meta">
                        {e.fach && <span className="badge purple">{e.fach}</span>}
                        {e.datum && <span>{formatDatum(e.datum)}</span>}
                        {e.privat && <span className="badge muted">10 Privat</span>}
                        <button
                          className="lb-link"
                          onClick={() => onOpenNote(e.path, e.line ?? undefined)}
                          title={e.line != null ? `${e.path}, Zeile ${e.line}` : e.path}
                        >
                          {Icon.datei}
                          {e.title}
                        </button>
                        {busy && !bestaetigung && (
                          <span className="lb-busy">
                            <span className="spinner" /> Vorschau …
                          </span>
                        )}
                      </div>
                      {dirty && <div className="hint lb-warn-text">„{e.title}“ hat ungespeicherte Änderungen – erst speichern.</div>}
                    </div>
                    <div className="auftrag-aktionen">
                      <button
                        className="btn small"
                        onClick={() => void vorschau(e)}
                        disabled={busy || laeuft !== null || dirty || e.line == null}
                        title={dirty ? "Erst die Notiz im Editor speichern" : "Als geklärt markieren"}
                      >
                        {Icon.check} Geklärt
                      </button>
                      <button
                        className="btn small icon-only"
                        onClick={() => void obsidian(e.path)}
                        title="In Obsidian öffnen"
                        aria-label={`„${e.title}“ in Obsidian öffnen`}
                      >
                        {Icon.obsidian}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        </div>
      )}

      {u && tab === "nochmal" && (
        <div className="aufgaben-scroll">
          {u.nochmal.length === 0 ? (
            <section className="panel glass">
              <div className="empty day-empty">
                <div>
                  <div className="empty-icon">{Icon.refresh}</div>
                  <p>Nichts markiert. Setze #nochmal an eine Zeile oder in die Tags einer Notiz.</p>
                </div>
              </div>
            </section>
          ) : (
            <div className="nochmal-grid">
              {u.nochmal.map((g) => (
                <section key={g.fach ?? OHNE} className="panel glass">
                  <div className="panel-head">
                    <span className="panel-title">{g.fach ?? "Ohne Fach"}</span>
                    <span className="badge muted">{g.eintraege.length}</span>
                  </div>
                  <div className="panel-body">
                    {g.eintraege.map((e) => (
                      <div key={eintragKey(e)} className="auftrag">
                        <div className="auftrag-main">
                          <div className="auftrag-text">{e.line == null ? e.title : klartext(e.text) || e.zeile}</div>
                          <div className="note-meta">
                            {e.line == null && (
                              <span className="badge muted" title="#nochmal steht in den Tags der Notiz">
                                ganze Notiz
                              </span>
                            )}
                            {e.datum && <span>{formatDatum(e.datum)}</span>}
                            {e.privat && <span className="badge muted">10 Privat</span>}
                            {e.line != null && (
                              <button className="lb-link" onClick={() => onOpenNote(e.path, e.line ?? undefined)} title={`${e.path}, Zeile ${e.line}`}>
                                {Icon.datei}
                                {e.title}
                              </button>
                            )}
                            {e.line == null && (
                              <button className="lb-link" onClick={() => onOpenNote(e.path)} title={e.path}>
                                {Icon.datei}
                                öffnen
                              </button>
                            )}
                          </div>
                        </div>
                        <button
                          className="btn small icon-only"
                          onClick={() => void obsidian(e.path)}
                          title="In Obsidian öffnen"
                          aria-label={`„${e.title}“ in Obsidian öffnen`}
                        >
                          {Icon.obsidian}
                        </button>
                      </div>
                    ))}
                  </div>
                </section>
              ))}
            </div>
          )}
        </div>
      )}

      {bestaetigung && (
        <GeklaertDialog b={bestaetigung} schreibt={schreibt} onConfirm={() => void bestaetigen()} onCancel={abbrechen} />
      )}
    </div>
  );
}

function GeklaertDialog({
  b,
  schreibt,
  onConfirm,
  onCancel,
}: {
  b: Bestaetigung;
  schreibt: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const titleId = useId();
  const v = b.vorschau;
  const e = b.eintrag;
  return (
    <Modal onClose={onCancel} labelledBy={titleId} className="modal-geklaert">
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.fragen} Frage als geklärt markieren
        </h2>
        <div className="hint">
          {e.title} · Zeile {e.line}
          {v.verschoben && v.line !== e.line ? ` → ${v.line}` : ""} · <span className="mono-path">{e.path}</span>
        </div>
      </div>
      <div className="modal-body geklaert-body">
        <div className="diff-label">Vorher</div>
        <pre className="diff-zeile vorher">{v.vorher}</pre>
        <div className="diff-label">Nachher</div>
        <pre className="diff-zeile nachher">{v.nachher}</pre>
        <div className="hint geklaert-hinweis">
          {v.verschoben
            ? v.abschnittNeu
              ? "Die Zeile wird in den Abschnitt „Geklärt“ verschoben – der Abschnitt wird am Ende der Notiz angelegt."
              : "Die Zeile wird in den Abschnitt „Geklärt“ verschoben (ans Ende des Abschnitts)."
            : "Das Tag wird durchgestrichen – die Zeile bleibt, wo sie ist. Sonst ändert sich nichts an der Datei."}
        </div>
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={onCancel} disabled={schreibt}>
          Abbrechen
        </button>
        <button className="btn primary" onClick={onConfirm} disabled={schreibt} data-autofocus>
          {Icon.check} {schreibt ? "Wird geschrieben …" : "Bestätigen"}
        </button>
      </div>
    </Modal>
  );
}
