// CodeMirror-6-Grundgerüst. `editorBase` teilen sich Haupteditor und Konfliktansicht (MergeView);
// nur der Haupteditor bekommt Verlauf, Tastenkürzel (Strg+S), Suche und den Änderungs-Listener.
import { Compartment, EditorState, type Extension } from "@codemirror/state";
import { drawSelection, EditorView, highlightActiveLine, keymap } from "@codemirror/view";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { search, searchKeymap } from "@codemirror/search";
import { tags as t } from "@lezer/highlight";

/** Farben aus den Design-Tokens (styles.css), damit Editor und App gleich aussehen. */
const cockpitTheme = EditorView.theme(
  {
    "&": {
      backgroundColor: "transparent",
      color: "var(--text)",
      fontSize: "13.5px",
      height: "100%",
    },
    "&.cm-focused": { outline: "none" },
    ".cm-scroller": {
      fontFamily: "var(--mono)",
      lineHeight: "1.6",
    },
    ".cm-content": {
      caretColor: "var(--purple-2)",
      padding: "18px 0 40px",
    },
    ".cm-line": { padding: "0 26px" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--purple-2)", borderLeftWidth: "2px" },
    "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: "rgba(139, 92, 246, 0.45)" },
    ".cm-activeLine": { backgroundColor: "rgba(255, 255, 255, 0.04)" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "var(--text-3)",
      border: "none",
    },
    ".cm-panels": {
      backgroundColor: "var(--glass-strong)",
      color: "var(--text)",
      backdropFilter: "var(--blur)",
    },
    ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--glass-border)" },
    ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--glass-border)" },
    ".cm-searchMatch": {
      backgroundColor: "rgba(94, 234, 212, 0.18)",
      outline: "1px solid rgba(94, 234, 212, 0.45)",
    },
    ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "rgba(94, 234, 212, 0.35)" },
  },
  { dark: true },
);

const cockpitHighlight = HighlightStyle.define([
  { tag: t.heading, color: "var(--text)", fontWeight: "700" },
  { tag: [t.heading1, t.heading2], color: "var(--text)", fontWeight: "700" },
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "700" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: [t.link, t.url], color: "var(--purple-2)" },
  { tag: t.monospace, color: "var(--mint)" },
  { tag: [t.meta, t.processingInstruction, t.contentSeparator], color: "var(--text-3)" },
  { tag: t.quote, color: "var(--text-2)" },
]);

/**
 * Für Haupteditor UND MergeView. Enthält bewusst keine Tastenkürzel und keinen Listener,
 * damit Strg+S im Konfliktdialog nie den alten Hauptpuffer speichert.
 */
export const editorBase: Extension[] = [
  // Nur "\n" trennt Zeilen – ein einzelnes "\r" in der Datei bleibt unangetastet.
  EditorState.lineSeparator.of("\n"),
  // Eingefügter/fallengelassener Windows-Text würde sonst "\r" auf jeder Zeile mitbringen.
  EditorView.clipboardInputFilter.of((text) => text.replace(/\r\n?/g, "\n")),
  markdown({ base: markdownLanguage }),
  syntaxHighlighting(cockpitHighlight),
  cockpitTheme,
];

/** Nur für den Haupteditor. */
export function mainEditorExtensions(
  onSave: () => void,
  onChange: () => void,
  readonlyCompartment: Compartment,
  readonly: boolean,
): Extension[] {
  return [
    history(),
    drawSelection(),
    highlightActiveLine(),
    EditorView.lineWrapping,
    search({ top: true }),
    keymap.of([
      {
        key: "Mod-s",
        preventDefault: true,
        run: () => {
          onSave();
          return true;
        },
      },
      ...searchKeymap,
      ...historyKeymap,
      ...defaultKeymap,
    ]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) onChange();
    }),
    readonlyCompartment.of(EditorState.readOnly.of(readonly)),
  ];
}
