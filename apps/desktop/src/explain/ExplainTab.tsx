import { Icon } from "../ui/Icon";
import { type CSSProperties, createElement, useEffect, useRef, useState } from "react";
import { api, type Explain, type PlanNode, type PlanSource, type SavedConnection, toCommandError } from "../api";
import type { TabBase, TabProps } from "../tabs/registry";
import { workbench } from "../workbench";
import { choose } from "../ui/ask";
import { locale, t, msg } from "../i18n";
import { ComparePlans } from "./ComparePlans";
import { stepName } from "./compare";
import { PlanDialog } from "./PlanDialog";
import { PlanGraph } from "./PlanGraph";

export type ExplainView = "tree" | "graph" | "text" | "table" | "compare";

/** The plan of one statement, as a tree, a graph, text or a table; or a plan read from a
 *  document. */
export interface ExplainTabModel extends TabBase {
  kind: "explain";
  sql: string;
  /** The statement runs (its changes rolled back) for actual rows and times. */
  analyze: boolean;
  view?: ExplainView;
  /** A plan pasted or opened from a file: shown as it is, nothing runs. */
  plan?: Explain;
  /** The pasted plan's name: its file's, or "Pasted plan". */
  name?: string;
  /** The plan this one is compared with, and its name. */
  baseline?: Explain | null;
  baselineName?: string;
}

declare module "../tabs/registry" {
  interface TabKinds {
    explain: ExplainTabModel;
  }
}

const VIEWS: [ExplainView, string][] = [
  ["tree", msg("Tree")],
  ["graph", msg("Graph")],
  ["text", msg("Text")],
  ["table", msg("Table")],
];

let plans = 0;

/** Asks for a plan to paste or open, and hands it to `onRead`. */
export function askPlan(title: string, onRead: (plan: Explain, name: string, source: PlanSource) => void) {
  workbench().openDialog((close) =>
    createElement(PlanDialog, { title, onClose: close, onRead: (read, name) => onRead(read.explain, name, read.source) }),
  );
}

/** Opens a plan pasted or opened from a file in an explain tab of its own, on no
 *  connection. */
export function openPlan() {
  askPlan(t("Open a query plan"), (plan, name, source) => {
    const connection: SavedConnection = {
      id: `plan:${++plans}`,
      name,
      // Each source is named as its driver.
      driver: source,
      host: "",
      port: null,
      database: "",
      user: "",
      ssl_mode: "disable",
      ssl_root_cert: null,
      save_password: false,
      read_only: true,
      color: null,
    };
    workbench().openTemporaryTab("explain", connection, { sql: "", analyze: false, plan, name }, () => {});
  });
}

/** A node with its place in the tree; `path` is its child indexes from the root. */
interface Step {
  node: PlanNode;
  depth: number;
  path: string;
}

function steps(node: PlanNode, collapsed?: Set<string>, depth = 0, path = "0", out: Step[] = []): Step[] {
  out.push({ node, depth, path });
  if (!collapsed?.has(path)) node.children.forEach((c, i) => steps(c, collapsed, depth + 1, `${path}.${i}`, out));
  return out;
}

/** Numbers in the interface's language, not the system's: the plan's own text (`0.0`) sits
 *  beside them. */
const num = (v: number | null) => (v === null ? "" : new Intl.NumberFormat(locale(), { maximumFractionDigits: 2 }).format(v));
const percent = { format: (v: number) => new Intl.NumberFormat(locale(), { style: "percent", maximumFractionDigits: 1 }).format(v) };

/** Each step's share of the work it does itself: of the time when analyzed, else of the
 *  cost. */
function shares(all: Step[], analyzed: boolean): Map<PlanNode, number> {
  const own = (n: PlanNode) => (analyzed ? n.self_ms : n.self_cost) ?? 0;
  const total = all.reduce((sum, s) => sum + own(s.node), 0);
  return new Map(all.map(({ node }) => [node, total > 0 ? own(node) / total : 0]));
}

/** An estimate this many times off, either way, is worth a look. */
const MISESTIMATE = 10;

/** How many times more (positive) or fewer (negative) rows the step returned than the
 *  planner expected, when that is far off. */
function misestimate(n: PlanNode): number | null {
  if (n.rows === null || n.actual_rows === null || n.loops === 0) return null;
  const factor = Math.max(n.rows, n.actual_rows) / Math.max(Math.min(n.rows, n.actual_rows), 1);
  if (factor < MISESTIMATE) return null;
  return n.actual_rows > n.rows ? factor : -factor;
}

function Misestimate({ node }: { node: PlanNode }) {
  const factor = misestimate(node);
  if (factor === null) return null;
  const times = num(Math.round(Math.abs(factor)));
  return (
    <span
      className="explain-misestimate"
      title={t("Estimated {rows} rows, got {actual}: the planner may have chosen badly", { rows: num(node.rows), actual: num(node.actual_rows) })}
    >
      {t(factor > 0 ? "{n}× more rows" : "{n}× fewer rows", { n: times })}
    </span>
  );
}

/** The step tinted by its share of the work. */
const heat = (share: number) => ({ "--heat": `${Math.round(share * 100)}%` }) as CSSProperties;

/** One line per step, indented, with the figures the database reported. */
function planText(all: Step[]) {
  return all
    .map(({ node, depth }) => {
      const estimate = [node.cost !== null && `cost=${num(node.cost)}`, node.rows !== null && `rows=${num(node.rows)}`];
      const actual = [
        node.actual_ms !== null && `time=${num(node.actual_ms)} ms`,
        node.actual_rows !== null && `rows=${num(node.actual_rows)}`,
        node.loops !== null && `loops=${num(node.loops)}`,
      ];
      const group = (parts: (string | false)[], prefix = "") => {
        const shown = parts.filter(Boolean);
        return shown.length ? `  (${prefix}${shown.join(" ")})` : "";
      };
      return `${"   ".repeat(depth)}-> ${stepName(node)}${group(estimate)}${group(actual, "actual ")}`;
    })
    .join("\n");
}

function rawPlan(explain: Explain) {
  if (explain.format !== "json") return explain.plan;
  try {
    return JSON.stringify(JSON.parse(explain.plan), null, 2);
  } catch {
    return explain.plan;
  }
}

export function ExplainTab({ tab, active, ensureSession, onChange }: TabProps<ExplainTabModel>) {
  const [explain, setExplain] = useState<Explain | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [version, setVersion] = useState(0);
  const [selected, setSelected] = useState("0");
  const [collapsed, setCollapsed] = useState(new Set<string>());
  /** The plan before the last refresh, to compare with. */
  const [previous, setPrevious] = useState<Explain | null>(null);
  const latest = useRef<Explain | null>(null);
  /** What the plan is of: the pasted plan's name, or the statement. */
  const subject = tab.plan ? tab.name : tab.sql;
  const view = tab.view === "compare" && !tab.baseline ? "tree" : (tab.view ?? "tree");

  useEffect(() => {
    // A plan read from a document: nothing to run.
    if (tab.plan) {
      setExplain(tab.plan);
      return;
    }
    let cancelled = false;
    void (async () => {
      setError(null);
      setRunning(true);
      try {
        // A session of its own: an analyzed statement's transaction must not be shared with
        // the navigator, and a long ANALYZE must not hold it up.
        const sid = await ensureSession();
        if (sid === null || cancelled) return;
        // Production sessions start in manual commit; the rollback of analyze needs no
        // transaction left open by an earlier plain explain.
        await api.setAutocommit(sid, true).catch(() => {});
        const plan = await api.explain(sid, tab.sql, tab.analyze);
        if (cancelled) return;
        if (latest.current) setPrevious(latest.current);
        latest.current = plan;
        setExplain(plan);
      } catch (e) {
        if (!cancelled) setError(toCommandError(e).message);
      } finally {
        if (!cancelled) setRunning(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [version]);

  const all = explain ? steps(explain.root) : [];
  const node = all.find((s) => s.path === selected)?.node ?? explain?.root;
  const share = shares(all, explain?.analyzed ?? false);
  const heated = [...share.values()].some((v) => v > 0);
  const own = t(explain?.analyzed ? "time" : "cost");
  const shareOf = (n: PlanNode) => share.get(n) ?? 0;
  const graphFigures = (n: PlanNode) =>
    [
      n.actual_rows !== null
        ? t("{n} rows", { n: num(n.actual_rows * (n.loops ?? 1)) })
        : n.rows !== null && t("est {n}", { n: num(n.rows) }),
      n.actual_ms !== null ? `${num(n.actual_ms)} ms` : n.cost !== null && t("cost {n}", { n: num(n.cost) }),
    ]
      .filter(Boolean)
      .join(" · ");

  async function compare() {
    const choice = await choose(t("Compare this plan with…"), [
      ...(previous ? [{ label: t("The previous run"), value: "previous" as const, primary: true }] : []),
      { label: t("Another plan…"), value: "other" as const },
    ]);
    if (choice === "previous" && previous) {
      onChange({ baseline: previous, baselineName: t("Previous run"), view: "compare" });
    } else if (choice === "other") {
      askPlan(t("Plan to compare with"), (plan, name) => onChange({ baseline: plan, baselineName: name, view: "compare" }));
    }
  }

  const toggle = (path: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(path)) next.add(path);
      return next;
    });

  return (
    <div className="object-tab explain-tab" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <div className="object-title">
          <strong>{t(tab.plan ? "Query plan" : tab.analyze ? "Explain analyze" : "Explain")}</strong>
          <code className="explain-sql muted small" title={subject}>
            {subject}
          </code>
        </div>
        <span className="spacer" />
        {explain && <Summary node={explain.root} />}
        {running && <span className="muted small">{t("Running…")}</span>}
        {explain && (
          <button onClick={() => void compare()} title={t("Compare this plan with the previous run or another plan")}>
            {t("Compare…")}
          </button>
        )}
        {explain && (
          <button
            className="square"
            onClick={() => void navigator.clipboard.writeText(rawPlan(explain))}
            title={t("Copy the plan, as the database returned it")}
            aria-label={t("Copy plan")}
          >
            <Icon name="copy" />
          </button>
        )}
        {!tab.plan && (
          <button
            disabled={running}
            onClick={() => setVersion((v) => v + 1)}
            title={t(tab.analyze ? "Refresh: runs the statement again" : "Refresh: asks for the plan again")}
            aria-label={t("Refresh")}
            className="square"
          >
            <Icon name="refresh" />
          </button>
        )}
      </header>
      <nav className="object-sections">
        {VIEWS.map(([v, label]) => (
          <button key={v} className={v === view ? "active" : ""} onClick={() => onChange({ view: v })}>
            {t(label)}
          </button>
        ))}
        {tab.baseline && (
          <button className={view === "compare" ? "active" : ""} onClick={() => onChange({ view: "compare" })}>
            {t("Comparison")}
          </button>
        )}
      </nav>
      <div className="object-body">
        {error && <div className="notice error">{error}</div>}
        {tab.analyze && explain && !explain.analyzed && (
          <div className="notice small">{t("This database cannot run a statement for its plan: estimates only.")}</div>
        )}
        {explain?.suggestions.map((s, i) => (
          <div key={i} className="notice warn small explain-suggestion">
            <strong>{s.table}</strong>{" "}
            {s.rows_read === null
              ? t("is read in full to keep the rows where")
              : t(explain.analyzed ? "is read in full ({n} rows) to keep the rows where" : "is read in full (about {n} rows) to keep the rows where", {
                  n: num(s.rows_read),
                })}{" "}
            <code>{s.filter}</code>
            {t(": an index on the columns of that condition may help.")}
          </div>
        ))}
        {explain && view === "tree" && (
          <div className="explain-split">
            <div className="explain-tree" role="tree">
              {steps(explain.root, collapsed).map(({ node: n, depth, path }) => (
                <div
                  key={path}
                  role="treeitem"
                  aria-selected={path === selected}
                  aria-expanded={n.children.length ? !collapsed.has(path) : undefined}
                  className={`explain-node${path === selected ? " selected" : ""}`}
                  style={{ paddingLeft: depth * 18 + 4, ...heat(shareOf(n)) }}
                  onClick={() => setSelected(path)}
                >
                  <button
                    className="explain-toggle"
                    aria-label={t(collapsed.has(path) ? "Expand" : "Collapse")}
                    style={{ visibility: n.children.length ? "visible" : "hidden" }}
                    onClick={(e) => {
                      e.stopPropagation();
                      toggle(path);
                    }}
                  >
                    {collapsed.has(path) ? "▸" : "▾"}
                  </button>
                  <span className="explain-op">{n.operation}</span>
                  {n.detail && <span className="explain-detail">{n.detail}</span>}
                  <span className="spacer" />
                  <Misestimate node={n} />
                  <Figures node={n} />
                  {heated && (
                    <span className="explain-bar" aria-hidden="true">
                      <i className={shareOf(n) >= 0.4 ? "hot" : undefined} />
                    </span>
                  )}
                  {heated && (
                    <span className="explain-share" title={t("Share of the {what} spent in this step itself", { what: own })}>
                      {percent.format(shareOf(n))}
                    </span>
                  )}
                </div>
              ))}
            </div>
            {node && <Properties node={node} />}
          </div>
        )}
        {explain && view === "graph" && (
          <div className="explain-split">
            <PlanGraph root={explain.root} selected={selected} onSelect={setSelected} shareOf={shareOf} figures={graphFigures} />
            {node && <Properties node={node} />}
          </div>
        )}
        {explain && view === "compare" && tab.baseline && (
          <>
            <div className="explain-compare-head">
              <span className="muted small">
                {t("{before} against {after}", {
                  before: tab.baselineName ?? "",
                  after: tab.plan ? (tab.name ?? "") : t("this plan"),
                })}
              </span>
              <span className="spacer" />
              <button onClick={() => onChange({ baseline: null, view: "tree" })}>{t("Stop comparing")}</button>
            </div>
            <ComparePlans
              before={tab.baseline}
              after={explain}
              beforeName={tab.baselineName ?? ""}
              afterName={tab.plan ? (tab.name ?? "") : t("This plan")}
              num={num}
            />
          </>
        )}
        {explain && view === "text" && <pre className="explain-text">{planText(all)}</pre>}
        {explain && view === "table" && (
          <table className="meta-table explain-table">
            <thead>
              <tr>
                <th>{t("Operation")}</th>
                <th>{t("Cost")}</th>
                <th>{t("Rows")}</th>
                <th>{t("Actual rows")}</th>
                <th>{t("Loops")}</th>
                <th>{t("Time (ms)")}</th>
                {heated && <th>{t("Own {what}", { what: own })}</th>}
              </tr>
            </thead>
            <tbody>
              {all.map(({ node: n, depth, path }) => (
                <tr key={path}>
                  <td style={{ paddingLeft: depth * 16 }}>{stepName(n)}</td>
                  <td>{num(n.cost)}</td>
                  <td>{num(n.rows)}</td>
                  <td>
                    {num(n.actual_rows)} <Misestimate node={n} />
                  </td>
                  <td>{num(n.loops)}</td>
                  <td>{num(n.actual_ms)}</td>
                  {heated && (
                    <td className="explain-heat" style={heat(shareOf(n))}>
                      {percent.format(shareOf(n))}
                    </td>
                  )}
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}

/** The totals of the plan, always in view: the cost, the time and the rows of its root. */
function Summary({ node }: { node: PlanNode }) {
  const rows = node.actual_rows ?? node.rows;
  const figures: [string, string][] = [
    ...(node.cost !== null ? [[t("Cost"), num(node.cost)] as [string, string]] : []),
    ...(node.actual_ms !== null ? [[t("Time (ms)"), num(node.actual_ms)] as [string, string]] : []),
    ...(rows !== null ? [[t(node.actual_rows !== null ? "Actual rows" : "Estimated rows"), num(rows)] as [string, string]] : []),
  ];
  if (figures.length === 0) return null;
  return (
    <div className="explain-summary">
      {figures.map(([label, value]) => (
        <div key={label} className="explain-metric">
          <span className="explain-metric-label">{label}</span>
          <span className="explain-metric-value">{value}</span>
        </div>
      ))}
    </div>
  );
}

function Figures({ node }: { node: PlanNode }) {
  return (
    <span className="explain-figures">
      {node.rows !== null && <span title={t("Rows the planner expects")}>{t("est {n}", { n: num(node.rows) })}</span>}
      {node.actual_rows !== null && (
        <span title={t("Rows per loop")}>
          {t("actual {n}", { n: num(node.actual_rows) })}
          {node.loops !== null && node.loops !== 1 ? ` × ${num(node.loops)}` : ""}
        </span>
      )}
      {node.actual_ms !== null && <span title={t("Time to the last row, per loop")}>{num(node.actual_ms)} ms</span>}
    </span>
  );
}

function Properties({ node }: { node: PlanNode }) {
  const rows: [string, string][] = [
    [t("Operation"), node.operation],
    ...(node.detail ? [[t("On"), node.detail] as [string, string]] : []),
    ...([
      [t("Cost"), node.cost],
      [t("Estimated rows"), node.rows],
      [t("Actual rows"), node.actual_rows],
      [t("Loops"), node.loops],
      [t("Time (ms)"), node.actual_ms],
      [t("Own cost"), node.self_cost],
      [t("Own time, all loops (ms)"), node.self_ms],
    ] as const)
      .filter(([, v]) => v !== null)
      .map(([k, v]) => [k, num(v)] as [string, string]),
    ...node.properties,
  ];
  return (
    <table className="meta-table explain-properties">
      <tbody>
        {rows.map(([k, v]) => (
          <tr key={k}>
            <th>{k}</th>
            <td>{v}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
