// Ansicht „Aufgaben“: Arbeitsaufträge aus allen Notizen (gerechnet wie „00 Start/Aufgaben.md“) und
// Lernaufgaben aus „07 Material/Aufgabensammlung“. Abgehakt wird in der Notiz selbst – das Backend
// ändert nur das Zeichen zwischen [ ] bzw. das Frontmatter-Feld `geloest`.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  errorText,
  samePath,
  type Arbeitsauftrag,
  type AufgabenUebersicht,
  type Lernaufgabe,
  type VaultChanged,
} from "../api";
import { abhaken, aufgabenText, faelligkeit, lernaufgabeSetzen } from "../aufgaben";
import { Icon } from "../icons";
import { useToast } from "../toast";

type Tab = "auftraege" | "lern";
type Faellig = "alle" | "ueberfaellig" | "woche" | "spaeter" | "ohne";

/** Filterwerte: "" = alle, OHNE = ohne Fach/Lernfeld */
const ALLE = "";
const OHNE = "\u0000ohne";

const zeilenKey = (path: string, line: number) => `${path}\n${line}`;

function passt(wert: string | null, filter: string): boolean {
  if (filter === ALLE) return true;
  if (filter === OHNE) return wert == null;
  return wert != null && wert.toLowerCase() === filter.toLowerCase();
}

/** Werte für ein Auswahlfeld: sortiert, „ohne“ zuletzt; ein gewählter Wert bleibt immer wählbar. */
function optionen(werte: (string | null)[], gewaehlt: string): { werte: string[]; ohne: boolean } {
  const m = new Map<string, string>();
  let ohne = gewaehlt === OHNE;
  for (const w of werte) {
    if (w == null) ohne = true;
    else if (!m.has(w.toLowerCase())) m.set(w.toLowerCase(), w);
  }
  if (gewaehlt !== ALLE && gewaehlt !== OHNE && !m.has(gewaehlt.toLowerCase())) m.set(gewaehlt.toLowerCase(), gewaehlt);
  return { werte: [...m.values()].sort((a, b) => a.localeCompare(b, "de", { numeric: true })), ohne };
}

/** Rang wie im Backend: leicht/1 → 1, mittel/2 → 2, schwer/3 → 3 */
function levelCls(s: string | null): string {
  const v = s?.trim().toLowerCase();
  if (v === "leicht" || v === "1") return "mint";
  if (v === "mittel" || v === "2") return "amber";
  if (v === "schwer" || v === "3") return "red";
  return "muted";
}

interface NotizGruppe {
  path: string;
  title: string;
  fach: string | null;
  privat: boolean;
  items: Arbeitsauftrag[];
}

/** „Ohne Frist“ nach Notiz gruppieren (Reihenfolge bleibt: path, line) */
function nachNotiz(l: Arbeitsauftrag[]): NotizGruppe[] {
  const out: NotizGruppe[] = [];
  const idx = new Map<string, number>();
  for (const a of l) {
    const i = idx.get(a.path);
    if (i === undefined) {
      idx.set(a.path, out.length);
      out.push({ path: a.path, title: a.title, fach: a.fach, privat: a.privat, items: [a] });
    } else out[i].items.push(a);
  }
  return out;
}

const FAELLIG_LABEL: Record<Faellig, string> = {
  alle: "Alle",
  ueberfaellig: "Überfällig",
  woche: "Diese Woche",
  spaeter: "Später",
  ohne: "Ohne Frist",
};

export function AufgabenView({
  active,
  change,
  onOpenNote,
  dirtyPath,
}: {
  active: boolean;
  change: { seq: number; data: VaultChanged } | null;
  onOpenNote: (path: string, line?: number) => void;
  /** Pfad der Notiz im Editor, wenn sie ungespeicherte Änderungen hat */
  dirtyPath: string | null;
}) {
  const { notify } = useToast();
  const [u, setU] = useState<AufgabenUebersicht | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("auftraege");
  const [fach, setFach] = useState(ALLE);
  const [lernfeld, setLernfeld] = useState(ALLE);
  const [faellig, setFaellig] = useState<Faellig>("alle");
  const [geloesteZeigen, setGeloesteZeigen] = useState(false);
  const [erledigtOffen, setErledigtOffen] = useState(false);
  // Zeilen, deren Schreibvorgang gerade läuft → angezeigter Zielzustand
  const [laeuft, setLaeuft] = useState<ReadonlyMap<string, boolean>>(new Map());
  const seq = useRef(0);

  const load = useCallback(async () => {
    seq.current += 1;
    const mine = seq.current;
    try {
      const r = await api.aufgabenUebersicht(null);
      if (mine !== seq.current) return;
      setU(r);
      setError(null);
    } catch (e) {
      if (mine !== seq.current) return;
      setError(errorText(e));
    }
  }, []);

  // Sichtbar werden → frisch laden (verpasste Vault-Ereignisse, Tageswechsel)
  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    if (active) void load();
  }, [active, load]);

  // Vault-Ereignisse (auch eigene Schreibvorgänge) → neu laden, solange sichtbar
  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    if (activeRef.current) void load();
  }, [change, load]);

  // Zurück ins Fenster (z. B. nach Mitternacht oder aus Obsidian) → frisch
  useEffect(() => {
    if (!active) return;
    const onFocus = () => void load();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [active, load]);

  // ---------- Schreiben ----------
  function markiere(k: string, ziel: boolean | null) {
    setLaeuft((m) => {
      const n = new Map(m);
      if (ziel === null) n.delete(k);
      else n.set(k, ziel);
      return n;
    });
  }

  async function setzeAuftrag(a: Arbeitsauftrag, erledigt: boolean) {
    const k = zeilenKey(a.path, a.line);
    if (laeuft.has(k)) return;
    markiere(k, erledigt);
    try {
      await abhaken(notify, a, erledigt);
    } finally {
      await load(); // nach Erfolg wie nach Fehler: frischer Stand
      markiere(k, null);
    }
  }

  async function setzeLernaufgabe(l: Lernaufgabe, geloest: boolean) {
    const k = zeilenKey(l.path, -1);
    if (laeuft.has(k)) return;
    markiere(k, geloest);
    try {
      await lernaufgabeSetzen(notify, l, geloest);
    } finally {
      await load();
      markiere(k, null);
    }
  }

  const obsidian = (path: string) => api.openInObsidian(path).catch((e) => notify({ kind: "error", text: errorText(e) }));
  const istDirty = (path: string) => samePath(path, dirtyPath);

  // ---------- Abgeleitete Werte ----------
  const alleAuftraege = useMemo(
    () => (u ? [...u.ueberfaellig, ...u.dieseWoche, ...u.spaeter, ...u.ohneFrist, ...u.zuletztErledigt] : []),
    [u],
  );
  const lernaufgaben = u?.lernaufgaben ?? [];
  const filterBasis: { fach: string | null; lernfeld: string | null }[] = tab === "auftraege" ? alleAuftraege : lernaufgaben;
  const fachOpt = optionen(filterBasis.map((x) => x.fach), fach);
  const lfOpt = optionen(filterBasis.map((x) => x.lernfeld), lernfeld);
  const filt = <T extends { fach: string | null; lernfeld: string | null }>(l: T[]): T[] =>
    l.filter((x) => passt(x.fach, fach) && passt(x.lernfeld, lernfeld));

  const ueberfaellig = filt(u?.ueberfaellig ?? []);
  const dieseWoche = filt(u?.dieseWoche ?? []);
  const spaeter = filt(u?.spaeter ?? []);
  const ohneFrist = filt(u?.ohneFrist ?? []);
  const erledigt = filt(u?.zuletztErledigt ?? []);
  const anzahl: Record<Faellig, number> = {
    alle: ueberfaellig.length + dieseWoche.length + spaeter.length + ohneFrist.length,
    ueberfaellig: ueberfaellig.length,
    woche: dieseWoche.length,
    spaeter: spaeter.length,
    ohne: ohneFrist.length,
  };
  const offenGesamt = u ? u.ueberfaellig.length + u.dieseWoche.length + u.spaeter.length + u.ohneFrist.length : 0;
  const lernOffenGesamt = lernaufgaben.filter((l) => !l.geloest).length;

  const lernGefiltert = filt(lernaufgaben);
  const lernSichtbar = geloesteZeigen ? lernGefiltert : lernGefiltert.filter((l) => !l.geloest);
  const lernGeloest = lernGefiltert.filter((l) => l.geloest).length;

  const filterAktiv = fach !== ALLE || lernfeld !== ALLE || (tab === "auftraege" && faellig !== "alle");
  function filterZuruecksetzen() {
    setFach(ALLE);
    setLernfeld(ALLE);
    setFaellig("alle");
  }

  const zeige = (f: Faellig) => faellig === "alle" || faellig === f;
  // nichts Offenes (eine einzelne Fälligkeit zeigt stattdessen ihren eigenen Leertext)
  const nichtsOffen = faellig === "alle" && anzahl.alle === 0;
  const leerText = filterAktiv
    ? "Keine offenen Arbeitsaufträge für diesen Filter."
    : alleAuftraege.length > 0
      ? "Alles erledigt – keine offenen Arbeitsaufträge."
      : "Keine Arbeitsaufträge gefunden. Sie stehen als „- [ ] …“ in deinen Notizen, eine Frist mit „📅 JJJJ-MM-TT“.";

  const zeileProps = (a: Arbeitsauftrag) => {
    const k = zeilenKey(a.path, a.line);
    return {
      a,
      heute: u?.heute ?? "",
      ziel: laeuft.get(k),
      dirty: istDirty(a.path),
      onSetzen: (erl: boolean) => void setzeAuftrag(a, erl),
      onOpen: () => onOpenNote(a.path, a.line),
      onObsidian: () => void obsidian(a.path),
    };
  };

  const abschnitt = (titel: string, items: Arbeitsauftrag[], leer: string, rot = false) => (
    <section className="panel glass">
      <div className="panel-head">
        <span className="panel-title">{titel}</span>
        <span className={`badge ${rot && items.length > 0 ? "red" : "muted"}`}>{items.length}</span>
      </div>
      <div className="panel-body">
        {items.length === 0 && <div className="hint pad">{leer}</div>}
        {items.map((a) => (
          <AuftragZeile key={zeilenKey(a.path, a.line)} {...zeileProps(a)} mitQuelle />
        ))}
      </div>
    </section>
  );

  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Aufgaben</h1>
          <div className="page-sub">
            {tab === "auftraege"
              ? "Arbeitsaufträge aus deinen Notizen – abgehakt wird direkt in der Notiz."
              : "Lernaufgaben aus „07 Material/Aufgabensammlung“ – „Gelöst“ setzt geloest im Frontmatter."}
          </div>
        </div>
      </div>

      <div className="filter-bar">
        <div className="pills" role="tablist" aria-label="Aufgabenart">
          <button
            className={`pill ${tab === "auftraege" ? "active" : ""}`}
            role="tab"
            aria-selected={tab === "auftraege"}
            onClick={() => setTab("auftraege")}
          >
            Arbeitsaufträge
            {u && <span className={`pill-count ${u.ueberfaellig.length > 0 ? "red" : "muted"}`}>{offenGesamt}</span>}
          </button>
          <button className={`pill ${tab === "lern" ? "active" : ""}`} role="tab" aria-selected={tab === "lern"} onClick={() => setTab("lern")}>
            Lernaufgaben
            {u && <span className="pill-count muted">{lernOffenGesamt}</span>}
          </button>
        </div>
        <span className="filter-spacer" />
        <label className="filter">
          <span>Fach</span>
          <select className="input" value={fach} onChange={(e) => setFach(e.target.value)}>
            <option value={ALLE}>Alle</option>
            {fachOpt.werte.map((w) => (
              <option key={w} value={w}>
                {w}
              </option>
            ))}
            {fachOpt.ohne && <option value={OHNE}>ohne Fach</option>}
          </select>
        </label>
        <label className="filter">
          <span>Lernfeld</span>
          <select className="input" value={lernfeld} onChange={(e) => setLernfeld(e.target.value)}>
            <option value={ALLE}>Alle</option>
            {lfOpt.werte.map((w) => (
              <option key={w} value={w}>
                {w}
              </option>
            ))}
            {lfOpt.ohne && <option value={OHNE}>ohne Lernfeld</option>}
          </select>
        </label>
        {tab === "auftraege" ? (
          <label className="filter">
            <span>Fälligkeit</span>
            <select className="input" value={faellig} onChange={(e) => setFaellig(e.target.value as Faellig)}>
              {(Object.keys(FAELLIG_LABEL) as Faellig[]).map((f) => (
                <option key={f} value={f}>
                  {FAELLIG_LABEL[f]} ({anzahl[f]})
                </option>
              ))}
            </select>
          </label>
        ) : (
          <label className="check">
            <input type="checkbox" checked={geloesteZeigen} onChange={(e) => setGeloesteZeigen(e.target.checked)} />
            Gelöste zeigen
          </label>
        )}
        {filterAktiv && (
          <button className="btn small" onClick={filterZuruecksetzen}>
            Filter zurücksetzen
          </button>
        )}
      </div>

      {error && <div className="error">{error}</div>}
      {!u && !error && <div className="hint pad">Lade …</div>}

      {u && tab === "auftraege" && (
        <div className="aufgaben-scroll">
          {nichtsOffen && (
            <section className="panel glass">
              <div className="empty day-empty">
                <div>
                  <div className="empty-icon">{Icon.aufgaben}</div>
                  <p>{leerText}</p>
                </div>
              </div>
            </section>
          )}

          {zeige("ueberfaellig") && (faellig !== "alle" || ueberfaellig.length > 0) &&
            abschnitt("Überfällig", ueberfaellig, "Nichts überfällig.", true)}
          {zeige("woche") && (faellig !== "alle" || dieseWoche.length > 0) &&
            abschnitt("Diese Woche fällig", dieseWoche, "In den nächsten 7 Tagen ist nichts fällig.")}
          {zeige("spaeter") && (faellig !== "alle" || spaeter.length > 0) &&
            abschnitt("Später fällig", spaeter, "Nichts mit späterer Frist.")}

          {zeige("ohne") && (faellig !== "alle" || ohneFrist.length > 0) && (
            <section className="panel glass">
              <div className="panel-head">
                <span className="panel-title">Ohne Frist</span>
                <span className="badge muted">{ohneFrist.length}</span>
              </div>
              <div className="panel-body">
                {ohneFrist.length === 0 && <div className="hint pad">Keine Arbeitsaufträge ohne Frist.</div>}
                {nachNotiz(ohneFrist).map((g) => (
                  <div key={g.path} className="auftrag-gruppe">
                    <div className="auftrag-gruppe-head">
                      <button className="lb-link" onClick={() => onOpenNote(g.path)} title={g.path}>
                        {Icon.datei}
                        {g.title}
                      </button>
                      {g.fach && <span className="badge purple">{g.fach}</span>}
                      {g.privat && <span className="badge muted">10 Privat</span>}
                      <span className="badge muted">{g.items.length}</span>
                      <span className="filter-spacer" />
                      <button
                        className="btn small icon-only"
                        onClick={() => void obsidian(g.path)}
                        title="In Obsidian öffnen"
                        aria-label={`„${g.title}“ in Obsidian öffnen`}
                      >
                        {Icon.obsidian}
                      </button>
                    </div>
                    {g.items.map((a) => (
                      <AuftragZeile key={zeilenKey(a.path, a.line)} {...zeileProps(a)} mitQuelle={false} />
                    ))}
                  </div>
                ))}
              </div>
            </section>
          )}

          {faellig === "alle" && erledigt.length > 0 && (
            <section className="panel glass">
              <div className="panel-head">
                <span className="panel-title">Zuletzt erledigt</span>
                <span className="badge mint">{erledigt.length}</span>
                <span className="filter-spacer" />
                <button className="btn small" onClick={() => setErledigtOffen((o) => !o)} aria-expanded={erledigtOffen}>
                  <span className={`chevron ${erledigtOffen ? "up" : ""}`}>{Icon.chevronDown}</span>
                  {erledigtOffen ? "Zuklappen" : "Anzeigen"}
                </button>
              </div>
              {erledigtOffen && (
                <div className="panel-body">
                  {erledigt.map((a) => (
                    <AuftragZeile key={zeilenKey(a.path, a.line)} {...zeileProps(a)} mitQuelle />
                  ))}
                </div>
              )}
            </section>
          )}
        </div>
      )}

      {u && tab === "lern" && (
        <div className="aufgaben-scroll">
          <section className="panel glass">
            <div className="panel-head">
              <span className="panel-title">Lernaufgaben</span>
              <span className="badge muted">
                {lernGefiltert.length - lernGeloest} offen{lernGeloest > 0 ? ` · ${lernGeloest} gelöst` : ""}
              </span>
            </div>
            <div className="panel-body">
              {lernSichtbar.length === 0 ? (
                <div className="hint pad">
                  {lernaufgaben.length === 0
                    ? "Keine Lernaufgaben in „07 Material/Aufgabensammlung“."
                    : lernGefiltert.length === 0
                      ? "Keine Lernaufgaben für diesen Filter."
                      : "Alle gelöst – „Gelöste zeigen“ blendet sie ein."}
                </div>
              ) : (
                <table className="lern-table">
                  <thead>
                    <tr>
                      <th>Aufgabe</th>
                      <th>LF</th>
                      <th>Fach</th>
                      <th>Typ</th>
                      <th>Level</th>
                      <th className="center">Gelöst</th>
                      <th aria-label="Aktionen" />
                    </tr>
                  </thead>
                  <tbody>
                    {lernSichtbar.map((l) => {
                      const ziel = laeuft.get(zeilenKey(l.path, -1));
                      const busy = ziel !== undefined;
                      const dirty = istDirty(l.path);
                      return (
                        <tr key={l.path} className={`${busy ? "busy" : ""} ${l.geloest ? "geloest" : ""}`} aria-busy={busy}>
                          <td className="lern-titel">
                            <div className="lern-titel-row">
                              <button className="lb-link" onClick={() => onOpenNote(l.path)} title={l.path}>
                                {l.title}
                              </button>
                              {l.nochmal && (
                                <span className="badge amber" title="Tag #nochmal – falsch gemacht, nochmal ran">
                                  nochmal
                                </span>
                              )}
                              {busy && <span className="spinner" aria-label="wird geschrieben" />}
                            </div>
                            {l.quelle && <div className="hint">aus {l.quelle}</div>}
                            {dirty && <div className="hint lb-warn-text">Ungespeicherte Änderungen im Editor – erst speichern.</div>}
                          </td>
                          <td>{l.lernfeld ? <span className="badge muted">{l.lernfeld}</span> : <span className="lern-leer">—</span>}</td>
                          <td>{l.fach ? <span className="badge purple">{l.fach}</span> : <span className="lern-leer">—</span>}</td>
                          <td>{l.typ ?? <span className="lern-leer">—</span>}</td>
                          <td>
                            {l.schwierigkeit ? (
                              <span className={`badge ${levelCls(l.schwierigkeit)}`}>{l.schwierigkeit}</span>
                            ) : (
                              <span className="lern-leer">—</span>
                            )}
                          </td>
                          <td className="center">
                            <input
                              type="checkbox"
                              className="auftrag-check"
                              checked={busy ? ziel : l.geloest}
                              disabled={busy || dirty}
                              onChange={(e) => void setzeLernaufgabe(l, e.target.checked)}
                              aria-label={`„${l.title}“ als gelöst markieren`}
                              title={dirty ? "Erst die Notiz im Editor speichern" : l.geloest ? "Wieder als offen markieren" : "Als gelöst markieren"}
                            />
                          </td>
                          <td className="lern-aktionen">
                            <button
                              className="btn small icon-only"
                              onClick={() => void obsidian(l.path)}
                              title="In Obsidian öffnen"
                              aria-label={`„${l.title}“ in Obsidian öffnen`}
                            >
                              {Icon.obsidian}
                            </button>
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              )}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

function AuftragZeile({
  a,
  heute,
  ziel,
  dirty,
  mitQuelle,
  onSetzen,
  onOpen,
  onObsidian,
}: {
  a: Arbeitsauftrag;
  heute: string;
  /** Zielzustand, solange der Schreibvorgang läuft */
  ziel: boolean | undefined;
  dirty: boolean;
  /** Notiztitel anzeigen (in „Ohne Frist“ steht er schon über der Gruppe) */
  mitQuelle: boolean;
  onSetzen: (erledigt: boolean) => void;
  onOpen: () => void;
  onObsidian: () => void;
}) {
  const busy = ziel !== undefined;
  const text = aufgabenText(a.text);
  const f = a.due && heute ? faelligkeit(a.due, heute) : null;
  const zeichen = !a.done && a.statusChar !== " " ? a.statusChar : null;
  // Einrückung nur in der Notiz-Reihenfolge („Ohne Frist“) sinnvoll
  const einzug = !mitQuelle && a.indent > 0 ? { paddingLeft: 12 + Math.min(a.indent, 12) * 4 } : undefined;
  return (
    <div className={`auftrag ${busy ? "busy" : ""} ${a.done ? "done" : ""}`} aria-busy={busy} style={einzug}>
      <input
        type="checkbox"
        className="auftrag-check"
        checked={busy ? ziel : a.done}
        disabled={busy || dirty}
        onChange={(e) => onSetzen(e.target.checked)}
        aria-label={`${a.done ? "Wieder öffnen" : "Abhaken"}: ${text}`}
        title={dirty ? "Erst die Notiz im Editor speichern" : a.done ? "Wieder als offen markieren" : "Abhaken"}
      />
      <div className="auftrag-main">
        <div className="auftrag-text">{text}</div>
        <div className="note-meta">
          {f && (
            <span className={`badge ${f.cls}`} title={f.title}>
              {f.text}
            </span>
          )}
          {zeichen && (
            <span className="badge muted" title={`Statuszeichen „${zeichen}“ – zählt als offen`}>
              [{zeichen}]
            </span>
          )}
          {mitQuelle && a.fach && <span className="badge purple">{a.fach}</span>}
          {a.lernfeld && <span className="badge muted">{a.lernfeld}</span>}
          {mitQuelle && a.privat && <span className="badge muted">10 Privat</span>}
          {mitQuelle ? (
            <button className="lb-link" onClick={onOpen} title={`${a.path}, Zeile ${a.line}`}>
              {Icon.datei}
              {a.title}
            </button>
          ) : (
            <button className="lb-link" onClick={onOpen} title={`${a.path}, Zeile ${a.line}`}>
              Zeile {a.line}
            </button>
          )}
          {busy && (
            <span className="lb-busy">
              <span className="spinner" /> wird geschrieben …
            </span>
          )}
        </div>
        {dirty && <div className="hint lb-warn-text">„{a.title}“ hat ungespeicherte Änderungen – erst speichern.</div>}
      </div>
      {mitQuelle && (
        <button className="btn small icon-only" onClick={onObsidian} title="In Obsidian öffnen" aria-label={`„${a.title}“ in Obsidian öffnen`}>
          {Icon.obsidian}
        </button>
      )}
    </div>
  );
}
