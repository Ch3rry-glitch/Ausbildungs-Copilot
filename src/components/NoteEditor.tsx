// Handgeschriebene CodeMirror-Hülle (kein @uiw/react-codemirror). Die Ansicht wird einmal erzeugt;
// jeder Notizwechsel baut einen frischen EditorState (eigener Verlauf je Notiz).
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { Compartment, EditorState, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { editorBase, mainEditorExtensions } from "../editor/setup";

export interface NoteEditorHandle {
  getText(): string;
  /** frischer Zustand mit leerem Verlauf – für JEDEN Notizwechsel und jede übernommene Fassung */
  load(text: string, opts: { readonly: boolean }): void;
  /** stilles Nachladen von außen: ein Dispatch ohne Verlaufseintrag, Cursor begrenzt */
  replaceDoc(text: string): void;
  setReadonly(b: boolean): void;
  /** 1-basiert; scrollt hin und setzt den Cursor an den Zeilenanfang */
  revealLine(n: number): void;
  focus(): void;
  requestMeasure(): void;
}

export const NoteEditor = forwardRef<NoteEditorHandle, { onSave: () => void; onChange: () => void }>(function NoteEditor(
  { onSave, onChange },
  ref,
) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const readonlyCompartment = useRef(new Compartment());
  // Rückrufe über Refs, damit die Erweiterungen nie neu gebaut werden müssen.
  const onSaveRef = useRef(onSave);
  const onChangeRef = useRef(onChange);
  onSaveRef.current = onSave;
  onChangeRef.current = onChange;

  function makeState(doc: string, readonly: boolean): EditorState {
    return EditorState.create({
      doc,
      extensions: [
        ...editorBase,
        ...mainEditorExtensions(
          () => onSaveRef.current(),
          () => onChangeRef.current(),
          readonlyCompartment.current,
          readonly,
        ),
      ],
    });
  }

  useEffect(() => {
    if (!host.current) return;
    const v = new EditorView({ state: makeState("", true), parent: host.current });
    view.current = v;
    return () => {
      v.destroy();
      view.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useImperativeHandle(
    ref,
    () => ({
      getText: () => view.current?.state.doc.toString() ?? "",
      load: (text, opts) => {
        view.current?.setState(makeState(text, opts.readonly));
      },
      replaceDoc: (text) => {
        const v = view.current;
        if (!v) return;
        const head = Math.min(v.state.selection.main.head, text.length);
        v.dispatch({
          changes: { from: 0, to: v.state.doc.length, insert: text },
          selection: { anchor: head },
          annotations: Transaction.addToHistory.of(false),
        });
      },
      setReadonly: (b) => {
        view.current?.dispatch({ effects: readonlyCompartment.current.reconfigure(EditorState.readOnly.of(b)) });
      },
      revealLine: (n) => {
        const v = view.current;
        if (!v) return;
        const doc = v.state.doc;
        const line = doc.line(Math.min(Math.max(1, Math.floor(n)), doc.lines));
        v.dispatch({
          selection: { anchor: line.from },
          effects: EditorView.scrollIntoView(line.from, { y: "center" }),
        });
        v.focus();
      },
      focus: () => view.current?.focus(),
      requestMeasure: () => view.current?.requestMeasure(),
    }),
    [],
  );

  return <div className="note-editor" ref={host} />;
});
