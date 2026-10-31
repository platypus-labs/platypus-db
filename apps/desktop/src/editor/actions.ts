// Commands of the SQL editor: formatting, line editing and multiple cursors. They live in
// the action registry, so the editor's context menu and the command palette list them
// and shortcuts can be changed in one place.
import {
  addCursorAbove,
  addCursorBelow,
  copyLineDown,
  copyLineUp,
  deleteLine,
  moveLineDown,
  moveLineUp,
  toggleComment,
} from "@codemirror/commands";
import { foldAll, unfoldAll } from "@codemirror/language";
import { startCompletion } from "@codemirror/autocomplete";
import { openSearchPanel, selectNextOccurrence, selectSelectionMatches } from "@codemirror/search";
import type { EditorView } from "@codemirror/view";
import { createElement } from "react";
import { isMac, registerAction } from "../actions";
import { api, type DriverKind, type FormatSettings, isNoSql } from "../api";
import { defineSetting, getSetting } from "../settings";
import { workbench } from "../workbench";
import type { EditorHost, RunMode } from "./SqlEditor";
import { SnippetsDialog } from "./SnippetsDialog";
import { msg } from "../i18n";

declare module "../actions" {
  interface ActionScopes {
    /** The SQL editor with focus or a context menu; `commands` are its owner's (run, save,
     *  open), absent where it has none. */
    editor: { view: EditorView; driver: DriverKind; readOnly: boolean; host?: EditorHost; commands?: EditorCommands };
  }
}

export const formatKeywordCase = defineSetting<string>({
  key: "editor.format_keyword_case",
  section: "SQL editor",
  label: "Keyword case when formatting and completing",
  default: "upper",
  control: {
    type: "select",
    options: [
      { value: "upper", label: "UPPER" },
      { value: "lower", label: "lower" },
      { value: "preserve", label: "As typed" },
    ],
  },
});

export const formatIndent = defineSetting({
  key: "editor.format_indent",
  section: "SQL editor",
  label: "Indentation when formatting (spaces)",
  default: 2,
  control: { type: "number", min: 1, max: 8, step: 1 },
});

const writable = ({ readOnly }: { readOnly: boolean }) => !readOnly;

/** Replaces `from..to` with `text`, changing only the part that differs so the cursor and
 *  the undo history stay where the edit was. */
function replace(view: EditorView, from: number, to: number, text: string, select: boolean) {
  const old = view.state.sliceDoc(from, to);
  let start = 0;
  while (start < old.length && start < text.length && old[start] === text[start]) start++;
  let end = 0;
  while (
    end < old.length - start &&
    end < text.length - start &&
    old[old.length - 1 - end] === text[text.length - 1 - end]
  ) {
    end++;
  }
  if (start === old.length && start === text.length) return;
  view.dispatch({
    changes: { from: from + start, to: to - end, insert: text.slice(start, text.length - end) },
    selection: select ? { anchor: from, head: from + text.length } : undefined,
    scrollIntoView: true,
    userEvent: "input.format",
  });
}

registerAction({
  id: "editor.format",
  scope: "editor",
  title: "Format SQL",
  shortcut: "Mod+Shift+F",
  group: "sql",
  order: 10,
  // The formatter knows SQL only.
  when: (t) => writable(t) && !isNoSql(t.driver),
  run: async ({ view, driver }) => {
    const sel = view.state.selection.main;
    const [from, to] = sel.empty ? [0, view.state.doc.length] : [sel.from, sel.to];
    const sql = view.state.sliceDoc(from, to);
    const settings: FormatSettings = {
      keyword_case: getSetting(formatKeywordCase) as FormatSettings["keyword_case"],
      indent: getSetting(formatIndent),
    };
    const formatted = await api.formatSql(sql, driver, settings);
    // Typing while the formatter ran wins.
    if (view.state.sliceDoc(from, to) !== sql) return;
    replace(view, from, to, formatted, !sel.empty);
  },
});

registerAction({
  id: "editor.select_statement",
  scope: "editor",
  title: "Select statement",
  shortcut: "Mod+Shift+A",
  group: "sql",
  order: 11,
  run: async ({ view, driver }) => {
    const text = view.state.doc.toString();
    const analysis = await api.analyzeSql(text, driver, view.state.selection.main.head);
    const s = analysis.at_cursor === null ? undefined : analysis.statements[analysis.at_cursor];
    if (s && view.state.doc.toString() === text) {
      view.dispatch({ selection: { anchor: s.start, head: s.end }, scrollIntoView: true });
    }
  },
});

/** A CodeMirror command as an action. */
function command(
  id: string,
  title: string,
  shortcut: string,
  group: string,
  order: number,
  run: (view: EditorView) => boolean,
  edits = true,
) {
  registerAction({
    id: `editor.${id}`,
    scope: "editor",
    title,
    shortcut,
    group,
    order,
    when: edits ? writable : undefined,
    run: ({ view }) => {
      run(view);
      view.focus();
    },
  });
}

command("toggle_comment", msg("Toggle comment"), "Mod+/", "sql", 12, toggleComment);
command("duplicate_line", msg("Duplicate line"), "Shift+Alt+ArrowDown", "lines", 20, copyLineDown);
command("duplicate_line_up", msg("Duplicate line above"), "Shift+Alt+ArrowUp", "lines", 21, copyLineUp);
command("move_line_up", msg("Move line up"), "Alt+ArrowUp", "lines", 22, moveLineUp);
command("move_line_down", msg("Move line down"), "Alt+ArrowDown", "lines", 23, moveLineDown);
command("delete_line", msg("Delete line"), "Mod+Shift+K", "lines", 24, deleteLine);
command("add_cursor_above", msg("Add cursor above"), "Mod+Alt+ArrowUp", "cursors", 30, addCursorAbove, false);
command("add_cursor_below", msg("Add cursor below"), "Mod+Alt+ArrowDown", "cursors", 31, addCursorBelow, false);
command("select_next", msg("Select next occurrence"), "Mod+D", "cursors", 32, selectNextOccurrence, false);
command("select_all_matches", msg("Select all occurrences"), "Mod+Shift+L", "cursors", 33, selectSelectionMatches, false);
command("fold_all", msg("Fold all"), "Mod+Alt+[", "folding", 40, foldAll, false);
command("unfold_all", msg("Unfold all"), "Mod+Alt+]", "folding", 41, unfoldAll, false);

/** DuckDB has none: its client cannot open a file the session holds. ClickHouse neither:
 *  clickhouse-client speaks the native protocol, not HTTP. */
const CLIENTS: Partial<Record<DriverKind, string>> = {
  postgres: "psql",
  mysql: "mysql",
  sqlite: "sqlite3",
  sqlserver: "sqlcmd",
};

/** What the editor's owner does on its commands. */
export interface EditorCommands {
  run?: (mode: RunMode) => void;
  save?: () => void;
  open?: () => void;
}

registerAction({
  id: "editor.run_statement",
  scope: "editor",
  title: "Run statement",
  shortcut: "Mod+Enter",
  group: "run",
  order: 1,
  when: ({ commands }) => !!commands?.run,
  run: ({ commands }) => commands?.run?.("statement"),
});

registerAction({
  id: "editor.run_script",
  scope: "editor",
  title: "Run script",
  // Alt+X first: the one menus and the Settings show.
  shortcut: ["Alt+X", "Mod+Shift+Enter"],
  group: "run",
  order: 2,
  when: ({ commands }) => !!commands?.run,
  run: ({ commands }) => commands?.run?.("script"),
});

/** The Cancel of the editor tab shown while its statement runs (or its rows still arrive):
 *  set by that tab, `null` when nothing there can be stopped. */
export const runningStatement: { cancel: (() => void) | null } = { cancel: null };

// Global: the palette lists it and its keys work wherever the focus is, also with the
// toolbar's Cancel button hidden.
registerAction({
  id: "editor.cancel",
  scope: "global",
  title: "Cancel the running statement",
  shortcut: "Mod+.",
  when: () => runningStatement.cancel !== null,
  run: () => runningStatement.cancel?.(),
});

registerAction({
  id: "editor.save",
  scope: "editor",
  title: "Save script",
  shortcut: "Mod+S",
  hidden: true,
  when: ({ commands }) => !!commands?.save,
  run: ({ commands }) => commands?.save?.(),
});

registerAction({
  id: "editor.open",
  scope: "editor",
  title: "Open script",
  shortcut: "Mod+O",
  hidden: true,
  when: ({ commands }) => !!commands?.open,
  run: ({ commands }) => commands?.open?.(),
});

// Not through `command`: the search field and the completion list keep the focus.
registerAction({
  id: "editor.find",
  scope: "editor",
  title: "Find and replace",
  shortcut: "Mod+F",
  group: "search",
  order: 50,
  run: ({ view }) => void openSearchPanel(view),
});

registerAction({
  id: "editor.autocomplete",
  scope: "editor",
  title: "Show completions",
  shortcut: isMac ? "Ctrl+Space" : "Mod+Space",
  hidden: true,
  run: ({ view }) => void startCompletion(view),
});

registerAction({
  id: "editor.run_append",
  scope: "editor",
  title: "Run in a new result tab",
  shortcut: "Mod+\\",
  group: "run",
  order: 4,
  when: ({ host }) => !!host?.runAppend,
  run: ({ host }) => host?.runAppend?.(),
});

registerAction({
  id: "editor.run_on_connections",
  scope: "editor",
  title: "Run on connections…",
  group: "run",
  order: 4.5,
  when: ({ host }) => !!host?.runOnConnections,
  run: ({ host }) => host?.runOnConnections?.(),
});

registerAction({
  id: "editor.run_cli",
  scope: "editor",
  title: "Run script with the command-line client",
  group: "run",
  order: 5,
  when: ({ host }) => !!host?.runCli,
  run: ({ host }) => host?.runCli?.(),
});

registerAction({
  id: "editor.snippets",
  scope: "global",
  title: "Edit SQL snippets…",
  run: () => workbench().openDialog((close) => createElement(SnippetsDialog, { onClose: close })),
});

/** The client's name for the menu's tooltip and the status bar; none for DuckDB. */
export const cliName = (driver: DriverKind) => CLIENTS[driver];
