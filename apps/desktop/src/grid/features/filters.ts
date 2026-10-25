// Quick filters of the data viewer: conditions on the focused cell's column, added to the
// viewer's query from the grid's menu: the editor's statement gets them in its top-level WHERE.
import { registerAction } from "../../actions";
import { api, isMongo } from "../../api";
import { quoteIdent } from "../../sqlNames";
import type { GridActionContext } from "../DataGrid";

/** The data viewer's filter, for the actions; set by the query tab for a query it can rewrite. */
export interface ViewerFilter {
  /** Adds the condition to the shown statement's WHERE, in the editor, and runs it. */
  addCondition(condition: string): void;
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

async function byValue(ctx: GridActionContext, equal: boolean) {
  const t = target(ctx);
  if (!t) return;
  const [literal] = await api.sqlLiterals(t.jobId, t.row, 1, t.col);
  if (literal === undefined) return;
  if (literal === "NULL") t.viewer.addCondition(`${t.name} IS ${equal ? "" : "NOT "}NULL`);
  else t.viewer.addCondition(`${t.name} ${equal ? "=" : "<>"} ${literal}`);
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
    t.viewer.addCondition(list && hasNull ? `(${list} OR ${nullTest})` : list || nullTest);
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
    if (t) t.viewer.addCondition(`${t.name} IS NULL`);
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
    if (t) t.viewer.addCondition(`${t.name} IS NOT NULL`);
  },
});
