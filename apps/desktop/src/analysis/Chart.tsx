/** The chart of a result as SVG, drawn by hand: bars, lines, scatter and pie, an axis of round
 *  values, a legend for several series and a tooltip on every mark. The colours come from the
 *  theme as values, so that an exported file keeps them. */
import { type Ref, useEffect, useState } from "react";
import type { ChartData } from "../api";
import { THEME_EVENT } from "../theme";
import type { ChartKind } from "./chartConfig";
import {
  arcPath,
  band,
  categoryTicks,
  extent,
  formatNumber,
  formatTime,
  linear,
  niceDomain,
  niceTicks,
  pieAngles,
  timeTicks,
  truncate,
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

/** Room around the plot: the legend above, the axes' labels left and below. */
const MARGIN = { top: 30, right: 20, bottom: 40, left: 64 };

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
}

interface ChartProps {
  data: ChartData;
  kind: ChartKind;
  width: number;
  height: number;
  colors: ChartColors;
  /** The word for the category that gathers the others. */
  otherLabel: string;
  ref?: Ref<SVGSVGElement>;
}

export function Chart({ data, kind, width, height, colors, otherLabel, ref }: ChartProps) {
  const categories = data.other ? [...data.categories.slice(0, -1), otherLabel] : data.categories;
  const plot: Plot = {
    left: MARGIN.left,
    right: Math.max(MARGIN.left + 10, width - MARGIN.right),
    top: MARGIN.top,
    bottom: Math.max(MARGIN.top + 10, height - MARGIN.bottom),
  };
  const color = (i: number) => colors.series[i % colors.series.length] || colors.text;
  const legend = kind === "pie" ? categories : data.series.length > 1 ? data.series.map((s) => s.name) : [];
  const shapes = { data, categories, plot, color, colors };
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
    >
      <rect width={width} height={height} fill={colors.background} />
      <Legend labels={legend} color={color} colors={colors} width={width} />
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
      <g key={i} transform={`translate(${at}, 14)`}>
        <rect y={-8} width={10} height={10} rx={2} fill={color(i)} />
        <text x={14} y={1} fill={colors.text}>
          {text}
        </text>
      </g>,
    ];
  });
  return <g className="chart-legend">{items}</g>;
}

function YAxis({ ticks, y, plot, colors }: { ticks: number[]; y: (v: number) => number; plot: Plot; colors: ChartColors }) {
  return (
    <g className="chart-axis">
      {ticks.map((v) => (
        <g key={v}>
          <line x1={plot.left} x2={plot.right} y1={y(v)} y2={y(v)} stroke={colors.grid} />
          <text x={plot.left - 6} y={y(v) + 4} textAnchor="end" fill={colors.muted}>
            {formatNumber(v)}
          </text>
        </g>
      ))}
    </g>
  );
}

function Bars({ data, categories, plot, color, colors }: ShapeProps) {
  const values = data.series.flatMap((s) => s.values);
  const [lo, hi] = niceDomain(Math.min(0, ...values), Math.max(0, ...values), 5);
  const y = linear([lo, hi], [plot.bottom, plot.top]);
  const outer = band(categories.length, plot.left, plot.right, 0.2);
  const inner = band(Math.max(1, data.series.length), 0, outer.width, 0.05);
  const labelled = categoryTicks(categories.length, Math.max(1, Math.floor((plot.right - plot.left) / 70)));
  return (
    <g>
      <YAxis ticks={niceTicks(lo, hi, 5)} y={y} plot={plot} colors={colors} />
      {categories.map((label, i) =>
        data.series.map((s, k) => {
          const v = s.values[i] ?? 0;
          return (
            <rect
              key={`${i}-${k}`}
              className="chart-bar"
              x={outer.at(i) + inner.at(k)}
              y={Math.min(y(v), y(0))}
              width={Math.max(1, inner.width)}
              height={Math.abs(y(v) - y(0))}
              fill={color(k)}
            >
              <title>{`${label} · ${s.name}: ${formatNumber(v)}`}</title>
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

function Points({ data, kind, categories, plot, color, colors }: ShapeProps & { kind: ChartKind }) {
  const all = data.series.flatMap((s) => s.points);
  const [x0, x1] = extent(all.map((p) => p[0]));
  const [ylo, yhi] = extent(all.map((p) => p[1]));
  const [lo, hi] = niceDomain(ylo, yhi, 5);
  const x = linear([x0, x1], [plot.left, plot.right]);
  const y = linear([lo, hi], [plot.bottom, plot.top]);
  const room = Math.max(2, Math.floor((plot.right - plot.left) / 90));
  const ticks =
    data.x_axis === "time" ? timeTicks(x0, x1, room) : data.x_axis === "number" ? niceTicks(x0, x1, room) : categoryTicks(categories.length, room);
  const label = (v: number) =>
    data.x_axis === "time" ? formatTime(v, x1 - x0) : data.x_axis === "number" ? formatNumber(v) : truncate(categories[Math.round(v)] ?? "", 14);
  return (
    <g>
      <YAxis ticks={niceTicks(lo, hi, 5)} y={y} plot={plot} colors={colors} />
      {ticks.map((v) => (
        <text key={v} x={x(v)} y={plot.bottom + 16} textAnchor="middle" fill={colors.muted}>
          {label(v)}
        </text>
      ))}
      {data.series.map((s, k) => (
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
                <title>{`${s.name} · ${label(a)}: ${formatNumber(b)}`}</title>
              </circle>
            ))}
        </g>
      ))}
    </g>
  );
}

function Pie({ data, categories, plot, color, colors }: ShapeProps) {
  const values = data.series[0]?.values ?? [];
  const r = Math.max(10, Math.min(plot.right - plot.left, plot.bottom - plot.top) / 2);
  const cx = (plot.left + plot.right) / 2;
  const cy = (plot.top + plot.bottom) / 2;
  return (
    <g>
      {pieAngles(values).map(([a0, a1], i) =>
        a1 > a0 ? (
          <path key={i} className="chart-slice" d={arcPath(cx, cy, r, a0, a1)} fill={color(i)} stroke={colors.background}>
            <title>{`${categories[i] ?? ""}: ${formatNumber(values[i] ?? 0)}`}</title>
          </path>
        ) : null,
      )}
    </g>
  );
}
