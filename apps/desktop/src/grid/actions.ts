// The grid's built-in actions. Features register more for the "grid" scope from their
// own modules; they appear in the grid's context menu and answer to their shortcuts.
import { registerAction } from "../actions";
import { api } from "../api";
import type { GridActionContext } from "./DataGrid";
import { clipboardText, rememberCopy } from "./edit/paste";

registerAction({
  id: "grid.copy",
  scope: "grid",
  title: "Copy",
  shortcut: "Mod+C",
  group: "copy",
  order: 10,
  when: ({ grid }) => grid.selection(true) !== null,
  run: async (ctx) => {
    const rows = await selectedValues(ctx);
    const text = clipboardText(rows);
    // The text writes NULL and '' alike: pasting it back in the grid takes these cells.
    rememberCopy(text, rows);
    await navigator.clipboard.writeText(text);
  },
});

/** The whole values of the selected cells (`null` is NULL), pending edits included. The grid
 *  holds only the rows it has drawn, cut for display: the result's rows come from the backend.
 *  Computed columns are not on the backend: theirs are the values the grid works out. */
async function selectedValues({ grid, source }: GridActionContext): Promise<(string | null)[][]> {
  const sel = grid.selection(true)!;
  // The new rows of the data editor come first, then the result's rows in order.
  const last = grid.dataRow(sel.r1);
  const first = grid.dataRow(sel.r0) ?? 0;
  const loaded =
    source?.jobId != null && last !== null && sel.cols.length ? await api.rowValues(source.jobId, first, last - first + 1, sel.cols) : null;
  const rows: (string | null)[][] = [];
  for (let r = sel.r0; r <= sel.r1; r++) {
    const row = grid.dataRow(r);
    rows.push(
      sel.shown.map((c) => {
        const j = sel.cols.indexOf(c);
        if (j < 0) return grid.value(r, c) ?? null;
        const edit = source?.edit?.pending(r, c);
        if (edit?.kind === "value") return edit.text;
        if (edit?.kind === "null") return null;
        if (!edit && loaded && row !== null) return loaded[row - first]?.[j] ?? null;
        return grid.value(r, c) ?? null;
      }),
    );
  }
  return rows;
}

registerAction({
  id: "grid.select_all",
  scope: "grid",
  title: "Select all",
  shortcut: "Mod+A",
  group: "select",
  order: 50,
  when: ({ grid }) => grid.rowCount() > 0 && grid.columns().length > 0,
  run: ({ grid }) => grid.selectAll(),
});
