/** The admin tab's views of what the server has done: its costliest statements, and the use
 *  of its tables and indexes, with their upkeep. */
import { type ReactNode, useState } from "react";
import { type Maintenance, type ObjectStatistics, type ServerQuery, toCommandError } from "../api";
import { formatBytes } from "../format";
import { formatCount, msg, t } from "../i18n";
import { ask } from "../ui/ask";
import { duration } from "./sessions";
import {
  analyzeKnown,
  deadShare,
  type Issue,
  issues,
  type QueryOrder,
  queryMatches,
  shownObjects,
  sortQueries,
  tableOf,
} from "./statistics";

const ORDERS: [QueryOrder, string][] = [
  ["total", msg("Total time")],
  ["mean", msg("Mean time")],
  ["calls", msg("Calls")],
];

/** Each operation's name, and what it does. */
export const MAINTENANCE: Record<Maintenance, { label: string; what: string }> = {
  vacuum: { label: msg("Vacuum"), what: msg("Reclaims the space of deleted and updated rows.") },
  analyze: { label: msg("Analyze"), what: msg("Updates the statistics the planner chooses plans by.") },
  reindex: { label: msg("Reindex"), what: msg("Rebuilds the indexes of the table, which stays locked meanwhile.") },
  optimize: {
    label: msg("Optimize"),
    what: msg("Rebuilds the table and its indexes, reclaiming unused space; the table stays locked meanwhile."),
  },
  check: { label: msg("Check"), what: msg("Checks the table and its indexes for errors.") },
};

const ISSUES: Record<Issue, { label: string; why: string }> = {
  unused: { label: msg("unused"), why: msg("No read has used this index since the statistics were reset: it only slows writes down.") },
  full_scans: { label: msg("full scans"), why: msg("Read whole more often than through an index: a query may lack one.") },
  dead_rows: { label: msg("dead rows"), why: msg("Many rows were deleted or updated: a vacuum reclaims their space.") },
  free_space: { label: msg("unused space"), why: msg("Much of its space is free: Optimize gives it back.") },
  never_analyzed: { label: msg("never analyzed"), why: msg("The planner has no statistics of this table: Analyze makes them.") },
};

/** Runs a maintenance operation on a table; resolves to what the server said, a line each. */
type OnMaintain = (schema: string, table: string, operation: Maintenance) => Promise<string[]>;

const count = (n: number | null) => (n === null ? "" : formatCount(n));
const size = (n: number | null) => (n === null ? "" : formatBytes(n));

export function TopQueries({
  queries,
  onOpen,
  onExplain,
}: {
  queries: ServerQuery[];
  onOpen: (sql: string) => void;
  onExplain: ((sql: string) => void) | null;
}) {
  const [order, setOrder] = useState<QueryOrder>("total");
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<ServerQuery | null>(null);
  const shown = sortQueries(queries, order).filter((q) => queryMatches(q, filter));
  const chosen = selected && queries.find((q) => q.query === selected.query && q.database === selected.database);
  return (
    <>
      <div className="admin-toolbar">
        <div className="admin-filters" role="group" aria-label={t("Order by")}>
          {ORDERS.map(([value, text]) => (
            <button
              key={value}
              className={`admin-filter${order === value ? " active" : ""}`}
              data-order={value}
              onClick={() => setOrder(value)}
            >
              {t(text)}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <input
          type="search"
          placeholder={t("Filter by statement, database or user")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      <div className="admin-split">
        <div className="admin-list">
          <table className="meta-table admin-table admin-statements">
            <thead>
              <tr>
                <th>{t("Statement")}</th>
                <th className="num">{t("Calls")}</th>
                <th className="num">{t("Total time")}</th>
                <th className="num">{t("Mean time")}</th>
                <th className="num">{t("Rows")}</th>
                <th>{t("Database")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((q, i) => (
                <tr
                  key={i}
                  className={`admin-session${chosen === q ? " selected" : ""}`}
                  onClick={() => setSelected(q)}
                >
                  <td className="admin-query" title={q.query}>
                    {q.query}
                  </td>
                  <td className="num">{formatCount(q.calls)}</td>
                  <td className="num">{duration(q.total_seconds)}</td>
                  <td className="num">{duration(q.mean_seconds)}</td>
                  <td className="num">{count(q.rows)}</td>
                  <td>{q.database}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {shown.length === 0 && <p className="muted small admin-empty">{t("No statements match.")}</p>}
        </div>
        <QueryDetails query={chosen ?? null} onOpen={onOpen} onExplain={onExplain} />
      </div>
    </>
  );
}

function QueryDetails({
  query,
  onOpen,
  onExplain,
}: {
  query: ServerQuery | null;
  onOpen: (sql: string) => void;
  onExplain: ((sql: string) => void) | null;
}) {
  if (!query) {
    return (
      <aside className="admin-details empty">
        <p className="muted small">{t("Select a statement to see it whole, open it in an editor or explain it.")}</p>
      </aside>
    );
  }
  // A run with its values explains as it is; the statement has its values replaced.
  const sql = query.sample ?? query.query;
  const fields: [string, string, ReactNode][] = [
    ["database", t("Database"), query.database],
    ["user", t("User"), query.user],
    ["calls", t("Calls"), formatCount(query.calls)],
    ["total", t("Total time"), duration(query.total_seconds)],
    ["mean", t("Mean time"), duration(query.mean_seconds)],
    ["max", t("Longest run"), duration(query.max_seconds)],
    ["rows", t("Rows"), count(query.rows)],
  ];
  return (
    <aside className="admin-details">
      <Fields fields={fields} />
      <pre className="admin-statement">{query.query}</pre>
      {!query.sample && (
        <p className="muted small">{t("Its values are replaced by placeholders: fill them in to run or explain it.")}</p>
      )}
      <div className="admin-actions">
        <button onClick={() => onOpen(sql)}>{t("Open in editor")}</button>
        {onExplain && <button onClick={() => onExplain(sql)}>{t("Explain")}</button>}
      </div>
    </aside>
  );
}

function Fields({ fields }: { fields: [string, string, ReactNode][] }) {
  return (
    <dl className="admin-fields">
      {fields
        .filter(([, , value]) => value)
        .map(([key, name, value]) => (
          <div key={key} data-field={key}>
            <dt>{name}</dt>
            <dd>{value}</dd>
          </div>
        ))}
    </dl>
  );
}

/** The columns that some object fills: the others stay hidden. */
type Column = "size" | "rows" | "dead" | "free" | "full" | "index" | "vacuum" | "analyze";

const COLUMNS: [Column, string, (o: ObjectStatistics) => ReactNode, (o: ObjectStatistics) => unknown][] = [
  ["size", msg("Size"), (o) => size(o.size_bytes), (o) => o.size_bytes],
  ["rows", msg("Rows"), (o) => count(o.rows), (o) => o.rows],
  ["dead", msg("Dead rows"), (o) => count(o.dead_rows), (o) => o.dead_rows],
  ["free", msg("Unused space"), (o) => size(o.free_bytes), (o) => o.free_bytes],
  ["full", msg("Full scans"), (o) => count(o.full_scans), (o) => o.full_scans],
  ["index", msg("Index reads"), (o) => count(o.index_scans), (o) => o.index_scans],
  ["vacuum", msg("Last vacuum"), (o) => o.last_vacuum, (o) => o.last_vacuum],
  ["analyze", msg("Last analyze"), (o) => o.last_analyze, (o) => o.last_analyze],
];

const keyOf = (o: ObjectStatistics) => `${o.schema}\u0000${o.kind}\u0000${o.table ?? ""}\u0000${o.name}`;

export function ObjectsView({
  objects,
  maintenance,
  readOnly,
  onMaintain,
}: {
  objects: ObjectStatistics[];
  maintenance: Maintenance[];
  readOnly: boolean;
  onMaintain: OnMaintain;
}) {
  const [filter, setFilter] = useState("");
  const [onlyIssues, setOnlyIssues] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  const known = analyzeKnown(objects);
  const shown = shownObjects(objects, filter, onlyIssues);
  const withIssues = objects.filter((o) => issues(o, known).length > 0).length;
  // The dates are in the details: the list keeps to the numbers.
  const columns = COLUMNS.filter(
    ([key, , , value]) => key !== "vacuum" && key !== "analyze" && objects.some((o) => value(o) !== null),
  );
  const chosen = objects.find((o) => keyOf(o) === selected) ?? null;
  return (
    <>
      <div className="admin-toolbar">
        <label className="check small">
          <input type="checkbox" checked={onlyIssues} onChange={(e) => setOnlyIssues(e.target.checked)} />
          {t("Only those worth a look ({n})", { n: formatCount(withIssues) })}
        </label>
        <span className="spacer" />
        <input
          type="search"
          placeholder={t("Filter by schema, table or index")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
      </div>
      <div className="admin-split">
        <div className="admin-list">
          <table className="meta-table admin-table admin-objects">
            <thead>
              <tr>
                <th>{t("Table or index")}</th>
                {columns.map(([key, name]) => (
                  <th key={key} className="num">
                    {t(name)}
                  </th>
                ))}
                <th>{t("Notes")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.map((o) => (
                <tr
                  key={keyOf(o)}
                  data-name={o.name}
                  className={`admin-session admin-kind-${o.kind}${keyOf(o) === selected ? " selected" : ""}`}
                  onClick={() => setSelected(keyOf(o))}
                >
                  <td className="admin-object-name">
                    {o.kind === "table" ? (
                      <>
                        <span className="muted">{o.schema}.</span>
                        {o.name}
                      </>
                    ) : (
                      o.name
                    )}
                    {o.unique && <span className="admin-own">{t("unique")}</span>}
                  </td>
                  {columns.map(([key, , render]) => (
                    <td key={key} className="num">
                      {render(o)}
                    </td>
                  ))}
                  <td>
                    <IssueBadges object={o} known={known} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {shown.length === 0 && <p className="muted small admin-empty">{t("No tables or indexes match.")}</p>}
        </div>
        <ObjectDetails
          key={chosen ? keyOf(chosen) : ""}
          object={chosen}
          known={known}
          maintenance={maintenance}
          readOnly={readOnly}
          onMaintain={onMaintain}
        />
      </div>
    </>
  );
}

function IssueBadges({ object, known }: { object: ObjectStatistics; known: boolean }) {
  return (
    <span className="admin-issues">
      {issues(object, known).map((issue) => (
        <span key={issue} className="admin-badge transaction" data-issue={issue} title={t(ISSUES[issue].why)}>
          <span className="admin-dot" />
          {issue === "dead_rows"
            ? t("{share}% dead rows", { share: Math.round(deadShare(object) * 100) })
            : t(ISSUES[issue].label)}
        </span>
      ))}
    </span>
  );
}

function ObjectDetails({
  object,
  known,
  maintenance,
  readOnly,
  onMaintain,
}: {
  object: ObjectStatistics | null;
  known: boolean;
  maintenance: Maintenance[];
  readOnly: boolean;
  onMaintain: OnMaintain;
}) {
  const [running, setRunning] = useState<Maintenance | null>(null);
  const [result, setResult] = useState<{ operation: Maintenance; lines: string[]; error: boolean } | null>(null);
  if (!object) {
    return (
      <aside className="admin-details empty">
        <p className="muted small">{t("Select a table or an index to see why it is worth a look and maintain its table.")}</p>
      </aside>
    );
  }
  const { schema } = object;
  const table = tableOf(object);
  async function run(operation: Maintenance) {
    const confirmed = await ask(
      t("{operation} {table}? {what}", {
        operation: t(MAINTENANCE[operation].label),
        table: `${schema}.${table}`,
        what: t(MAINTENANCE[operation].what),
      }),
      { confirm: t(MAINTENANCE[operation].label) },
    );
    if (!confirmed) return;
    setRunning(operation);
    setResult(null);
    try {
      setResult({ operation, lines: await onMaintain(schema, table, operation), error: false });
    } catch (e) {
      setResult({ operation, lines: [toCommandError(e).message], error: true });
    } finally {
      setRunning(null);
    }
  }
  const found = issues(object, known);
  const fields: [string, string, ReactNode][] = [
    ["table", t("Table"), object.kind === "index" ? `${schema}.${table}` : null],
    ...COLUMNS.map(([key, name, render]): [string, string, ReactNode] => [key, t(name), render(object)]),
  ];
  return (
    <aside className="admin-details" data-object={object.name}>
      <div className="admin-details-title">
        <strong>{object.kind === "table" ? `${object.schema}.${object.name}` : object.name}</strong>
        <span className="muted small">{t(object.kind === "table" ? "table" : "index")}</span>
      </div>
      <Fields fields={fields} />
      {found.map((issue) => (
        <div key={issue} className="notice warn small" data-issue={issue}>
          {t(ISSUES[issue].why)}
        </div>
      ))}
      {maintenance.length > 0 && (
        <>
          <h3 className="admin-heading">{t("Maintenance of {table}", { table })}</h3>
          <div className="admin-actions">
            {maintenance.map((operation) => (
              <button
                key={operation}
                data-operation={operation}
                disabled={readOnly || running !== null}
                title={readOnly ? t("The connection is read-only") : t(MAINTENANCE[operation].what)}
                onClick={() => void run(operation)}
              >
                {running === operation ? t("Running…") : t(MAINTENANCE[operation].label)}
              </button>
            ))}
          </div>
        </>
      )}
      {result && (
        <div className={`notice small ${result.error ? "error" : "ok"}`} data-field="maintenance-result">
          {result.error
            ? result.lines[0]
            : t("{operation} done.", { operation: t(MAINTENANCE[result.operation].label) })}
        </div>
      )}
      {result && !result.error && result.lines.length > 0 && (
        <pre className="admin-statement admin-maintenance-output">{result.lines.join("\n")}</pre>
      )}
    </aside>
  );
}
