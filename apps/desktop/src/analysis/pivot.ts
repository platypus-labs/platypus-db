/**
 * Pivot of a result: what the form chooses and the DuckDB query it makes on the in-memory copy
 * of the rows (`result`). Pure: the unit tests import it, and the larger editor shows the text
 * so that the user can edit what it makes.
 */

export type PivotAggregate = "count" | "count_distinct" | "sum" | "avg" | "min" | "max";

/** How the rows are ordered: by their fields, or by the aggregate (the "Total" column when
 *  there is a column field; it needs totals). The row of totals stays last. */
export type PivotSort = "keys" | "keys_desc" | "value_desc" | "value_asc";

/** What a pivot is made of, columns by name so that it survives a run that reorders them. */
export interface PivotSettings {
  /** The fields down the left, at least one. */
  rows: string[];
  /** The field whose distinct values become columns; `null` for a plain grouping. */
  column: string | null;
  /** The field the aggregate reads; `null` counts rows. */
  value: string | null;
  aggregate: PivotAggregate;
  /** A row of totals and, with a column field, a column of them. */
  totals: boolean;
  sort: PivotSort;
}

/** A result column as the pivot sees it (`ColumnInfo` fits). */
export interface PivotColumn {
  name: string;
  numeric: boolean;
}

/** The pivot columns allowed; more make a grid nobody reads and a query that is slow to plan. */
export const MAX_PIVOT_COLUMNS = 200;

/** The label of a NULL in the column field, which `PIVOT` would drop. */
const NULL_LABEL = "NULL";

/** Stands for the totals in the query's own rows and columns, so that a real value named
 *  "Total" is not mistaken for them. */
const TOTAL = "__total__";

/** Every aggregate, in the order the form lists them. */
export const AGGREGATES: readonly PivotAggregate[] = ["count", "count_distinct", "sum", "avg", "min", "max"];

/** Aggregates that add up numbers. */
const NUMERIC_ONLY: readonly PivotAggregate[] = ["sum", "avg"];

/** The aggregates `value` can take: none counts rows; numeric aggregates need a number column. */
export function aggregatesFor(value: PivotColumn | null): PivotAggregate[] {
  if (!value) return ["count"];
  return AGGREGATES.filter((a) => value.numeric || !NUMERIC_ONLY.includes(a));
}

/** `name` as a DuckDB identifier. */
export const quote = (name: string): string => `"${name.replace(/"/g, '""')}"`;

/** `text` as a DuckDB string literal. */
const literal = (text: string): string => `'${text.replace(/'/g, "''")}'`;

/** The pivot columns a column field makes, with NULL as a column of its own. */
const label = (column: string): string => `coalesce(CAST(${quote(column)} AS VARCHAR), ${literal(NULL_LABEL)})`;

/** The query that counts the pivot columns of `column`, for the limit. */
export function pivotCountSql(column: string): string {
  return `SELECT count(DISTINCT ${label(column)}) FROM result`;
}

/** The settings as the form's rules read them: no field twice, an aggregate the value allows
 *  (else count), a sort that exists. `null` when no row field is left or a column is missing. */
export function normalize(settings: PivotSettings, columns: readonly PivotColumn[]): PivotSettings | null {
  const byName = new Map(columns.map((c) => [c.name, c]));
  const rows = [...new Set(settings.rows)].filter((n) => byName.has(n));
  if (rows.length === 0) return null;
  const column = settings.column !== null && byName.has(settings.column) && !rows.includes(settings.column) ? settings.column : null;
  const value = settings.value !== null && byName.has(settings.value) ? settings.value : null;
  let aggregate = settings.aggregate;
  if (!aggregatesFor(value === null ? null : byName.get(value)!).includes(aggregate)) aggregate = "count";
  if (value === null && aggregate !== "count") aggregate = "count";
  const totals = settings.totals;
  // The aggregate has a column to sort by when there is no column field, or with totals.
  const byValue = column === null || totals;
  const sort = !byValue && settings.sort.startsWith("value") ? "keys" : settings.sort;
  return { rows, column, value, aggregate, totals, sort };
}

/** What the aggregate reads: `v` is the value inside the source. */
function aggregateExpr(settings: PivotSettings, v: string): string {
  const { value, aggregate } = settings;
  if (value === null) return "count(*)";
  switch (aggregate) {
    case "count":
      return `count(${v})`;
    case "count_distinct":
      return `count(DISTINCT ${v})`;
    default:
      return `${aggregate}(${v})`;
  }
}

/** The name of the aggregate's column when there is no column field. */
function aggregateName(settings: PivotSettings): string {
  const base = settings.value === null ? settings.aggregate : `${settings.aggregate}_${settings.value}`;
  // A row field of the same name would make the sort and the grid ambiguous.
  return settings.rows.includes(base) ? `${base}_` : base;
}

const keyName = (i: number) => quote(`__k${i + 1}`);

/** The fields the source reads, the value last: with `totals` the row fields as text (the
 *  totals put a label among them) and the column field as `__c`. The value is always `__v`,
 *  else it would be twice in the source when it is also a field. */
function sourceSelect(settings: PivotSettings, totals: boolean): string[] {
  const { rows, column, value } = settings;
  const items = rows.map((r, i) => (totals ? `CAST(${quote(r)} AS VARCHAR) AS ${keyName(i)}` : quote(r)));
  if (column !== null) items.push(`${label(column)} AS ${totals ? quote("__c") : quote(column)}`);
  if (value !== null) items.push(`${quote(value)} AS ${quote("__v")}`);
  return items;
}

/** `ORDER BY` for the rows, the row of totals last. `keys` are the row fields' names in the
 *  query being sorted, `total` the expression that is true in the row of totals (`null` when
 *  there is none) and `value` what the sort by value reads. */
function orderBy(settings: PivotSettings, keys: string[], total: string | null, value: string): string {
  const items = total === null ? [] : [total];
  if (settings.sort === "value_desc") items.push(`${value} DESC NULLS LAST`);
  else if (settings.sort === "value_asc") items.push(`${value} ASC NULLS LAST`);
  else items.push(...keys.map((k) => (settings.sort === "keys_desc" ? `${k} DESC` : k)));
  return `ORDER BY ${items.join(", ")}`;
}

/** The row fields' output for the grid of totals: the label of the totals in the first one,
 *  blank in the others, named as the fields are. */
function keyOutputs(settings: PivotSettings, totalLabel: string): string[] {
  return settings.rows.map((r, i) => {
    const k = keyName(i);
    return `CASE WHEN ${k} = ${literal(TOTAL)} THEN ${i === 0 ? literal(totalLabel) : "NULL"} ELSE ${k} END AS ${quote(r)}`;
  });
}

/** A plain grouping, no column field. */
function grouping(settings: PivotSettings, totalLabel: string): string {
  const name = quote(aggregateName(settings));
  const agg = `${aggregateExpr(settings, quote(settings.value ?? ""))} AS ${name}`;
  if (!settings.totals) {
    const keys = settings.rows.map(quote);
    return [
      `SELECT ${[...keys, agg].join(", ")}`,
      "FROM result",
      `GROUP BY ${keys.join(", ")}`,
      orderBy(settings, keys, null, name),
    ].join("\n");
  }
  const keys = settings.rows.map((_, i) => keyName(i));
  const aggregate = `${aggregateExpr(settings, quote("__v"))} AS ${name}`;
  const select = sourceSelect(settings, true);
  // The totals row is the rows again under a label, so that every aggregate (average,
  // distinct count) is computed over all of them rather than from the groups.
  const copies = [...keys.map(() => literal(TOTAL)), ...(settings.value === null ? [] : [quote("__v")])];
  return [
    `WITH src AS (SELECT ${select.join(", ")} FROM result),`,
    `rolled AS (SELECT * FROM src UNION ALL SELECT ${copies.join(", ")} FROM src)`,
    `SELECT ${[...keyOutputs(settings, totalLabel), aggregate].join(", ")}`,
    "FROM rolled",
    `GROUP BY ${keys.join(", ")}`,
    orderBy(settings, keys, `${keys[0]!} IS NOT DISTINCT FROM ${literal(TOTAL)}`, name),
  ].join("\n");
}

/** The four parts of the rows with totals: as they are, with the column field as the totals'
 *  label, the totals row for every column, and for the total column. */
function pivotSources(settings: PivotSettings): string {
  const { rows, value } = settings;
  const keys = rows.map((_, i) => keyName(i));
  const v = value === null ? [] : [quote("__v")];
  const list = (k: string[], c: string) => [...k, c, ...v].join(", ");
  const total = literal(TOTAL);
  return [
    "SELECT * FROM src",
    `SELECT ${list(keys, total)} FROM src`,
    `SELECT ${list(keys.map(() => total), quote("__c"))} FROM src`,
    `SELECT ${list(keys.map(() => total), total)} FROM src`,
  ].join("\nUNION ALL ");
}

/** A pivot with a column field. */
function pivot(settings: PivotSettings, totalLabel: string): string {
  const agg = aggregateExpr(settings, quote("__v"));
  if (!settings.totals) {
    const keys = settings.rows.map(quote);
    return [
      `PIVOT (SELECT ${sourceSelect(settings, false).join(", ")} FROM result)`,
      `ON ${quote(settings.column!)} USING ${agg}`,
      `GROUP BY ${keys.join(", ")}`,
      orderBy(settings, keys, null, quote(TOTAL)),
    ].join("\n");
  }
  const keys = settings.rows.map((_, i) => keyName(i));
  const total = quote(TOTAL);
  return [
    `WITH src AS (SELECT ${sourceSelect(settings, true).join(", ")} FROM result),`,
    `rolled AS (${pivotSources(settings)})`,
    `SELECT ${keyOutputs(settings, totalLabel).join(", ")}, * EXCLUDE (${[...keys, total].join(", ")}), ${total} AS ${quote(totalLabel)}`,
    `FROM (PIVOT rolled ON "__c" USING ${agg} GROUP BY ${keys.join(", ")})`,
    orderBy(settings, keys, `${keys[0]!} IS NOT DISTINCT FROM ${literal(TOTAL)}`, total),
  ].join("\n");
}

/** The query that makes the pivot of `result`. `settings` must be normalized
 *  (`normalize`); `totalLabel` names the row and the column of totals. */
export function pivotSql(settings: PivotSettings, totalLabel: string): string {
  return settings.column === null ? grouping(settings, totalLabel) : pivot(settings, totalLabel);
}
