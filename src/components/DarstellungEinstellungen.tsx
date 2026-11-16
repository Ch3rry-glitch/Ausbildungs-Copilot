// Einstellungen → Darstellung: Farbthema und Glasstufe mit kleinen Live-Vorschauen.
// Jede Kachel trägt ihr eigenes data-theme/data-glas und zeigt so das echte Thema.
// Ein Klick gilt sofort und wird gleich gespeichert (kein eigener Speichern-Knopf).
import { useEffect, useRef, useState } from "react";
import { errorText } from "../api";
import { Icon } from "../icons";
import {
  anwenden,
  darstellungLesen,
  darstellungSpeichern,
  FARBTHEMEN,
  GLASSTUFEN,
  STANDARD,
  type Darstellung,
} from "../theme";

export function DarstellungEinstellungen() {
  const [d, setD] = useState<Darstellung | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  // zuletzt vom Backend bestätigte Wahl (dahin zurück, wenn Speichern scheitert)
  const bestaetigt = useRef<Darstellung | null>(null);
  // Speichervorgänge nacheinander, damit bei schnellem Klicken der letzte Klick gewinnt
  const kette = useRef<Promise<void>>(Promise.resolve());
  const nummer = useRef(0);

  useEffect(() => {
    let alive = true;
    darstellungLesen()
      .then((g) => {
        bestaetigt.current = g;
        if (alive) setD(g);
      })
      .catch((e) => alive && setMsg(errorText(e)));
    return () => {
      alive = false;
    };
  }, []);

  function waehle(patch: Partial<Darstellung>) {
    if (!d) return;
    const neu = { ...d, ...patch };
    if (neu.farbe === d.farbe && neu.glas === d.glas) return;
    setD(neu);
    anwenden(neu);
    setMsg(null);
    const nr = ++nummer.current;
    kette.current = kette.current.then(async () => {
      try {
        const g = await darstellungSpeichern(neu);
        bestaetigt.current = g;
        if (nr === nummer.current) {
          setD(g);
          anwenden(g);
        }
      } catch (e) {
        if (nr === nummer.current) {
          const zurueck = bestaetigt.current ?? STANDARD;
          setD(zurueck);
          anwenden(zurueck);
          setMsg(`Nicht gespeichert: ${errorText(e)}`);
        }
      }
    });
  }

  return (
    <section className="settings glass">
      <div>
        <div className="settings-title-row">
          <h2>Darstellung</h2>
        </div>
        <div className="hint">
          Farbthema und Durchsichtigkeit der Karten. Gilt sofort und bleibt gespeichert – in den App-Einstellungen, nicht
          im Vault.
        </div>
      </div>

      {d && (
        <>
          <div className="field">
            <span>Farbthema</span>
            <div className="dz-gruppe" role="radiogroup" aria-label="Farbthema">
              {FARBTHEMEN.map((t) => (
                <Kachel
                  key={t.value}
                  name="dz-farbe"
                  value={t.value}
                  label={t.label}
                  hint={t.hint}
                  aktiv={d.farbe === t.value}
                  theme={t.value}
                  glas={d.glas}
                  onWahl={() => waehle({ farbe: t.value })}
                />
              ))}
            </div>
          </div>

          <div className="field">
            <span>Glas</span>
            <div className="dz-gruppe" role="radiogroup" aria-label="Glas">
              {GLASSTUFEN.map((g) => (
                <Kachel
                  key={g.value}
                  name="dz-glas"
                  value={g.value}
                  label={g.label}
                  hint={g.hint}
                  aktiv={d.glas === g.value}
                  theme={d.farbe}
                  glas={g.value}
                  onWahl={() => waehle({ glas: g.value })}
                />
              ))}
            </div>
          </div>
        </>
      )}

      {msg && <div className="error">{msg}</div>}
    </section>
  );
}

function Kachel({
  name,
  value,
  label,
  hint,
  aktiv,
  theme,
  glas,
  onWahl,
}: {
  name: string;
  value: string;
  label: string;
  hint: string;
  aktiv: boolean;
  theme: string;
  glas: string;
  onWahl: () => void;
}) {
  return (
    <label className={`dz-kachel ${aktiv ? "aktiv" : ""}`}>
      <input type="radio" className="dz-radio" name={name} value={value} checked={aktiv} onChange={onWahl} />
      {/* Mini-Cockpit: Seitenleiste + Karte, gezeichnet mit den Farben dieses Themas */}
      <span className="dz-vorschau" data-theme={theme} data-glas={glas} aria-hidden="true">
        <span className="dz-seite">
          <i className="aktiv" />
          <i />
          <i />
          <i />
        </span>
        <span className="dz-karte">
          <span className="dz-titel">Heute</span>
          <span className="dz-zeile">
            2 Aufgaben · <b>GEC</b>
          </span>
          <span className="dz-zeile leise">Block 3 · 10:15</span>
          <span className="dz-fuss">
            <span className="dz-knopf">Neu</span>
            <span className="dz-balken">
              <i />
              <i />
              <i />
              <i className="hi" />
            </span>
          </span>
        </span>
      </span>
      <span className="dz-name">
        {label}
        {aktiv && Icon.check}
      </span>
      <span className="dz-hint">{hint}</span>
    </label>
  );
}
