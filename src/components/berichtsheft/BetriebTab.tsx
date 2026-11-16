// Reiter „Betrieb.md“: neue Überschneidungen Schule ↔ Betrieb aus der Lernfeld-Tabelle der Woche
// in die vorhandene Tabelle von „04 Betrieb/Betrieb.md“ – nur nach Diff und Bestätigung (Regel 9).
import { useEffect, useRef, useState } from "react";
import { api, errorText, samePath, type BerichtsWoche, type BetriebVorschlag } from "../../api";
import { Icon } from "../../icons";
import { useToast } from "../../toast";
import { anzahl, Hinweise, neueId, useSchreibFluss } from "./gemeinsam";

interface EditZeile {
  id: number;
  zellen: string[];
  taetigkeit: string;
  grund: string;
  uebernehmen: boolean;
  /** in dieser Sitzung schon in Betrieb.md geschrieben */
  geschrieben: boolean;
}

export function BetriebTab({
  woche,
  gesperrt,
  dirtyPath,
  onGeschrieben,
  onOpenNote,
}: {
  woche: BerichtsWoche;
  /** Wochennotiz ungespeichert im Editor */
  gesperrt: boolean;
  dirtyPath: string | null;
  onGeschrieben: () => void;
  onOpenNote: (path: string) => void;
}) {
  const { notify } = useToast();
  const fluss = useSchreibFluss();
  const [vorschlag, setVorschlag] = useState<BetriebVorschlag | null>(null);
  const [zeilen, setZeilen] = useState<EditZeile[]>([]);
  const [laedt, setLaedt] = useState(false);
  const [fehler, setFehler] = useState<string | null>(null);
  const seq = useRef(0);

  async function laden() {
    seq.current += 1;
    const mine = seq.current;
    setLaedt(true);
    setFehler(null);
    try {
      const v = await api.betriebVorschlag(woche.path);
      if (mine !== seq.current) return;
      setVorschlag(v);
      setZeilen(
        v.zeilen.map((z) => ({
          id: neueId(),
          zellen: v.spalten.map((_, i) => z.zellen[i] ?? ""),
          taetigkeit: z.taetigkeit,
          grund: z.grund,
          uebernehmen: z.uebernehmen,
          geschrieben: false,
        })),
      );
    } catch (e) {
      if (mine !== seq.current) return;
      setFehler(errorText(e));
    } finally {
      if (mine === seq.current) setLaedt(false);
    }
  }

  const ladenRef = useRef(laden);
  ladenRef.current = laden;
  useEffect(() => {
    void ladenRef.current();
  }, []);

  function aendere(id: number, f: (z: EditZeile) => EditZeile) {
    setZeilen((zs) => zs.map((z) => (z.id === id ? f(z) : z)));
  }

  const betriebPath = vorschlag?.betriebPath ?? null;
  const betriebDirty = samePath(betriebPath, dirtyPath);
  const busy = laedt || fluss.laeuft;
  const gewaehlt = zeilen.filter((z) => z.uebernehmen && !z.geschrieben);

  function vorschau() {
    if (gewaehlt.length === 0) return;
    const neu = gewaehlt.map((z) => z.zellen.map((c) => c.trim()));
    const ids = new Set(gewaehlt.map((z) => z.id));
    void fluss.starten({
      aktion: "Neue Zeilen in die Tabelle von Betrieb.md schreiben",
      knopf: "In Betrieb.md schreiben",
      erfolg: `Betrieb.md fortgeschrieben (${anzahl(neu.length, "Zeile", "Zeilen")})`,
      vorschau: () => api.betriebFortschreiben({ wochePath: woche.path, zeilen: neu, vorschau: true, basis: null }),
      schreiben: (basis) => api.betriebFortschreiben({ wochePath: woche.path, zeilen: neu, vorschau: false, basis }),
      danach: (r) => {
        if (r.report) setZeilen((zs) => zs.map((z) => (ids.has(z.id) ? { ...z, uebernehmen: false, geschrieben: true } : z)));
        onGeschrieben();
      },
    });
  }

  const obsidian = (p: string) => api.openInObsidian(p).catch((e) => notify({ kind: "error", text: errorText(e) }));

  return (
    <div className="bh-tab">
      <div className="lb-ki">
        <div className="lb-ki-line">
          Ziel: <strong>{betriebPath ?? "04 Betrieb/Betrieb.md"}</strong> – erste Tabelle mit den Spalten Tätigkeit/Betrieb und Lernfeld
        </div>
        <div className="row lb-ki-buttons">
          <button className="btn small" onClick={() => void laden()} disabled={busy}>
            {Icon.refresh} Vorschlag neu laden
          </button>
          {betriebPath && (
            <>
              <button className="btn small" onClick={() => onOpenNote(betriebPath)}>
                {Icon.edit} Im Editor öffnen
              </button>
              <button className="btn small" onClick={() => void obsidian(betriebPath)}>
                {Icon.obsidian} In Obsidian öffnen
              </button>
            </>
          )}
          {laedt && (
            <span className="lb-busy">
              <span className="spinner" /> Lade Vorschlag …
            </span>
          )}
        </div>
        <div className="hint">Vorgeschlagen werden Zeilen aus „Bezug zu Lernfeldern“ dieser Woche, die in Betrieb.md noch fehlen.</div>
      </div>

      {fehler && <div className="error">{fehler}</div>}
      {betriebDirty && (
        <div className="banner lb-banner">
          {Icon.warning}
          <span>„Betrieb.md“ hat ungespeicherte Änderungen im Editor – erst speichern.</span>
        </div>
      )}

      {vorschlag && (
        <>
          <Hinweise texte={vorschlag.warnungen} />
          {vorschlag.schonVorhanden.length > 0 && (
            <div className="hint lb-hinweis">
              {Icon.check}
              <span>Schon in Betrieb.md: {vorschlag.schonVorhanden.join(", ")}</span>
            </div>
          )}
          <section className="lb-section">
            <div className="lb-section-title">Neue Zeilen</div>
            {zeilen.length === 0 ? (
              <div className="hint">
                Nichts Neues – {woche.lernfeldTabelle.length === 0 ? "die Lernfeld-Tabelle der Woche ist noch leer (Reiter „Lernfelder“)." : "alles steht schon in Betrieb.md."}
              </div>
            ) : (
              <BetriebTabelle spalten={vorschlag.spalten} zeilen={zeilen} busy={busy} onAendern={aendere} />
            )}
            {zeilen.length > 0 && (
              <div className="lb-actions">
                <div className="row">
                  <button className="btn primary" onClick={vorschau} disabled={gesperrt || betriebDirty || busy || gewaehlt.length === 0}>
                    {Icon.diff} Vorschau ({anzahl(gewaehlt.length, "Zeile", "Zeilen")})
                  </button>
                  {fluss.laeuft && (
                    <span className="lb-busy">
                      <span className="spinner" /> Vorschau …
                    </span>
                  )}
                </div>
                <div className="hint">Vor dem Schreiben siehst du den Diff von Betrieb.md – sonst ändert sich keine Datei.</div>
              </div>
            )}
          </section>
        </>
      )}

      {fluss.dialog}
    </div>
  );
}

function BetriebTabelle({
  spalten,
  zeilen,
  busy,
  onAendern,
}: {
  spalten: string[];
  zeilen: EditZeile[];
  busy: boolean;
  onAendern: (id: number, f: (z: EditZeile) => EditZeile) => void;
}) {
  return (
    <div className="bh-tabelle-scroll">
      <table className="lern-table bh-table">
        <thead>
          <tr>
            <th className="center">übernehmen</th>
            {spalten.map((s, i) => (
              <th key={i}>{s}</th>
            ))}
            <th>Grund</th>
          </tr>
        </thead>
        <tbody>
          {zeilen.map((z) => (
            <tr key={z.id} className={z.uebernehmen || z.geschrieben ? "" : "bh-zeile-aus"}>
              <td className="center">
                {z.geschrieben ? (
                  <span className="badge mint">{Icon.check} eingetragen</span>
                ) : (
                  <input
                    type="checkbox"
                    className="auftrag-check"
                    checked={z.uebernehmen}
                    onChange={(e) => onAendern(z.id, (x) => ({ ...x, uebernehmen: e.target.checked }))}
                    disabled={busy}
                    aria-label={`„${z.taetigkeit}“ übernehmen`}
                  />
                )}
              </td>
              {z.zellen.map((c, i) => (
                <td key={i} className="bh-zelle-breit">
                  <input
                    className="input bh-cell-input"
                    value={c}
                    onChange={(e) => onAendern(z.id, (x) => ({ ...x, zellen: x.zellen.map((y, k) => (k === i ? e.target.value : y)) }))}
                    disabled={busy || z.geschrieben}
                    spellCheck
                    aria-label={spalten[i]}
                  />
                </td>
              ))}
              <td>{z.grund || (z.uebernehmen ? "" : "nur Schule")}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
