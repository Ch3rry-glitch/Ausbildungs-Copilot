import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type AbschnittQuelle, type AppStatus, type KiModell, type KiStatus, type LogbuchFach } from "../api";
import { Icon } from "../icons";

export function SettingsView({
  status,
  onStatus,
  editorDirty,
  onLogbuchDirty,
}: {
  status: AppStatus;
  onStatus: (s: AppStatus) => void;
  /** offene Notiz mit ungespeicherten Änderungen → Vault-Knöpfe gesperrt */
  editorDirty: boolean;
  /** ungespeicherte Logbuch-Einstellungen (App fragt vor dem Verlassen) */
  onLogbuchDirty?: (b: boolean) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  async function run(f: () => Promise<AppStatus>, okText: string) {
    setBusy(true);
    setMsg(null);
    try {
      const s = await f();
      onStatus(s);
      setMsg({ ok: true, text: okText.replace("{n}", String(s.noteCount)) });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  }

  async function changeVault() {
    const dir = await open({ directory: true, title: "Obsidian-Vault auswählen" });
    if (typeof dir === "string") await run(() => api.setVault(dir), "Vault gewechselt – {n} Notizen eingelesen.");
  }

  return (
    <>
      <div className="page-head">
        <div>
          <h1 className="page-title">Einstellungen</h1>
          <div className="page-sub">Einstellungen liegen im App-Datenordner, nie im Vault.</div>
        </div>
      </div>
      <div className="settings-scroll">
        <section className="settings glass">
          <div>
            <h2>Vault</h2>
            <dl className="kv">
              <dt>Ordner</dt>
              <dd>{status.vaultPath}</dd>
              <dt>Name in Obsidian</dt>
              <dd>{status.vaultName}</dd>
              <dt>Notizen im Index</dt>
              <dd>{status.noteCount}</dd>
              <dt>Dateiwächter</dt>
              <dd>{status.watching ? "aktiv" : "aus"}</dd>
            </dl>
          </div>
          <div className="row">
            <button className="btn" onClick={changeVault} disabled={busy || editorDirty}>
              {Icon.ordnerOeffnen} Vault wechseln
            </button>
            <button className="btn" onClick={() => run(api.reindex, "Neu eingelesen – {n} Notizen.")} disabled={busy || editorDirty}>
              {Icon.refresh} Neu einlesen
            </button>
          </div>
          {editorDirty && <div className="hint">Erst die offene Notiz speichern.</div>}
          {msg && <div className={msg.ok ? "result" : "error"}>{msg.text}</div>}
          <div>
            <h2>App</h2>
            <dl className="kv">
              <dt>Version</dt>
              <dd>{status.version}</dd>
              <dt>App-Datenordner</dt>
              <dd>{status.configDir}</dd>
            </dl>
          </div>
        </section>

        {/* neu laden, wenn der Vault wechselt (Fächer kommen aus Stundenplan und „01 Fächer“) */}
        <LogbuchEinstellungen key={status.vaultPath ?? ""} onDirty={onLogbuchDirty} />
        <KiEinstellungen />
      </div>
    </>
  );
}

// ---------- Logbuch je Fach ----------

const QUELLEN: { value: AbschnittQuelle; label: string }[] = [
  { value: "erarbeitet", label: "Dies habe ich erarbeitet (Inhalt, Merksätze, Begriffe)" },
  { value: "fragen", label: "Fragen (#frage, Noch nicht verstanden)" },
  { value: "naechstes", label: "Als Nächstes (offene Aufgaben, Klausuren)" },
  { value: "leer", label: "leer lassen" },
];

const STANDARD_SKALA = 10;

/** Vor dem Speichern: Leerzeichen am Rand weg (die Abschnittstitel müssen später exakt passen). */
function bereinigt(f: LogbuchFach): LogbuchFach {
  return {
    ...f,
    moodleUrl: f.moodleUrl.trim(),
    bewertungTitel: f.bewertungTitel.trim(),
    abschnitte: f.abschnitte.map((a) => ({ ...a, titel: a.titel.trim() })),
  };
}

/** Schnelle Prüfung vor dem Senden; die verbindliche Prüfung macht das Backend. */
function pruefe(faecher: LogbuchFach[]): string | null {
  for (const f of faecher) {
    if (f.abschnitte.some((a) => a.titel.trim() === "")) return `${f.kurz}: Jeder Abschnitt braucht einen Titel.`;
  }
  return null;
}

const MAX_ABSCHNITTE = 8;

function LogbuchEinstellungen({ onDirty }: { onDirty?: (b: boolean) => void }) {
  const [gespeichert, setGespeichert] = useState<LogbuchFach[] | null>(null);
  const [faecher, setFaecher] = useState<LogbuchFach[] | null>(null);
  const [offen, setOffen] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  // zuletzt gewählte Skala je Fach, damit Aus- und wieder Einschalten sie nicht vergisst
  const skalaMerk = useRef(new Map<string, number>());

  useEffect(() => {
    let alive = true;
    api
      .logbuchEinstellungen()
      .then((l) => {
        if (!alive) return;
        setGespeichert(l);
        setFaecher(l);
        setLoadError(null);
      })
      .catch((e) => alive && setLoadError(errorText(e)));
    return () => {
      alive = false;
    };
  }, []);

  const geaendert = useMemo(() => JSON.stringify(faecher) !== JSON.stringify(gespeichert), [faecher, gespeichert]);
  // Zahlenfelder mit ungültiger Eingabe (Schlüssel „Fach:Feld“) – Speichern bleibt gesperrt
  const [ungueltig, setUngueltig] = useState<string[]>([]);
  const meldeGueltig = useCallback((key: string, ok: boolean) => {
    setUngueltig((u) => (ok ? (u.includes(key) ? u.filter((k) => k !== key) : u) : u.includes(key) ? u : [...u, key]));
  }, []);

  const dirtyMelden = useRef(onDirty);
  dirtyMelden.current = onDirty;
  useEffect(() => {
    dirtyMelden.current?.(geaendert || ungueltig.length > 0);
  }, [geaendert, ungueltig]);
  useEffect(() => () => dirtyMelden.current?.(false), []);

  function aendere(kurz: string, patch: Partial<LogbuchFach>) {
    setFaecher((fs) => fs && fs.map((f) => (f.kurz === kurz ? { ...f, ...patch } : f)));
    setMsg(null);
  }

  function bewertungUmschalten(f: LogbuchFach, an: boolean) {
    if (an) {
      aendere(f.kurz, { selbstbewertung: skalaMerk.current.get(f.kurz) ?? STANDARD_SKALA });
    } else {
      if (f.selbstbewertung != null) skalaMerk.current.set(f.kurz, f.selbstbewertung);
      aendere(f.kurz, { selbstbewertung: null });
    }
  }

  async function speichern() {
    if (!faecher || busy) return;
    if (ungueltig.length > 0) {
      setMsg({ ok: false, text: "Bitte zuerst die markierten Zahlen korrigieren." });
      return;
    }
    const fehler = pruefe(faecher);
    if (fehler) {
      setMsg({ ok: false, text: fehler });
      return;
    }
    setBusy(true);
    setMsg(null);
    try {
      const l = await api.logbuchEinstellungenSpeichern(faecher.map(bereinigt));
      setGespeichert(l);
      setFaecher(l);
      setMsg({ ok: true, text: "Logbuch-Einstellungen gespeichert." });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="settings glass">
      <div>
        <div className="settings-title-row">
          <h2>Logbuch je Fach</h2>
          {geaendert && <span className="badge amber">● ungespeichert</span>}
        </div>
        <div className="hint">
          Für welche Fächer die App Logbucheinträge für Moodle erzeugt – und wie ein Eintrag aufgebaut ist.
        </div>
      </div>

      {loadError && <div className="error">{loadError}</div>}
      {faecher && faecher.length === 0 && <div className="hint">Keine Fächer gefunden (Stundenplan oder „01 Fächer“).</div>}

      {faecher && faecher.length > 0 && (
        <div className="lb-faecher">
          {faecher.map((f) => {
            const auf = offen === f.kurz;
            return (
              <div key={f.kurz} className={`lb-fach ${auf ? "open" : ""}`}>
                <div className="lb-fach-head">
                  <strong className="lb-fach-kurz">{f.kurz}</strong>
                  <label className="check">
                    <input type="checkbox" checked={f.aktiv} onChange={(e) => aendere(f.kurz, { aktiv: e.target.checked })} />
                    Logbuch aktiv
                  </label>
                  <span className="lb-fach-sum">
                    {f.abschnitte.length} {f.abschnitte.length === 1 ? "Abschnitt" : "Abschnitte"}
                    {f.selbstbewertung != null ? ` · Bewertung 1–${f.selbstbewertung}` : ""}
                  </span>
                  <button className="btn small" onClick={() => setOffen(auf ? null : f.kurz)} aria-expanded={auf}>
                    <span className={`chevron ${auf ? "up" : ""}`}>{Icon.chevronDown}</span>
                    {auf ? "Zuklappen" : "Bearbeiten"}
                  </button>
                </div>
                {auf && (
                  <FachEditor
                    f={f}
                    onChange={(patch) => aendere(f.kurz, patch)}
                    onBewertung={(an) => bewertungUmschalten(f, an)}
                    onGueltig={(feld, ok) => meldeGueltig(`${f.kurz}:${feld}`, ok)}
                  />
                )}
              </div>
            );
          })}
        </div>
      )}

      {faecher && (
        <div className="row">
          <button className="btn primary" onClick={() => void speichern()} disabled={busy || !geaendert || ungueltig.length > 0}>
            {Icon.save} {busy ? "Speichert …" : "Speichern"}
          </button>
        </div>
      )}
      {msg && <div className={msg.ok ? "result" : "error"}>{msg.text}</div>}
    </section>
  );
}

function FachEditor({
  f,
  onChange,
  onBewertung,
  onGueltig,
}: {
  f: LogbuchFach;
  onChange: (patch: Partial<LogbuchFach>) => void;
  onBewertung: (an: boolean) => void;
  onGueltig: (feld: string, ok: boolean) => void;
}) {
  const abschnitte = f.abschnitte;
  const setAbschnitte = (a: typeof abschnitte) => onChange({ abschnitte: a });
  const verschiebe = (i: number, d: -1 | 1) => {
    const j = i + d;
    if (j < 0 || j >= abschnitte.length) return;
    const neu = [...abschnitte];
    [neu[i], neu[j]] = [neu[j], neu[i]];
    setAbschnitte(neu);
  };
  const mitBewertung = f.selbstbewertung != null;

  return (
    <div className="lb-fach-editor">
      <label className="field span2">
        <span>Moodle-Adresse</span>
        <input
          className="input"
          value={f.moodleUrl}
          onChange={(e) => onChange({ moodleUrl: e.target.value })}
          placeholder="https://www.lebk-moodle.de/course/view.php?id=…"
          spellCheck={false}
          autoComplete="off"
        />
      </label>

      <div className="field span2">
        <span>Abschnitte</span>
        {abschnitte.length === 0 && <div className="hint">Noch keine Abschnitte.</div>}
        {abschnitte.map((a, i) => (
          <div key={i} className="abschnitt-row">
            <input
              className="input"
              value={a.titel}
              onChange={(e) => setAbschnitte(abschnitte.map((x, k) => (k === i ? { ...x, titel: e.target.value } : x)))}
              placeholder="Titel, z. B. Dies habe ich erarbeitet"
              aria-label={`Abschnitt ${i + 1}: Titel`}
            />
            <select
              className="input"
              value={a.quelle}
              onChange={(e) =>
                setAbschnitte(abschnitte.map((x, k) => (k === i ? { ...x, quelle: e.target.value as AbschnittQuelle } : x)))
              }
              aria-label={`Abschnitt ${i + 1}: Quelle`}
            >
              {QUELLEN.map((q) => (
                <option key={q.value} value={q.value}>
                  {q.label}
                </option>
              ))}
            </select>
            <button className="btn small icon-only" onClick={() => verschiebe(i, -1)} disabled={i === 0} title="Nach oben" aria-label="Nach oben">
              ↑
            </button>
            <button
              className="btn small icon-only"
              onClick={() => verschiebe(i, 1)}
              disabled={i === abschnitte.length - 1}
              title="Nach unten"
              aria-label="Nach unten"
            >
              ↓
            </button>
            <button
              className="btn small icon-only"
              onClick={() => setAbschnitte(abschnitte.filter((_, k) => k !== i))}
              disabled={abschnitte.length <= 1}
              title="Abschnitt entfernen"
              aria-label="Abschnitt entfernen"
            >
              ×
            </button>
          </div>
        ))}
        <div>
          <button
            className="btn small"
            onClick={() => setAbschnitte([...abschnitte, { titel: "", quelle: "leer" }])}
            disabled={abschnitte.length >= MAX_ABSCHNITTE}
          >
            + Abschnitt
          </button>
        </div>
      </div>

      <div className="field">
        <span>Selbstbewertung</span>
        <div className="inline-fields">
          <label className="check">
            <input type="checkbox" checked={mitBewertung} onChange={(e) => onBewertung(e.target.checked)} />
            verlangt
          </label>
          <label className="check">
            Skala 1 bis
            <NumField
              value={f.selbstbewertung ?? STANDARD_SKALA}
              min={2}
              max={100}
              disabled={!mitBewertung}
              onChange={(n) => onChange({ selbstbewertung: n })}
              onGueltig={(ok) => onGueltig("skala", ok)}
              ariaLabel="Skala bis"
            />
          </label>
        </div>
      </div>

      <label className="field">
        <span>Titel der Bewertung</span>
        <input
          className="input"
          value={f.bewertungTitel}
          onChange={(e) => onChange({ bewertungTitel: e.target.value })}
          disabled={!mitBewertung}
          placeholder="Bewertung dieser Lerneinheit"
        />
        <span className="hint">leer = „Bewertung dieser Lerneinheit“</span>
      </label>

      <label className="field">
        <span>Einträge pro Woche</span>
        <NumField
          value={f.eintraegeProWoche}
          min={0}
          max={7}
          onChange={(n) => onChange({ eintraegeProWoche: n })}
          onGueltig={(ok) => onGueltig("woche", ok)}
        />
        <span className="hint">0 = kein Wochenziel</span>
      </label>

      <label className="field">
        <span>Umlaute</span>
        <select className="input" value={f.umlaute} onChange={(e) => onChange({ umlaute: e.target.value as LogbuchFach["umlaute"] })}>
          <option value="erhalten">erhalten</option>
          <option value="umschreiben">umschreiben (ä → ae)</option>
        </select>
      </label>

      <label className="field">
        <span>Offen zählen ab</span>
        <input type="date" className="input" value={f.offenAb ?? ""} onChange={(e) => onChange({ offenAb: e.target.value || null })} />
        <span className="hint">leer = alle Unterrichtstage</span>
      </label>

      <div className="field">
        <span>Markdown entfernen</span>
        <label className="check">
          <input type="checkbox" checked={f.markdownEntfernen} onChange={(e) => onChange({ markdownEntfernen: e.target.checked })} />
          ja – Moodle bekommt reinen Text
        </label>
      </div>
    </div>
  );
}

/** Ganzzahl-Eingabe: gemeldet werden nur gültige Werte; eine ungültige Eingabe bleibt sichtbar stehen,
 *  bekommt einen Hinweis und sperrt über `onGueltig` das Speichern (nie stilles Zurücksetzen). */
function NumField({
  value,
  min,
  max,
  disabled,
  onChange,
  onGueltig,
  ariaLabel,
}: {
  value: number;
  min: number;
  max: number;
  disabled?: boolean;
  onChange: (n: number) => void;
  onGueltig?: (ok: boolean) => void;
  ariaLabel?: string;
}) {
  const [text, setText] = useState(String(value));
  useEffect(() => {
    setText((t) => (t.trim() !== "" && Number(t) === value ? t : String(value)));
  }, [value]);
  // gesperrtes Feld zeigt wieder den gültigen Wert
  useEffect(() => {
    if (disabled) setText(String(value));
  }, [disabled, value]);
  const t = text.trim();
  const ok = !!disabled || (/^\d+$/.test(t) && Number(t) >= min && Number(t) <= max);
  const gueltigMelden = useRef(onGueltig);
  gueltigMelden.current = onGueltig;
  useEffect(() => {
    gueltigMelden.current?.(ok);
  }, [ok]);
  useEffect(() => () => gueltigMelden.current?.(true), []);
  return (
    <>
      {/* type="text": Mausrad/Pfeiltasten ändern die Zahl nicht unbemerkt */}
      <input
        type="text"
        className={`input num-input ${ok ? "" : "invalid"}`}
        inputMode="numeric"
        value={text}
        disabled={disabled}
        aria-label={ariaLabel}
        aria-invalid={!ok}
        onChange={(e) => {
          const v = e.target.value;
          setText(v);
          if (/^\d+$/.test(v.trim())) {
            const n = Number(v);
            if (n >= min && n <= max) onChange(n);
          }
        }}
      />
      {!ok && (
        <span className="hint lb-warn-text">
          Bitte eine ganze Zahl von {min} bis {max}.
        </span>
      )}
    </>
  );
}

// ---------- KI ----------

function usd(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toLocaleString("de-DE", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

function modellLabel(m: KiModell): string {
  return `${m.name} · ${usd(m.eingabeUsdProMio)} $ / ${usd(m.ausgabeUsdProMio)} $ je 1 Mio. Tokens`;
}

function KiEinstellungen() {
  const [ki, setKi] = useState<KiStatus | null>(null);
  // Der Schlüssel steht nur bis zum Klick auf „Speichern“ im Eingabefeld – danach nie wieder.
  const [schluessel, setSchluessel] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .kiStatus()
      .then((s) => alive && setKi(s))
      .catch((e) => alive && setMsg({ ok: false, text: errorText(e) }));
    return () => {
      alive = false;
    };
  }, []);

  async function run(f: () => Promise<KiStatus>, okText: string) {
    setBusy(true);
    setMsg(null);
    try {
      setKi(await f());
      setMsg({ ok: true, text: okText });
    } catch (e) {
      setMsg({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  }

  function speichern(e: FormEvent) {
    e.preventDefault();
    const k = schluessel.trim();
    if (!k || busy) return;
    setSchluessel(""); // sofort leeren – der Schlüssel wird nie wieder angezeigt
    void run(() => api.kiSchluesselSpeichern(k), "Schlüssel gespeichert.");
  }

  const modelle = ki?.modelle ?? [];
  const modellBekannt = !!ki && modelle.some((m) => m.id === ki.modell);

  return (
    <section className="settings glass">
      <div>
        <div className="settings-title-row">
          <h2>KI</h2>
          {ki &&
            (ki.schluesselGespeichert ? (
              <span className="badge mint">{Icon.check} Schlüssel gespeichert</span>
            ) : (
              <span className="badge amber">Kein Schlüssel hinterlegt</span>
            ))}
        </div>
        <div className="hint">
          Der Schlüssel liegt im Windows-Anmeldeinformationsspeicher, nie in einer Datei. Die KI wird nur auf deinen Klick
          benutzt; ohne Schlüssel füllt die App die Vorlage ohne KI.
        </div>
      </div>

      {ki?.fehler && <div className="error">{ki.fehler}</div>}

      <form className="row ki-key-row" onSubmit={speichern}>
        <input
          type="password"
          className="input ki-key-input"
          value={schluessel}
          onChange={(e) => setSchluessel(e.target.value)}
          placeholder={ki?.schluesselGespeichert ? "Neuen Schlüssel eintragen …" : "Anthropic-API-Schlüssel"}
          autoComplete="off"
          spellCheck={false}
          aria-label="KI-Schlüssel"
          disabled={busy}
        />
        <button type="submit" className="btn" disabled={busy || schluessel.trim() === ""}>
          {Icon.save} Speichern
        </button>
        <button
          type="button"
          className="btn danger"
          onClick={() => void run(api.kiSchluesselLoeschen, "Schlüssel entfernt.")}
          disabled={busy || !ki?.schluesselGespeichert}
        >
          Entfernen
        </button>
      </form>

      {ki && (
        <label className="field">
          <span>Modell</span>
          <select
            className="input"
            value={ki.modell}
            onChange={(e) => {
              const id = e.target.value;
              void run(() => api.kiModellSetzen(id), "Modell gespeichert.");
            }}
            disabled={busy}
          >
            {!modellBekannt && <option value={ki.modell}>{ki.modell}</option>}
            {modelle.map((m) => (
              <option key={m.id} value={m.id}>
                {modellLabel(m)}
              </option>
            ))}
          </select>
        </label>
      )}

      {msg && <div className={msg.ok ? "result" : "error"}>{msg.text}</div>}
    </section>
  );
}
