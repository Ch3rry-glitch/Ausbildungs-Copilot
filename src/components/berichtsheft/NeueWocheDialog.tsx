// Dialog „Wochennotiz anlegen“: Probelauf im Backend (Dateiname, Inhalt, eingetragene
// Unterrichtsnotizen, Warnungen) – erst „Anlegen“ schreibt die neue Datei (nie überschreiben).
import { useEffect, useId, useState } from "react";
import { api, baseName, errorText, type NeueWoche, type WochenVorschau } from "../../api";
import { Icon } from "../../icons";
import { useToast } from "../../toast";
import { MarkdownPreview } from "../MarkdownPreview";
import { Modal } from "../Modal";
import { Hinweise } from "./gemeinsam";

export function NeueWocheDialog({
  req,
  onClose,
  onFertig,
}: {
  req: NeueWoche;
  onClose: () => void;
  /** angelegt oder schon vorhanden → diese Woche auswählen */
  onFertig: (relPath: string) => void;
}) {
  const { notify } = useToast();
  const titleId = useId();
  const [vorschau, setVorschau] = useState<WochenVorschau | null>(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [ansicht, setAnsicht] = useState<"gerendert" | "markdown">("gerendert");

  useEffect(() => {
    let alive = true;
    api
      .berichtsheftWocheVorschau(req)
      .then((v) => alive && setVorschau(v))
      .catch((e) => alive && setFehler(errorText(e)));
    return () => {
      alive = false;
    };
  }, [req]);

  async function anlegen() {
    if (!vorschau || vorschau.existiert || busy) return;
    setBusy(true);
    setFehler(null);
    try {
      const r = await api.berichtsheftWocheAnlegen(req);
      if (r.status === "created") {
        const rep = r.report;
        notify({
          kind: "created",
          text: "Angelegt: " + rep.relPath,
          path: rep.relPath,
          detail: [
            ...rep.createdFolders.map((f) => "Ordner angelegt: " + f),
            ...vorschau.eingetragen.map((p) => "Eingetragen: " + baseName(p)),
            ...rep.warnungen,
          ],
        });
        onFertig(rep.relPath);
      } else {
        notify({ kind: "info", text: `Die Wochennotiz gibt es schon: ${r.relPath}`, path: r.relPath });
        onFertig(r.relPath);
      }
    } catch (e) {
      setFehler(errorText(e));
      setBusy(false);
    }
  }

  return (
    <Modal onClose={onClose} labelledBy={titleId} className="modal-bh-neu">
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.berichtsheft} Wochennotiz anlegen
        </h2>
        {vorschau && (
          <div className="hint">
            <span className="mono-path">{vorschau.relPath}</span> · aus der Vorlage „Berichtsheft-Woche“
          </div>
        )}
      </div>
      <div className="modal-body bh-neu-body">
        {!vorschau && !fehler && (
          <div className="lb-busy">
            <span className="spinner" /> Vorschau wird erstellt …
          </div>
        )}
        {vorschau && (
          <>
            <Hinweise texte={vorschau.warnungen} />
            {vorschau.existiert && (
              <div className="warn warn-row">
                <span>Diese Wochennotiz gibt es schon: {vorschau.existiert}</span>
                <button className="btn small" onClick={() => onFertig(vorschau.existiert!)}>
                  Vorhandene Woche öffnen
                </button>
              </div>
            )}
            <div className="lb-quellen">
              <span className="lb-label">Schultage aus</span>
              {vorschau.eingetragen.length === 0 ? (
                <span className="hint">keinen Unterrichtsnotizen (für diese Woche gibt es noch keine)</span>
              ) : (
                vorschau.eingetragen.map((p) => (
                  <span key={p} className="badge purple" title={p}>
                    {baseName(p)}
                  </span>
                ))
              )}
            </div>
            <div className="bh-neu-kopf">
              <span className="lb-label">Inhalt</span>
              <div className="pills" role="tablist" aria-label="Darstellung">
                <button className={`pill ${ansicht === "gerendert" ? "active" : ""}`} role="tab" aria-selected={ansicht === "gerendert"} onClick={() => setAnsicht("gerendert")}>
                  Vorschau
                </button>
                <button className={`pill ${ansicht === "markdown" ? "active" : ""}`} role="tab" aria-selected={ansicht === "markdown"} onClick={() => setAnsicht("markdown")}>
                  Markdown
                </button>
              </div>
            </div>
            <div className="bh-neu-vorschau">{ansicht === "gerendert" ? <MarkdownPreview text={vorschau.text} /> : <pre className="lb-vorschau">{vorschau.text}</pre>}</div>
          </>
        )}
      </div>
      {fehler && <div className="error modal-error">{fehler}</div>}
      <div className="modal-foot">
        <button className="btn" onClick={onClose} disabled={busy}>
          Abbrechen
        </button>
        <button className="btn primary" onClick={() => void anlegen()} disabled={!vorschau || !!vorschau.existiert || busy}>
          {Icon.plus} {busy ? "Legt an …" : "Anlegen"}
        </button>
      </div>
    </Modal>
  );
}
