// Quick filters of the data viewer: conditions on the focused cell's column, added to the
// viewer's query from the grid's menu: the editor's statement gets them in its top-level WHERE.
import { registerAction } from "../../actions";
import { type FilterTarget, api, isNoSql } from "../../api";
import { msg } from "../../i18n";
import type { GridActionContext } from "../DataGrid";

/** The data viewer's filter, for the actions; set by the query tab for a query it can rewrite. */
export interface ViewerFilter {
  /** Adds the condition to the shown statement's WHERE, in the editor, and runs it. */
  addCondition(condition: string): void;
  /** What a condition on each result column names (see `api.sqlFilterColumns`): `undefined`
   *  while it is asked, `null` when the statement cannot take a condition. */
  columns: FilterTarget[] | null | undefined;
}

declare module "../DataGrid" {
  interface GridSourceExtensions {
    viewer?: ViewerFilter;
  }
}

/** Rows read at most for "In selected values". */
const IN_LIMIT = 1000;

/** Why a quick filter is not offered for the focused cell's column. */
const REASONS = {
  pending: msg("Reading the columns of the result…"),
  computed: msg("This column is computed: filter it in the SQL"),
  ambiguous: msg("This column's table cannot be told: filter it in the SQL"),
  query: msg("This query cannot be filtered from the grid: add the condition in the SQL"),
};

/** The focused cell's result row and column, when it is a loaded row of a data viewer. */
function focusedCell({ grid, source }: GridActionContext) {
  const focused = grid.focused();
  const row = focused ? grid.dataRow(focused.row) : null;
  if (!source?.viewer || source.jobId === null || row === null || !focused) return null;
  // The conditions are SQL; a MongoDB filter is a query document typed by hand, a Redis one
  // the keys' pattern of BROWSE.
  if (isNoSql(source.connection.driver)) return null;
  return { viewer: source.viewer, jobId: source.jobId, row, col: focused.col };
}

/** What a condition on the cell's column names, or why none can be written. */
function columnOf(cell: NonNullable<ReturnType<typeof focusedCell>>): { name: string } | { reason: string } {
  const { columns } = cell.viewer;
  if (columns === undefined) return { reason: REASONS.pending };
  if (columns === null) return { reason: REASONS.query };
  const column = columns[cell.col];
  if (column?.kind === "column") return { name: column.sql };
  return { reason: column?.kind === "ambiguous" ? REASONS.ambiguous : REASONS.computed };
}

/** The focused cell, when a condition on its column can be written. */
function target(ctx: GridActionContext) {
  const cell = focusedCell(ctx);
  const column = cell && columnOf(cell);
  return cell && column && "name" in column ? { ...cell, name: column.name } : null;
}

async function byValue(ctx: GridActionContext, equal: boolean) {
  const t = target(ctx);
  if (!t) return;
  const [literal] = await api.sqlLiterals(t.jobId, t.row, 1, t.col);
  if (literal === undefined) return;
  if (literal === "NULL") t.viewer.addCondition(`${t.name} IS ${equal ? "" : "NOT "}NULL`);
  else t.viewer.addCondition(`${t.name} ${equal ? "=" : "<>"} ${literal}`);
}

const available = (ctx: GridActionContext) => focusedCell(ctx) !== null;

/** Offered but not run where no condition can be written; see `REASONS`. */
function disabled(ctx: GridActionContext): string | null {
  const cell = focusedCell(ctx);
  const column = cell && columnOf(cell);
  return column && "reason" in column ? column.reason : null;
}

registerAction({
  id: "grid.filter_equal",
  scope: "grid",
  title: "Equal to this value",
  submenu: "Filter",
  group: "filter",
  order: 70,
  when: available,
  disabled,
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
  disabled,
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
  disabled,
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
  disabled,
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
  disabled,
  run: (ctx) => {
    const t = target(ctx);
    if (t) t.viewer.addCondition(`${t.name} IS NOT NULL`);
  },
});
