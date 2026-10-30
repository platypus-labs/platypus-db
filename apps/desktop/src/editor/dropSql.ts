/**
 * A table or column dragged from the navigator and dropped in a SQL editor, or in a text
 * field (e.g. a filter box), which gets its name. The drag itself is ui/pointerDrag.ts.
 */
import { EditorView } from "@codemirror/view";
import type { SqlDrag } from "../sqlNames";
import type { Point } from "../ui/pointerDrag";

/** Where a dragged name can be dropped. */
export type SqlDropTarget =
  | { kind: "editor"; view: EditorView }
  | { kind: "field"; field: HTMLInputElement | HTMLTextAreaElement };

const TEXT_INPUT = new Set(["text", "search", ""]);

/** The editor or text field at or above `el` that takes a drop; `null` when none does. */
export function sqlDropTarget(el: Element | null): SqlDropTarget | null {
  const editor = el?.closest<HTMLElement>(".cm-editor");
  if (editor) {
    const view = EditorView.findFromDOM(editor);
    return view && !view.state.readOnly ? { kind: "editor", view } : null;
  }
  const field = el instanceof HTMLInputElement && TEXT_INPUT.has(el.type) ? el : el instanceof HTMLTextAreaElement ? el : null;
  return field && !field.readOnly && !field.disabled ? { kind: "field", field } : null;
}

/** Drops `sql` at `at`: into an editor its name, or a query of it into an empty editor or
 *  with Shift held; into a text field its name. */
export function dropSql(target: SqlDropTarget, sql: SqlDrag, at: Point, shift: boolean) {
  if (target.kind === "field") {
    target.field.focus();
    // As typing would, so the field's owner hears of it.
    document.execCommand("insertText", false, sql.name);
    return;
  }
  const { view } = target;
  const empty = view.state.doc.toString().trim() === "";
  const insert = sql.select && (empty || shift) ? sql.select : sql.name;
  const from = empty ? 0 : (view.posAtCoords(at) ?? view.state.doc.length);
  view.dispatch({
    changes: { from, to: empty ? view.state.doc.length : from, insert },
    selection: { anchor: from + insert.length },
    userEvent: "input.drop",
  });
  view.focus();
}
