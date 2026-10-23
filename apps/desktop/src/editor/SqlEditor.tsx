import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab, toggleComment } from "@codemirror/commands";
import { MySQL, PostgreSQL, SQLite, sql } from "@codemirror/lang-sql";
import {
  HighlightStyle,
  bracketMatching,
  indentOnInput,
  syntaxHighlighting,
} from "@codemirror/language";
import { type Diagnostic, lintGutter, setDiagnostics } from "@codemirror/lint";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  drawSelection,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import type { CompletionCatalog, DriverKind } from "../api";

export type RunMode = "statement" | "script";

export interface SqlEditorHandle {
  getText(): string;
  /** Selection in UTF-16 offsets; `from === to` when nothing is selected. */
  getSelection(): { from: number; to: number };
  setText(text: string): void;
  /** Inserts `text` on its own line(s) at the cursor and selects it. */
  insert(text: string): void;
  focus(): void;
  /** Underlines `from..to` with an error message; `null` clears it. */
  setError(error: { from: number; to: number; message: string } | null): void;
  /** Briefly highlights the statement being executed. */
  flash(from: number, to: number): void;
}

interface Props {
  initialValue: string;
  driver: DriverKind;
  onChange: (text: string) => void;
  onRun: (mode: RunMode) => void;
  onSave: () => void;
  onOpen: () => void;
  /** Tables and columns to complete; keywords complete without it. */
  catalog?: CompletionCatalog | null;
}

const highlight = HighlightStyle.define([
  { tag: [t.keyword, t.operatorKeyword, t.modifier], color: "var(--syn-keyword)", fontWeight: "600" },
  { tag: [t.string, t.special(t.string)], color: "var(--syn-string)" },
  { tag: [t.number, t.bool, t.null], color: "var(--syn-number)" },
  { tag: [t.lineComment, t.blockComment], color: "var(--syn-comment)", fontStyle: "italic" },
  { tag: [t.typeName, t.standard(t.name)], color: "var(--syn-type)" },
  { tag: [t.special(t.name), t.quote], color: "var(--syn-ident)" },
  { tag: [t.operator, t.punctuation, t.bracket], color: "var(--fg-muted)" },
]);

const theme = EditorView.theme({
  "&": { height: "100%", fontSize: "13px", backgroundColor: "var(--bg)", color: "var(--fg)" },
  // Ligatures would render `<=` as `≤`, which misleads in SQL.
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.55", fontVariantLigatures: "none" },
  ".cm-content": { caretColor: "var(--fg)", padding: "6px 0" },
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  "&.cm-focused": { outline: "none" },
  ".cm-gutters": {
    backgroundColor: "var(--bg)",
    color: "var(--fg-muted)",
    border: "none",
  },
  ".cm-activeLine": { backgroundColor: "var(--hover)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--hover)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground":
    { backgroundColor: "var(--grid-selection)" },
  ".cm-executing": { backgroundColor: "var(--grid-selection)", transition: "background-color 0.4s" },
  ".cm-tooltip": {
    backgroundColor: "var(--bg-sidebar)",
    border: "1px solid var(--border)",
    color: "var(--fg)",
  },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "var(--accent-fg)",
  },
  ".cm-panels": { backgroundColor: "var(--bg-sidebar)", color: "var(--fg)" },
  ".cm-searchMatch": { backgroundColor: "rgba(250, 176, 5, 0.3)" },
});

const dialects = { postgres: PostgreSQL, mysql: MySQL, sqlite: SQLite };

function language(driver: DriverKind, catalog: CompletionCatalog | null | undefined) {
  return sql({
    dialect: dialects[driver],
    upperCaseKeywords: true,
    schema: catalog?.schemas,
    defaultSchema: catalog?.default_schema ?? undefined,
  });
}

const flashEffect = StateEffect.define<{ from: number; to: number } | null>();
const flashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(flashEffect)) {
        deco =
          e.value && e.value.to > e.value.from
            ? Decoration.set([Decoration.mark({ class: "cm-executing" }).range(e.value.from, e.value.to)])
            : Decoration.none;
      }
    }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

export const SqlEditor = forwardRef<SqlEditorHandle, Props>(function SqlEditor(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const languageConf = useRef(new Compartment());
  // Keymaps read the latest callbacks without rebuilding the editor.
  const callbacks = useRef(props);
  callbacks.current = props;

  useEffect(() => {
    const run = (mode: RunMode) => () => {
      callbacks.current.onRun(mode);
      return true;
    };
    const state = EditorState.create({
      doc: props.initialValue,
      extensions: [
        lineNumbers(),
        highlightActiveLineGutter(),
        highlightSpecialChars(),
        history(),
        drawSelection(),
        indentOnInput(),
        bracketMatching(),
        closeBrackets(),
        autocompletion(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        search({ top: true }),
        lintGutter(),
        flashField,
        syntaxHighlighting(highlight),
        languageConf.current.of(language(props.driver, props.catalog)),
        keymap.of([
          { key: "Mod-Enter", run: run("statement"), preventDefault: true },
          { key: "Mod-Shift-Enter", run: run("script"), preventDefault: true },
          { key: "Alt-x", run: run("script"), preventDefault: true },
          { key: "Mod-/", run: toggleComment },
          {
            key: "Mod-s",
            run: () => (callbacks.current.onSave(), true),
            preventDefault: true,
          },
          {
            key: "Mod-o",
            run: () => (callbacks.current.onOpen(), true),
            preventDefault: true,
          },
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...completionKeymap,
          indentWithTab,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            callbacks.current.onChange(u.state.doc.toString());
            // Edits invalidate the reported error position.
            u.view.dispatch(setDiagnostics(u.state, []));
          }
        }),
        theme,
      ],
    });
    const v = new EditorView({ state, parent: host.current! });
    view.current = v;
    return () => {
      v.destroy();
      view.current = null;
    };
    // The editor is created once; later prop changes go through the handle or compartments.
  }, []);

  useEffect(() => {
    view.current?.dispatch({
      effects: languageConf.current.reconfigure(language(props.driver, props.catalog)),
    });
  }, [props.driver, props.catalog]);

  useImperativeHandle(ref, () => ({
    getText: () => view.current?.state.doc.toString() ?? "",
    getSelection: () => {
      const sel = view.current?.state.selection.main;
      return { from: sel?.from ?? 0, to: sel?.to ?? 0 };
    },
    setText: (text) => {
      const v = view.current;
      v?.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text } });
    },
    insert: (text) => {
      const v = view.current;
      if (!v) return;
      const pos = v.state.selection.main.head;
      const line = v.state.doc.lineAt(pos);
      // Separate from surrounding SQL so the inserted statement runs on its own.
      const before = line.text.trim() === "" ? "" : "\n\n";
      const insert = `${before}${text.trimEnd()}${text.trimEnd().endsWith(";") ? "" : ";"}\n`;
      const at = line.text.trim() === "" ? line.from : line.to;
      v.dispatch({
        changes: { from: at, insert },
        selection: { anchor: at + before.length, head: at + insert.length - 1 },
        scrollIntoView: true,
      });
      v.focus();
    },
    focus: () => view.current?.focus(),
    setError: (error) => {
      const v = view.current;
      if (!v) return;
      const diagnostics: Diagnostic[] = [];
      if (error) {
        const len = v.state.doc.length;
        const from = Math.min(error.from, len);
        diagnostics.push({
          from,
          to: Math.max(from, Math.min(error.to, len)),
          severity: "error",
          message: error.message,
        });
      }
      v.dispatch(setDiagnostics(v.state, diagnostics));
      if (error) {
        v.dispatch({ selection: { anchor: Math.min(error.from, v.state.doc.length) }, scrollIntoView: true });
        v.focus();
      }
    },
    flash: (from, to) => {
      const v = view.current;
      if (!v) return;
      v.dispatch({ effects: flashEffect.of({ from, to }) });
      setTimeout(() => view.current?.dispatch({ effects: flashEffect.of(null) }), 600);
    },
  }));

  return <div className="editor" ref={host} />;
});
