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

/** The editor or text field at or above `el` that takes a drop; `null` when none does.
 *  In an editor only its text takes one, not its tooltips or panels (search, ...). */
export function sqlDropTarget(el: Element | null): SqlDropTarget | null {
  const editor = el?.closest<HTMLElement>(".cm-editor");
  if (editor) {
    const view = el?.closest(".cm-scroller") ? EditorView.findFromDOM(editor) : null;
    return view && !view.state.readOnly && view.contentDOM.isContentEditable ? { kind: "editor", view } : null;
  }
  const field = el instanceof HTMLInputElement && TEXT_INPUT.has(el.type) ? el : el instanceof HTMLTextAreaElement ? el : null;
  return field && !field.readOnly && !field.disabled ? { kind: "field", field } : null;
}

/** Drops `sql` at `at`: into an editor its name where it is dropped, or a query of it into
 *  an empty editor or with Shift held; into a text field its name, at its caret. */
export function dropSql(target: SqlDropTarget, sql: SqlDrag, at: Point, shift: boolean) {
  if (target.kind === "field") {
    const { field } = target;
    // The caret cannot be read from a point inside a field: the name goes where it is.
    const end = field.value.length;
    field.setRangeText(sql.name, field.selectionStart ?? end, field.selectionEnd ?? end, "end");
    // As typing does, so the field's owner (React) takes the new value.
    field.dispatchEvent(new Event("input", { bubbles: true }));
    field.focus();
    return;
  }
  const { view } = target;
  const empty = view.state.doc.toString().trim() === "";
  const insert = sql.select && (empty || shift) ? sql.select : sql.name;
  const from = empty ? 0 : view.posAtCoords(at);
  if (from === null) return;
  view.dispatch({
    changes: { from, to: empty ? view.state.doc.length : from, insert },
    selection: { anchor: from + insert.length },
    userEvent: "input.drop",
  });
  view.focus();
}
