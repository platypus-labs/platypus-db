/**
 * A notebook: SQL and Markdown cells on one connection, with parameters they share. Each SQL
 * cell is named by its place among them (`c1`, `c2`, …): a cell that runs on the results reads
 * the rows the others loaded as the tables of those names, in the in-memory DuckDB of "Query
 * with SQL", without running them again on the server. The file format and the plan as
 * lines are here too (the export to HTML is in `html.ts`). Pure: the unit tests import it.
 */
import type { PlanNode, ResultTable } from "../api";

export interface NotebookCell {
  id: string;
  kind: "sql" | "markdown";
  text: string;
  /** SQL: runs on the results of the other cells, not on the connection. */
  onResults: boolean;
  /** SQL: its result shown as a chart; `config` is the chart's (`ChartConfig`), `null`
   *  until chosen. */
  chart: { shown: boolean; config: unknown } | null;
  /** SQL: its plan shown under it. */
  plan: boolean;
}

/** What a notebook keeps: its cells and parameter values. */
export interface NotebookContent {
  cells: NotebookCell[];
  params: Record<string, string>;
}

let next = 0;
const newCellId = () => `${Date.now().toString(36)}-${(next++).toString(36)}`;

export const newCell = (kind: NotebookCell["kind"], text = ""): NotebookCell => ({
  id: newCellId(),
  kind,
  text,
  onResults: false,
  chart: null,
  plan: false,
});

/** The name of each SQL cell, `c1` for the first. */
export function cellNames(cells: readonly NotebookCell[]): Map<string, string> {
  const names = new Map<string, string>();
  for (const cell of cells) if (cell.kind === "sql") names.set(cell.id, `c${names.size + 1}`);
  return names;
}

/** The tables a cell on the results reads: every other SQL cell whose rows are loaded, by
 *  its name (all of them without `except`). `jobs` holds the job of each cell with rows. */
export function resultTables(
  cells: readonly NotebookCell[],
  jobs: ReadonlyMap<string, number>,
  except?: string,
): ResultTable[] {
  const names = cellNames(cells);
  return cells.flatMap((cell) => {
    const job = jobs.get(cell.id);
    const table = names.get(cell.id);
    return cell.id !== except && job !== undefined && table ? [{ job_id: job, table }] : [];
  });
}

/** Whether a copy of `loaded` has every table of `wanted`, with the same rows. */
export function copyHolds(loaded: readonly ResultTable[], wanted: readonly ResultTable[]): boolean {
  return wanted.every((w) => loaded.some((l) => l.table === w.table && l.job_id === w.job_id));
}

const FORMAT = "platypus-notebook";
const VERSION = 1;

/** The notebook as a `.pnb` file: JSON. */
export function notebookFile(content: NotebookContent): string {
  return `${JSON.stringify({ format: FORMAT, version: VERSION, cells: content.cells.map(({ id: _, ...cell }) => cell), params: content.params }, null, 2)}\n`;
}

/** The cells and parameters of `value` (a file, or what a tab saved), whatever it lacks or
 *  holds wrongly; `chart` reads a cell's chart configuration. */
export function readContent(value: unknown, chart: (v: unknown) => unknown): NotebookContent {
  const v = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const cells = Array.isArray(v.cells)
    ? v.cells.flatMap((c: unknown): NotebookCell[] => {
        if (!c || typeof c !== "object") return [];
        const cell = c as Record<string, unknown>;
        if ((cell.kind !== "sql" && cell.kind !== "markdown") || typeof cell.text !== "string") return [];
        const saved = cell.chart && typeof cell.chart === "object" ? (cell.chart as Record<string, unknown>) : null;
        return [
          {
            id: typeof cell.id === "string" ? cell.id : newCellId(),
            kind: cell.kind,
            text: cell.text,
            onResults: cell.kind === "sql" && cell.onResults === true,
            chart: cell.kind === "sql" && saved ? { shown: saved.shown === true, config: chart(saved.config) } : null,
            plan: cell.kind === "sql" && cell.plan === true,
          },
        ];
      })
    : [];
  const params: Record<string, string> = {};
  if (v.params && typeof v.params === "object") {
    for (const [key, value] of Object.entries(v.params)) if (typeof value === "string") params[key] = value;
  }
  return { cells, params };
}

/** Reads a `.pnb` file; throws on another kind of file. */
export function parseNotebookFile(text: string, chart: (v: unknown) => unknown): NotebookContent {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new Error("not a notebook file");
  }
  if (!value || typeof value !== "object" || (value as { format?: unknown }).format !== FORMAT) {
    throw new Error("not a notebook file");
  }
  return readContent(value, chart);
}

/** The parameters `keys` lacks a value for. */
export const missingParams = (keys: readonly string[], params: Readonly<Record<string, string>>) =>
  keys.filter((key) => !(key in params) || params[key] === "");

/** One step of a plan, as the cell and the export list it. */
export interface PlanLine {
  depth: number;
  operation: string;
  detail: string | null;
  rows: number | null;
  cost: number | null;
}

export function planLines(root: PlanNode, depth = 0): PlanLine[] {
  return [
    { depth, operation: root.operation, detail: root.detail, rows: root.rows, cost: root.cost },
    ...root.children.flatMap((child) => planLines(child, depth + 1)),
  ];
}
