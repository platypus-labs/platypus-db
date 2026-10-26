/**
 * Templates typed by name: `sel` then Tab writes a SELECT with fields to fill, and Tab moves
 * from field to field. Fields are written `#{name}`; fields with the same name change
 * together. The list is the user's, stored as JSON in the settings under `editor.snippets`.
 */
import { type Completion, type CompletionSource, snippet, snippetCompletion } from "@codemirror/autocomplete";
import type { EditorView } from "@codemirror/view";
import { getStored, setStored } from "../settings";

export interface Snippet {
  name: string;
  template: string;
}

export const DEFAULT_SNIPPETS: Snippet[] = [
  { name: "sel", template: "SELECT #{*}\nFROM #{table}\nWHERE #{condition};" },
  { name: "selc", template: "SELECT COUNT(*)\nFROM #{table};" },
  { name: "ins", template: "INSERT INTO #{table} (#{columns})\nVALUES (#{values});" },
  { name: "upd", template: "UPDATE #{table}\nSET #{column} = #{value}\nWHERE #{condition};" },
  { name: "del", template: "DELETE FROM #{table}\nWHERE #{condition};" },
  { name: "join", template: "JOIN #{table} ON #{condition}" },
  { name: "cte", template: "WITH #{name} AS (\n  #{query}\n)\nSELECT *\nFROM #{name};" },
  { name: "case", template: "CASE WHEN #{condition} THEN #{value} ELSE #{other} END" },
];

const KEY = "editor.snippets";

export function snippets(): Snippet[] {
  const stored = getStored(KEY);
  if (typeof stored !== "string") return DEFAULT_SNIPPETS;
  try {
    const list = JSON.parse(stored) as unknown;
    return Array.isArray(list)
      ? list.filter((s): s is Snippet => typeof s?.name === "string" && typeof s?.template === "string")
      : DEFAULT_SNIPPETS;
  } catch {
    return DEFAULT_SNIPPETS;
  }
}

/** Saves the list; the defaults when it equals them, so later defaults reach the user. */
export function saveSnippets(list: Snippet[]) {
  const same = JSON.stringify(list) === JSON.stringify(DEFAULT_SNIPPETS);
  return setStored(KEY, same ? null : JSON.stringify(list));
}

/** Expands the snippet named by the word before the cursor; false when there is none. */
export function expandSnippet(view: EditorView): boolean {
  const sel = view.state.selection;
  if (sel.ranges.length > 1 || !sel.main.empty) return false;
  const head = sel.main.head;
  const line = view.state.doc.lineAt(head);
  const word = /[A-Za-z_]\w*$/.exec(line.text.slice(0, head - line.from))?.[0];
  const found = word && snippets().find((s) => s.name === word);
  if (!found) return false;
  snippet(found.template)(view, null, head - word.length, head);
  return true;
}

/** The snippets in the completion list, after their names. */
export const snippetSource: CompletionSource = (context) => {
  const word = context.matchBefore(/\w+/);
  if (!word && !context.explicit) return null;
  // Not after a dot, where a column is written.
  const from = word?.from ?? context.pos;
  if (context.state.sliceDoc(from - 1, from) === ".") return null;
  const options: Completion[] = snippets().map((s) =>
    snippetCompletion(s.template, {
      label: s.name,
      type: "text",
      detail: "snippet",
      info: s.template.replace(/#\{([^}]*)\}/g, "$1"),
      boost: -2,
    }),
  );
  return { from, options, validFor: /^\w*$/ };
};
