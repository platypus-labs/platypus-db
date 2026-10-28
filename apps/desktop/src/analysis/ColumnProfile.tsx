/** Column statistics: after the mouse rests on a header, a box under it with the column's
 *  NULLs, distinct values, extremes and a small histogram or the most frequent values. */
import { Fragment, type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { api, type ColumnInfo, type ColumnProfile } from "../api";
import type { HeaderHover } from "../grid/DataGrid";
import { locale, t } from "../i18n";

/** How long the mouse rests on a header before the box shows. */
const DELAY_MS = 500;
/** The box's width, to keep it inside the grid. */
const WIDTH = 300;
/** Counts and shares as the interface language writes them, not the system. */
const count = (n: number) => n.toLocaleString(locale());
const percent = (n: number) => n.toLocaleString(locale(), { style: "percent", maximumFractionDigits: 1 });

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
            <dd>{profile.min}</dd>
            <dt>{t("Maximum")}</dt>
            <dd>{profile.max}</dd>
          </>
        )}
      </dl>
      {profile.histogram.length > 0 && <Histogram bins={profile.histogram} />}
      {profile.top.length > 0 && (
        <>
          <div className="column-profile-label muted">{t("Most frequent")}</div>
          <div className="column-profile-top">
            {profile.top.map((v) => (
              <Fragment key={v.value}>
                <span>{v.value}</span>
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

/** The ranges of a number or date column as bars, with the first and last bound under them. */
function Histogram({ bins }: { bins: ColumnProfile["histogram"] }) {
  const max = Math.max(1, ...bins.map((b) => b.count));
  const step = 200 / bins.length;
  return (
    <>
      <svg className="column-profile-bars" viewBox="0 0 200 36" preserveAspectRatio="none" aria-hidden="true">
        {bins.map((b, i) => {
          const h = (b.count / max) * 36;
          return <rect key={i} x={i * step + 0.5} y={36 - h} width={Math.max(1, step - 1)} height={h} />;
        })}
      </svg>
      <div className="column-profile-range muted">
        <span>{bins[0]?.from}</span>
        <span>{bins.at(-1)?.to}</span>
      </div>
    </>
  );
}
