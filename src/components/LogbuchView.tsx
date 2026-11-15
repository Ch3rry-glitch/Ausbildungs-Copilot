// Ansicht „Logbuch“: Unterrichtstage je Fach mit Status, Entwurf (ohne oder mit KI), bearbeitbare
// Vorschau und „Kopieren und Moodle öffnen“.
// Regel 1: Der Entwurf und der Eintragstext leben nur im React-Zustand dieser Ansicht – nie in
// localStorage/sessionStorage/IndexedDB, nie im Vault, nie in der Konsole.
// Regel 2: Die Bewertung wird nie vorbelegt; ohne Zahl bleibt „Kopieren“ gesperrt.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  aeltesterOffen,
  api,
  baseName,
  errorText,
  formatTagDatum,
  isoWoche,
  LOGBUCH_STATUS_BADGE,
  samePath,
  type AbschnittQuelle,
  type KiAufruf,
  type KiModell,
  type KiStatus,
  type LogbuchEntwurf,
  type LogbuchFachTage,
  type LogbuchStatus,
  type LogbuchStatusReport,
  type LogbuchTag,
  type LogbuchTextAnfrage,
  type LogbuchUebersicht,
  type StatusFehler,
  type VaultChanged,
} from "../api";
import { Icon } from "../icons";
import { useToast } from "../toast";

export interface LogbuchRequest {
  path?: string;
  fach?: string;
  datum?: string;
  seq: number;
}

interface EditPunkt {
  id: number;
  text: string;
  vorschlag: boolean;
}
interface EditAbschnitt {
  titel: string;
  quelle: AbschnittQuelle;
  punkte: EditPunkt[];
}

type Busy = null | "entwurf" | "ki" | "kopieren" | "status";

const STATI: LogbuchStatus[] = ["offen", "kopiert", "abgegeben"];

/** Eigene Statusschreibungen melden sich als `own` – so lange danach gelten sie nicht als Änderung der Quelle. */
const EIGENE_SCHREIBUNG_MS = 4000;

let nextPunktId = 0;
function neuerPunkt(text = "", vorschlag = false): EditPunkt {
  nextPunktId += 1;
  return { id: nextPunktId, text, vorschlag };
}

const gleich = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();
/** wie `titel_norm` im Backend: Rand und abschließender Doppelpunkt zählen nicht */
const titelNorm = (s: string) => s.trim().replace(/:+$/, "").trim();
const notizenText = (n: number) => `${n} ${n === 1 ? "Notiz" : "Notizen"}`;

function kiZeile(k: KiAufruf, modelle: KiModell[] | undefined): string {
  const name = modelle?.find((m) => m.id === k.modell)?.name ?? k.modell;
  const tok = (n: number) => n.toLocaleString("de-DE");
  const kosten = k.kostenUsd.toLocaleString("de-DE", {
    minimumFractionDigits: 2,
    maximumFractionDigits: k.kostenUsd < 0.1 ? 4 : 2,
  });
  const versuche = k.versuche > 1 ? ` · ${k.versuche} Versuche` : "";
  return `KI: ${name} · ${tok(k.eingabeTokens)}+${tok(k.ausgabeTokens)} Tokens · ≈ ${kosten} $${versuche}`;
}

function statusDetail(r: LogbuchStatusReport): string[] {
  return [
    ...r.geschrieben.map((w) => `Geschrieben: ${w.relPath}`),
    ...r.geschrieben.flatMap((w) => w.warnungen),
    ...(r.sicherung ? [`Sicherung: ${r.sicherung}`] : []),
    ...r.fehler.map((f) => `${baseName(f.path)}: ${f.meldung}`),
  ];
}

/** Tage nach ISO-Kalenderwoche gruppieren (Reihenfolge bleibt: neueste zuerst). */
function nachWochen(tage: LogbuchTag[]): { key: string; kw: number; tage: LogbuchTag[] }[] {
  const out: { key: string; kw: number; tage: LogbuchTag[] }[] = [];
  for (const t of tage) {
    const w = isoWoche(t.datum);
    const key = `${w.jahr}-${w.kw}`;
    const last = out[out.length - 1];
    if (last && last.key === key) last.tage.push(t);
    else out.push({ key, kw: w.kw, tage: [t] });
  }
  return out;
}

export function LogbuchView({
  active,
  vaultPath,
  change,
  request,
  onOpenNote,
  dirtyPath,
  onGoSettings,
  onUngesichert,
}: {
  active: boolean;
  vaultPath: string | null;
  change: { seq: number; data: VaultChanged } | null;
  request: LogbuchRequest | null;
  onOpenNote: (path: string) => void;
  /** Pfad der Notiz im Editor, wenn sie ungespeicherte Änderungen hat */
  dirtyPath: string | null;
  onGoSettings: () => void;
  /** true, solange ein bearbeiteter Entwurf noch nicht kopiert ist (Schutz bei F5/Schließen) */
  onUngesichert?: (b: boolean) => void;
}) {
  const { notify } = useToast();

  // ---------- Übersicht ----------
  const [uebersicht, setUebersicht] = useState<LogbuchUebersicht | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [fach, setFach] = useState<string | null>(null);
  const [datum, setDatum] = useState<string | null>(null);
  const [kiStatus, setKiStatus] = useState<KiStatus | null>(null);
  const uebersichtSeq = useRef(0);

  // ---------- Entwurf (nur im Speicher) ----------
  const [entwurf, setEntwurf] = useState<LogbuchEntwurf | null>(null);
  const [abschnitte, setAbschnitte] = useState<EditAbschnitt[]>([]);
  const [bewertung, setBewertung] = useState(""); // nie vorbelegt (Regel 2)
  const [privatErlauben, setPrivatErlauben] = useState(false);
  const [busy, setBusy] = useState<Busy>(null);
  const [entwurfError, setEntwurfError] = useState<string | null>(null);
  const [vorschau, setVorschau] = useState<string | null>(null);
  const [vorschauError, setVorschauError] = useState<string | null>(null);
  const [statusFehler, setStatusFehler] = useState<StatusFehler[]>([]);
  const [justCopied, setJustCopied] = useState(false);
  const [stale, setStale] = useState(false);
  // Einstellungen des Fachs (Abschnitte, Skala) passen nicht mehr zum Entwurf
  const [konfigVeraltet, setKonfigVeraltet] = useState(false);
  // Entwurf bearbeitet und noch nicht kopiert
  const [ungesichert, setUngesichert] = useState(false);
  const entwurfSeq = useRef(0);
  // zählt Änderungen an Quellnotizen – eine Antwort, während der sich etwas geändert hat, ist veraltet
  const quellSeq = useRef(0);
  const vorschauSeq = useRef(0);
  const tagSeq = useRef(0);
  const eigeneSchreibung = useRef<{ laeuft: boolean; bis: number; pfade: string[] }>({ laeuft: false, bis: 0, pfade: [] });
  const felder = useRef(new Map<number, HTMLTextAreaElement>());
  const fokus = useRef<{ id: number; pos: "start" | "end" } | null>(null);

  const loadUebersicht = useCallback(async (): Promise<LogbuchUebersicht | null> => {
    uebersichtSeq.current += 1;
    const mine = uebersichtSeq.current;
    try {
      const u = await api.logbuchUebersicht();
      if (mine === uebersichtSeq.current) {
        setUebersicht(u);
        setLoadError(null);
      }
      return u;
    } catch (e) {
      if (mine === uebersichtSeq.current) setLoadError(errorText(e));
      return null;
    }
  }, []);

  const loadKi = useCallback(async () => {
    try {
      setKiStatus(await api.kiStatus());
    } catch {
      setKiStatus(null); // ohne Status gilt: kein Schlüssel → nur „Ohne KI befüllen“
    }
  }, []);

  function verwerfeEntwurf() {
    entwurfSeq.current += 1; // laufende Antworten zählen nicht mehr
    vorschauSeq.current += 1;
    tagSeq.current += 1;
    setEntwurf(null);
    setAbschnitte([]);
    setBewertung("");
    setPrivatErlauben(false);
    setBusy(null);
    setEntwurfError(null);
    setVorschau(null);
    setVorschauError(null);
    setStatusFehler([]);
    setJustCopied(false);
    setStale(false);
    setKonfigVeraltet(false);
    setUngesichert(false);
  }

  function waehleFach(kurz: string | null) {
    if (kurz === fach) return;
    setFach(kurz);
    setDatum(null);
    verwerfeEntwurf();
  }

  function waehleTag(kurz: string, d: string) {
    if (kurz === fach && d === datum) return; // gleicher Tag: Entwurf bleibt
    setFach(kurz);
    setDatum(d);
    verwerfeEntwurf();
    void erzeuge(kurz, d, false);
  }

  async function erzeuge(kurz: string, d: string, mitKi: boolean) {
    entwurfSeq.current += 1;
    const mine = entwurfSeq.current;
    const q0 = quellSeq.current;
    const privat = mitKi && privatErlauben && hatPrivat;
    if (mitKi) setPrivatErlauben(false); // Regel 10: „10 Privat/“ nur für genau diesen Vorgang
    setBusy(mitKi ? "ki" : "entwurf");
    setEntwurfError(null);
    try {
      const e = await api.logbuchEntwurf({ fach: kurz, datum: d, mitKi, privatErlauben: privat });
      if (mine !== entwurfSeq.current) return;
      setEntwurf(e);
      setAbschnitte(
        e.abschnitte.map((a) => ({
          titel: a.titel,
          quelle: a.quelle,
          punkte: a.punkte.map((p) => neuerPunkt(p.text, p.vorschlag)),
        })),
      );
      // hat sich eine Quelle während der Anfrage geändert, ist der Entwurf schon wieder veraltet
      setStale(quellSeq.current !== q0);
      setKonfigVeraltet(false);
      setUngesichert(false);
      setStatusFehler([]);
    } catch (err) {
      if (mine !== entwurfSeq.current) return;
      setEntwurfError(errorText(err)); // ein vorhandener Entwurf bleibt stehen
    } finally {
      if (mine === entwurfSeq.current) setBusy(null);
    }
  }

  // ---------- Anfragen von außen (Strg+L, Notiz, Heute-Karte) ----------
  async function resolveRequest(req: LogbuchRequest) {
    const u = await loadUebersicht();
    if (!u) return;
    let hit: { fach: string; datum: string | null } | null = null;
    if (req.path) {
      for (const f of u.faecher) {
        const t = f.tage.find((x) => x.notizen.some((n) => samePath(n.path, req.path)));
        if (t) {
          hit = { fach: f.kurz, datum: t.datum };
          break;
        }
      }
    }
    if (!hit && req.fach) {
      const f = u.faecher.find((x) => gleich(x.kurz, req.fach!));
      if (f) {
        const t = req.datum ? f.tage.find((x) => x.datum === req.datum) : aeltesterOffen(f);
        hit = { fach: f.kurz, datum: t?.datum ?? null };
      }
    }
    if (!hit) {
      notify({ kind: "info", text: "Für dieses Fach ist kein Logbuch aktiv." });
      return;
    }
    // nach dem Warten: die aktuellen Funktionen (gewählter Tag kann sich geändert haben)
    if (hit.datum) actions.current.anfrageFuerTag(hit.fach, hit.datum, req.path);
    else actions.current.waehleFach(hit.fach);
  }

  /** Wie waehleTag; kommt die Anfrage aus einer Notiz, die der Entwurf des Tages noch nicht kennt
   *  (z. B. Block 2 der Doppelstunde), wird ein unbearbeiteter Entwurf neu erzeugt. Ein bearbeiteter
   *  bleibt stehen – der Hinweis „Notizen geändert“ sperrt dann das Kopieren. */
  function anfrageFuerTag(kurz: string, d: string, path?: string) {
    const gleicherTag = kurz === fach && d === datum;
    const fehlt = !!path && !!entwurf && !entwurf.quellen.some((q) => samePath(q.path, path));
    if (gleicherTag && fehlt && !ungesichert && busy === null) void erzeuge(kurz, d, false);
    else waehleTag(kurz, d);
  }

  // Aktuelle Funktionen für Effekte (keine veralteten Closures)
  const actions = useRef({ resolveRequest, waehleFach, waehleTag, anfrageFuerTag, verwerfeEntwurf });
  actions.current = { resolveRequest, waehleFach, waehleTag, anfrageFuerTag, verwerfeEntwurf };

  // ---------- Effekte ----------
  // Sichtbar werden → Übersicht und KI-Status frisch (Einstellungen können sich geändert haben)
  useEffect(() => {
    if (!active) return;
    void loadUebersicht();
    void loadKi();
  }, [active, loadUebersicht, loadKi]);

  // Gewähltes Fach nicht (mehr) aktiv → erstes Fach wählen, Entwurf verwerfen
  useEffect(() => {
    if (!uebersicht) return;
    if (fach && uebersicht.faecher.some((f) => f.kurz === fach)) return;
    actions.current.waehleFach(uebersicht.faecher[0]?.kurz ?? null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uebersicht]);

  // Anfragen der App (StrictMode-sicher: jede seq nur einmal)
  const handledRequest = useRef(0);
  useEffect(() => {
    if (!request || request.seq <= handledRequest.current) return;
    handledRequest.current = request.seq;
    void actions.current.resolveRequest(request);
  }, [request]);

  // Vault-Ereignisse: Übersicht neu laden; geänderte Quellnotiz → Hinweis „Entwurf veraltet“
  const handledChange = useRef(change?.seq ?? 0);
  useEffect(() => {
    if (!change || change.seq <= handledChange.current) return;
    handledChange.current = change.seq;
    void loadUebersicht();
    // Quellen: die des Entwurfs und die des gewählten Tages (auch während ein Entwurf noch lädt)
    const quellen = [...(entwurf?.quellen.map((q) => q.path) ?? []), ...(tag?.notizen.map((n) => n.path) ?? [])];
    const betrifft = (ps: string[]) => ps.some((p) => quellen.some((q) => samePath(q, p)));
    // eigene Statusschreibungen zählen nur für genau die geschriebenen Pfade nicht als Änderung
    const eigen = eigeneSchreibung.current;
    const eigenAktiv = eigen.laeuft || Date.now() < eigen.bis;
    const own = change.data.own.filter((p) => !(eigenAktiv && eigen.pfade.some((q) => samePath(q, p))));
    // verschoben/umbenannt = alter Pfad in `removed`
    if (betrifft(change.data.changed) || betrifft(change.data.removed) || betrifft(own)) {
      quellSeq.current += 1;
      if (entwurf) setStale(true);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [change]);

  // Vault gewechselt → alles verwerfen
  const prevVault = useRef(vaultPath);
  useEffect(() => {
    if (prevVault.current === vaultPath) return;
    prevVault.current = vaultPath;
    setFach(null);
    setDatum(null);
    actions.current.verwerfeEntwurf();
    setUebersicht(null);
    void loadUebersicht();
  }, [vaultPath, loadUebersicht]);

  // Sichtbar werden mit vorhandenem Entwurf → passen Abschnitte und Bewertung noch zu den Einstellungen?
  useEffect(() => {
    if (!active || !entwurf) return;
    let alive = true;
    api
      .logbuchEinstellungen()
      .then((l) => {
        if (!alive) return;
        const f = l.find((x) => gleich(x.kurz, entwurf.fach));
        if (!f) return; // Fach nicht mehr aktiv → die Übersicht verwirft den Entwurf
        const abschnittePassen =
          f.abschnitte.length === entwurf.abschnitte.length &&
          f.abschnitte.every((a, i) => titelNorm(a.titel) === titelNorm(entwurf.abschnitte[i].titel));
        const skalaPasst = (f.selbstbewertung ?? null) === (entwurf.bewertung?.skala ?? null);
        setKonfigVeraltet(!abschnittePassen || !skalaPasst);
      })
      .catch(() => {
        // ohne Einstellungen keine Aussage; das Backend prüft beim Kopieren ohnehin
      });
    return () => {
      alive = false;
    };
  }, [active, entwurf]);

  // Neu angelegter oder per Enter/Backspace gewechselter Punkt bekommt den Fokus
  useLayoutEffect(() => {
    const f = fokus.current;
    if (!f) return;
    fokus.current = null;
    const el = felder.current.get(f.id);
    if (!el) return;
    el.focus();
    const pos = f.pos === "start" ? 0 : el.value.length;
    el.setSelectionRange(pos, pos);
  }, [abschnitte]);

  // ---------- Abgeleitete Werte ----------
  const faecher = uebersicht?.faecher ?? [];
  const fachTage: LogbuchFachTage | null = faecher.find((f) => f.kurz === fach) ?? null;
  const tag = fachTage && datum ? (fachTage.tage.find((t) => t.datum === datum) ?? null) : null;
  const wochen = useMemo(() => (fachTage ? nachWochen(fachTage.tage) : []), [fachTage]);
  const status: LogbuchStatus = tag?.status ?? entwurf?.status ?? "offen";

  const skala = entwurf?.bewertung?.skala ?? null;
  const bewertungZahl = useMemo(() => {
    if (skala == null) return null;
    const s = bewertung.trim();
    if (!/^\d+$/.test(s)) return null;
    const n = Number(s);
    return n >= 1 && n <= skala ? n : null;
  }, [bewertung, skala]);
  const bewertungFehlt = skala != null && bewertungZahl === null;
  const bewertungUngueltig = skala != null && bewertung.trim() !== "" && bewertungZahl === null;

  // Pfade des Tages (für den Status) und Titel einer davon, die im Editor ungespeichert ist
  const tagPfade = useMemo(
    () => (tag ? tag.notizen.map((n) => n.path) : (entwurf?.quellen.map((q) => q.path) ?? [])),
    [tag, entwurf],
  );
  const dirtyTitel = useMemo(() => {
    if (!dirtyPath) return null;
    const q = entwurf?.quellen.find((x) => samePath(x.path, dirtyPath));
    if (q) return q.title;
    const n = tag?.notizen.find((x) => samePath(x.path, dirtyPath));
    return n ? n.title : null;
  }, [dirtyPath, entwurf, tag]);

  const kiOk = !!kiStatus?.schluesselGespeichert;
  // Was die KI bekäme, kommt aus der frischen Übersicht – das Backend liest beim Klick ebenso neu (Regel 10)
  const basis = tag?.notizen ?? [];
  const hatPrivat = basis.some((n) => n.privat);
  const privatAn = privatErlauben && hatPrivat;
  const mitgeschickt = basis.filter((n) => !n.privat || privatAn).map((n) => n.title);
  // Notizen des Tages ≠ Quellen des Entwurfs (neue Notiz, verschoben, entfernt, Ordner „10 Privat/“ gewechselt)
  // (Tag ganz aus der Übersicht verschwunden = alle Notizen verschoben/entfernt)
  const quellenAbweichung =
    !!fachTage &&
    !!entwurf &&
    (!tag ||
      tag.notizen.length !== entwurf.quellen.length ||
      !tag.notizen.every((n) => entwurf.quellen.some((q) => samePath(q.path, n.path) && q.privat === n.privat)));
  const gesperrt = busy !== null;
  const kannKopieren =
    !!entwurf && !gesperrt && !bewertungFehlt && !dirtyTitel && !vorschauError && !quellenAbweichung && !konfigVeraltet;
  const statusGesperrt = gesperrt || !!dirtyTitel || tagPfade.length === 0;
  const zeigeAbgegeben = (justCopied || status === "kopiert") && status !== "abgegeben";

  // Keine private Quelle mehr → die Erlaubnis gilt nicht weiter
  useEffect(() => {
    if (!hatPrivat && privatErlauben) setPrivatErlauben(false);
  }, [hatPrivat, privatErlauben]);

  // App erfährt, ob ein bearbeiteter Entwurf verloren ginge (F5, Strg+R, Fenster schließen)
  const ungesichertMelden = useRef(onUngesichert);
  ungesichertMelden.current = onUngesichert;
  const istUngesichert = !!entwurf && ungesichert;
  useEffect(() => {
    ungesichertMelden.current?.(istUngesichert);
  }, [istUngesichert]);
  useEffect(() => () => ungesichertMelden.current?.(false), []);

  // ---------- Vorschau für Moodle (entprellt) ----------
  useEffect(() => {
    vorschauSeq.current += 1;
    const mine = vorschauSeq.current;
    if (!entwurf) return;
    const req: LogbuchTextAnfrage = {
      fach: entwurf.fach,
      datum: entwurf.datum,
      abschnitte: textAbschnitte(abschnitte),
      bewertung: entwurf.bewertung ? bewertungZahl : null,
    };
    const t = setTimeout(() => {
      api
        .logbuchText(req)
        .then((text) => {
          if (mine !== vorschauSeq.current) return;
          setVorschau(text);
          setVorschauError(null);
        })
        .catch((e) => {
          if (mine !== vorschauSeq.current) return;
          setVorschauError(errorText(e));
        });
    }, 150);
    return () => clearTimeout(t);
  }, [entwurf, abschnitte, bewertungZahl]);

  // ---------- Punkte bearbeiten ----------
  function setPunkte(ai: number, punkte: EditPunkt[]) {
    setAbschnitte(abschnitte.map((a, i) => (i === ai ? { ...a, punkte } : a)));
    setUngesichert(true);
  }

  function aendern(ai: number, idx: number, text: string) {
    const a = abschnitte[ai];
    const alt = a.punkte[idx];
    const zeilen = text.split(/\r\n|\r|\n/);
    if (zeilen.length === 1) {
      // Einen Vorschlag zu bearbeiten lässt die Markierung stehen
      setPunkte(ai, a.punkte.map((p, i) => (i === idx ? { ...p, text } : p)));
      return;
    }
    // Eingefügter Text mit Zeilenumbrüchen → je Zeile ein Punkt
    const weitere = zeilen.slice(1).map((z) => neuerPunkt(z));
    fokus.current = { id: weitere[weitere.length - 1].id, pos: "end" };
    setPunkte(ai, [...a.punkte.slice(0, idx), { ...alt, text: zeilen[0] }, ...weitere, ...a.punkte.slice(idx + 1)]);
  }

  function taste(e: KeyboardEvent<HTMLTextAreaElement>, ai: number, idx: number) {
    if (e.nativeEvent.isComposing) return;
    const a = abschnitte[ai];
    const p = a.punkte[idx];
    if (e.key === "Enter") {
      // Enter: neuer Punkt darunter; Text hinter dem Cursor wandert mit
      e.preventDefault();
      const el = e.currentTarget;
      const vor = p.text.slice(0, el.selectionStart);
      const nach = p.text.slice(el.selectionEnd);
      const neu = neuerPunkt(nach, nach !== "" && p.vorschlag);
      fokus.current = { id: neu.id, pos: "start" };
      setPunkte(ai, [...a.punkte.slice(0, idx), { ...p, text: vor }, neu, ...a.punkte.slice(idx + 1)]);
    } else if (e.key === "Backspace" && p.text === "" && idx > 0) {
      // Rücktaste im leeren Punkt: Punkt entfernen, zurück ans Ende des vorigen
      e.preventDefault();
      fokus.current = { id: a.punkte[idx - 1].id, pos: "end" };
      setPunkte(ai, a.punkte.filter((_, i) => i !== idx));
    }
  }

  function entfernen(ai: number, idx: number) {
    setPunkte(ai, abschnitte[ai].punkte.filter((_, i) => i !== idx));
  }

  function hinzufuegen(ai: number) {
    const neu = neuerPunkt();
    fokus.current = { id: neu.id, pos: "start" };
    setPunkte(ai, [...abschnitte[ai].punkte, neu]);
  }

  // ---------- Schreibende Aktionen ----------
  async function eigeneStatusSchreibung<T>(pfade: string[], fn: () => Promise<T>): Promise<T> {
    eigeneSchreibung.current = { laeuft: true, bis: 0, pfade };
    try {
      return await fn();
    } finally {
      eigeneSchreibung.current = { laeuft: false, bis: Date.now() + EIGENE_SCHREIBUNG_MS, pfade };
    }
  }

  async function kopieren() {
    if (!entwurf || !kannKopieren) return;
    const meinTag = tagSeq.current;
    const pfade = entwurf.quellen.map((q) => q.path);
    setBusy("kopieren");
    try {
      const r = await eigeneStatusSchreibung(pfade, () =>
        api.logbuchKopieren({
          fach: entwurf.fach,
          datum: entwurf.datum,
          abschnitte: textAbschnitte(abschnitte),
          bewertung: entwurf.bewertung ? bewertungZahl : null,
          pfade,
        }),
      );
      const n = r.status.geschrieben.length;
      const teile = ["Kopiert"];
      if (r.moodleGeoeffnet) teile.push("Moodle geöffnet");
      teile.push(n > 0 ? `Status „kopiert“ in ${notizenText(n)}` : "Status unverändert");
      notify({
        kind: n > 0 ? "saved" : "info",
        text: teile.join(" · "),
        path: r.status.geschrieben[0]?.relPath,
        detail: statusDetail(r.status),
      });
      if (!r.moodleGeoeffnet) {
        notify({ kind: "error", text: r.moodleFehler ?? `Moodle konnte nicht geöffnet werden: ${r.moodleUrl}` });
      }
      if (meinTag === tagSeq.current) {
        setJustCopied(true);
        setUngesichert(false); // der Text ist in der Zwischenablage
        setStatusFehler(r.status.fehler);
      }
      void loadUebersicht();
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    } finally {
      if (meinTag === tagSeq.current) setBusy(null);
    }
  }

  async function statusSetzen(s: LogbuchStatus) {
    if (statusGesperrt || s === status) return;
    const meinTag = tagSeq.current;
    const pfade = tagPfade;
    setBusy("status");
    try {
      const r = await eigeneStatusSchreibung(pfade, () => api.logbuchStatusSetzen(pfade, s));
      const n = r.geschrieben.length;
      notify({
        kind: n > 0 ? "saved" : "info",
        text: n > 0 ? `Status „${s}“ in ${notizenText(n)}` : `Status „${s}“ – keine Änderung`,
        path: r.geschrieben[0]?.relPath,
        detail: statusDetail(r),
      });
      if (meinTag === tagSeq.current) {
        setStatusFehler(r.fehler);
        if (s !== "kopiert") setJustCopied(false);
      }
      void loadUebersicht();
    } catch (e) {
      notify({ kind: "error", text: errorText(e) });
    } finally {
      if (meinTag === tagSeq.current) setBusy(null);
    }
  }

  // ---------- Darstellung ----------
  return (
    <div className="view-wrap">
      <div className="page-head">
        <div>
          <h1 className="page-title">Logbuch</h1>
          <div className="page-sub">Einträge für Moodle – der Text landet nur in der Zwischenablage, nie im Vault.</div>
        </div>
      </div>

      {loadError && <div className="error">{loadError}</div>}

      {uebersicht && faecher.length === 0 && (
        <section className="panel glass">
          <div className="empty day-empty">
            <div>
              <div className="empty-icon">{Icon.logbuch}</div>
              <p>Noch kein Fach mit Logbuch – in den Einstellungen einschalten.</p>
              <button className="btn small" onClick={onGoSettings}>
                {Icon.einstellungen} Einstellungen öffnen
              </button>
            </div>
          </div>
        </section>
      )}

      {faecher.length > 0 && (
        <>
          <div className="pills fach-pills">
            {faecher.map((f) => (
              <button
                key={f.kurz}
                className={`pill ${fach === f.kurz ? "active" : ""}`}
                onClick={() => waehleFach(f.kurz)}
                title={f.ziel}
              >
                {f.kurz}
                {f.offen > 0 && <span className="pill-count">{f.offen}</span>}
              </button>
            ))}
          </div>

          <div className="logbuch-grid">
            <section className="panel glass">
              <div className="panel-head">
                <span className="panel-title">Unterrichtstage</span>
                {fachTage && fachTage.offen > 0 && <span className="badge amber">{fachTage.offen} offen</span>}
              </div>
              <div className="panel-body">
                {fachTage && fachTage.tage.length === 0 && <div className="empty">Noch keine Unterrichtsnotizen in diesem Fach.</div>}
                {wochen.map((w) => {
                  const erledigt = w.tage.filter((t) => t.status !== "offen").length;
                  const soll = fachTage?.eintraegeProWoche ?? 0;
                  return (
                    <div key={w.key} className="lb-week">
                      <div className="task-group-head">
                        KW {w.kw} · {soll > 0 ? `${erledigt} von ${soll}` : erledigt}
                        {soll > 0 && erledigt >= soll && <span className="badge mint">{Icon.check}</span>}
                      </div>
                      {w.tage.map((t) => (
                        <TagZeile
                          key={t.datum}
                          t={t}
                          aktiv={t.datum === datum}
                          onClick={() => fachTage && waehleTag(fachTage.kurz, t.datum)}
                        />
                      ))}
                    </div>
                  );
                })}
              </div>
            </section>

            <section className="panel glass lb-detail">
              {!datum && (
                <div className="empty">
                  <div>
                    <div className="empty-icon">{Icon.logbuch}</div>
                    Wähle links einen Unterrichtstag.
                  </div>
                </div>
              )}
              {datum && fach && (
                <div className="lb-detail-body">
                  <div className="lb-head">
                    <div className="editor-title-row">
                      <h2 className="preview-title">
                        {fach} · {formatTagDatum(datum)}
                      </h2>
                      <span className={`badge ${LOGBUCH_STATUS_BADGE[status]}`}>{status}</span>
                    </div>
                    <div className="pills lb-status-pills" role="group" aria-label="Status setzen">
                      {STATI.map((s) => (
                        <button
                          key={s}
                          className={`pill ${status === s ? `active st-${s}` : ""}`}
                          disabled={statusGesperrt}
                          onClick={() => void statusSetzen(s)}
                          aria-pressed={status === s}
                        >
                          {s}
                        </button>
                      ))}
                    </div>
                  </div>

                  <div className="lb-quellen">
                    <span className="lb-label">Quellnotizen</span>
                    {(entwurf ? entwurf.quellen : (tag?.notizen ?? [])).map((q) => (
                      <span key={q.path} className="lb-quelle">
                        <button className="lb-link" onClick={() => onOpenNote(q.path)} title={q.path}>
                          {Icon.datei}
                          {q.title}
                        </button>
                        {q.privat && <span className="badge muted">10 Privat</span>}
                        {"ekEntfernt" in q && q.ekEntfernt && <span className="badge muted">EK entfernt</span>}
                      </span>
                    ))}
                  </div>

                  {dirtyTitel && (
                    <div className="banner lb-banner">
                      {Icon.warning}
                      <span>„{dirtyTitel}“ hat ungespeicherte Änderungen – erst speichern.</span>
                    </div>
                  )}
                  {quellenAbweichung ? (
                    <div className="banner lb-banner">
                      {Icon.warning}
                      <span>
                        Die Notizen dieses Tages haben sich seit dem Entwurf geändert (neu, verschoben oder entfernt) – „Ohne KI
                        befüllen“ übernimmt den neuen Stand. Bis dahin ist Kopieren gesperrt.
                      </span>
                    </div>
                  ) : (
                    stale && (
                      <div className="banner lb-banner">
                        {Icon.warning}
                        <span>Eine Quellnotiz wurde seit dem Entwurf geändert – „Ohne KI befüllen“ übernimmt den neuen Stand.</span>
                      </div>
                    )
                  )}
                  {konfigVeraltet && (
                    <div className="banner lb-banner">
                      {Icon.warning}
                      <span>
                        Die Logbuch-Einstellungen für {fach} haben sich geändert – bitte „Ohne KI befüllen“. Bis dahin ist Kopieren
                        gesperrt.
                      </span>
                    </div>
                  )}

                  {entwurf && entwurf.warnungen.length > 0 && (
                    <div className="lb-hinweise">
                      {entwurf.warnungen.map((w, i) => (
                        <div key={i} className="hint lb-hinweis">
                          {Icon.warning}
                          <span>{w}</span>
                        </div>
                      ))}
                    </div>
                  )}

                  {!entwurf && !entwurfError && busy === "entwurf" && (
                    <div className="lb-busy">
                      <span className="spinner" /> Lade Entwurf …
                    </div>
                  )}

                  {(entwurf || entwurfError) && (
                    <div className="lb-ki">
                      <div className="lb-ki-line">
                        Mitgeschickt werden: <strong>{mitgeschickt.length > 0 ? mitgeschickt.join(", ") : "—"}</strong>
                      </div>
                      <div className="hint">
                        {privatAn
                          ? "EK-Angaben werden vorher entfernt · „10 Privat/“ wird diesmal mitgeschickt"
                          : "EK-Angaben werden vorher entfernt · „10 Privat/“ bleibt aus"}
                      </div>
                      {hatPrivat && (
                        <label className="check">
                          <input
                            type="checkbox"
                            checked={privatErlauben}
                            onChange={(e) => setPrivatErlauben(e.target.checked)}
                            disabled={gesperrt}
                          />
                          „10 Privat/“ für diesen Vorgang einschließen
                        </label>
                      )}
                      <div className="row lb-ki-buttons">
                        <button
                          className="btn small primary"
                          onClick={() => void erzeuge(fach, datum, true)}
                          disabled={!kiOk || gesperrt || !tag}
                        >
                          {Icon.sparkle} Mit KI formulieren
                        </button>
                        <button className="btn small" onClick={() => void erzeuge(fach, datum, false)} disabled={gesperrt}>
                          Ohne KI befüllen
                        </button>
                        {busy === "ki" && (
                          <span className="lb-busy">
                            <span className="spinner" /> KI formuliert …
                          </span>
                        )}
                        {busy === "entwurf" && (
                          <span className="lb-busy">
                            <span className="spinner" /> Lade Entwurf …
                          </span>
                        )}
                      </div>
                      {!kiOk && (
                        <div className="hint">
                          {kiStatus?.fehler ?? "Kein KI-Schlüssel hinterlegt – Einstellungen → KI, oder „Ohne KI befüllen“."}{" "}
                          <button className="lb-link inline" onClick={onGoSettings}>
                            Zu den Einstellungen
                          </button>
                        </div>
                      )}
                      {entwurf?.ki && <div className="hint lb-ki-kosten">{kiZeile(entwurf.ki, kiStatus?.modelle)}</div>}
                    </div>
                  )}

                  {entwurfError && <div className="error">{entwurfError}</div>}

                  {abschnitte.map((a, ai) => (
                    <div key={`${ai}-${a.titel}`} className="lb-section">
                      <div className="lb-section-title">{a.titel}</div>
                      {a.punkte.length === 0 && <div className="hint">Leer – in Moodle steht dann „- “.</div>}
                      {a.punkte.map((p, idx) => (
                        <div key={p.id} className={`lb-punkt ${p.vorschlag ? "vorschlag" : ""}`}>
                          <span className="lb-bullet">–</span>
                          <textarea
                            ref={(el) => {
                              if (el) felder.current.set(p.id, el);
                              else felder.current.delete(p.id);
                            }}
                            className="input lb-punkt-input"
                            rows={1}
                            value={p.text}
                            onChange={(e) => aendern(ai, idx, e.target.value)}
                            onKeyDown={(e) => taste(e, ai, idx)}
                            disabled={busy === "ki"}
                            spellCheck
                            aria-label={`${a.titel}: Punkt ${idx + 1}`}
                          />
                          {p.vorschlag && <span className="badge mint lb-vorschlag">Vorschlag (KI)</span>}
                          <button
                            className="lb-remove"
                            onClick={() => entfernen(ai, idx)}
                            title="Punkt entfernen"
                            aria-label="Punkt entfernen"
                            disabled={busy === "ki"}
                          >
                            ×
                          </button>
                        </div>
                      ))}
                      <button className="btn small lb-add" onClick={() => hinzufuegen(ai)} disabled={busy === "ki"}>
                        + Punkt
                      </button>
                    </div>
                  ))}

                  {entwurf?.bewertung && (
                    <div className="lb-section lb-bewertung">
                      <label className="lb-bewertung-row">
                        <span className="lb-section-title">{entwurf.bewertung.titel}</span>
                        {/* type="text": ein Mausrad- oder Pfeiltasten-Schritt darf keine Zahl eintragen (Regel 2) */}
                        <input
                          type="text"
                          className="input lb-bewertung-input"
                          inputMode="numeric"
                          autoComplete="off"
                          maxLength={3}
                          value={bewertung}
                          onChange={(e) => {
                            setBewertung(e.target.value);
                            setUngesichert(true);
                          }}
                          placeholder="__"
                        />
                        <span className="lb-skala">/ {entwurf.bewertung.skala}</span>
                      </label>
                      <div className="hint">Die Bewertung trägst du selbst ein – ohne Zahl kein Kopieren.</div>
                      {bewertungUngueltig && (
                        <div className="hint lb-warn-text">Bitte eine ganze Zahl von 1 bis {entwurf.bewertung.skala} eintragen.</div>
                      )}
                    </div>
                  )}

                  {entwurf && (
                    <div className="lb-section">
                      <div className="lb-section-title">Vorschau für Moodle</div>
                      {vorschauError ? <div className="error">{vorschauError}</div> : <pre className="lb-vorschau">{vorschau ?? ""}</pre>}
                    </div>
                  )}

                  {statusFehler.length > 0 && (
                    <div>
                      {statusFehler.map((f) => (
                        <div key={f.path} className="warn">
                          {baseName(f.path)}: {f.meldung}
                        </div>
                      ))}
                    </div>
                  )}

                  {entwurf && (
                    <div className="lb-actions">
                      <div className="row">
                        <button className="btn primary" onClick={() => void kopieren()} disabled={!kannKopieren}>
                          {Icon.kopieren} {busy === "kopieren" ? "Wird kopiert …" : "Kopieren und Moodle öffnen"}
                        </button>
                        {zeigeAbgegeben && (
                          <button className="btn" onClick={() => void statusSetzen("abgegeben")} disabled={statusGesperrt}>
                            {Icon.check} Als abgegeben markieren
                          </button>
                        )}
                      </div>
                      {bewertungFehlt && <div className="hint lb-warn-text">Die Bewertung trägst du selbst ein – ohne Zahl kein Kopieren.</div>}
                      {dirtyTitel && <div className="hint lb-warn-text">„{dirtyTitel}“ hat ungespeicherte Änderungen – erst speichern.</div>}
                      <div className="hint">
                        Moodle: <span className="mono-path">{entwurf.moodleUrl}</span>
                      </div>
                    </div>
                  )}
                </div>
              )}
            </section>
          </div>
        </>
      )}
    </div>
  );
}

/** Leere Punkte fallen weg; jeder Punkt ist eine Zeile (Umbrüche hat aendern() schon aufgeteilt). */
function textAbschnitte(abschnitte: EditAbschnitt[]) {
  return abschnitte.map((a) => ({
    titel: a.titel,
    punkte: a.punkte.map((p) => p.text.trim()).filter((t) => t !== ""),
  }));
}

function TagZeile({ t, aktiv, onClick }: { t: LogbuchTag; aktiv: boolean; onClick: () => void }) {
  const themen = t.notizen.map((n) => n.thema ?? n.title).join(" · ");
  return (
    <button
      className={`note-item lb-day ${aktiv ? "active" : ""} ${t.zaehlt ? "" : "zaehlt-nicht"}`}
      onClick={onClick}
      title={t.zaehlt ? undefined : "Vor „Offen zählen ab“ – zählt nicht als offen"}
    >
      <div className="lb-day-row">
        <span className="lb-day-date">{formatTagDatum(t.datum)}</span>
        <span className={`badge ${LOGBUCH_STATUS_BADGE[t.status]}`}>{t.status}</span>
      </div>
      {themen && (
        <div className="note-meta">
          <span className="lb-day-themen">{themen}</span>
        </div>
      )}
    </button>
  );
}
