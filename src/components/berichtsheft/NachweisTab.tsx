// Reiter „Nachweis“: Fließtext je Tag aus den Stichpunkten – ohne KI (nur umgeformt) oder mit KI
// (nur auf Klick). Lücken stehen als gelbe, nicht editierbare Hinweise da und werden so in die
// Wochennotiz geschrieben (Regel 4). Geschrieben wird nur nach Diff und Bestätigung (Regel 9).
// Der Entwurf lebt nur im Zustand dieser Ansicht.
import { useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  kiZeile,
  type BerichtsWoche,
  type KiStatus,
  type NachweisEntwurf,
  type NachweisQuelle,
  type NachweisTag,
} from "../../api";
import { Icon } from "../../icons";
import {
  ABSCHNITT_BADGE,
  ABSCHNITT_LABEL,
  anzahl,
  Hinweise,
  KiFehltHinweis,
  NachweisBadge,
  neueId,
  tagTitel,
  useSchreibFluss,
} from "./gemeinsam";

interface EditAbsatz {
  id: number;
  text: string;
}
interface EditTag {
  key: string;
  quelle: NachweisTag;
  uebernehmen: boolean;
  ueberschrift: string;
  absaetze: EditAbsatz[];
  /** in dieser Sitzung schon in die Wochennotiz geschrieben */
  geschrieben: boolean;
}

const tagKey = (t: NachweisTag) => `${t.abschnitt}-${t.datum}`;

function zuEdit(t: NachweisTag): EditTag {
  return {
    key: tagKey(t),
    quelle: t,
    uebernehmen: t.uebernehmen,
    ueberschrift: t.ueberschrift,
    absaetze: t.absaetze.map((text) => ({ id: neueId(), text })),
    geschrieben: false,
  };
}

/** Genau die Felder von `NachweisTag`; Lücken gehen unverändert zurück. */
function zuNachweisTag(t: EditTag): NachweisTag {
  const q = t.quelle;
  return {
    datum: q.datum,
    abschnitt: q.abschnitt,
    wochentag: q.wochentag,
    ueberschrift: t.ueberschrift.trim(),
    absaetze: t.absaetze.map((a) => a.text.trim()).filter((a) => a !== ""),
    luecken: q.luecken,
    vorhanden: q.vorhanden,
    uebernehmen: true,
    ki: q.ki,
  };
}

export function NachweisTab({
  woche,
  gesperrt,
  kiStatus,
  onGeschrieben,
  onOpenNote,
  onGoSettings,
}: {
  woche: BerichtsWoche;
  gesperrt: boolean;
  kiStatus: KiStatus | null;
  onGeschrieben: () => void;
  onOpenNote: (path: string) => void;
  onGoSettings: () => void;
}) {
  const fluss = useSchreibFluss();
  const [entwurf, setEntwurf] = useState<NachweisEntwurf | null>(null);
  const [tage, setTage] = useState<EditTag[]>([]);
  // Was mit KI gesendet würde – aus dem letzten Entwurf ohne KI
  const [quellenOhneKi, setQuellenOhneKi] = useState<NachweisQuelle[] | null>(null);
  const [laedt, setLaedt] = useState<null | "ohne" | "ki">(null);
  const [fehler, setFehler] = useState<string | null>(null);
  const [privatErlauben, setPrivatErlauben] = useState(false);
  const seq = useRef(0);

  const kiOk = !!kiStatus?.schluesselGespeichert;
  const quellen = quellenOhneKi ?? [];
  const hatPrivat = quellen.some((q) => q.privat);
  const privatAn = privatErlauben && hatPrivat;
  const wuerdeGesendet = quellen.filter((q) => q.gesendet || (q.privat && privatAn));

  async function erzeuge(mitKi: boolean) {
    seq.current += 1;
    const mine = seq.current;
    const privat = mitKi && privatAn;
    if (mitKi) setPrivatErlauben(false); // Regel 10: „10 Privat/“ nur für genau diesen Vorgang
    setLaedt(mitKi ? "ki" : "ohne");
    setFehler(null);
    try {
      const e = await api.berichtsheftNachweis({ path: woche.path, mitKi, privatErlauben: privat });
      if (mine !== seq.current) return;
      setEntwurf(e);
      setTage(e.tage.map(zuEdit));
      if (!e.mitKi) setQuellenOhneKi(e.quellen);
    } catch (err) {
      if (mine !== seq.current) return;
      setFehler(errorText(err)); // ein vorhandener Entwurf bleibt stehen
    } finally {
      if (mine === seq.current) setLaedt(null);
    }
  }

  // Beim ersten Öffnen: Entwurf ohne KI (zeigt auch, was mit KI gesendet würde)
  const erzeugeRef = useRef(erzeuge);
  erzeugeRef.current = erzeuge;
  useEffect(() => {
    void erzeugeRef.current(false);
  }, []);

  function aendereTag(key: string, f: (t: EditTag) => EditTag) {
    setTage((ts) => ts.map((t) => (t.key === key ? f(t) : t)));
  }

  const gewaehlt = tage.filter((t) => t.uebernehmen);
  const busy = laedt !== null || fluss.laeuft;

  function vorschau() {
    if (gewaehlt.length === 0) return;
    const zuSchreiben = gewaehlt.map(zuNachweisTag);
    const keys = new Set(gewaehlt.map((t) => t.key));
    void fluss.starten({
      aktion: "Ausformulierten Nachweis in die Wochennotiz schreiben",
      knopf: "In Wochennotiz schreiben",
      erfolg: `Nachweis geschrieben (${anzahl(zuSchreiben.length, "Tag", "Tage")})`,
      vorschau: () => api.berichtsheftNachweisSchreiben({ path: woche.path, tage: zuSchreiben, vorschau: true, basis: null }),
      schreiben: (basis) => api.berichtsheftNachweisSchreiben({ path: woche.path, tage: zuSchreiben, vorschau: false, basis }),
      danach: (r) => {
        if (r.report) {
          // geschriebene Tage: jetzt „vorhanden“, nicht noch einmal vorschlagen
          setTage((ts) =>
            ts.map((t) =>
              keys.has(t.key) ? { ...t, uebernehmen: false, geschrieben: true, quelle: { ...t.quelle, vorhanden: "vorhanden" } } : t,
            ),
          );
        }
        onGeschrieben();
      },
    });
  }

  return (
    <div className="bh-tab">
      <div className="lb-ki">
        <div className="lb-ki-line">
          Mit KI würden gesendet:{" "}
          <strong>{quellenOhneKi === null ? "…" : wuerdeGesendet.length > 0 ? wuerdeGesendet.map((q) => q.title).join(", ") : "—"}</strong>
        </div>
        {quellen.length > 0 && (
          <div className="lb-quellen">
            {quellen.map((q) => (
              <span key={q.path} className="lb-quelle">
                <button className="lb-link" onClick={() => onOpenNote(q.path)} title={q.path}>
                  {Icon.datei}
                  {q.title}
                </button>
                {q.privat && <span className="badge muted">privat</span>}
                {q.ekEntfernt && <span className="badge muted">EK entfernt</span>}
                {!(q.gesendet || (q.privat && privatAn)) && <span className="badge muted">wird nicht gesendet</span>}
              </span>
            ))}
          </div>
        )}
        <div className="hint">
          Nur die Stichpunkte, Fach-Zeilen und Unterrichtsnotizen der Schultage – keine Sondertage, keine Gesundheitsangaben; EK-Angaben
          werden vorher entfernt · „10 Privat/“ {privatAn ? "wird diesmal mitgeschickt" : "bleibt aus"}
        </div>
        {hatPrivat && (
          <label className="check">
            <input type="checkbox" checked={privatErlauben} onChange={(e) => setPrivatErlauben(e.target.checked)} disabled={busy} />
            „10 Privat“ für diesen Vorgang erlauben
          </label>
        )}
        <div className="row lb-ki-buttons">
          <button className="btn small primary" onClick={() => void erzeuge(true)} disabled={!kiOk || busy || quellenOhneKi === null}>
            {Icon.sparkle} Mit KI formulieren
          </button>
          <button className="btn small" onClick={() => void erzeuge(false)} disabled={busy}>
            Ohne KI formulieren
          </button>
          {laedt === "ki" && (
            <span className="lb-busy">
              <span className="spinner" /> KI formuliert …
            </span>
          )}
          {laedt === "ohne" && (
            <span className="lb-busy">
              <span className="spinner" /> Entwurf wird erstellt …
            </span>
          )}
        </div>
        <KiFehltHinweis kiStatus={kiStatus} onGoSettings={onGoSettings} ohne="Ohne KI formulieren" />
        {entwurf?.ki && (
          <div className="hint lb-ki-kosten">
            {kiZeile(entwurf.ki, kiStatus?.modelle)}
            {entwurf.mitKi && ` · gesendet: ${entwurf.quellen.filter((q) => q.gesendet).map((q) => q.title).join(", ") || "—"}`}
          </div>
        )}
      </div>

      {fehler && <div className="error">{fehler}</div>}

      {entwurf && (
        <>
          <Hinweise texte={entwurf.warnungen} />
          {entwurf.fehlendeTage.length > 0 && (
            <div className="hint lb-hinweis">
              {Icon.warning}
              <span>Ohne Inhalt, daher nicht formuliert: {entwurf.fehlendeTage.join(", ")} – erst im Reiter „Tage“ Stichpunkte eintragen.</span>
            </div>
          )}
          {tage.length === 0 && <div className="hint pad">Kein Tag mit Inhalt – nichts zu formulieren.</div>}

          {tage.map((t) => (
            <NachweisKarte key={t.key} t={t} gesperrt={busy} onChange={(f) => aendereTag(t.key, f)} />
          ))}

          {tage.length > 0 && (
            <div className="lb-actions">
              <div className="row">
                <button className="btn primary" onClick={vorschau} disabled={gesperrt || busy || gewaehlt.length === 0}>
                  {Icon.diff} Vorschau ({anzahl(gewaehlt.length, "Tag", "Tage")})
                </button>
                {fluss.laeuft && (
                  <span className="lb-busy">
                    <span className="spinner" /> Vorschau …
                  </span>
                )}
              </div>
              <div className="hint">Die Vorschau zeigt genau, was sich in der Wochennotiz ändert – geschrieben wird erst danach.</div>
            </div>
          )}
        </>
      )}

      {fluss.dialog}
    </div>
  );
}

function NachweisKarte({ t, gesperrt, onChange }: { t: EditTag; gesperrt: boolean; onChange: (f: (t: EditTag) => EditTag) => void }) {
  const q = t.quelle;
  const label = tagTitel(q.wochentag, q.datum);
  return (
    <div className={`lb-section bh-tag ${t.uebernehmen ? "" : "aus"}`}>
      <div className="bh-tag-head">
        <label className="check">
          <input
            type="checkbox"
            checked={t.uebernehmen}
            onChange={(e) => onChange((x) => ({ ...x, uebernehmen: e.target.checked }))}
            disabled={gesperrt}
          />
          übernehmen
        </label>
        <span className={`badge ${ABSCHNITT_BADGE[q.abschnitt]}`}>{ABSCHNITT_LABEL[q.abschnitt]}</span>
        <span className="bh-tag-titel">{label}</span>
        {q.ki && (
          <span className="badge mint" title="Von der KI formuliert">
            {Icon.sparkle} KI
          </span>
        )}
        {t.geschrieben && <span className="badge mint">{Icon.check} geschrieben</span>}
        <span className="filter-spacer" />
        {t.uebernehmen && q.vorhanden === "vorhanden" && <span className="hint lb-warn-text">ersetzt vorhandenen Text</span>}
        <NachweisBadge stand={q.vorhanden} />
      </div>

      {q.abschnitt === "betrieb" && (
        <label className="field">
          <span>Überschrift</span>
          <input
            className="input"
            value={t.ueberschrift}
            onChange={(e) => onChange((x) => ({ ...x, ueberschrift: e.target.value }))}
            disabled={gesperrt}
            placeholder="ohne Überschrift"
            spellCheck
          />
        </label>
      )}

      {t.absaetze.map((a, i) => (
        <div key={a.id} className="lb-punkt">
          <textarea
            className="input lb-punkt-input bh-absatz"
            rows={2}
            value={a.text}
            onChange={(e) => onChange((x) => ({ ...x, absaetze: x.absaetze.map((y) => (y.id === a.id ? { ...y, text: e.target.value } : y)) }))}
            disabled={gesperrt}
            spellCheck
            aria-label={`${label}: Absatz ${i + 1}`}
          />
          <button
            className="lb-remove"
            onClick={() => onChange((x) => ({ ...x, absaetze: x.absaetze.filter((y) => y.id !== a.id) }))}
            title="Absatz entfernen"
            aria-label="Absatz entfernen"
            disabled={gesperrt}
          >
            ×
          </button>
        </div>
      ))}
      <button
        className="btn small lb-add"
        onClick={() => onChange((x) => ({ ...x, absaetze: [...x.absaetze, { id: neueId(), text: "" }] }))}
        disabled={gesperrt}
      >
        + Absatz
      </button>

      {q.luecken.map((l, i) => (
        <div key={i} className="bh-luecke" title="Lücke – wird so in die Wochennotiz geschrieben">
          ⟨Lücke: „{l}“ – Gegenstand fehlt⟩
        </div>
      ))}
      {q.luecken.length > 0 && (
        <div className="hint">Lücken lassen sich hier nicht füllen – ergänze den Stichpunkt im Reiter „Tage“ und formuliere neu.</div>
      )}
    </div>
  );
}
