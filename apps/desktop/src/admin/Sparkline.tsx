/** A line of the last readings of a figure, small enough for a tile of the dashboard. It takes
 *  the colours of the charts of the analysis (`useChartColors`) and draws a break where a
 *  reading is missing. */
import { extent, linear } from "../analysis/chartScale";
import { useChartColors } from "../analysis/Chart";

const WIDTH = 140;
const HEIGHT = 32;
const PAD = 3;

/** The runs of consecutive readings of `values`, as lists of `[index, value]`. */
export function runs(values: (number | null)[]): [number, number][][] {
  const out: [number, number][][] = [];
  let run: [number, number][] = [];
  values.forEach((v, i) => {
    if (v === null || !Number.isFinite(v)) {
      if (run.length) out.push(run);
      run = [];
    } else {
      run.push([i, v]);
    }
  });
  if (run.length) out.push(run);
  return out;
}

export function Sparkline({
  values,
  domain,
  label,
}: {
  values: (number | null)[];
  /** The scale, when it is fixed (a ratio goes from 0 to 1); else from 0 to the largest. */
  domain?: [number, number];
  label: string;
}) {
  const colors = useChartColors();
  const drawn = runs(values);
  const known = drawn.flat().map(([, v]) => v);
  const [lo, hi] = domain ?? [0, Math.max(extent(known)[1], 1e-9)];
  const x = linear([0, Math.max(values.length - 1, 1)], [PAD, WIDTH - PAD]);
  const y = linear([lo, hi], [HEIGHT - PAD, PAD]);
  const line = colors.series[0] || colors.text;
  // The latest reading is the dot.
  const last = drawn.flat().at(-1);
  return (
    <svg className="dash-spark" width={WIDTH} height={HEIGHT} viewBox={`0 0 ${WIDTH} ${HEIGHT}`} role="img" aria-label={label}>
      <line x1={PAD} x2={WIDTH - PAD} y1={HEIGHT - PAD} y2={HEIGHT - PAD} stroke={colors.grid} />
      {drawn.map((run) => (
        <polyline
          key={run[0]?.[0]}
          points={run.map(([i, v]) => `${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ")}
          fill="none"
          stroke={line}
          strokeWidth={1.5}
          strokeLinejoin="round"
          strokeLinecap="round"
        />
      ))}
      {last && <circle cx={x(last[0])} cy={y(last[1])} r={2.5} fill={line} />}
    </svg>
  );
}
