// Konfliktansicht: links die eigene Fassung (bearbeitbar), rechts die Fassung auf der Platte.
// Der Unterschied wird im Frontend von @codemirror/merge berechnet.
import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { history, historyKeymap } from "@codemirror/commands";
import { MergeView } from "@codemirror/merge";
import { editorBase } from "../editor/setup";
import { Icon } from "../icons";
import { Modal } from "./Modal";

export function ConflictDialog({
  name,
  mine,
  diskText,
  error,
  onSaveMerged,
  onTakeDisk,
  onCancel,
}: {
  name: string;
  mine: string;
  diskText: string;
  /** Fehler des letzten Speicherversuchs aus dem Dialog (z. B. gesperrt) */
  error: string | null;
  onSaveMerged: (merged: string) => Promise<void>;
  onTakeDisk: () => void;
  onCancel: () => void;
}) {
  const titleId = useId();
  const host = useRef<HTMLDivElement>(null);
  const merge = useRef<MergeView | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!host.current) return;
    const mv = new MergeView({
      // Verlauf nur links (nicht in editorBase): ein falsch geklickter Pfeil lässt sich mit Strg+Z zurücknehmen
      a: { doc: mine, extensions: [...editorBase, EditorView.lineWrapping, history(), keymap.of(historyKeymap)] },
      b: {
        doc: diskText,
        extensions: [...editorBase, EditorView.lineWrapping, EditorState.readOnly.of(true), EditorView.editable.of(false)],
      },
      parent: host.current,
      revertControls: "b-to-a",
      highlightChanges: true,
      gutter: true,
      collapseUnchanged: { margin: 3, minSize: 6 },
    });
    merge.current = mv;
    return () => {
      mv.destroy();
      merge.current = null;
    };
  }, [mine, diskText]);

  async function saveMerged() {
    const mv = merge.current;
    if (!mv || busy) return;
    setBusy(true);
    try {
      await onSaveMerged(mv.a.state.doc.toString());
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    // Eigenes Strg+S: speichert die zusammengeführte Fassung, nie den alten Hauptpuffer.
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      e.stopPropagation();
      void saveMerged();
    }
  }

  return (
    <Modal onClose={onCancel} className="modal-conflict" labelledBy={titleId} onKeyDown={onKeyDown}>
      <div className="modal-head">
        <h2 id={titleId} className="modal-title">
          {Icon.diff} Konflikt: {name}
        </h2>
        <div className="hint">
          Die Datei wurde außerhalb der App geändert. Übernimm einzelne Abschnitte mit den Pfeilen in der Mitte und
          speichere dann deine Fassung.
        </div>
      </div>
      <div className="merge-heads">
        <span>Deine Fassung (App) – bearbeitbar</span>
        <span>Fassung auf der Platte (Obsidian)</span>
      </div>
      <div className="merge-host" ref={host} />
      {error && <div className="error modal-error">{error}</div>}
      <div className="modal-foot">
        <button className="btn" onClick={onCancel} disabled={busy}>
          Abbrechen
        </button>
        <button className="btn" onClick={onTakeDisk} disabled={busy}>
          {Icon.obsidian} Obsidian-Fassung übernehmen
        </button>
        <button className="btn primary" onClick={saveMerged} disabled={busy}>
          {Icon.save} {busy ? "Speichert …" : "Diese Fassung speichern"}
        </button>
      </div>
    </Modal>
  );
}
