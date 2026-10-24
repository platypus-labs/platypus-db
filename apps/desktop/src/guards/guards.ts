/**
 * Checks before statements run: changes that hit every row of a table (UPDATE or DELETE
 * without WHERE, TRUNCATE) and any change on a production connection are confirmed first,
 * and an open transaction shows how long it has been open.
 */
import { api, type SavedConnection, type StatementSpan } from "../api";
import { isProduction } from "../connections";
import { defineSetting, getSetting } from "../settings";

export const unfilteredSetting = defineSetting<string>({
  key: "safety.unfiltered",
  section: "Safety",
  label: "UPDATE or DELETE without WHERE",
  description: "Also TRUNCATE. The estimated number of rows is shown.",
  default: "confirm",
  control: {
    type: "select",
    options: [
      { value: "confirm", label: "Ask before running" },
      { value: "block", label: "Never run" },
      { value: "off", label: "Run without asking" },
    ],
  },
});

export const productionSetting = defineSetting<boolean>({
  key: "safety.confirm_production",
  section: "Safety",
  label: "Ask before changes on production connections",
  description: "DDL and DML, with the statements listed; the data editor shows its SQL first.",
  default: true,
  control: { type: "boolean" },
});

export const transactionWarning = defineSetting<number>({
  key: "safety.transaction_warning_minutes",
  section: "Safety",
  label: "Warn when a transaction stays open for (minutes)",
  description: "0 never warns. Open transactions hold locks and connections.",
  default: 10,
  control: { type: "number", min: 0, max: 1440, step: 1 },
});

/** A statement about to run, with what it does. */
export interface CheckedStatement {
  sql: string;
  span: StatementSpan;
}

export interface UnfilteredChange {
  sql: string;
  table: string;
  /** About how many rows; `null` when unknown. */
  rows: number | null;
}

/** What to show before running; `null` when nothing needs asking. */
export interface GuardRequest {
  connection: SavedConnection;
  unfiltered: UnfilteredChange[];
  /** Changes on a production connection; empty elsewhere. */
  production: string[];
  /** Running is not allowed by the settings, only shown. */
  blocked: boolean;
}

/** Whether a production connection must confirm `statements` before they change it. */
export function needsProductionConfirm(connection: SavedConnection): boolean {
  return isProduction(connection) && getSetting(productionSetting);
}

/** What the statements would do that needs confirming, estimating rows on `session`. */
export async function check(
  connection: SavedConnection,
  statements: CheckedStatement[],
  session: () => Promise<number | null>,
): Promise<GuardRequest | null> {
  const mode = getSetting(unfilteredSetting);
  const risky = mode === "off" ? [] : statements.filter((s) => s.span.unfiltered);
  const production = needsProductionConfirm(connection)
    ? statements.filter((s) => s.span.kind === "dml" || s.span.kind === "ddl").map((s) => s.sql)
    : [];
  if (risky.length === 0 && production.length === 0) return null;
  const id = risky.length ? await session().catch(() => null) : null;
  const unfiltered = await Promise.all(
    risky.map(async ({ sql, span }) => {
      const t = span.unfiltered!;
      const rows = id === null ? null : await api.estimateRows(id, t.schema, t.name).catch(() => null);
      return { sql, table: t.schema ? `${t.schema}.${t.name}` : t.name, rows };
    }),
  );
  return { connection, unfiltered, production, blocked: mode === "block" && unfiltered.length > 0 };
}

/** The open transaction's age as `m:ss`, or `h:mm:ss` past an hour. */
export function elapsed(ms: number): string {
  const s = Math.floor(ms / 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return h > 0 ? `${h}:${pad(m)}:${pad(s % 60)}` : `${m}:${pad(s % 60)}`;
}
