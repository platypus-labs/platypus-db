/** The chart of a result as SVG, drawn by hand: bars, lines, scatter and pie, an axis of round
 *  values, a legend for several series and a tooltip on every mark. The colours come from the
 *  theme as values, so that an exported file keeps them. */
import { type Ref, useEffect, useState } from "react";
import type { ChartData } from "../api";
import { t } from "../i18n";
import { THEME_EVENT } from "../theme";
import type { ChartKind } from "./chartConfig";
import {
  arcPath,
  band,
  categoryLabel,
  categoryTicks,
  extent,
  formatNumber,
  formatTime,
  linear,
  niceDomain,
  niceTicks,
  groupSmallSlices,
  pieAngles,
  pieParts,
  rightAxisSeries,
  timeTicks,
  truncate,
  valueAxisTitle,
} from "./chartScale";

export interface ChartColors {
  series: string[];
  text: string;
  muted: string;
  grid: string;
  background: string;
  font: string;
}

/** The series take the theme's accent and syntax colours, in this order. */
const SERIES = ["--accent", "--syn-string", "--syn-number", "--syn-keyword", "--syn-type", "--obj-view", "--obj-routine", "--warn"];

function readColors(): ChartColors {
  const style = getComputedStyle(document.documentElement);
  const v = (name: string) => style.getPropertyValue(name).trim();
  return { series: SERIES.map(v), text: v("--fg"), muted: v("--fg-muted"), grid: v("--border"), background: v("--bg"), font: v("--font-ui") };
}

/** The chart's colours, read again whenever the theme changes. */
export function useChartColors(): ChartColors {
  const [colors, setColors] = useState(readColors);
  useEffect(() => {
    const update = () => setColors(readColors());
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    window.addEventListener(THEME_EVENT, update);
    media.addEventListener("change", update);
    return () => {
      window.removeEventListener(THEME_EVENT, update);
      media.removeEventListener("change", update);
    };
  }, []);
  return colors;
}

/** Room around the plot: the title and legend above, the axes' labels and titles around. */
const MARGIN = { top: 52, right: 20, bottom: 48, left: 64 };
/** The right margin when a second value axis is drawn there. */
const RIGHT_AXIS = 64;

interface Plot {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

interface ShapeProps {
  data: ChartData;
  categories: string[];
  plot: Plot;
  color: (i: number) => string;
  colors: ChartColors;
  /** Numbers as this language writes them. */
  locale: string;
  /** The series drawn on the right-hand axis, if any. */
  right: number | null;
}

interface ChartProps {
  data: ChartData;
  kind: ChartKind;
  width: number;
  height: number;
  colors: ChartColors;
  /** The word for the category that gathers the others. */
  otherLabel: string;
  /** The chart's accessible name. */
  label: string;
  /** The category (X) column's name, for the title and the X axis. */
  xName: string;
  /** Numbers as this language writes them: the interface's. */
  locale: string;
  ref?: Ref<SVGSVGElement>;
}

export function Chart({ data: full, kind, width, height, colors, otherLabel, label, xName, locale, ref }: ChartProps) {
  const labels = full.categories.map(categoryLabel);
  let categories = full.other ? [...labels.slice(0, -1), otherLabel] : labels;
  let data = full;
  if (kind === "pie" && full.series[0]) {
    // Many slices make an unreadable legend: the smallest ones go into one.
    const grouped = groupSmallSlices(full.series[0].values, categories, otherLabel, !!full.other);
    categories = grouped.categories;
    data = { ...full, series: [{ ...full.series[0], values: grouped.values }, ...full.series.slice(1)] };
  }
  const max = (s: ChartData["series"][number]) =>
    Math.max(0, ...(kind === "bar" ? s.values.filter((v): v is number => v !== null) : s.points.map((p) => p[1])).map(Math.abs));
  const right = kind === "pie" ? null : rightAxisSeries(data.series.map(max));
  const plot: Plot = {
    left: MARGIN.left,
    right: Math.max(MARGIN.left + 10, width - (right === null ? MARGIN.right : RIGHT_AXIS)),
    top: MARGIN.top,
    bottom: Math.max(MARGIN.top + 10, height - MARGIN.bottom),
  };
  const color = (i: number) => colors.series[i % colors.series.length] || colors.text;
  // A pie's legend names the slices drawn, in their colours.
  const slices = pieParts(data.series[0]?.values ?? []).drawn;
  const names = data.series.map((s) => s.name);
  const legend =
    kind === "pie"
      ? slices.map((i) => categories[i] ?? "")
      : names.length > 1
        ? names.map((n, i) => (i === right ? t("{name} (right axis)", { name: n }) : n))
        : [];
  const shapes = { data, categories, plot, color, colors, locale, right };
  const left = right === null ? names : names.filter((_, i) => i !== right);
  const title = t("{y} by {x}", { y: names.join(", "), x: xName });
  const axisText = { fill: colors.muted, textAnchor: "middle" as const };
  return (
    <svg
      ref={ref}
      className="chart-svg"
      xmlns="http://www.w3.org/2000/svg"
      width={width}
      height={height}
      viewBox={`0 0 ${width} ${height}`}
      fontFamily={colors.font}
      fontSize={11}
      role="img"
      aria-label={label}
    >
      <rect width={width} height={height} fill={colors.background} />
      <text className="chart-title" x={width / 2} y={16} textAnchor="middle" fontWeight={600} fontSize={13} fill={colors.text}>
        {truncate(title, Math.max(10, Math.floor(width / 7)))}
      </text>
      <Legend labels={legend} color={color} colors={colors} width={width} />
      {kind !== "pie" && (
        <g className="chart-axis-titles">
          <text className="chart-x-title" x={(plot.left + plot.right) / 2} y={height - 6} {...axisText}>
            {xName}
          </text>
          <text className="chart-y-title" transform={`translate(12 ${(plot.top + plot.bottom) / 2}) rotate(-90)`} {...axisText}>
            {valueAxisTitle(left, t("Value"))}
          </text>
          {right !== null && (
            <text className="chart-y2-title" transform={`translate(${width - 12} ${(plot.top + plot.bottom) / 2}) rotate(-90)`} {...axisText}>
              {names[right]}
            </text>
          )}
        </g>
      )}
      {kind === "pie" ? <Pie {...shapes} /> : kind === "bar" ? <Bars {...shapes} /> : <Points {...shapes} kind={kind} />}
    </svg>
  );
}

function Legend({ labels, color, colors, width }: { labels: string[]; color: (i: number) => string; colors: ChartColors; width: number }) {
  let x = MARGIN.left;
  const items = labels.flatMap((label, i) => {
    const text = truncate(label, 24);
    const w = 16 + text.length * 6.5;
    const at = x;
    x += w + 14;
    if (at + w > width) return [];
    return [
      <g key={i} transform={`translate(${at}, 36)`}>
        <rect y={-8} width={10} height={10} rx={2} fill={color(i)} />
        <text x={14} y={1} fill={colors.text}>
          {text}
        </text>
      </g>,
    ];
  });
  return <g className="chart-legend">{items}</g>;
}

function YAxis({ ticks, y, plot, colors, locale, side = "left" }: { ticks: number[]; y: (v: number) => number; plot: Plot; colors: ChartColors; locale: string; side?: "left" | "right" }) {
  return (
    <g className={side === "left" ? "chart-axis" : "chart-axis chart-axis-right"}>
      {ticks.map((v) => (
        <g key={v}>
          {side === "left" && <line x1={plot.left} x2={plot.right} y1={y(v)} y2={y(v)} stroke={colors.grid} />}
          <text x={side === "left" ? plot.left - 6 : plot.right + 6} y={y(v) + 4} textAnchor={side === "left" ? "end" : "start"} fill={colors.muted}>
            {formatNumber(v, locale)}
          </text>
        </g>
      ))}
    </g>
  );
}

/** The nice domain of the numbers in `values`, zero included. */
const domainOf = (values: (number | null)[]) => {
  const v = values.filter((n): n is number => n !== null);
  return niceDomain(Math.min(0, ...v), Math.max(0, ...v), 5);
};

function Bars({ data, categories, plot, color, colors, locale, right }: ShapeProps) {
  const sides = (r: boolean) => data.series.filter((_, k) => (k === right) === r).flatMap((s) => s.values);
  const [lo, hi] = domainOf(sides(false));
  const y = linear([lo, hi], [plot.bottom, plot.top]);
  const [lo2, hi2] = domainOf(sides(true));
  const y2 = linear([lo2, hi2], [plot.bottom, plot.top]);
  const outer = band(categories.length, plot.left, plot.right, 0.2);
  const inner = band(Math.max(1, data.series.length), 0, outer.width, 0.05);
  const labelled = categoryTicks(categories.length, Math.max(1, Math.floor((plot.right - plot.left) / 70)), data.other);
  return (
    <g>
      <YAxis ticks={niceTicks(lo, hi, 5)} y={y} plot={plot} colors={colors} locale={locale} />
      {right !== null && <YAxis ticks={niceTicks(lo2, hi2, 5)} y={y2} plot={plot} colors={colors} locale={locale} side="right" />}
      {categories.map((label, i) =>
        data.series.map((s, k) => {
          const yk = k === right ? y2 : y;
          // No number to aggregate: no bar, rather than one of 0.
          const v = s.values[i] ?? null;
          if (v === null) return null;
          return (
            <rect
              key={`${i}-${k}`}
              className="chart-bar"
              x={outer.at(i) + inner.at(k)}
              y={Math.min(yk(v), yk(0))}
              width={Math.max(1, inner.width)}
              height={Math.abs(yk(v) - yk(0))}
              fill={color(k)}
            >
              <title>{`${label} · ${s.name}: ${formatNumber(v, locale)}`}</title>
            </rect>
          );
        }),
      )}
      {labelled.map((i) => (
        <text key={i} x={outer.at(i) + outer.width / 2} y={plot.bottom + 16} textAnchor="middle" fill={colors.muted}>
          {truncate(categories[i] ?? "", 14)}
        </text>
      ))}
    </g>
  );
}

function Points({ data, kind, categories, plot, color, colors, locale, right }: ShapeProps & { kind: ChartKind }) {
  const all = data.series.flatMap((s) => s.points);
  const [x0, x1] = extent(all.map((p) => p[0]));
  const domain = (r: boolean) =>
    niceDomain(...extent(data.series.filter((_, k) => (k === right) === r).flatMap((s) => s.points.map((p) => p[1]))), 5);
  const [lo, hi] = domain(false);
  const [lo2, hi2] = domain(true);
  const x = linear([x0, x1], [plot.left, plot.right]);
  const y1 = linear([lo, hi], [plot.bottom, plot.top]);
  const y2 = linear([lo2, hi2], [plot.bottom, plot.top]);
  const room = Math.max(2, Math.floor((plot.right - plot.left) / 90));
  const ticks =
    data.x_axis === "time" ? timeTicks(x0, x1, room) : data.x_axis === "number" ? niceTicks(x0, x1, room) : categoryTicks(categories.length, room);
  const label = (v: number) =>
    data.x_axis === "time" ? formatTime(v, x1 - x0) : data.x_axis === "number" ? formatNumber(v, locale) : truncate(categories[Math.round(v)] ?? "", 14);
  return (
    <g>
      <YAxis ticks={niceTicks(lo, hi, 5)} y={y1} plot={plot} colors={colors} locale={locale} />
      {right !== null && <YAxis ticks={niceTicks(lo2, hi2, 5)} y={y2} plot={plot} colors={colors} locale={locale} side="right" />}
      {ticks.map((v) => (
        <text key={v} x={x(v)} y={plot.bottom + 16} textAnchor="middle" fill={colors.muted}>
          {label(v)}
        </text>
      ))}
      {data.series.map((s, k) => {
        const y = k === right ? y2 : y1;
        return (
        <g key={k}>
          {kind === "line" && (
            <polyline
              className="chart-line"
              fill="none"
              stroke={color(k)}
              strokeWidth={1.5}
              points={s.points.map(([a, b]) => `${x(a)},${y(b)}`).join(" ")}
            />
          )}
          {(kind === "scatter" || s.points.length <= 200) &&
            s.points.map(([a, b], i) => (
              <circle
                key={i}
                className="chart-point"
                cx={x(a)}
                cy={y(b)}
                r={kind === "scatter" ? 3 : 2.5}
                fill={color(k)}
                fillOpacity={kind === "scatter" ? 0.75 : 1}
              >
                <title>{`${s.name} · ${label(a)}: ${formatNumber(b, locale)}`}</title>
              </circle>
            ))}
        </g>
        );
      })}
    </g>
  );
}

/** The slices of the first series' positive values; the others are left out (the chart's note
 *  says which), so the colours follow the slices drawn, as the legend does. */
function Pie({ data, categories, plot, color, colors, locale }: ShapeProps) {
  const all = data.series[0]?.values ?? [];
  const drawn = pieParts(all).drawn;
  const values = drawn.map((i) => all[i] ?? 0);
  const r = Math.max(10, Math.min(plot.right - plot.left, plot.bottom - plot.top) / 2);
  const cx = (plot.left + plot.right) / 2;
  const cy = (plot.top + plot.bottom) / 2;
  return (
    <g>
      {pieAngles(values).map(([a0, a1], k) => (
        <path key={k} className="chart-slice" d={arcPath(cx, cy, r, a0, a1)} fill={color(k)} stroke={colors.background}>
          <title>{`${categories[drawn[k] ?? 0] ?? ""}: ${formatNumber(values[k] ?? 0, locale)}`}</title>
        </path>
      ))}
    </g>
  );
}
