// Reiter „Lernfelder“: Vorschläge Tätigkeit → Lernfeld(er) (ohne KI beim Öffnen, mit KI auf Klick),
// bearbeitbar; „Vorschau“ zeigt den Diff für Tabelle „Bezug zu Lernfeldern“ und Frontmatter `lernfelder`.
import { useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  kiZeile,
  type BerichtsWoche,
  type KiStatus,
  type LernfeldEntwurf,
  type LernfeldInfo,
  type LernfeldVorschlag,
} from "../../api";
import { klartext } from "../../markdown";
import { Icon } from "../../icons";
import { ABSCHNITT_LABEL, anzahl, Hinweise, KiFehltHinweis, lfKurz, lfTitel, neueId, useSchreibFluss } from "./gemeinsam";

interface EditZeile {
  id: number;
  quelle: LernfeldVorschlag;
  taetigkeit: string;
  lernfelder: number[];
  uebernehmen: boolean;
  /** steht (inzwischen) schon in der Tabelle der Woche */
  inTabelle: boolean;
}

function zuEdit(v: LernfeldVorschlag): EditZeile {
  return {
    id: neueId(),
    quelle: v,
    taetigkeit: v.taetigkeit,
    lernfelder: [...v.lernfelder].sort((a, b) => a - b),
    uebernehmen: v.lernfelder.length > 0 && !v.schonInTabelle,
    inTabelle: v.schonInTabelle,
  };
}

export function LernfelderTab({
  woche,
  gesperrt,
  kiStatus,
  onGeschrieben,
  onGoSettings,
}: {
  woche: BerichtsWoche;
  gesperrt: boolean;
  kiStatus: KiStatus | null;
  onGeschrieben: () => void;
  onGoSettings: () => void;
}) {
  const fluss = useSchreibFluss();
  const [entwurf, setEntwurf] = useState<LernfeldEntwurf | null>(null);
  const [zeilen, setZeilen] = useState<EditZeile[]>([]);
  const [laedt, setLaedt] = useState<null | "ohne" | "ki">(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [privatErlauben, setPrivatErlauben] = useState(false);
  const seq = useRef(0);

  const kiOk = !!kiStatus?.schluesselGespeichert;
  const hatPrivat = woche.tage.some((t) => t.unterricht.some((n) => n.privat));
  const privatAn = privatErlauben && hatPrivat;
  const busy = laedt !== null || fluss.laeuft;
  const lernfelder = (entwurf?.lernfelder ?? []).filter((l): l is LernfeldInfo & { nummer: number } => l.nummer != null);

  async function laden(mitKi: boolean) {
    seq.current += 1;
    const mine = seq.current;
    const privat = mitKi && privatAn;
    if (mitKi) setPrivatErlauben(false); // Regel 10: nur für genau diesen Vorgang
    setLaedt(mitKi ? "ki" : "ohne");
    setFehler(null);
    try {
      const e = await api.berichtsheftLernfelder({ path: woche.path, mitKi, privatErlauben: privat });
      if (mine !== seq.current) return;
      setEntwurf(e);
      setZeilen(e.vorschlaege.map(zuEdit));
    } catch (err) {
      if (mine !== seq.current) return;
      setFehler(errorText(err));
    } finally {
      if (mine === seq.current) setLaedt(null);
    }
  }

  // Beim ersten Öffnen: Vorschläge ohne KI
  const ladenRef = useRef(laden);
  ladenRef.current = laden;
  useEffect(() => {
    void ladenRef.current(false);
  }, []);

  function aendere(id: number, f: (z: EditZeile) => EditZeile) {
    setZeilen((zs) => zs.map((z) => (z.id === id ? f(z) : z)));
  }

  /** Lernfeld hinzu/weg; das erste gewählte Lernfeld hakt „übernehmen“ an, das letzte entfernte ab. */
  function setzeLernfelder(id: number, lf: number[]) {
    aendere(id, (z) => {
      const sortiert = [...new Set(lf)].sort((a, b) => a - b);
      let uebernehmen = z.uebernehmen;
      if (sortiert.length === 0) uebernehmen = false;
      else if (z.lernfelder.length === 0 && !z.inTabelle) uebernehmen = true;
      return { ...z, lernfelder: sortiert, uebernehmen };
    });
  }

  const gewaehlt = zeilen.filter((z) => z.uebernehmen && z.lernfelder.length > 0 && z.taetigkeit.trim() !== "");

  function vorschau() {
    if (gewaehlt.length === 0) return;
    const neu = gewaehlt.map((z) => ({ taetigkeit: z.taetigkeit.trim(), lernfelder: z.lernfelder }));
    const ids = new Set(gewaehlt.map((z) => z.id));
    void fluss.starten({
      aktion: "Lernfeldbezug in die Wochennotiz schreiben (Tabelle und Frontmatter „lernfelder“)",
      knopf: "Übernehmen",
      erfolg: `Lernfeldbezug übernommen (${anzahl(neu.length, "Zeile", "Zeilen")})`,
      vorschau: () => api.berichtsheftLernfelderSchreiben({ path: woche.path, zeilen: neu, vorschau: true, basis: null }),
      schreiben: (basis) => api.berichtsheftLernfelderSchreiben({ path: woche.path, zeilen: neu, vorschau: false, basis }),
      danach: (r) => {
        if (r.report) setZeilen((zs) => zs.map((z) => (ids.has(z.id) ? { ...z, uebernehmen: false, inTabelle: true } : z)));
        onGeschrieben();
      },
    });
  }

  return (
    <div className="bh-tab">
      <div className="lb-ki">
        <div className="row lb-ki-buttons">
          <button className="btn small" onClick={() => void laden(false)} disabled={busy}>
            {Icon.refresh} Vorschläge laden
          </button>
          <button className="btn small primary" onClick={() => void laden(true)} disabled={!kiOk || busy}>
            {Icon.sparkle} Mit KI vorschlagen
          </button>
          {laedt === "ki" && (
            <span className="lb-busy">
              <span className="spinner" /> KI ordnet zu …
            </span>
          )}
          {laedt === "ohne" && (
            <span className="lb-busy">
              <span className="spinner" /> Vorschläge werden geladen …
            </span>
          )}
        </div>
        <div className="hint">
          Ohne KI: Lernfelder aus „Betrieb.md“ bzw. aus den verlinkten Unterrichtsnotizen. Die KI bekommt die Tätigkeiten dieser Liste
          und die Lernfelder zur Auswahl · „10 Privat/“ {privatAn ? "wird diesmal mitgeschickt" : "bleibt aus"}
        </div>
        {hatPrivat && (
          <label className="check">
            <input type="checkbox" checked={privatErlauben} onChange={(e) => setPrivatErlauben(e.target.checked)} disabled={busy} />
            „10 Privat“ für diesen Vorgang erlauben
          </label>
        )}
        <KiFehltHinweis kiStatus={kiStatus} onGoSettings={onGoSettings} ohne="Vorschläge laden" />
        {entwurf?.ki && <div className="hint lb-ki-kosten">{kiZeile(entwurf.ki, kiStatus?.modelle)}</div>}
      </div>

      {fehler && <div className="error">{fehler}</div>}
      {entwurf && <Hinweise texte={entwurf.warnungen} />}

      {entwurf && (
        <section className="lb-section">
          <div className="lb-section-title">Vorschläge</div>
          {zeilen.length === 0 ? (
            <div className="hint">Keine Tätigkeiten gefunden – erst im Reiter „Tage“ Stichpunkte eintragen.</div>
          ) : (
            <div className="bh-tabelle-scroll">
              <table className="lern-table bh-table">
                <thead>
                  <tr>
                    <th>Tätigkeit</th>
                    <th>Quelle</th>
                    <th>Lernfeld(er)</th>
                    <th>Grund</th>
                    <th className="center">übernehmen</th>
                  </tr>
                </thead>
                <tbody>
                  {zeilen.map((z) => (
                    <tr key={z.id} className={z.uebernehmen ? "" : "bh-zeile-aus"}>
                      <td className="bh-zelle-breit">
                        <input
                          className="input bh-cell-input"
                          value={z.taetigkeit}
                          onChange={(e) => aendere(z.id, (x) => ({ ...x, taetigkeit: e.target.value }))}
                          disabled={busy}
                          spellCheck
                          aria-label="Tätigkeit"
                        />
                        {z.inTabelle && <span className="badge muted bh-zelle-badge">schon in der Tabelle</span>}
                      </td>
                      <td>
                        <div>{z.quelle.quelle}</div>
                        <div className="hint">{ABSCHNITT_LABEL[z.quelle.abschnitt]}</div>
                      </td>
                      <td>
                        <LernfeldAuswahl
                          gewaehlt={z.lernfelder}
                          lernfelder={lernfelder}
                          disabled={busy}
                          onChange={(lf) => setzeLernfelder(z.id, lf)}
                        />
                      </td>
                      <td>{z.quelle.grund || <span className="lern-leer">—</span>}</td>
                      <td className="center">
                        <input
                          type="checkbox"
                          className="auftrag-check"
                          checked={z.uebernehmen}
                          onChange={(e) => aendere(z.id, (x) => ({ ...x, uebernehmen: e.target.checked }))}
                          disabled={busy || z.lernfelder.length === 0}
                          title={z.lernfelder.length === 0 ? "Erst ein Lernfeld wählen" : "In die Tabelle der Woche übernehmen"}
                          aria-label={`„${z.taetigkeit}“ übernehmen`}
                        />
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {zeilen.length > 0 && (
            <div className="lb-actions">
              <div className="row">
                <button className="btn primary" onClick={vorschau} disabled={gesperrt || busy || gewaehlt.length === 0}>
                  {Icon.diff} Vorschau ({anzahl(gewaehlt.length, "Zeile", "Zeilen")})
                </button>
                {fluss.laeuft && (
                  <span className="lb-busy">
                    <span className="spinner" /> Vorschau …
                  </span>
                )}
              </div>
            </div>
          )}
        </section>
      )}

      <section className="lb-section">
        <div className="lb-section-title">Bezug zu Lernfeldern in der Wochennotiz</div>
        <div className="hint">
          Frontmatter „lernfelder“: {woche.lernfelder.length > 0 ? woche.lernfelder.map(lfKurz).join(", ") : "leer"}
        </div>
        {woche.lernfeldTabelle.length === 0 ? (
          <div className="hint">Die Tabelle ist noch leer.</div>
        ) : (
          <table className="lern-table bh-table">
            <thead>
              <tr>
                <th>Tätigkeit</th>
                <th>Lernfeld</th>
              </tr>
            </thead>
            <tbody>
              {woche.lernfeldTabelle.map((z) => (
                <tr key={z.line}>
                  <td>{klartext(z.taetigkeit) || z.taetigkeit}</td>
                  <td title={z.lernfelder.map((n) => lfTitel(n, lernfelder)).join("\n")}>{klartext(z.zelle) || z.zelle}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      {fluss.dialog}
    </div>
  );
}

/** Gewählte Lernfelder als Chips (× entfernt), weitere über die Auswahl „+ LF“. */
function LernfeldAuswahl({
  gewaehlt,
  lernfelder,
  disabled,
  onChange,
}: {
  gewaehlt: number[];
  lernfelder: (LernfeldInfo & { nummer: number })[];
  disabled: boolean;
  onChange: (lf: number[]) => void;
}) {
  const frei = lernfelder.filter((l) => !gewaehlt.includes(l.nummer));
  return (
    <div className="bh-lf-chips">
      {gewaehlt.map((n) => (
        <button
          key={n}
          className="badge purple bh-lf-chip"
          onClick={() => onChange(gewaehlt.filter((x) => x !== n))}
          disabled={disabled}
          title={`${lfTitel(n, lernfelder)} – entfernen`}
          aria-label={`${lfKurz(n)} entfernen`}
        >
          {lfKurz(n)} <span aria-hidden>×</span>
        </button>
      ))}
      {frei.length > 0 && (
        <select
          className="input bh-lf-add"
          value=""
          onChange={(e) => {
            const n = Number(e.target.value);
            if (Number.isFinite(n) && e.target.value !== "") onChange([...gewaehlt, n]);
          }}
          disabled={disabled}
          aria-label="Lernfeld hinzufügen"
        >
          <option value="">+ LF</option>
          {frei.map((l) => (
            <option key={l.nummer} value={l.nummer} title={l.titel}>
              {lfKurz(l.nummer)} – {l.titel.replace(/^LF\s*0?\d+\s*[–—-]\s*/i, "")}
            </option>
          ))}
        </select>
      )}
      {gewaehlt.length === 0 && frei.length === 0 && <span className="lern-leer">—</span>}
    </div>
  );
}
