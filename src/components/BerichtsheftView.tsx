// Ansicht „Berichtsheft“: links die Wochen (neueste zuerst), rechts die gewählte Wochennotiz mit den
// Reitern Tage · Nachweis · Lernfelder · Betrieb.md. Jede Änderung an einer bestehenden Datei läuft
// über Vorschau und Diff (Regel 9); Sondertage haben kein Textfeld (Regel 3), Lücken werden nur
// markiert (Regel 4). Entwürfe leben nur im Zustand dieser Ansicht – nie im Browser-Speicher.
import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  errorText,
  formatDatum,
  isNoteNotFound,
  samePath,
  type BerichtsheftUebersicht,
  type BerichtsWoche,
  type KiStatus,
  type NeueWoche,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";
import { klartext } from "../markdown";
import { useToast } from "../toast";
import { BetriebTab } from "./berichtsheft/BetriebTab";
import { Hinweise, lfKurz, statusBadge } from "./berichtsheft/gemeinsam";
import { LernfelderTab } from "./berichtsheft/LernfelderTab";
import { NachweisTab } from "./berichtsheft/NachweisTab";
import { NeueWocheDialog } from "./berichtsheft/NeueWocheDialog";
import { TageTab } from "./berichtsheft/TageTab";
import { WochenListe } from "./berichtsheft/WochenListe";

type Tab = "tage" | "nachweis" | "lernfelder" | "betrieb";

const TABS: { key: Tab; label: string }[] = [
  { key: "tage", label: "Tage" },
  { key: "nachweis", label: "Nachweis" },
  { key: "lernfelder", label: "Lernfelder" },
  { key: "betrieb", label: "Betrieb.md" },
];

/** Vorauswahl: aktuelle Woche mit Notiz, sonst die neueste mit Notiz */
function vorauswahl(u: BerichtsheftUebersicht): string | null {
  return (u.wochen.find((w) => w.aktuell && w.path) ?? u.wochen.find((w) => w.path))?.path ?? null;
}

export function BerichtsheftView({
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
  const [u, setU] = useState<BerichtsheftUebersicht | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [gewaehlt, setGewaehlt] = useState<string | null>(null);
  const [woche, setWoche] = useState<BerichtsWoche | null>(null);
  const [wocheError, setWocheError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("tage");
  // Reiter, die für die gewählte Woche schon offen waren (bleiben eingehängt → Entwürfe bleiben)
  const [besucht, setBesucht] = useState<ReadonlySet<Tab>>(new Set(["tage"]));
  const [neu, setNeu] = useState<NeueWoche | null>(null);
  const [kiStatus, setKiStatus] = useState<KiStatus | null>(null);
  const uSeq = useRef(0);
  const wSeq = useRef(0);
  const gewaehltRef = useRef(gewaehlt);
  gewaehltRef.current = gewaehlt;

  const loadUebersicht = useCallback(async () => {
    uSeq.current += 1;
    const mine = uSeq.current;
    try {
      const r = await api.berichtsheftUebersicht(null);
      if (mine !== uSeq.current) return;
      setU(r);
      setError(null);
      // Auswahl gibt es nicht (mehr) → Vorauswahl
      const g = gewaehltRef.current;
      if (!g || !r.wochen.some((w) => samePath(w.path, g))) setGewaehlt(vorauswahl(r));
    } catch (e) {
      if (mine !== uSeq.current) return;
      setError(errorText(e));
    }
  }, []);

  const loadWoche = useCallback(async (path: string) => {
    wSeq.current += 1;
    const mine = wSeq.current;
    try {
      const w = await api.berichtsheftWoche(path);
      if (mine !== wSeq.current) return;
      setWoche(w);
      setWocheError(null);
    } catch (e) {
      if (mine !== wSeq.current) return;
      if (isNoteNotFound(e)) {
        setWoche(null);
        setGewaehlt(null);
        void loadUebersicht();
      } else setWocheError(errorText(e));
    }
  }, [loadUebersicht]);

  const loadKi = useCallback(async () => {
    try {
      setKiStatus(await api.kiStatus());
    } catch {
      setKiStatus(null); // ohne Status: KI-Knöpfe gesperrt
    }
  }, []);

  // Sichtbar werden → frisch (verpasste Vault-Ereignisse, Einstellungen, Tageswechsel)
  const activeRef = useRef(active);
  activeRef.current = active;
  useEffect(() => {
    if (!active) return;
    void loadUebersicht();
    void loadKi();
    if (gewaehltRef.current) void loadWoche(gewaehltRef.current);
  }, [active, loadUebersicht, loadKi, loadWoche]);

  // Andere Woche gewählt → laden; Reiter-Entwürfe gehören zur alten Woche
  const geladenFuer = useRef<string | null>(null);
  useEffect(() => {
    if (samePath(geladenFuer.current, gewaehlt)) return;
    geladenFuer.current = gewaehlt;
    wSeq.current += 1;
    setWoche(null);
    setWocheError(null);
    setBesucht(new Set([tab]));
    if (gewaehlt) void loadWoche(gewaehlt);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [gewaehlt, loadWoche]);

  // Vault-Ereignisse (auch eigene Schreibvorgänge) → Übersicht und Woche neu; Entwürfe bleiben
  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    if (!activeRef.current) return; // beim Sichtbarwerden wird ohnehin neu geladen
    void loadUebersicht();
    if (gewaehltRef.current) void loadWoche(gewaehltRef.current);
  }, [change, loadUebersicht, loadWoche]);

  function nachSchreiben() {
    void loadUebersicht();
    if (gewaehltRef.current) void loadWoche(gewaehltRef.current);
  }

  function zeigeTab(t: Tab) {
    setTab(t);
    setBesucht((s) => (s.has(t) ? s : new Set([...s, t])));
  }

  function waehlen(path: string) {
    if (samePath(path, gewaehlt)) return;
    setGewaehlt(path);
  }

  const obsidian = (path: string) => api.openInObsidian(path).catch((e) => notify({ kind: "error", text: errorText(e) }));

  const aktuelleWoche = woche && samePath(woche.path, gewaehlt) ? woche : null;
  const gesperrt = !!aktuelleWoche && samePath(aktuelleWoche.path, dirtyPath);
  const fehlendeTage = aktuelleWoche ? aktuelleWoche.tage.filter((t) => t.fehlt).length : 0;

  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Berichtsheft</h1>
          <div className="page-sub">Wochennotizen aus „04 Betrieb/Wochen“ – geschrieben wird erst nach Vorschau und Bestätigung.</div>
        </div>
        {u && !u.aktuelleVorhanden && (
          <button className="btn primary bh-neu-knopf" onClick={() => setNeu({ datum: u.heute })}>
            {Icon.plus} Diese Woche anlegen
          </button>
        )}
      </div>

      {error && <div className="error">{error}</div>}
      {!u && !error && <div className="hint pad">Lade …</div>}

      {u && (
        <div className="logbuch-grid bh-grid">
          <section className="panel glass">
            <div className="panel-head">
              <span className="panel-title">Wochen</span>
              {u.offenVorwochen > 0 && (
                <span className="badge amber" title="Vorwochen mit fehlenden Tagen oder offenen Punkten">
                  {u.offenVorwochen} offen
                </span>
              )}
            </div>
            <div className="panel-body">
              <WochenListe u={u} gewaehlt={gewaehlt} onWaehlen={waehlen} onAnlegen={(montag) => setNeu({ datum: montag })} />
            </div>
          </section>

          <section className="panel glass lb-detail">
            {!gewaehlt && (
              <div className="empty">
                <div>
                  <div className="empty-icon">{Icon.berichtsheft}</div>
                  {u.wochen.some((w) => w.path) ? "Wähle links eine Woche." : "Noch keine Wochennotiz – lege die aktuelle Woche an."}
                </div>
              </div>
            )}
            {gewaehlt && !aktuelleWoche && (
              <div className="lb-detail-body">
                {wocheError ? (
                  <div className="error">{wocheError}</div>
                ) : (
                  <div className="lb-busy">
                    <span className="spinner" /> Lade Wochennotiz …
                  </div>
                )}
              </div>
            )}
            {aktuelleWoche && (
              <div className="lb-detail-body">
                <WochenKopf
                  woche={aktuelleWoche}
                  onEditor={() => onOpenNote(aktuelleWoche.path)}
                  onObsidian={() => void obsidian(aktuelleWoche.path)}
                />
                <Hinweise texte={aktuelleWoche.warnungen} />
                {wocheError && <div className="error">{wocheError}</div>}
                {gesperrt && (
                  <div className="banner lb-banner">
                    {Icon.warning}
                    <span>Die Wochennotiz hat ungespeicherte Änderungen im Editor – erst speichern. Bis dahin schreibt das Berichtsheft nichts.</span>
                  </div>
                )}
                <OffenePunkte woche={aktuelleWoche} onOpen={(line) => onOpenNote(aktuelleWoche.path, line)} />

                <div className="pills bh-tabs" role="tablist" aria-label="Wochennotiz">
                  {TABS.map((t) => (
                    <button key={t.key} className={`pill ${tab === t.key ? "active" : ""}`} role="tab" aria-selected={tab === t.key} onClick={() => zeigeTab(t.key)}>
                      {t.label}
                      {t.key === "tage" && fehlendeTage > 0 && <span className="pill-count">{fehlendeTage}</span>}
                    </button>
                  ))}
                </div>

                {/* Reiter bleiben nach dem ersten Besuch eingehängt – Entwürfe überleben den Wechsel */}
                <div key={aktuelleWoche.path} className="bh-tabs-host">
                  <div hidden={tab !== "tage"}>
                    <TageTab woche={aktuelleWoche} gesperrt={gesperrt} onGeschrieben={nachSchreiben} onOpenNote={onOpenNote} />
                  </div>
                  {besucht.has("nachweis") && (
                    <div hidden={tab !== "nachweis"}>
                      <NachweisTab
                        woche={aktuelleWoche}
                        gesperrt={gesperrt}
                        kiStatus={kiStatus}
                        onGeschrieben={nachSchreiben}
                        onOpenNote={onOpenNote}
                        onGoSettings={onGoSettings}
                      />
                    </div>
                  )}
                  {besucht.has("lernfelder") && (
                    <div hidden={tab !== "lernfelder"}>
                      <LernfelderTab
                        woche={aktuelleWoche}
                        gesperrt={gesperrt}
                        kiStatus={kiStatus}
                        onGeschrieben={nachSchreiben}
                        onGoSettings={onGoSettings}
                      />
                    </div>
                  )}
                  {besucht.has("betrieb") && (
                    <div hidden={tab !== "betrieb"}>
                      <BetriebTab
                        woche={aktuelleWoche}
                        gesperrt={gesperrt}
                        dirtyPath={dirtyPath}
                        onGeschrieben={nachSchreiben}
                        onOpenNote={onOpenNote}
                      />
                    </div>
                  )}
                </div>
              </div>
            )}
          </section>
        </div>
      )}

      {neu && (
        <NeueWocheDialog
          req={neu}
          onClose={() => setNeu(null)}
          onFertig={(relPath) => {
            setNeu(null);
            setGewaehlt(relPath);
            void loadUebersicht();
          }}
        />
      )}
    </div>
  );
}

function WochenKopf({ woche, onEditor, onObsidian }: { woche: BerichtsWoche; onEditor: () => void; onObsidian: () => void }) {
  return (
    <div className="lb-head">
      <div>
        <div className="editor-title-row bh-kopf-titel">
          <h2 className="preview-title">
            KW {woche.kw} · {formatDatum(woche.montag)} – {formatDatum(woche.sonntag)}
          </h2>
          {woche.status && <span className={`badge ${statusBadge(woche.status)}`}>{woche.status}</span>}
          {woche.abgezeichnet ? (
            <span className="badge mint">{Icon.check} abgezeichnet</span>
          ) : (
            <span className="badge muted">nicht abgezeichnet</span>
          )}
        </div>
        <div className="note-meta">
          {woche.stunden && <span>{woche.stunden} Std.</span>}
          {woche.lernfelder.map((n) => (
            <span key={n} className="badge muted">
              {lfKurz(n)}
            </span>
          ))}
          <span className="mono-path">{woche.path}</span>
        </div>
      </div>
      <div className="row">
        <button className="btn small" onClick={onEditor}>
          {Icon.edit} Im Editor öffnen
        </button>
        <button className="btn small" onClick={onObsidian}>
          {Icon.obsidian} In Obsidian öffnen
        </button>
      </div>
    </div>
  );
}

/** „Offen vor dem Abzeichnen“ – nur lesen; ein Klick öffnet die Zeile im Editor. */
function OffenePunkte({ woche, onOpen }: { woche: BerichtsWoche; onOpen: (line: number) => void }) {
  if (woche.offenePunkte.length === 0) return null;
  return (
    <div className="lb-section bh-offen">
      <div className="lb-section-title">Offen vor dem Abzeichnen</div>
      {woche.offenePunkte.map((p) => (
        <div key={p.line} className="bh-offen-punkt">
          <span className="bh-offen-box" aria-hidden />
          <span className="bh-offen-text">{klartext(p.text) || p.text}</span>
          <button className="lb-link" onClick={() => onOpen(p.line)} title={`${woche.path}, Zeile ${p.line}`}>
            Zeile {p.line}
          </button>
        </div>
      ))}
    </div>
  );
}
