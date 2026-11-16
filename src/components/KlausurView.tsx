// Klausur-Trainer: Quiz aus den eigenen Notizen eines Fachs – mit KI (nur auf Klick, Quellen vorher
// sichtbar, jede Frage mit wörtlichem Beleg) oder ohne KI aus Karteikarten, Glossar, Lernaufgaben
// und Merksätzen. Ein Quiz lebt nur im Speicher; gespeichert wird nur auf Wunsch als neues
// Karteikarten-Set mit Vorschau.
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  api,
  errorText,
  type FrageArt,
  type KarteikartenSet,
  type KiAufruf,
  type KiStatus,
  type KlausurUebersicht,
  type QuellArt,
  type Quiz,
  type QuizFrage,
  type QuizQuellen,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";
import { useToast } from "../toast";
import { Modal } from "./Modal";

const ARTEN: { key: FrageArt; label: string }[] = [
  { key: "auswahl", label: "Multiple Choice" },
  { key: "kurz", label: "Kurzantwort" },
  { key: "karte", label: "Karteikarte" },
];
const ART_LABEL: Record<FrageArt, string> = { auswahl: "Multiple Choice", kurz: "Kurzantwort", karte: "Karteikarte" };
const ANZAHLEN = [5, 10, 15];
const QUELL_LABEL: Record<QuellArt, string> = {
  unterricht: "Unterricht",
  zusammenfassung: "Zusammenfassungen",
  karteikarten: "Karteikarten",
  lernaufgabe: "Lernaufgaben",
  glossar: "Glossar",
};
const QUELL_REIHE: QuellArt[] = ["unterricht", "zusammenfassung", "karteikarten", "lernaufgabe", "glossar"];

/** Antwort des Lernenden auf eine Frage */
interface Antwort {
  aufgedeckt: boolean;
  gewaehlt: number | null;
  eingabe: string;
  /** richtig/gewusst; null = noch offen */
  gewusst: boolean | null;
}
const LEER: Antwort = { aufgedeckt: false, gewaehlt: null, eingabe: "", gewusst: null };

function kiZeile(k: KiAufruf, ki: KiStatus | null): string {
  const name = ki?.modelle.find((m) => m.id === k.modell)?.name ?? k.modell;
  const tok = (n: number) => n.toLocaleString("de-DE");
  const kosten = k.kostenUsd.toLocaleString("de-DE", { minimumFractionDigits: 2, maximumFractionDigits: k.kostenUsd < 0.1 ? 4 : 2 });
  return `KI: ${name} · ${tok(k.eingabeTokens)}+${tok(k.ausgabeTokens)} Tokens · ≈ ${kosten} $${k.versuche > 1 ? ` · ${k.versuche} Versuche` : ""}`;
}

export function KlausurView({
  active,
  change,
  onOpenNote,
  onGoSettings,
}: {
  active: boolean;
  change: { seq: number; data: VaultChanged } | null;
  onOpenNote: (path: string, line?: number) => void;
  onGoSettings: () => void;
}) {
  const { notify } = useToast();
  const [ueb, setUeb] = useState<KlausurUebersicht | null>(null);
  const [fach, setFach] = useState<string>("");
  const [quellen, setQuellen] = useState<QuizQuellen | null>(null);
  const [auswahl, setAuswahl] = useState<Set<string>>(new Set());
  const [anzahl, setAnzahl] = useState(10);
  const [arten, setArten] = useState<FrageArt[]>(["auswahl", "kurz", "karte"]);
  const [privatErlauben, setPrivatErlauben] = useState(false);
  const [ki, setKi] = useState<KiStatus | null>(null);
  const [busy, setBusy] = useState<null | "ki" | "lokal">(null);
  const [error, setError] = useState<string | null>(null);
  const [quiz, setQuiz] = useState<Quiz | null>(null);
  const [antworten, setAntworten] = useState<Antwort[]>([]);
  const [pos, setPos] = useState(0);
  const [vorschau, setVorschau] = useState<KarteikartenSet | null>(null);
  const [speichert, setSpeichert] = useState(false);
  const activeRef = useRef(active);
  activeRef.current = active;
  const fachRef = useRef(fach);
  fachRef.current = fach;

  const ladeUebersicht = useCallback(async () => {
    try {
      const [u, k] = await Promise.all([api.klausurUebersicht(), api.kiStatus()]);
      setUeb(u);
      setKi(k);
      setFach((f) => (f && u.faecher.some((x) => x.kurz === f) ? f : (u.faecher.find((x) => x.quellen > 0)?.kurz ?? u.faecher[0]?.kurz ?? "")));
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  const ladeQuellen = useCallback(async (f: string, auswahlBehalten: boolean) => {
    if (!f) return;
    try {
      const q = await api.klausurQuellen(f);
      if (fachRef.current !== f) return;
      setQuellen(q);
      setAuswahl((alt) => {
        const da = new Set(q.quellen.map((x) => x.path));
        if (auswahlBehalten && alt.size > 0) return new Set([...alt].filter((p) => da.has(p)));
        return new Set(q.quellen.filter((x) => x.vorausgewaehlt).map((x) => x.path));
      });
    } catch (e) {
      setError(errorText(e));
    }
  }, []);

  useEffect(() => {
    if (active) void ladeUebersicht();
  }, [active, ladeUebersicht]);

  useEffect(() => {
    setPrivatErlauben(false);
    setQuellen(null);
    void ladeQuellen(fach, false);
  }, [fach, ladeQuellen]);

  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    if (activeRef.current) {
      void ladeUebersicht();
      void ladeQuellen(fachRef.current, true);
    }
  }, [change, ladeUebersicht, ladeQuellen]);

  const gewaehlt = useMemo(() => (quellen?.quellen ?? []).filter((q) => auswahl.has(q.path)), [quellen, auswahl]);
  const hatPrivat = gewaehlt.some((q) => q.privat);
  const mitgeschickt = gewaehlt.filter((q) => !q.privat || privatErlauben);
  const kiOk = !!ki?.schluesselGespeichert;
  const fachInfo = ueb?.faecher.find((f) => f.kurz === fach) ?? null;

  function toggleQuelle(path: string) {
    setAuswahl((alt) => {
      const n = new Set(alt);
      if (n.has(path)) n.delete(path);
      else n.add(path);
      return n;
    });
  }

  function toggleArt(a: FrageArt) {
    setArten((alt) => (alt.includes(a) ? (alt.length > 1 ? alt.filter((x) => x !== a) : alt) : [...alt, a]));
  }

  async function starten(mitKi: boolean) {
    setError(null);
    setBusy(mitKi ? "ki" : "lokal");
    try {
      const q = await api.klausurQuiz({ fach, pfade: gewaehlt.map((x) => x.path), mitKi, privatErlauben, anzahl, arten });
      setQuiz(q);
      setAntworten(q.fragen.map(() => ({ ...LEER })));
      setPos(0);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(null);
    }
  }

  function nurFalsche() {
    if (!quiz) return;
    const fragen = quiz.fragen.filter((_, i) => antworten[i]?.gewusst === false);
    if (fragen.length === 0) return;
    setQuiz({ ...quiz, fragen, warnungen: [] });
    setAntworten(fragen.map(() => ({ ...LEER })));
    setPos(0);
  }

  function setze(i: number, a: Partial<Antwort>) {
    setAntworten((alt) => alt.map((x, j) => (j === i ? { ...x, ...a } : x)));
  }

  async function oeffneVorschau() {
    if (!quiz) return;
    try {
      setVorschau(await api.klausurKarteikarten({ fach: quiz.fach, fragen: quiz.fragen, relPath: null }));
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    }
  }

  async function anlegen() {
    if (!quiz || !vorschau) return;
    setSpeichert(true);
    try {
      const r = await api.klausurKarteikarten({ fach: quiz.fach, fragen: quiz.fragen, relPath: vorschau.relPath });
      setVorschau(null);
      notify({
        kind: "created",
        text: `${r.karten} Karteikarten angelegt: ${r.relPath}`,
        path: r.relPath,
        detail: [...(r.report?.createdFolders ?? []).map((f) => `Ordner angelegt: ${f}`), ...r.warnungen],
      });
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    } finally {
      setSpeichert(false);
    }
  }

  const fertig = !!quiz && antworten.length > 0 && antworten.every((a) => a.gewusst !== null);
  const richtig = antworten.filter((a) => a.gewusst === true).length;

  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Klausur-Trainer</h1>
          <div className="page-sub">Üben mit Fragen aus deinen eigenen Notizen – jede Frage zeigt, woher sie stammt.</div>
        </div>
      </div>

      <div className="kt-grid">
        {/* ---------- Einstellen ---------- */}
        <section className="glass panel kt-setup">
          <div className="panel-head">
            <span className="panel-title">Quiz einstellen</span>
          </div>
          <div className="panel-body kt-setup-body">
            <label className="kt-feld">
              <span className="lb-label">Fach</span>
              <select className="input" value={fach} onChange={(e) => setFach(e.target.value)} disabled={!!busy}>
                {(ueb?.faecher ?? []).map((f) => (
                  <option key={f.kurz} value={f.kurz}>
                    {f.kurz}
                    {f.quellen > 0 ? ` · ${f.quellen} Quellen` : " · noch keine Quellen"}
                  </option>
                ))}
              </select>
            </label>

            <div className="kt-feld">
              <span className="lb-label">Fragen</span>
              <div className="pills" role="group" aria-label="Anzahl der Fragen">
                {ANZAHLEN.map((n) => (
                  <button key={n} className={`pill ${anzahl === n ? "active" : ""}`} onClick={() => setAnzahl(n)} aria-pressed={anzahl === n}>
                    {n}
                  </button>
                ))}
              </div>
            </div>

            <div className="kt-feld">
              <span className="lb-label">Fragetypen</span>
              <div className="pills kt-arten" role="group" aria-label="Fragetypen">
                {ARTEN.map((a) => (
                  <button
                    key={a.key}
                    className={`pill ${arten.includes(a.key) ? "active" : ""}`}
                    onClick={() => toggleArt(a.key)}
                    aria-pressed={arten.includes(a.key)}
                  >
                    {a.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="kt-feld">
              <span className="lb-label">
                Quellen · {gewaehlt.length} von {quellen?.quellen.length ?? 0}
              </span>
              {quellen && quellen.quellen.length === 0 && (
                <div className="hint">
                  Für {fach} gibt es noch keine Unterrichtsnotizen, Karteikarten, Lernaufgaben oder Glossarbegriffe.
                </div>
              )}
              <div className="kt-quellen">
                {QUELL_REIHE.map((art) => {
                  const liste = (quellen?.quellen ?? []).filter((q) => q.art === art);
                  if (liste.length === 0) return null;
                  return (
                    <div key={art} className="kt-quellgruppe">
                      <div className="task-group-head">
                        {QUELL_LABEL[art]} <span className="badge muted">{liste.length}</span>
                      </div>
                      {liste.map((q) => (
                        <div key={q.path} className="kt-quelle">
                          <label className="check">
                            <input type="checkbox" checked={auswahl.has(q.path)} onChange={() => toggleQuelle(q.path)} disabled={!!busy} />
                            <span className="kt-quelle-titel">{q.title}</span>
                          </label>
                          {q.privat && <span className="badge muted">10 Privat</span>}
                          <button className="lb-link kt-oeffnen" onClick={() => onOpenNote(q.path)} title={`Öffnen: ${q.path}`}>
                            {Icon.datei}
                          </button>
                        </div>
                      ))}
                    </div>
                  );
                })}
              </div>
            </div>

            {quellen && quellen.nochmal.length > 0 && (
              <div className="kt-feld">
                <span className="lb-label">Schwerpunkte (#nochmal)</span>
                <ul className="kt-nochmal">
                  {quellen.nochmal.slice(0, 6).map((n, i) => (
                    <li key={`${n.path}:${n.line ?? i}`}>
                      <button className="lb-link inline" onClick={() => onOpenNote(n.path, n.line ?? undefined)}>
                        {n.text}
                      </button>{" "}
                      <span className="hint">{n.title}</span>
                    </li>
                  ))}
                </ul>
                <div className="hint">Mit KI fließen die Schwerpunkte aus den mitgeschickten Notizen ins Quiz ein.</div>
              </div>
            )}

            <div className="lb-ki">
              <div className="lb-ki-line">
                Mitgeschickt werden: <strong>{mitgeschickt.length > 0 ? `${mitgeschickt.length} Notizen` : "—"}</strong>
              </div>
              <div className="hint">
                {privatAnText(privatErlauben)} · Jede KI-Frage braucht eine wörtliche Stelle aus deinen Notizen, sonst wird sie verworfen.
              </div>
              {hatPrivat && (
                <label className="check">
                  <input type="checkbox" checked={privatErlauben} onChange={(e) => setPrivatErlauben(e.target.checked)} disabled={!!busy} />
                  „10 Privat/“ für diesen Vorgang einschließen
                </label>
              )}
              <div className="row lb-ki-buttons">
                <button
                  className="btn small primary"
                  onClick={() => void starten(true)}
                  disabled={!kiOk || !!busy || mitgeschickt.length === 0}
                >
                  {Icon.sparkle} Quiz mit KI erzeugen
                </button>
                <button className="btn small" onClick={() => void starten(false)} disabled={!!busy || gewaehlt.length === 0}>
                  Ohne KI üben
                </button>
                {busy && (
                  <span className="lb-busy">
                    <span className="spinner" /> {busy === "ki" ? "KI schreibt Fragen …" : "Stelle Fragen zusammen …"}
                  </span>
                )}
              </div>
              {!kiOk && (
                <div className="hint">
                  {ki?.fehler ?? "Kein KI-Schlüssel hinterlegt – „Ohne KI üben“ nutzt Karteikarten, Glossar, Lernaufgaben und Merksätze."}{" "}
                  <button className="lb-link inline" onClick={onGoSettings}>
                    Zu den Einstellungen
                  </button>
                </div>
              )}
            </div>
            {error && <div className="error">{error}</div>}
          </div>
        </section>

        {/* ---------- Spielen ---------- */}
        <section className="glass panel kt-spiel">
          {!quiz ? (
            <div className="kt-leer">
              <div className="kt-leer-icon">{Icon.klausur}</div>
              <h2 className="preview-title">Bereit für {fach || "dein Fach"}?</h2>
              <p className="hint">
                Wähle links die Quellen und starte ein Quiz.
                {fachInfo && fachInfo.nochmal > 0 ? ` ${fachInfo.nochmal} Stellen hast du als „nochmal“ markiert.` : ""}
              </p>
            </div>
          ) : fertig ? (
            <Ergebnis
              quiz={quiz}
              antworten={antworten}
              richtig={richtig}
              ki={ki}
              onNochmalFalsche={nurFalsche}
              onNeu={() => setQuiz(null)}
              onSpeichern={() => void oeffneVorschau()}
              onOpenNote={onOpenNote}
            />
          ) : (
            <Spiel
              quiz={quiz}
              pos={pos}
              antworten={antworten}
              onSetze={setze}
              onPos={setPos}
              onOpenNote={onOpenNote}
              ki={ki}
            />
          )}
        </section>
      </div>

      {vorschau && (
        <SetVorschau set={vorschau} schreibt={speichert} onCancel={() => setVorschau(null)} onConfirm={() => void anlegen()} />
      )}
    </div>
  );
}

function privatAnText(an: boolean): string {
  return an
    ? "EK-Angaben werden vorher entfernt · „10 Privat/“ wird diesmal mitgeschickt"
    : "EK-Angaben werden vorher entfernt · „10 Privat/“ bleibt aus";
}

function Quelle({ f, onOpenNote }: { f: QuizFrage; onOpenNote: (path: string) => void }) {
  return (
    <div className="kt-beleg">
      <button className="lb-link" onClick={() => onOpenNote(f.quellePfad)} title={f.quellePfad}>
        {Icon.datei}
        {f.quelleTitel}
      </button>
      {/* ohne KI ist der Beleg oft die Antwort selbst – dann nicht doppelt zeigen */}
      {f.beleg.trim() !== f.antwort.trim() && <blockquote>„{f.beleg}“</blockquote>}
    </div>
  );
}

function Spiel({
  quiz,
  pos,
  antworten,
  onSetze,
  onPos,
  onOpenNote,
  ki,
}: {
  quiz: Quiz;
  pos: number;
  antworten: Antwort[];
  onSetze: (i: number, a: Partial<Antwort>) => void;
  onPos: (i: number) => void;
  onOpenNote: (path: string) => void;
  ki: KiStatus | null;
}) {
  const f = quiz.fragen[pos];
  const a = antworten[pos] ?? LEER;
  const n = quiz.fragen.length;
  const beantwortet = antworten.filter((x) => x.gewusst !== null).length;
  const weiter = () => {
    const naechste = antworten.findIndex((x, i) => i > pos && x.gewusst === null);
    const erste = antworten.findIndex((x) => x.gewusst === null);
    onPos(naechste >= 0 ? naechste : erste >= 0 ? erste : pos);
  };
  if (!f) return null;
  return (
    <div className="kt-spiel-body">
      <div className="kt-kopf">
        <span className="badge purple">{ART_LABEL[f.art]}</span>
        <span className="hint">
          Frage {pos + 1} von {n} · {beantwortet} beantwortet · {quiz.mitKi ? "mit KI" : "ohne KI"}
        </span>
      </div>
      <div className="kt-fortschritt" aria-hidden>
        {antworten.map((x, i) => (
          <button
            key={i}
            className={`kt-punkt ${i === pos ? "jetzt" : ""} ${x.gewusst === true ? "ok" : x.gewusst === false ? "falsch" : ""}`}
            onClick={() => onPos(i)}
            title={`Frage ${i + 1}`}
          />
        ))}
      </div>

      {quiz.warnungen.length > 0 && pos === 0 && a.gewusst === null && (
        <div className="lb-hinweise">
          {quiz.warnungen.map((w, i) => (
            <div key={i} className="hint lb-hinweis">
              {Icon.warning}
              <span>{w}</span>
            </div>
          ))}
        </div>
      )}

      <h2 className="kt-frage">{f.frage}</h2>

      {f.art === "auswahl" && (
        <div className="kt-optionen">
          {f.optionen.map((o, i) => {
            const zeigen = a.gewaehlt !== null;
            const cls = zeigen ? (i === f.richtig ? "richtig" : i === a.gewaehlt ? "falsch" : "aus") : "";
            return (
              <button
                key={i}
                className={`kt-option ${cls}`}
                disabled={zeigen}
                onClick={() => onSetze(pos, { gewaehlt: i, aufgedeckt: true, gewusst: i === f.richtig })}
              >
                <span className="kt-buchstabe">{String.fromCharCode(65 + i)}</span>
                {o}
              </button>
            );
          })}
        </div>
      )}

      {f.art === "kurz" && !a.aufgedeckt && (
        <textarea
          className="input kt-eingabe"
          rows={3}
          placeholder="Deine Antwort (wird nicht gespeichert) …"
          value={a.eingabe}
          onChange={(e) => onSetze(pos, { eingabe: e.target.value })}
        />
      )}

      {f.art !== "auswahl" && !a.aufgedeckt && (
        <div className="row">
          <button className="btn primary" onClick={() => onSetze(pos, { aufgedeckt: true })}>
            {Icon.eye} {f.art === "karte" ? "Karte umdrehen" : "Lösung zeigen"}
          </button>
        </div>
      )}

      {a.aufgedeckt && (
        <div className={`kt-loesung ${f.art === "auswahl" ? (a.gewusst ? "ok" : "falsch") : ""}`}>
          {f.art === "auswahl" ? (
            <div className="kt-urteil">{a.gewusst ? "Richtig!" : `Leider falsch – richtig ist: ${f.antwort}`}</div>
          ) : (
            <>
              {f.art === "kurz" && a.eingabe.trim() && <div className="hint">Deine Antwort: {a.eingabe}</div>}
              <div className="kt-antwort">{f.antwort}</div>
            </>
          )}
          {f.erklaerung && <div className="hint">{f.erklaerung}</div>}
          <Quelle f={f} onOpenNote={onOpenNote} />
        </div>
      )}

      <div className="row kt-aktionen">
        {f.art !== "auswahl" && a.aufgedeckt && a.gewusst === null && (
          <>
            <button className="btn mint" onClick={() => { onSetze(pos, { gewusst: true }); weiter(); }}>
              {Icon.check} Gewusst
            </button>
            <button className="btn" onClick={() => { onSetze(pos, { gewusst: false }); weiter(); }}>
              Nicht gewusst
            </button>
          </>
        )}
        {f.art === "auswahl" && a.gewusst !== null && beantwortet < n && (
          <button className="btn primary" onClick={weiter}>
            Weiter {Icon.chevronRight}
          </button>
        )}
      </div>
      {quiz.ki && <div className="hint lb-ki-kosten">{kiZeile(quiz.ki, ki)}</div>}
    </div>
  );
}

function Ergebnis({
  quiz,
  antworten,
  richtig,
  ki,
  onNochmalFalsche,
  onNeu,
  onSpeichern,
  onOpenNote,
}: {
  quiz: Quiz;
  antworten: Antwort[];
  richtig: number;
  ki: KiStatus | null;
  onNochmalFalsche: () => void;
  onNeu: () => void;
  onSpeichern: () => void;
  onOpenNote: (path: string) => void;
}) {
  const n = quiz.fragen.length;
  const prozent = n > 0 ? Math.round((richtig / n) * 100) : 0;
  const falsche = quiz.fragen.filter((_, i) => antworten[i]?.gewusst === false);
  const lob = prozent >= 90 ? "Stark – das sitzt." : prozent >= 70 ? "Gut! Noch ein paar Lücken." : prozent >= 50 ? "Halb geschafft – wiederhole die falschen." : "Da geht noch was – schau dir die Quellen an.";
  return (
    <div className="kt-spiel-body">
      <div className="kt-ergebnis">
        <div className="kt-zahl">
          {richtig}
          <span>/ {n}</span>
        </div>
        <div className="kt-balken" aria-hidden>
          <div style={{ width: `${prozent}%` }} />
        </div>
        <div className="kt-lob">
          {prozent} % · {lob}
        </div>
      </div>
      <div className="row">
        <button className="btn primary" onClick={onNochmalFalsche} disabled={falsche.length === 0}>
          {Icon.refresh} Nur die falschen wiederholen ({falsche.length})
        </button>
        <button className="btn" onClick={onNeu}>
          Neues Quiz
        </button>
        {quiz.mitKi && (
          <button className="btn" onClick={onSpeichern}>
            {Icon.karten} Als Karteikarten speichern …
          </button>
        )}
      </div>
      {falsche.length > 0 && (
        <div className="kt-falsche">
          <div className="task-group-head">Noch einmal ansehen</div>
          {falsche.map((f, i) => (
            <div key={i} className="kt-falsch-item">
              <div className="kt-falsch-frage">{f.frage}</div>
              <div className="hint">Antwort: {f.antwort}</div>
              <Quelle f={f} onOpenNote={onOpenNote} />
            </div>
          ))}
        </div>
      )}
      {quiz.ki && <div className="hint lb-ki-kosten">{kiZeile(quiz.ki, ki)}</div>}
    </div>
  );
}

function SetVorschau({
  set,
  schreibt,
  onCancel,
  onConfirm,
}: {
  set: KarteikartenSet;
  schreibt: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const titleId = useId();
  return (
    <Modal onClose={onCancel} labelledBy={titleId} className="modal-geklaert kt-modal">
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.karten} Neues Karteikarten-Set
        </h2>
        <div className="hint">
          {set.karten} Karten · neue Datei <span className="mono-path">{set.relPath}</span>
        </div>
      </div>
      <div className="modal-body">
        <pre className="kt-vorschau">{set.text}</pre>
        {set.warnungen.map((w, i) => (
          <div key={i} className="hint lb-hinweis">
            {Icon.warning}
            <span>{w}</span>
          </div>
        ))}
        <div className="hint">Format „Frage / ? / Antwort“ – lesbar für das Obsidian-Plugin Spaced Repetition. Es wird nichts überschrieben.</div>
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={onCancel} disabled={schreibt}>
          Abbrechen
        </button>
        <button className="btn primary" onClick={onConfirm} disabled={schreibt} data-autofocus>
          {Icon.check} {schreibt ? "Wird angelegt …" : "Anlegen"}
        </button>
      </div>
    </Modal>
  );
}
