import { type CSSProperties, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { PlanNode } from "../api";
import { t } from "../i18n";
import { stepName } from "./compare";
import { BOX, layoutPlan } from "./graph";

const ZOOMS = [0.5, 0.75, 1, 1.25];

/** The smallest zoom "Fit" goes to: below it the text cannot be read. */
const MIN_FIT = 0.4;

/** `text` cut to `max` characters, with an ellipsis. */
const cut = (text: string, max: number) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);

/** The rows a step passes up: actual over all loops when it ran, else the estimate. */
const flow = (n: PlanNode) => (n.actual_rows !== null ? n.actual_rows * (n.loops ?? 1) : (n.rows ?? 0));

/** The plan as boxes joined by lines: each step tinted by its share of the work, as in the
 *  tree, and each line as thick as the rows it carries. A click selects a step. */
export function PlanGraph({
  root,
  selected,
  onSelect,
  shareOf,
  figures,
}: {
  root: PlanNode;
  selected: string;
  onSelect: (path: string) => void;
  shareOf: (n: PlanNode) => number;
  /** The step's figures on one line: rows, time or cost. */
  figures: (n: PlanNode) => string;
}) {
  /** `null` fits the graph's width in the view, down to `MIN_FIT`. */
  const [zoom, setZoom] = useState<number | null>(null);
  const [fit, setFit] = useState(1);
  const view = useRef<HTMLDivElement>(null);
  // Laid out once a plan: a click or a resize does not walk it again.
  const { layout, at, most } = useMemo(() => {
    const layout = layoutPlan(root);
    return {
      layout,
      at: new Map(layout.boxes.map((b) => [b.path, b])),
      most: layout.boxes.reduce((m, b) => Math.max(m, flow(b.node)), 1),
    };
  }, [root]);
  const pad = 12;
  const scale = zoom ?? fit;
  const top = at.get("0");

  // The fitting zoom follows the view's width.
  useLayoutEffect(() => {
    const el = view.current;
    if (!el) return;
    const measure = () => setFit(Math.max(MIN_FIT, Math.min(1, (el.clientWidth - 2) / (layout.width + pad * 2))));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [layout.width]);

  // The top step in view: a wide plan puts it far from the left edge.
  useLayoutEffect(() => {
    const el = view.current;
    if (!el || !top) return;
    el.scrollLeft = Math.max(0, (top.x + pad + BOX.width / 2) * scale - el.clientWidth / 2);
    el.scrollTop = 0;
  }, [root, scale]);

  return (
    <div className="explain-graph-wrap">
      <div className="explain-graph-zoom" role="group" aria-label={t("Zoom")}>
        <button className={zoom === null ? "active" : ""} onClick={() => setZoom(null)}>
          {t("Fit")}
        </button>
        {ZOOMS.map((z) => (
          <button key={z} className={z === zoom ? "active" : ""} onClick={() => setZoom(z)}>
            {Math.round(z * 100)}%
          </button>
        ))}
      </div>
      <div className="explain-graph" ref={view}>
        <svg
          width={(layout.width + pad * 2) * scale}
          height={(layout.height + pad * 2) * scale}
          viewBox={`${-pad} ${-pad} ${layout.width + pad * 2} ${layout.height + pad * 2}`}
          role="tree"
        >
          {layout.edges.map(([from, to]) => {
            const a = at.get(from)!;
            const b = at.get(to)!;
            const x1 = a.x + BOX.width / 2;
            const y1 = a.y + BOX.height;
            const x2 = b.x + BOX.width / 2;
            const y2 = b.y;
            const mid = (y1 + y2) / 2;
            const width = 1 + 4 * Math.sqrt(flow(b.node) / most);
            return (
              <path
                key={to}
                className="explain-edge"
                d={`M${x1},${y1} C${x1},${mid} ${x2},${mid} ${x2},${y2}`}
                style={{ strokeWidth: width }}
              />
            );
          })}
          {layout.boxes.map(({ node, path, x, y }) => {
            const share = shareOf(node);
            return (
              <g
                key={path}
                role="treeitem"
                aria-selected={path === selected}
                className={`explain-box${path === selected ? " selected" : ""}`}
                transform={`translate(${x},${y})`}
                style={{ "--heat": `${Math.round(share * 100)}%` } as CSSProperties}
                onClick={() => onSelect(path)}
              >
                <title>{stepName(node)}</title>
                <rect width={BOX.width} height={BOX.height} rx={6} />
                <text x={10} y={20} className="explain-box-op">
                  {cut(node.operation, 26)}
                </text>
                {share > 0 && (
                  <text x={BOX.width - 10} y={20} textAnchor="end" className="explain-box-share">
                    {Math.round(share * 100)}%
                  </text>
                )}
                <text x={10} y={39} className="explain-box-detail">
                  {cut(node.detail ?? "", 32)}
                </text>
                <text x={10} y={60} className="explain-box-figures">
                  {cut(figures(node), 34)}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </div>
  );
}
