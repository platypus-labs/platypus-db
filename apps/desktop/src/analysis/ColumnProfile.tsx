/** Column statistics: after the mouse rests on a header, a box under it with the column's
 *  NULLs, distinct values, extremes and a small histogram or the most frequent values. */
import { Fragment, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api, type ColumnInfo, type ColumnProfile } from "../api";
import { formatShare } from "../format";
import { barShare, yMax } from "./histogramAxis";
import type { HeaderHover } from "../grid/DataGrid";
import { shown as masked, useMask } from "../guards/mask";
import { locale, t } from "../i18n";

/** How long the mouse rests on a header before the box shows. */
const DELAY_MS = 500;
/** The box's width, to keep it inside the grid. */
const WIDTH = 300;
/** Counts and shares as the interface language writes them, not the system. */
export const count = (n: number) => n.toLocaleString(locale());
export const percent = (n: number) => formatShare(n, locale());

/** The profile of the column whose header the mouse rests on, cached by result, column and
 *  rows loaded: more rows compute it again. */
export function useColumnProfile(
  jobId: number,
  columns: ColumnInfo[],
  rowCount: number,
): { onHeaderHover: (hover: HeaderHover | null) => void; popover: ReactNode } {
  const cache = useRef(new Map<string, ColumnProfile>());
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [hover, setHover] = useState<HeaderHover | null>(null);
  const [shown, setShown] = useState<{ key: string; profile: ColumnProfile } | null>(null);
  useEffect(() => () => clearTimeout(timer.current), []);
  // Another result: nothing cached applies.
  useEffect(() => cache.current.clear(), [jobId]);

  const onHeaderHover = useCallback((h: HeaderHover | null) => {
    clearTimeout(timer.current);
    setHover(null);
    if (h) timer.current = setTimeout(() => setHover(h), DELAY_MS);
  }, []);

  const key = hover ? `${jobId}:${hover.col}:${rowCount}` : null;
  useEffect(() => {
    if (!hover || key === null) return;
    const cached = cache.current.get(key);
    if (cached) return setShown({ key, profile: cached });
    let live = true;
    api.columnProfile(jobId, hover.col).then(
      (profile) => {
        cache.current.set(key, profile);
        if (live) setShown({ key, profile });
      },
      // A result closed meanwhile shows nothing.
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, [key]);

  const column = hover ? columns[hover.col] : undefined;
  const popover =
    hover && column && shown?.key === key ? <ProfileBox hover={hover} column={column} profile={shown.profile} /> : null;
  return { onHeaderHover, popover };
}

function ProfileBox({ hover, column, profile }: { hover: HeaderHover; column: ColumnInfo; profile: ColumnProfile }) {
  const left = Math.max(0, Math.min(hover.left, hover.gridWidth - WIDTH - 8));
  const nulls = profile.rows > 0 ? profile.nulls / profile.rows : 0;
  // A masked column's values show as dots here too; its counts tell nothing of them.
  const hide = !!useMask()?.(column);
  return (
    <div className="column-profile" role="tooltip" style={{ left, top: hover.top + 4, width: WIDTH }}>
      <div className="column-profile-head">
        <strong>{column.name}</strong>
        <span className="muted">{column.db_type}</span>
      </div>
      <dl>
        <dt>{t("NULL values")}</dt>
        <dd>{percent(nulls)}</dd>
        <dt>{t("Distinct")}</dt>
        <dd>{count(profile.distinct)}</dd>
        {profile.min !== null && (
          <>
            <dt>{t("Minimum")}</dt>
            <dd>{masked(profile.min, hide)}</dd>
            <dt>{t("Maximum")}</dt>
            <dd>{masked(profile.max, hide)}</dd>
          </>
        )}
      </dl>
      {profile.histogram.length > 0 && <Histogram bins={profile.histogram} hide={hide} />}
      {profile.top.length > 0 && (
        <>
          <div className="column-profile-label muted">{t("Most frequent")}</div>
          <div className="column-profile-top">
            {profile.top.map((v) => (
              <Fragment key={v.value}>
                <span>{masked(v.value, hide)}</span>
                <span>{count(v.count)}</span>
              </Fragment>
            ))}
          </div>
        </>
      )}
      <div className="column-profile-scope muted">
        {profile.complete
          ? t(profile.rows === 1 ? "of all {n} row" : "of all {n} rows", { n: count(profile.rows) })
          : t(profile.rows === 1 ? "of {n} loaded row" : "of {n} loaded rows", { n: count(profile.rows) })}
      </div>
    </div>
  );
}

/** The ranges of a number or date column as bars, titled and explained, with the row-count scale
 *  beside them and the first and last bound under them. Each bar's tooltip has its range and count. */
export function Histogram({ bins, hide }: { bins: ColumnProfile["histogram"]; hide: boolean }) {
  const max = yMax(bins);
  const step = 200 / bins.length;
  const first = masked(bins[0]?.from, hide);
  const last = masked(bins.at(-1)?.to, hide);
  return (
    <div className="column-profile-hist">
      <div className="column-profile-hist-title">
        <strong>{t("Distribution")}</strong>
        <span className="muted">{t("How many rows fall in each range of values")}</span>
      </div>
      <div className="column-profile-hist-plot">
        <div className="column-profile-axis muted" title={t("Row count")}>
          <span>{count(max)}</span>
          <span>0</span>
        </div>
        <svg className="column-profile-bars" viewBox="0 0 200 36" preserveAspectRatio="none" role="img" aria-label={t("Distribution")}>
          {bins.map((b, i) => {
            const h = barShare(b, max) * 36;
            const label = t(b.count === 1 ? "{from} to {to}: {n} row" : "{from} to {to}: {n} rows", {
              from: masked(b.from, hide),
              to: masked(b.to, hide),
              n: count(b.count),
            });
            return (
              <rect key={i} x={i * step + 0.5} y={36 - h} width={Math.max(1, step - 1)} height={h}>
                <title>{label}</title>
              </rect>
            );
          })}
        </svg>
      </div>
      <div className="column-profile-range muted">
        <span title={first}>{first}</span>
        <span title={last}>{last}</span>
      </div>
    </div>
  );
}
