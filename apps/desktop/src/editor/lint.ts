/**
 * The SQL linter: columns that do not exist in the table their qualifier names (`o.totl`),
 * `SELECT *` on a production connection, and joins without ON or USING. The backend finds
 * the facts; the columns are checked here against the catalog the editor completes with.
 */
import type { Diagnostic } from "@codemirror/lint";
import type { EditorView } from "@codemirror/view";
import { api, type CompletionCatalog, type DriverKind } from "../api";
import { defineSetting, getSetting } from "../settings";
import { t } from "../i18n";

const lintSetting = defineSetting<boolean>({
  key: "editor.lint",
  section: "SQL editor",
  label: "Check the SQL while typing",
  description: "Unknown columns, SELECT * on production connections, joins without a condition.",
  default: true,
  control: { type: "boolean" },
});

interface LintContext {
  driver: DriverKind;
  catalog: CompletionCatalog | null | undefined;
  production: boolean;
}

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** A table's columns from the catalog, ignoring case; `undefined` when it is not there. */
function columnsOf(catalog: CompletionCatalog, schema: string | null, name: string): string[] | undefined {
  const schemas = Object.entries(catalog.schemas);
  const wanted = schema ?? catalog.default_schema;
  const tables = schemas.find(([s]) => wanted !== null && same(s, wanted))?.[1];
  const found = tables && Object.entries(tables).find(([t]) => same(t, name));
  return found?.[1];
}

export async function lintSql(view: EditorView, context: LintContext): Promise<Diagnostic[]> {
  if (!getSetting(lintSetting)) return [];
  const text = view.state.doc.toString();
  if (!text.trim()) return [];
  const lint = await api.lintSql(text, context.driver);
  // Typing since then moved the offsets; the next run reports them.
  if (view.state.doc.toString() !== text) return [];
  const out: Diagnostic[] = [];
  for (const issue of lint.issues) {
    if (issue.kind === "select_star" && context.production) {
      out.push({
        from: issue.from,
        to: issue.to,
        severity: "warning",
        message: t("SELECT * on a production connection: name the columns you need."),
      });
    } else if (issue.kind === "join_without_condition") {
      out.push({
        from: issue.from,
        to: issue.to,
        severity: "warning",
        message: t("JOIN without ON or USING joins every row with every row; write CROSS JOIN if that is meant."),
      });
    }
  }
  const catalog = context.catalog;
  if (!catalog) return out;
  for (const statement of lint.statements) {
    for (const ref of statement.columns) {
      const table = statement.tables.find((t) => same(t.alias, ref.qualifier));
      if (!table?.name) continue;
      const columns = columnsOf(catalog, table.schema, table.name);
      if (!columns || columns.some((c) => same(c, ref.column))) continue;
      out.push({
        from: ref.from,
        to: ref.to,
        severity: "error",
        message: t("Column {column} does not exist in {table}.", { column: ref.column, table: `${table.schema ? `${table.schema}.` : ""}${table.name}` }),
      });
    }
  }
  return out;
}
