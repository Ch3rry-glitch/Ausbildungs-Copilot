// Dialog „Neue Unterrichtsnotiz“ mit Live-Vorschau (Probelauf im Backend, nichts wird geschrieben).
import { useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import {
  api,
  errorText,
  stdText,
  type FachInfo,
  type HeuteTag,
  type LernfeldInfo,
  type NeueUnterrichtsnotiz,
  type NotizVorschau,
} from "../api";
import { Icon } from "../icons";
import { useToast } from "../toast";
import { Modal } from "./Modal";

const gleich = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

export function CreateNoteDialog({
  initial,
  onClose,
  onCreated,
  onOpenExisting,
}: {
  initial: Partial<NeueUnterrichtsnotiz>;
  onClose: () => void;
  onCreated: (relPath: string) => void;
  onOpenExisting: (relPath: string) => void;
}) {
  const { notify } = useToast();
  const titleId = useId();
  const [datum, setDatum] = useState<string | null>(initial.datum ?? null);
  const [fach, setFach] = useState(initial.fach ?? "");
  const [block, setBlock] = useState(initial.block ?? 1);
  const [lernfeld, setLernfeld] = useState(initial.lernfeld ?? "");
  const [thema, setThema] = useState(initial.thema ?? "");

  const [faecher, setFaecher] = useState<FachInfo[]>([]);
  const [lernfelder, setLernfelder] = useState<LernfeldInfo[]>([]);
  const [tag, setTag] = useState<HeuteTag | null>(null);
  const [vorschau, setVorschau] = useState<NotizVorschau | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const previewSeq = useRef(0);
  const tagSeq = useRef(0);

  useEffect(() => {
    let alive = true;
    Promise.all([api.listFaecher(), api.listLernfelder()])
      .then(([f, l]) => {
        if (!alive) return;
        setFaecher(f);
        setLernfelder(l);
      })
      .catch((e) => alive && setError(errorText(e)));
    return () => {
      alive = false;
    };
  }, []);

  // Vorbelegtes Kürzel auf die Schreibweise der Fächerliste bringen, damit die Auswahl passt.
  useEffect(() => {
    setFach((cur) => faecher.find((f) => gleich(f.kurz, cur))?.kurz ?? cur);
  }, [faecher]);

  // Tag laden (für Blockanzahl und Hinweise); ohne Datum liefert heute(null) das heutige Datum.
  useEffect(() => {
    tagSeq.current += 1;
    const seq = tagSeq.current;
    api
      .heute(datum)
      .then((t) => {
        if (seq !== tagSeq.current) return;
        setTag(t);
        if (datum === null) setDatum(t.datum);
      })
      .catch((e) => seq === tagSeq.current && setError(errorText(e)));
  }, [datum]);

  const bloeckeDesFachs = useMemo(
    () => (tag && fach ? tag.bloecke.filter((b) => gleich(b.fach.kurz, fach)) : []),
    [tag, fach],
  );
  const fachAnzahl = bloeckeDesFachs.length;
  const showBlock = fachAnzahl > 1 || (initial.block ?? 1) > 1;
  const blockMax = Math.max(fachAnzahl, initial.block ?? 1, 1);
  const effBlock = showBlock ? Math.min(block, blockMax) : 1;
  const heuteBlock = bloeckeDesFachs.find((b) => b.fachNr === effBlock) ?? null;

  const req: NeueUnterrichtsnotiz | null = useMemo(
    () =>
      datum && fach
        ? {
            datum,
            fach,
            block: effBlock,
            lernfeld: lernfeld || null,
            thema: thema.trim() ? thema : null,
          }
        : null,
    [datum, fach, effBlock, lernfeld, thema],
  );

  // Vorschau, 250 ms entprellt, nur für vollständige Anfragen
  useEffect(() => {
    previewSeq.current += 1;
    const seq = previewSeq.current;
    if (!req) {
      setVorschau(null);
      setPreviewError(null);
      return;
    }
    const t = setTimeout(() => {
      api
        .previewUnterrichtNote(req)
        .then((v) => {
          if (seq !== previewSeq.current) return;
          setVorschau(v);
          setPreviewError(null);
        })
        .catch((e) => {
          if (seq !== previewSeq.current) return;
          setVorschau(null);
          setPreviewError(errorText(e));
        });
    }, 250);
    return () => clearTimeout(t);
  }, [req]);

  async function submit(e?: FormEvent) {
    e?.preventDefault();
    if (!req || busy || vorschau?.existiert) return;
    setBusy(true);
    setError(null);
    try {
      const r = await api.createUnterrichtNote(req);
      if (r.status === "created") {
        const rep = r.report;
        notify({
          kind: "created",
          text: "Angelegt: " + rep.relPath,
          path: rep.relPath,
          detail: [...rep.createdFolders.map((f) => "Ordner angelegt: " + f), ...rep.warnungen],
        });
        onCreated(rep.relPath);
      } else {
        notify({ kind: "info", text: `Notiz existiert bereits – geöffnet: ${r.relPath}` });
        onOpenExisting(r.relPath);
      }
    } catch (err) {
      setError(errorText(err));
      setBusy(false);
    }
  }

  const fachBekannt = !fach || faecher.some((f) => gleich(f.kurz, fach));

  return (
    <Modal onClose={onClose} className="modal-create" labelledBy={titleId}>
      <form onSubmit={submit} className="create-form">
        <div className="modal-head">
          <h2 id={titleId} className="modal-title">
            {Icon.plus} Neue Unterrichtsnotiz
          </h2>
        </div>

        <div className="modal-body create-grid">
          <div className="form-fields">
            <label className="field">
              <span>Datum</span>
              <input
                type="date"
                className="input"
                value={datum ?? ""}
                onChange={(e) => setDatum(e.target.value || null)}
                required
              />
            </label>

            <label className="field">
              <span>Fach</span>
              <select
                className="input"
                value={fach}
                onChange={(e) => {
                  setFach(e.target.value);
                  setBlock(1);
                }}
                required
              >
                <option value="" disabled>
                  Fach wählen …
                </option>
                {!fachBekannt && <option value={fach}>{fach}</option>}
                {faecher.map((f) => (
                  <option key={f.kurz} value={f.kurz}>
                    {f.kurz === f.ziel ? f.kurz : `${f.kurz} – ${f.ziel.replace(new RegExp(`^${escapeRe(f.kurz)}\\s*[–—-]\\s*`), "")}`}
                  </option>
                ))}
              </select>
            </label>

            {showBlock && (
              <label className="field">
                <span>Block</span>
                <select className="input" value={effBlock} onChange={(e) => setBlock(Number(e.target.value))}>
                  {Array.from({ length: blockMax }, (_, i) => i + 1).map((n) => {
                    const b = bloeckeDesFachs.find((x) => x.fachNr === n);
                    return (
                      <option key={n} value={n}>
                        {n}. Block{b ? ` (Std. ${stdText(b.block.vonStunde, b.block.bisStunde)})` : ""}
                      </option>
                    );
                  })}
                </select>
              </label>
            )}

            <label className="field">
              <span>Lernfeld</span>
              <select className="input" value={lernfeld} onChange={(e) => setLernfeld(e.target.value)}>
                <option value="">— kein Lernfeld —</option>
                {lernfelder.map((l) => (
                  <option key={l.path} value={l.path} title={l.titel}>
                    {l.kurz ?? l.titel}
                  </option>
                ))}
              </select>
            </label>

            <label className="field">
              <span>Thema</span>
              <input
                className="input"
                value={thema}
                onChange={(e) => setThema(e.target.value)}
                placeholder="z. B. Handelskalkulation"
                autoFocus
                spellCheck
              />
            </label>

            <div className="field">
              <span>Lehrkraft</span>
              <div className="readonly-value">{vorschau?.lehrkraft ?? "—"}</div>
            </div>

            {effBlock > 1 && heuteBlock?.ersteNotiz && (
              <div className="banner">
                <div>{heuteBlock.hinweis ?? "In der Notiz des 1. Blocks weiterschreiben oder eine eigene Notiz anlegen."}</div>
                <button type="button" className="btn small" onClick={() => onOpenExisting(heuteBlock.ersteNotiz!.path)}>
                  {Icon.edit} In 1. Notiz weiterschreiben
                </button>
              </div>
            )}
          </div>

          <div className="create-preview">
            {!req && <div className="hint">Wähle Datum und Fach – dann erscheint hier die Vorschau.</div>}
            {previewError && <div className="error">{previewError}</div>}
            {vorschau && (
              <>
                <div className="field">
                  <span>Dateiname</span>
                  <code className="mono-path">{vorschau.relPath}</code>
                </div>
                {vorschau.warnungen.map((w, i) => (
                  <div key={i} className="warn">
                    {w}
                  </div>
                ))}
                {vorschau.existiert && (
                  <div className="warn warn-row">
                    <span>Diese Notiz gibt es schon: {vorschau.existiert}</span>
                    <button type="button" className="btn small" onClick={() => onOpenExisting(vorschau.existiert!)}>
                      Vorhandene Notiz öffnen
                    </button>
                  </div>
                )}
                <details className="content-preview">
                  <summary>Vorschau des Inhalts</summary>
                  <pre>{vorschau.text}</pre>
                </details>
              </>
            )}
          </div>
        </div>

        {error && <div className="error modal-error">{error}</div>}

        <div className="modal-foot">
          <button type="button" className="btn" onClick={onClose}>
            Abbrechen
          </button>
          <button type="submit" className="btn primary" disabled={!req || busy || !!vorschau?.existiert}>
            {Icon.plus} {busy ? "Legt an …" : "Anlegen"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

function escapeRe(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
