import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import {
  acceptCompletion,
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
} from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import { MSSQL, MySQL, PLSQL, PostgreSQL, SQLite, StandardSQL } from "@codemirror/lang-sql";
import {
  HighlightStyle,
  LanguageSupport,
  bracketMatching,
  foldGutter,
  foldKeymap,
  indentOnInput,
  syntaxHighlighting,
  syntaxTree,
} from "@codemirror/language";
import { type Diagnostic, forceLinting, lintGutter, linter, setDiagnostics } from "@codemirror/lint";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";
import { Compartment, EditorState, Prec, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  crosshairCursor,
  drawSelection,
  dropCursor,
  highlightActiveLine,
  highlightActiveLineGutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  rectangularSelection,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";
import { dispatchKey } from "../actions";
import { type CompletionCatalog, type DriverKind, isMongo, toCommandError } from "../api";
import { annotation, catalogEntry, catalogKey, useCatalogVersion } from "../catalog/catalog";
import { ContextMenu, type MenuState } from "../components/ContextMenu";
import { useSetting } from "../settings";
import { workbench } from "../workbench";
import { SQL_DRAG, type SqlDrag } from "../sqlNames";
import "./actions";
import { type CatalogNote, closeOnTypedWord, codeOnly, keywordCompletion, nameCompletion } from "./completion";
import { functionCompletion } from "./functions";
import { joinCompletion } from "./joinCompletion";
import { lintSql } from "./lint";
import { mongoLanguage } from "./mongo";
import { expandSnippet, snippetSource } from "./snippets";
import { spellcheck, spellcheckSetting } from "./spellcheck";

/** `append` runs the statement into a new result tab, keeping the others. */
export type RunMode = "statement" | "script" | "append";

/** What the tab around the editor offers its actions. */
export interface EditorHost {
  /** Opens the object a name in the SQL refers to (Ctrl+click); `schema` when qualified. */
  openObject?: (schema: string | null, name: string) => void;
  /** Runs the editor's script with the database's command-line client. */
  runCli?: () => void;
  /** Runs the statement into a new result tab, keeping the others. */
  runAppend?: () => void;
  /** Opens the plan of the statement under the cursor; `analyze` runs it. */
  explain?: (analyze: boolean) => void;
}

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
  /** Selects `from..to` and scrolls to it. */
  select(from: number, to: number): void;
}

interface Props {
  initialValue: string;
  driver: DriverKind;
  onChange?: (text: string) => void;
  onRun?: (mode: RunMode) => void;
  onSave?: () => void;
  onOpen?: () => void;
  /** Shows SQL that cannot be edited, e.g. an object's DDL. */
  readOnly?: boolean;
  /** Tables and columns to complete; keywords complete without it. */
  catalog?: CompletionCatalog | null;
  /** With `database`, finds the notes shown in the completion info. */
  connectionId?: string;
  database?: string | null;
  /** The connection is a production one: the linter warns about `SELECT *`. */
  production?: boolean;
  host?: EditorHost;
}

/** The name under `pos` as `[schema, name]` when it is an identifier, e.g. a table. */
function nameAt(state: EditorState, pos: number): [string | null, string] | null {
  const node = syntaxTree(state).resolveInner(pos, 1);
  if (node.name !== "Identifier" && node.name !== "QuotedIdentifier") return null;
  const unquote = (n: { from: number; to: number; name: string }) => {
    const text = state.sliceDoc(n.from, n.to);
    return n.name === "QuotedIdentifier" ? text.slice(1, -1) : text;
  };
  const parent = node.parent;
  if (parent?.name !== "CompositeIdentifier") return [null, unquote(node)];
  const parts: { from: number; to: number; name: string }[] = [];
  for (let c = parent.firstChild; c; c = c.nextSibling) {
    if (c.name === "Identifier" || c.name === "QuotedIdentifier") parts.push({ from: c.from, to: c.to, name: c.name });
  }
  const i = parts.findIndex((p) => p.from === node.from);
  return [i > 0 ? unquote(parts[i - 1]!) : null, unquote(node)];
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
  "&": { height: "100%", fontSize: "var(--editor-font-size)", backgroundColor: "var(--bg)", color: "var(--fg)" },
  // Ligatures would render `<=` as `≤`, which misleads in SQL.
  ".cm-scroller": { fontFamily: "var(--font-mono)", lineHeight: "1.55", fontVariantLigatures: "none" },
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
  // The glass of the menus (styles/dialogs.css), solid where it is not supported.
  ".cm-tooltip": {
    backgroundColor: "var(--glass-menu, var(--bg-sidebar))",
    backdropFilter: "var(--glass-blur, none)",
    WebkitBackdropFilter: "var(--glass-blur, none)",
    border: "1px solid var(--border-strong)",
    borderRadius: "var(--radius-lg)",
    boxShadow: "var(--shadow)",
    color: "var(--fg)",
  },
  // The list keeps the rounded corners; the tooltip itself must not clip, as the description
  // of the selected entry (.cm-completionInfo) is placed inside it but beside its box.
  ".cm-tooltip-autocomplete > ul": { borderRadius: "inherit" },
  // Beside the list it would blur nothing (its backdrop is the list's own box): solid.
  ".cm-tooltip.cm-completionInfo": { backgroundColor: "var(--bg-sidebar)", backdropFilter: "none", WebkitBackdropFilter: "none" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
    backgroundColor: "var(--accent)",
    color: "var(--accent-fg)",
  },
  ".cm-panels": { backgroundColor: "var(--bg-sidebar)", color: "var(--fg)" },
  ".cm-searchMatch": { backgroundColor: "rgba(250, 176, 5, 0.3)" },
});

// DuckDB and ClickHouse have no CodeMirror dialect: DuckDB's syntax follows PostgreSQL,
// ClickHouse's backtick names and `#` comments MySQL's. MongoDB is mongosh.
const dialects = {
  postgres: PostgreSQL,
  mysql: MySQL,
  sqlite: SQLite,
  duckdb: PostgreSQL,
  sqlserver: MSSQL,
  clickhouse: MySQL,
  libsql: SQLite,
  mongo: PostgreSQL,
  snowflake: StandardSQL,
  trino: StandardSQL,
  adbc: StandardSQL,
  oracle: PLSQL,
  db2: StandardSQL,
  odbc: StandardSQL,
  // Backtick names, like MySQL.
  bigquery: MySQL,
  databricks: MySQL,
};

/** The local notes of the catalog's tables and columns, shown as completion info. */
function catalogNotes(connectionId: string | undefined, database: string | null | undefined): CatalogNote | undefined {
  if (!connectionId) return undefined;
  return (schema, table, column) =>
    annotation(catalogEntry(connectionId, catalogKey(database, schema, table, column)), undefined, { full: true });
}

function language(
  driver: DriverKind,
  catalog: CompletionCatalog | null | undefined,
  connectionId?: string,
  database?: string | null,
) {
  if (isMongo(driver)) return mongoLanguage(catalog);
  const dialect = dialects[driver];
  const data = dialect.language.data;
  return new LanguageSupport(dialect.language, [
    catalog ? data.of({ autocomplete: nameCompletion(catalog, driver, catalogNotes(connectionId, database)) }) : [],
    data.of({ autocomplete: keywordCompletion(dialect, driver) }),
    data.of({ autocomplete: codeOnly(joinCompletion(catalog, driver)) }),
    data.of({ autocomplete: codeOnly(functionCompletion(driver)) }),
    data.of({ autocomplete: codeOnly(snippetSource) }),
  ]);
}

const reportError = (e: unknown) => workbench().showError(toCommandError(e).message);

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
  const catalogVersion = useCatalogVersion();
  const languageConf = useRef(new Compartment());
  const spellConf = useRef(new Compartment());
  const spellMode = useSetting(spellcheckSetting);
  // Keymaps read the latest callbacks without rebuilding the editor.
  const callbacks = useRef(props);
  callbacks.current = props;
  const [menu, setMenu] = useState<MenuState | null>(null);
  const closeMenu = useCallback(() => setMenu(null), []);

  useEffect(() => {
    const target = (view: EditorView) => ({
      view,
      driver: callbacks.current.driver,
      readOnly: !!callbacks.current.readOnly,
      host: callbacks.current.host,
    });
    const run = (mode: RunMode) => () => {
      callbacks.current.onRun?.(mode);
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
        dropCursor(),
        EditorState.allowMultipleSelections.of(true),
        // Ctrl+click opens the object under it, so Alt+click adds a cursor and
        // Shift+Alt+drag selects a rectangle, as in VS Code.
        EditorView.clickAddsSelectionRange.of((e) => e.altKey && !e.shiftKey),
        rectangularSelection({ eventFilter: (e) => e.altKey && e.shiftKey }),
        crosshairCursor(),
        indentOnInput(),
        bracketMatching(),
        closeBrackets(),
        // Before the list's own Enter, which would otherwise take a newline.
        Prec.highest(keymap.of([{ key: "Enter", run: closeOnTypedWord }])),
        autocompletion(),
        highlightActiveLine(),
        highlightSelectionMatches(),
        search({ top: true }),
        foldGutter(),
        lintGutter(),
        linter(
          (view) =>
            isMongo(callbacks.current.driver)
              ? []
              : lintSql(view, {
              driver: callbacks.current.driver,
              catalog: callbacks.current.catalog,
              production: !!callbacks.current.production,
            }).catch(() => []),
          { delay: 600 },
        ),
        spellConf.current.of(spellcheck(spellMode)),
        flashField,
        EditorState.readOnly.of(!!props.readOnly),
        syntaxHighlighting(highlight),
        languageConf.current.of(language(props.driver, props.catalog, props.connectionId, props.database)),
        // Registered editor actions take their shortcuts before the keymaps below.
        Prec.highest(
          EditorView.domEventHandlers({
            keydown: (e, view) => dispatchKey("editor", target(view), e, reportError),
            // Ctrl+click on a name opens its object; elsewhere it adds a cursor as usual.
            mousedown: (e, view) => {
              const open = callbacks.current.host?.openObject;
              if (!(e.ctrlKey || e.metaKey) || e.button !== 0 || !open) return false;
              const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
              const name = pos === null ? null : nameAt(view.state, pos);
              if (!name) return false;
              e.preventDefault();
              open(...name);
              return true;
            },
            dragover: (e) => {
              if (e.dataTransfer?.types.includes(SQL_DRAG)) e.preventDefault();
              return false;
            },
            // A table from the navigator: its name, or a query of it in an empty editor or
            // with Shift held.
            drop: (e, view) => {
              const data = e.dataTransfer?.getData(SQL_DRAG);
              if (!data || view.state.readOnly) return false;
              e.preventDefault();
              const { name, select } = JSON.parse(data) as SqlDrag;
              const empty = view.state.doc.toString().trim() === "";
              const insert = select && (empty || e.shiftKey) ? select : name;
              const from = empty ? 0 : (view.posAtCoords({ x: e.clientX, y: e.clientY }) ?? view.state.doc.length);
              view.dispatch({
                changes: { from, to: empty ? view.state.doc.length : from, insert },
                selection: { anchor: from + insert.length },
                userEvent: "input.drop",
              });
              view.focus();
              return true;
            },
            contextmenu: (e, view) => {
              e.preventDefault();
              // A right-click outside the selection moves the cursor there first.
              const pos = view.posAtCoords({ x: e.clientX, y: e.clientY });
              const sel = view.state.selection.main;
              if (pos !== null && (pos < sel.from || pos > sel.to)) view.dispatch({ selection: { anchor: pos } });
              view.focus();
              setMenu({ x: e.clientX, y: e.clientY, target: { scope: "editor", arg: target(view) } });
              return true;
            },
          }),
        ),
        keymap.of([
          { key: "Mod-Enter", run: run("statement"), preventDefault: true },
          { key: "Mod-Shift-Enter", run: run("script"), preventDefault: true },
          { key: "Alt-x", run: run("script"), preventDefault: true },
          { key: "Mod-\\", run: run("append"), preventDefault: true },
          {
            key: "Mod-s",
            run: () => (callbacks.current.onSave?.(), true),
            preventDefault: true,
          },
          {
            key: "Mod-o",
            run: () => (callbacks.current.onOpen?.(), true),
            preventDefault: true,
          },
          ...closeBracketsKeymap,
          ...defaultKeymap,
          ...searchKeymap,
          ...historyKeymap,
          ...completionKeymap,
          ...foldKeymap,
          { key: "Tab", run: (view) => !view.state.readOnly && expandSnippet(view) },
          { key: "Tab", run: acceptCompletion },
          indentWithTab,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) {
            callbacks.current.onChange?.(u.state.doc.toString());
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
      effects: languageConf.current.reconfigure(language(props.driver, props.catalog, props.connectionId, props.database)),
    });
    // Columns are checked against the catalog, which may arrive after the text.
    if (view.current) forceLinting(view.current);
  }, [props.driver, props.catalog, props.connectionId, props.database, catalogVersion]);

  useEffect(() => {
    view.current?.dispatch({ effects: spellConf.current.reconfigure(spellcheck(spellMode)) });
  }, [spellMode]);

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
    select: (from, to) => {
      const v = view.current;
      if (!v) return;
      const len = v.state.doc.length;
      v.dispatch({ selection: { anchor: Math.min(from, len), head: Math.min(to, len) }, scrollIntoView: true });
      v.focus();
    },
  }));

  return (
    <>
      <div className="editor" ref={host} />
      {menu && <ContextMenu menu={menu} onClose={closeMenu} onError={reportError} />}
    </>
  );
});
