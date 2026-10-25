// Quick filters of the data viewer: conditions on the focused cell's column, added to the
// viewer's WHERE from the grid's menu.
import { registerAction } from "../../actions";
import { api, isMongo } from "../../api";
import { quoteIdent } from "../../sqlNames";
import type { GridActionContext } from "../DataGrid";

/** The data viewer's filter, for the actions; set by the query tab. */
export interface ViewerFilter {
  /** The current `WHERE` condition, empty when none. */
  filter: string;
  /** Applies a new condition and reloads the data. */
  setFilter(filter: string): void;
}

declare module "../DataGrid" {
  interface GridSourceExtensions {
    viewer?: ViewerFilter;
  }
}

/** Rows read at most for "In selected values". */
const IN_LIMIT = 1000;

/** The focused cell's result row and column, when it is a loaded row of a data viewer. */
function target({ grid, source }: GridActionContext) {
  const focused = grid.focused();
  const row = focused ? grid.dataRow(focused.row) : null;
  const column = focused ? grid.columns()[focused.col] : undefined;
  if (!source?.viewer || source.jobId === null || row === null || !column || !focused) return null;
  // The conditions are SQL; a MongoDB filter is a query document typed by hand.
  if (isMongo(source.connection.driver)) return null;
  return {
    viewer: source.viewer,
    jobId: source.jobId,
    row,
    col: focused.col,
    name: quoteIdent(column.name, source.connection.driver),
  };
}

function add(viewer: ViewerFilter, condition: string) {
  const current = viewer.filter.trim();
  // An OR at the top would bind looser than the new AND.
  const base = /\bor\b/i.test(current) ? `(${current})` : current;
  viewer.setFilter(base ? `${base} AND ${condition}` : condition);
}

async function byValue(ctx: GridActionContext, equal: boolean) {
  const t = target(ctx);
  if (!t) return;
  const [literal] = await api.sqlLiterals(t.jobId, t.row, 1, t.col);
  if (literal === undefined) return;
  if (literal === "NULL") add(t.viewer, `${t.name} IS ${equal ? "" : "NOT "}NULL`);
  else add(t.viewer, `${t.name} ${equal ? "=" : "<>"} ${literal}`);
}

const available = (ctx: GridActionContext) => target(ctx) !== null;

registerAction({
  id: "grid.filter_equal",
  scope: "grid",
  title: "Equal to this value",
  submenu: "Filter",
  group: "filter",
  order: 70,
  when: available,
  run: (ctx) => byValue(ctx, true),
});

registerAction({
  id: "grid.filter_not_equal",
  scope: "grid",
  title: "Not equal to this value",
  submenu: "Filter",
  group: "filter",
  order: 71,
  when: available,
  run: (ctx) => byValue(ctx, false),
});

registerAction({
  id: "grid.filter_in",
  scope: "grid",
  title: "In selected values",
  submenu: "Filter",
  group: "filter",
  order: 72,
  when: available,
  run: async (ctx) => {
    const t = target(ctx);
    const sel = ctx.grid.selection();
    if (!t || !sel) return;
    const first = ctx.grid.dataRow(sel.r0) ?? 0;
    const last = ctx.grid.dataRow(sel.r1) ?? first;
    const literals = await api.sqlLiterals(t.jobId, first, Math.min(last - first + 1, IN_LIMIT), t.col);
    const values = [...new Set(literals.filter((l) => l !== "NULL"))];
    const hasNull = literals.includes("NULL");
    const list = values.length ? `${t.name} IN (${values.join(", ")})` : "";
    const nullTest = `${t.name} IS NULL`;
    add(t.viewer, list && hasNull ? `(${list} OR ${nullTest})` : list || nullTest);
  },
});

registerAction({
  id: "grid.filter_null",
  scope: "grid",
  title: "Is NULL",
  submenu: "Filter",
  group: "filter",
  order: 73,
  when: available,
  run: (ctx) => {
    const t = target(ctx);
    if (t) add(t.viewer, `${t.name} IS NULL`);
  },
});

registerAction({
  id: "grid.filter_not_null",
  scope: "grid",
  title: "Is not NULL",
  submenu: "Filter",
  group: "filter",
  order: 74,
  when: available,
  run: (ctx) => {
    const t = target(ctx);
    if (t) add(t.viewer, `${t.name} IS NOT NULL`);
  },
});

registerAction({
  id: "grid.filter_clear",
  scope: "grid",
  title: "Clear filter",
  submenu: "Filter",
  group: "filter",
  order: 75,
  when: ({ source }) => !!source?.viewer?.filter.trim(),
  run: ({ source }) => source!.viewer!.setFilter(""),
});
