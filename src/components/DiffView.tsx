// Zeilen-Diff (LCS) für die Bestätigung vor jedem Schreiben (Regel 9): nur geänderte Stellen mit
// zwei Zeilen Kontext, „−“ rot, „+“ mint. Der Text wird nur angezeigt – nie geloggt, nie gespeichert.
import { useId, useMemo } from "react";
import { baseName, type Aenderung } from "../api";
import { Icon } from "../icons";
import { Modal } from "./Modal";

export type DiffTyp = "gleich" | "weg" | "neu";

export interface DiffZeile {
  typ: DiffTyp;
  text: string;
  /** Zeilennummer vorher bzw. nachher (1-basiert); null = gibt es auf dieser Seite nicht */
  alt: number | null;
  neu: number | null;
}

export type DiffStueck = { typ: "zeilen"; zeilen: DiffZeile[] } | { typ: "luecke"; anzahl: number };

/** Mehr Zellen rechnet der LCS nicht (Dateien < 1000 Zeilen bleiben weit darunter). */
const MAX_ZELLEN = 4_000_000;

/** `vorher`/`nachher` kommen dekodiert mit „\n“; ein abschließender Umbruch ergibt eine leere letzte Zeile. */
function zeilenVon(text: string): string[] {
  return text === "" ? [] : text.split("\n");
}

/** Zeilen-Diff: gemeinsamer Anfang und Ende direkt, dazwischen längste gemeinsame Teilfolge. */
export function zeilenDiff(vorher: string, nachher: string): DiffZeile[] {
  const a = zeilenVon(vorher);
  const b = zeilenVon(nachher);
  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start++;
  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA--;
    endB--;
  }

  const out: DiffZeile[] = [];
  for (let i = 0; i < start; i++) out.push({ typ: "gleich", text: a[i], alt: i + 1, neu: i + 1 });

  const n = endA - start;
  const m = endB - start;
  if (n * m > MAX_ZELLEN) {
    // zu groß für den LCS: alles dazwischen als ersetzt zeigen
    for (let i = start; i < endA; i++) out.push({ typ: "weg", text: a[i], alt: i + 1, neu: null });
    for (let j = start; j < endB; j++) out.push({ typ: "neu", text: b[j], alt: null, neu: j + 1 });
  } else {
    // L[i][j] = Länge der LCS von a[start+i..endA] und b[start+j..endB]
    const breite = m + 1;
    const L = new Uint32Array((n + 1) * breite);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        L[i * breite + j] =
          a[start + i] === b[start + j]
            ? L[(i + 1) * breite + j + 1] + 1
            : Math.max(L[(i + 1) * breite + j], L[i * breite + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n || j < m) {
      if (i < n && j < m && a[start + i] === b[start + j]) {
        out.push({ typ: "gleich", text: a[start + i], alt: start + i + 1, neu: start + j + 1 });
        i++;
        j++;
      } else if (j >= m || (i < n && L[(i + 1) * breite + j] >= L[i * breite + j + 1])) {
        out.push({ typ: "weg", text: a[start + i], alt: start + i + 1, neu: null });
        i++;
      } else {
        out.push({ typ: "neu", text: b[start + j], alt: null, neu: start + j + 1 });
        j++;
      }
    }
  }

  for (let k = 0; endA + k < a.length; k++) {
    out.push({ typ: "gleich", text: a[endA + k], alt: endA + k + 1, neu: endB + k + 1 });
  }
  return out;
}

/** Nur geänderte Zeilen mit `kontext` Zeilen davor/danach; der Rest wird zu „… n unveränderte Zeilen“. */
export function diffStuecke(zeilen: DiffZeile[], kontext = 2): DiffStueck[] {
  const zeigen = new Array<boolean>(zeilen.length).fill(false);
  zeilen.forEach((z, i) => {
    if (z.typ === "gleich") return;
    for (let k = Math.max(0, i - kontext); k <= Math.min(zeilen.length - 1, i + kontext); k++) zeigen[k] = true;
  });
  const out: DiffStueck[] = [];
  for (let i = 0; i < zeilen.length; ) {
    const an = zeigen[i];
    let k = i;
    while (k < zeilen.length && zeigen[k] === an) k++;
    out.push(an ? { typ: "zeilen", zeilen: zeilen.slice(i, k) } : { typ: "luecke", anzahl: k - i });
    i = k;
  }
  return out;
}

const ZEICHEN: Record<DiffTyp, string> = { gleich: " ", weg: "−", neu: "+" };

export function DiffView({ vorher, nachher, unveraendert }: { vorher: string; nachher: string; unveraendert?: boolean }) {
  const { stuecke, plus, minus } = useMemo(() => {
    const z = zeilenDiff(vorher, nachher);
    return {
      stuecke: diffStuecke(z),
      plus: z.filter((x) => x.typ === "neu").length,
      minus: z.filter((x) => x.typ === "weg").length,
    };
  }, [vorher, nachher]);

  if (unveraendert || (plus === 0 && minus === 0)) {
    return <div className="diff-leer">Keine Änderung – die Datei bliebe genau so, wie sie ist.</div>;
  }

  return (
    <div className="diff-view">
      <div className="diff-summe">
        <span className="diff-plus">+{plus}</span> <span className="diff-minus">−{minus}</span> {plus + minus === 1 ? "Zeile" : "Zeilen"}
      </div>
      <div className="diff-body" role="table" aria-label="Änderungen an der Datei">
        {stuecke.map((s, si) =>
          s.typ === "luecke" ? (
            <div key={si} className="diff-gap" role="row">
              ⋯ {s.anzahl} {s.anzahl === 1 ? "unveränderte Zeile" : "unveränderte Zeilen"}
            </div>
          ) : (
            s.zeilen.map((z, zi) => (
              <div key={`${si}-${zi}`} className={`diff-line ${z.typ}`} role="row">
                <span className="diff-nr">{z.alt ?? ""}</span>
                <span className="diff-nr">{z.neu ?? ""}</span>
                <span className="diff-sign" aria-label={z.typ === "neu" ? "neu" : z.typ === "weg" ? "entfernt" : undefined}>
                  {ZEICHEN[z.typ]}
                </span>
                <span className="diff-text">{z.text === "" ? " " : z.text}</span>
              </div>
            ))
          ),
        )}
      </div>
    </div>
  );
}

/** Bestätigung vor dem Schreiben: Dateiname, Warnungen, Diff, „Abbrechen“ / Schreiben. */
export function DiffDialog({
  aenderung,
  aktion,
  knopf,
  schreibt,
  onConfirm,
  onCancel,
}: {
  aenderung: Aenderung;
  /** was passiert, z. B. „Nachweis in die Wochennotiz schreiben“ */
  aktion: string;
  /** Beschriftung des Schreib-Knopfs */
  knopf: string;
  schreibt: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const titleId = useId();
  const nichts = aenderung.unveraendert;
  return (
    <Modal onClose={onCancel} labelledBy={titleId} className="modal-diff">
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.diff} {baseName(aenderung.path)}
        </h2>
        <div className="hint">
          {aktion} · <span className="mono-path">{aenderung.path}</span>
        </div>
      </div>
      <div className="modal-body diff-dialog-body">
        {aenderung.warnungen.length > 0 && (
          <div className="lb-hinweise">
            {aenderung.warnungen.map((w, i) => (
              <div key={i} className="hint lb-hinweis">
                {Icon.warning}
                <span>{w}</span>
              </div>
            ))}
          </div>
        )}
        <DiffView vorher={aenderung.vorher} nachher={aenderung.nachher} unveraendert={nichts} />
        {!nichts && <div className="hint">Alles außer den markierten Zeilen bleibt in der Datei unverändert.</div>}
      </div>
      <div className="modal-foot">
        <button className="btn" onClick={onCancel} disabled={schreibt} data-autofocus={nichts ? true : undefined}>
          {nichts ? "Schließen" : "Abbrechen"}
        </button>
        {!nichts && (
          <button className="btn primary" onClick={onConfirm} disabled={schreibt} data-autofocus>
            {Icon.save} {schreibt ? "Wird geschrieben …" : knopf}
          </button>
        )}
      </div>
    </Modal>
  );
}
