// Reiter „Tage“: je Tag eine Karte mit Stichpunkten, Lücken (gelb) und Gesundheitsangaben (rot).
// Bearbeiten: Tagesart wählen; nur bei „Stichpunkte“ gibt es ein Textfeld (Regel 3 – kein Ort, an
// dem eine Krankheit eingetragen werden könnte). Lücken werden angezeigt, nie automatisch gefüllt (Regel 4).
import { useState } from "react";
import { api, errorText, type BerichtsTag, type BerichtsTagArt, type BerichtsWoche } from "../../api";
import { klartext } from "../../markdown";
import { Icon } from "../../icons";
import { useToast } from "../../toast";
import {
  ABSCHNITT_BADGE,
  ABSCHNITT_LABEL,
  anzahl,
  ART_LABEL,
  ARTEN,
  meldeAenderung,
  NachweisBadge,
  sondertagZeile,
  tagTitel,
  useSchreibFluss,
} from "./gemeinsam";

const tagKey = (t: BerichtsTag) => `${t.abschnitt}-${t.datum}`;

interface Bearbeitung {
  key: string;
  art: BerichtsTagArt;
  /** Stichpunkte, einer je Zeile – wird nur bei Art „Stichpunkte“ gezeigt und gesendet */
  text: string;
}

/** Zeilen des Textfelds → Stichpunkte (leere weg, ein getipptes „- “ am Anfang zählt nicht mit) */
function stichpunkteAus(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((z) => z.trim().replace(/^[-*+]\s+/, "").trim())
    .filter((z) => z !== "");
}

export function TageTab({
  woche,
  gesperrt,
  onGeschrieben,
  onOpenNote,
}: {
  woche: BerichtsWoche;
  /** Wochennotiz hat ungespeicherte Änderungen im Editor */
  gesperrt: boolean;
  onGeschrieben: () => void;
  onOpenNote: (path: string, line?: number) => void;
}) {
  const { notify } = useToast();
  const fluss = useSchreibFluss();
  const [bearbeitung, setBearbeitung] = useState<Bearbeitung | null>(null);
  const [speichert, setSpeichert] = useState(false);

  // Unterrichtsnotizen der Woche, die noch nicht verlinkt sind („10 Privat“ trägt das Backend nie ein)
  const offeneNotizen = woche.tage.flatMap((t) => t.unterricht.filter((n) => !n.verlinkt && !n.privat));

  function schultageUebernehmen() {
    void fluss.starten({
      aktion: "Unterrichtsnotizen in die Schultage eintragen",
      knopf: "In Wochennotiz schreiben",
      erfolg: "Schultage aus den Unterrichtsnotizen übernommen",
      vorschau: () => api.berichtsheftSchultage({ path: woche.path, vorschau: true, basis: null }),
      schreiben: (basis) => api.berichtsheftSchultage({ path: woche.path, vorschau: false, basis }),
      danach: onGeschrieben,
    });
  }

  function bearbeiten(t: BerichtsTag) {
    setBearbeitung({
      key: tagKey(t),
      art: t.art,
      text: t.art === "stichpunkte" ? t.stichpunkte.map((p) => p.text).join("\n") : "",
    });
  }

  async function speichern(t: BerichtsTag) {
    const b = bearbeitung;
    if (!b || b.key !== tagKey(t) || speichert) return;
    setSpeichert(true);
    try {
      const r = await api.berichtsheftTagSpeichern({
        path: woche.path,
        abschnitt: t.abschnitt,
        datum: t.datum,
        art: b.art,
        // Regel 3: Sondertage gehen immer ohne Text hinaus
        stichpunkte: b.art === "stichpunkte" ? stichpunkteAus(b.text) : [],
        basis: woche.version,
      });
      meldeAenderung(notify, `${tagTitel(t.wochentag, t.datum)} (${ABSCHNITT_LABEL[t.abschnitt]}) gespeichert`, r);
      setBearbeitung(null);
      onGeschrieben();
    } catch (e) {
      notify({ kind: "error", text: errorText(e) }); // Eingabe bleibt stehen
    } finally {
      setSpeichert(false);
    }
  }

  return (
    <div className="bh-tab">
      {offeneNotizen.length > 0 && (
        <div className="lb-ki bh-toolbar">
          <div className="lb-ki-line">
            {anzahl(offeneNotizen.length, "Unterrichtsnotiz steht", "Unterrichtsnotizen stehen")} noch nicht in der Wochennotiz:{" "}
            <strong>{offeneNotizen.map((n) => n.title).join(", ")}</strong>
          </div>
          <div className="row">
            <button className="btn small primary" onClick={schultageUebernehmen} disabled={gesperrt || fluss.laeuft}>
              {Icon.unterricht} Aus Unterrichtsnotizen übernehmen
            </button>
            {fluss.laeuft && (
              <span className="lb-busy">
                <span className="spinner" /> Vorschau …
              </span>
            )}
          </div>
          <div className="hint">Eigener Text in den Schultagen bleibt unverändert – du siehst vorher genau, welche Zeilen dazukommen.</div>
        </div>
      )}

      {woche.tage.length === 0 && <div className="hint pad">In dieser Wochennotiz gibt es keine Tage unter „Berufsschule“ oder „Betrieb“.</div>}

      {woche.tage.map((t) => (
        <TagKarte
          key={tagKey(t)}
          t={t}
          wochePath={woche.path}
          bearbeitung={bearbeitung?.key === tagKey(t) ? bearbeitung : null}
          dirty={gesperrt}
          belegt={speichert || (bearbeitung !== null && bearbeitung.key !== tagKey(t))}
          speichert={speichert}
          onBearbeiten={() => bearbeiten(t)}
          onAendern={(b) => setBearbeitung(b)}
          onAbbrechen={() => setBearbeitung(null)}
          onSpeichern={() => void speichern(t)}
          onOpenNote={onOpenNote}
        />
      ))}

      {fluss.dialog}
    </div>
  );
}

function TagKarte({
  t,
  wochePath,
  bearbeitung,
  dirty,
  belegt,
  speichert,
  onBearbeiten,
  onAendern,
  onAbbrechen,
  onSpeichern,
  onOpenNote,
}: {
  t: BerichtsTag;
  wochePath: string;
  bearbeitung: Bearbeitung | null;
  /** Wochennotiz im Editor ungespeichert → nichts schreiben */
  dirty: boolean;
  /** ein anderer Tag wird gerade bearbeitet oder gespeichert */
  belegt: boolean;
  speichert: boolean;
  onBearbeiten: () => void;
  onAendern: (b: Bearbeitung) => void;
  onAbbrechen: () => void;
  onSpeichern: () => void;
  onOpenNote: (path: string, line?: number) => void;
}) {
  return (
    <div className={`lb-section bh-tag ${t.fehlt ? "fehlt" : ""}`}>
      <div className="bh-tag-head">
        <span className={`badge ${ABSCHNITT_BADGE[t.abschnitt]}`}>{ABSCHNITT_LABEL[t.abschnitt]}</span>
        <span className="bh-tag-titel">{tagTitel(t.wochentag, t.datum)}</span>
        {t.fehlt && <span className="badge amber">fehlt</span>}
        {t.art !== "stichpunkte" && <span className="badge lilac">{ART_LABEL[t.art]}</span>}
        {t.ueberschriftZeile == null && (
          <span className="badge muted" title="Die Tagesüberschrift steht noch nicht in der Wochennotiz – beim Speichern wird sie angelegt.">
            ohne Überschrift
          </span>
        )}
        {/* rechts als Gruppe – bricht bei schmaler Karte gemeinsam um und bleibt rechtsbündig */}
        <span className="bh-tag-aktionen">
          <NachweisBadge stand={t.nachweis} />
          {t.bearbeitbar && !bearbeitung && (
            <button
              className="btn small"
              onClick={onBearbeiten}
              disabled={dirty || belegt}
              title={dirty ? "Erst die Wochennotiz im Editor speichern" : belegt ? "Erst den anderen Tag speichern oder abbrechen" : "Tagesart und Stichpunkte bearbeiten"}
            >
              {Icon.edit} Bearbeiten
            </button>
          )}
        </span>
      </div>

      {t.hinweis && (
        <div className="hint lb-hinweis">
          {Icon.warning}
          <span>{t.hinweis}</span>
        </div>
      )}

      {bearbeitung ? (
        <TagEditor t={t} b={bearbeitung} dirty={dirty} speichert={speichert} onAendern={onAendern} onAbbrechen={onAbbrechen} onSpeichern={onSpeichern} />
      ) : (
        <ul className="bh-punkte">
          {t.stichpunkte.length === 0 && <li className="hint">Noch keine Stichpunkte.</li>}
          {t.stichpunkte.map((p) => (
            <li key={p.line} className={`bh-punkt ${p.diagnose ? "diagnose" : p.luecke ? "luecke" : ""}`}>
              <span className="bh-punkt-text">{klartext(p.text) || p.text}</span>
              {p.diagnose ? (
                <span className="bh-punkt-hinweis">
                  {Icon.warning} Gesundheitsangabe – bitte in Obsidian entfernen
                  <button className="lb-link" onClick={() => onOpenNote(wochePath, p.line)} title={`${wochePath}, Zeile ${p.line}`}>
                    Zeile {p.line}
                  </button>
                </span>
              ) : (
                p.luecke && (
                  <span className="bh-punkt-hinweis">
                    Gegenstand fehlt – was genau?
                    <button className="lb-link" onClick={() => onOpenNote(wochePath, p.line)} title={`${wochePath}, Zeile ${p.line}`}>
                      Zeile {p.line}
                    </button>
                  </span>
                )
              )}
            </li>
          ))}
        </ul>
      )}

      {t.abschnitt === "berufsschule" && t.unterricht.length > 0 && (
        <div className="lb-quellen bh-unterricht">
          <span className="lb-label">Unterrichtsnotizen</span>
          {t.unterricht.map((n) => (
            <span key={n.path} className="lb-quelle">
              <button className="lb-link" onClick={() => onOpenNote(n.path)} title={n.path}>
                {Icon.datei}
                {n.title}
              </button>
              {n.verlinkt ? <span className="badge mint">verknüpft</span> : <span className="badge amber">nicht eingetragen</span>}
              {n.privat && <span className="badge muted">10 Privat</span>}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function TagEditor({
  t,
  b,
  dirty,
  speichert,
  onAendern,
  onAbbrechen,
  onSpeichern,
}: {
  t: BerichtsTag;
  b: Bearbeitung;
  dirty: boolean;
  speichert: boolean;
  onAendern: (b: Bearbeitung) => void;
  onAbbrechen: () => void;
  onSpeichern: () => void;
}) {
  const diagnosen = b.art === "stichpunkte" ? t.stichpunkte.filter((p) => p.diagnose) : [];
  const zeilen = b.text.split("\n").length;
  return (
    <div className="bh-editor">
      <div className="field">
        <span>Tagesart</span>
        <div className="pills bh-arten" role="radiogroup" aria-label="Tagesart">
          {ARTEN.map((a) => (
            <button
              key={a}
              className={`pill ${b.art === a ? "active" : ""}`}
              role="radio"
              aria-checked={b.art === a}
              onClick={() => onAendern({ ...b, art: a })}
              disabled={speichert}
            >
              {ART_LABEL[a]}
            </button>
          ))}
        </div>
      </div>

      {b.art === "stichpunkte" ? (
        <label className="field">
          <span>Stichpunkte – einer je Zeile</span>
          <textarea
            className="input bh-textarea"
            rows={Math.min(14, Math.max(3, zeilen + 1))}
            value={b.text}
            onChange={(e) => onAendern({ ...b, text: e.target.value })}
            disabled={speichert}
            spellCheck
            autoFocus
            placeholder={t.abschnitt === "betrieb" ? "z. B. Artikelbeschreibungen für die Herbstkollektion im Shop angelegt" : "z. B. **GEC** — Handelskalkulation"}
          />
        </label>
      ) : (
        <div className="hint bh-sondertag">
          In der Wochennotiz steht dann nur: <strong>{sondertagZeile(b.art, t.abschnitt)}</strong>
          {b.art === "krank" && " – ohne Grund, ohne weitere Angaben."}
        </div>
      )}

      {diagnosen.map((p) => (
        <div key={p.line} className="hint bh-diagnose-hinweis">
          {Icon.warning} „{p.diagnose}“ ist eine Gesundheitsangabe – bitte die Zeile entfernen; im Berichtsheft steht nur
          „arbeitsunfähig erkrankt“ (Tagesart wählen).
        </div>
      ))}

      <div className="row">
        <button className="btn small" onClick={onAbbrechen} disabled={speichert}>
          Abbrechen
        </button>
        <button className="btn small primary" onClick={onSpeichern} disabled={speichert || dirty} title={dirty ? "Erst die Wochennotiz im Editor speichern" : undefined}>
          {Icon.save} {speichert ? "Wird gespeichert …" : "Speichern"}
        </button>
      </div>
    </div>
  );
}
