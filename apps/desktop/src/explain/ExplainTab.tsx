import { Icon } from "../ui/Icon";
import { type CSSProperties, useEffect, useState } from "react";
import { api, type Explain, type PlanNode, toCommandError } from "../api";
import { fmt } from "../components/ResultPanel";
import type { TabBase, TabProps } from "../tabs/registry";

export type ExplainView = "tree" | "text" | "table";

/** The plan of one statement, as a tree, as text or as a table. */
export interface ExplainTabModel extends TabBase {
  kind: "explain";
  sql: string;
  /** The statement runs (its changes rolled back) for actual rows and times. */
  analyze: boolean;
  view?: ExplainView;
}

declare module "../tabs/registry" {
  interface TabKinds {
    explain: ExplainTabModel;
  }
}

const VIEWS: [ExplainView, string][] = [
  ["tree", "Tree"],
  ["text", "Text"],
  ["table", "Table"],
];

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

const num = (v: number | null) => (v === null ? "" : fmt.format(v));
const percent = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 1 });

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
  const times = fmt.format(Math.round(Math.abs(factor)));
  return (
    <span
      className="explain-misestimate"
      title={`Estimated ${num(node.rows)} rows, got ${num(node.actual_rows)}: the planner may have chosen badly`}
    >
      {times}× {factor > 0 ? "more" : "fewer"} rows
    </span>
  );
}

/** The step tinted by its share of the work. */
const heat = (share: number) => ({ "--heat": `${Math.round(share * 100)}%` }) as CSSProperties;

function title(node: PlanNode) {
  return node.detail ? `${node.operation} ${node.detail}` : node.operation;
}

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
      return `${"   ".repeat(depth)}-> ${title(node)}${group(estimate)}${group(actual, "actual ")}`;
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
  const view = tab.view ?? "tree";

  useEffect(() => {
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
        if (!cancelled) setExplain(plan);
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
  const own = explain?.analyzed ? "time" : "cost";
  const shareOf = (n: PlanNode) => share.get(n) ?? 0;
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
          <strong>{tab.analyze ? "Explain analyze" : "Explain"}</strong>
          <code className="explain-sql muted small" title={tab.sql}>
            {tab.sql}
          </code>
        </div>
        <span className="spacer" />
        {running && <span className="muted small">Running…</span>}
        {explain && (
          <button
            className="square"
            onClick={() => void navigator.clipboard.writeText(rawPlan(explain))}
            title="Copy the plan, as the database returned it"
            aria-label="Copy plan"
          >
            <Icon name="copy" />
          </button>
        )}
        <button
          disabled={running}
          onClick={() => setVersion((v) => v + 1)}
          title={tab.analyze ? "Refresh: runs the statement again" : "Refresh: asks for the plan again"}
          aria-label="Refresh"
          className="square"
        >
          <Icon name="refresh" />
        </button>
      </header>
      <nav className="object-sections">
        {VIEWS.map(([v, label]) => (
          <button key={v} className={v === view ? "active" : ""} onClick={() => onChange({ view: v })}>
            {label}
          </button>
        ))}
      </nav>
      <div className="object-body">
        {error && <div className="notice error">{error}</div>}
        {tab.analyze && explain && !explain.analyzed && (
          <div className="notice small">This database cannot run a statement for its plan: estimates only.</div>
        )}
        {explain?.suggestions.map((s, i) => (
          <div key={i} className="notice warn small explain-suggestion">
            <strong>{s.table}</strong> is read in full
            {s.rows_read === null ? "" : ` (${explain.analyzed ? "" : "about "}${num(s.rows_read)} rows)`} to keep the rows
            where <code>{s.filter}</code>: an index on the columns of that condition may help.
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
                    aria-label={collapsed.has(path) ? "Expand" : "Collapse"}
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
                    <span className="explain-share" title={`Share of the ${own} spent in this step itself`}>
                      {percent.format(shareOf(n))}
                    </span>
                  )}
                </div>
              ))}
            </div>
            {node && <Properties node={node} />}
          </div>
        )}
        {explain && view === "text" && <pre className="explain-text">{planText(all)}</pre>}
        {explain && view === "table" && (
          <table className="meta-table explain-table">
            <thead>
              <tr>
                <th>Operation</th>
                <th>Cost</th>
                <th>Rows</th>
                <th>Actual rows</th>
                <th>Loops</th>
                <th>Time (ms)</th>
                {heated && <th>Own {own}</th>}
              </tr>
            </thead>
            <tbody>
              {all.map(({ node: n, depth, path }) => (
                <tr key={path}>
                  <td style={{ paddingLeft: depth * 16 }}>{title(n)}</td>
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

function Figures({ node }: { node: PlanNode }) {
  return (
    <span className="explain-figures">
      {node.rows !== null && <span title="Rows the planner expects">est {num(node.rows)}</span>}
      {node.actual_rows !== null && (
        <span title="Rows per loop">
          actual {num(node.actual_rows)}
          {node.loops !== null && node.loops !== 1 ? ` × ${num(node.loops)}` : ""}
        </span>
      )}
      {node.actual_ms !== null && <span title="Time to the last row, per loop">{num(node.actual_ms)} ms</span>}
    </span>
  );
}

function Properties({ node }: { node: PlanNode }) {
  const rows: [string, string][] = [
    ["Operation", node.operation],
    ...(node.detail ? [["On", node.detail] as [string, string]] : []),
    ...([
      ["Cost", node.cost],
      ["Estimated rows", node.rows],
      ["Actual rows", node.actual_rows],
      ["Loops", node.loops],
      ["Time (ms)", node.actual_ms],
      ["Own cost", node.self_cost],
      ["Own time, all loops (ms)", node.self_ms],
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
