/** "Profile data": one card per column of a table or view, from its rows read on the tab's
 *  side session: NULLs, distinct values, extremes, the spread of numbers and their outliers,
 *  the length of text and the shapes its values take, with a histogram or the most frequent
 *  values. A value or an outlier opens the table's data filtered to it. */
import { useEffect, useMemo, useState } from "react";
import { api, type DriverKind, type ObjectKind, type TableColumnProfile, type TableProfile, type ValuePattern, toCommandError } from "../api";
import { shown as masked, useMask } from "../guards/mask";
import { locale, msg, plural, t } from "../i18n";
import { quoteIdent } from "../sqlIdent";
import { qualifiedName, viewerSql } from "../sqlNames";
import type { TabBase, TabProps } from "../tabs/registry";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { Histogram, count, percent } from "./ColumnProfile";
import { equalsCondition, matchingColumns, outlierCondition } from "./tableProfileModel";
import { EmptyState } from "../ui/State";

export interface TableProfileTabModel extends TabBase {
  kind: "table-profile";
  schema: string;
  /** Whether `schema` is the session's current one: names are then left unqualified. */
  currentSchema: boolean;
  name: string;
  objectKind: ObjectKind;
  /** Rows read at most; `null` reads the whole table. */
  sample: number | null;
}

declare module "../tabs/registry" {
  interface TabKinds {
    "table-profile": TableProfileTabModel;
  }
}

/** What each pattern is called in the cards. */
const PATTERNS: Record<ValuePattern, string> = {
  email: msg("Email address"),
  url: msg("URL"),
  uuid: msg("UUID"),
  iso_date: msg("ISO date"),
  date_like: msg("Date (day/month/year style)"),
  integer: msg("Integer stored as text"),
  decimal: msg("Decimal number stored as text"),
  phone: msg("Phone number"),
  spaces: msg("Leading or trailing spaces"),
  mixed_case: msg("Same value in different cases"),
};

/** What each kind of column is called in its card. */
const KINDS: Record<TableColumnProfile["kind"], string> = {
  number: msg("number"),
  time: msg("date/time"),
  text: msg("text"),
  other: msg("other"),
};

/** The Sample select's options: the sizes of `SAMPLES`. */
const SAMPLE_OPTIONS: [number | null, string][] = [
  [10_000, msg("First 10,000 rows")],
  [100_000, msg("First 100,000 rows")],
  [1_000_000, msg("First 1,000,000 rows")],
  [null, msg("Whole table")],
];

/** Databases that do not speak SQL: no `SELECT DISTINCT *` to count identical rows with. */
const NOT_SQL = new Set<DriverKind>(["mongo", "redis", "elasticsearch", "cassandra"]);

/** The Duplicates card: the number of rows identical to an earlier one, or why it is not
 *  counted. */
interface Duplicates {
  value: string;
  /** Why there is no number; `null` when there is. */
  why: string | null;
}

/** Tables with more rows than this are not searched for identical rows. */
const COUNT_LIMIT = 1_000_000;

const unavailable = (why: string): Duplicates => ({ value: "—", why });

/** A figure of the spread of numbers, with the decimals that tell something. */
const figure = (n: number) => n.toLocaleString(locale(), { maximumFractionDigits: 4 });

export function TableProfileTab({ tab, connection, active, sideSession, onChange }: TabProps<TableProfileTabModel>) {
  const [profile, setProfile] = useState<TableProfile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [version, setVersion] = useState(0);
  const [filter, setFilter] = useState("");
  // What the last click did when it could not open the data: the value went to the clipboard.
  const [note, setNote] = useState<string | null>(null);
  // Counted after the profile, on its own: a failure leaves the card without a number.
  const [duplicates, setDuplicates] = useState<Duplicates | null>(null);
  const hide = useMask({ connectionId: connection.id, database: tab.database, schema: tab.schema, table: tab.name });

  useEffect(() => {
    let live = true;
    setBusy(true);
    setError(null);
    setNote(null);
    void (async () => {
      try {
        const sid = await sideSession();
        if (sid === null || !live) return;
        const read = await api.tableProfile(sid, tab.schema, tab.name, tab.sample);
        if (live) setProfile(read);
      } catch (e) {
        if (live) {
          setProfile(null);
          setError(toCommandError(e).message);
        }
      } finally {
        if (live) setBusy(false);
      }
    })();
    return () => {
      live = false;
    };
  }, [tab.schema, tab.name, tab.sample, version]);

  const target = qualifiedName(connection.driver, { name: tab.schema, current: tab.currentSchema }, tab.name);

  useEffect(() => {
    setDuplicates(null);
    if (!profile || profile.rows === 0) return;
    if (NOT_SQL.has(connection.driver)) return setDuplicates(unavailable(t("This database cannot count identical rows.")));
    // Telling identical rows apart reads the whole table: only a table read in full is counted.
    if (profile.cut !== null) return setDuplicates(unavailable(t("Only counted when the whole table was read: choose a larger sample.")));
    let live = true;
    void (async () => {
      try {
        const sid = await sideSession();
        if (sid === null || !live) return;
        // A table known to be large is not counted: the count reads all of it, and cannot be cancelled.
        const known = await api.estimateRows(sid, tab.schema, tab.name).catch(() => null);
        if (!live) return;
        if (known !== null && known > COUNT_LIMIT) return setDuplicates(unavailable(t("Not counted on tables above {n} rows", { n: count(COUNT_LIMIT) })));
        const sql = `SELECT COUNT(*) - (SELECT COUNT(*) FROM (SELECT DISTINCT * FROM ${target}) d) FROM ${target}`;
        const value = await api.queryValue(sid, sql);
        if (!live) return;
        setDuplicates(value === null || Number.isNaN(Number(value)) ? unavailable(t("The database could not count identical rows.")) : { value: count(Number(value)), why: null });
      } catch {
        // Columns that cannot be compared whole (JSON, large objects) refuse DISTINCT.
        if (live) setDuplicates(unavailable(t("The database could not count identical rows.")));
      }
    })();
    return () => {
      live = false;
    };
  }, [profile]);
  const openRows = (condition: string) =>
    workbench().openTab("query", connection.id, {
      sql: viewerSql(connection.driver, target, condition),
      autorun: true,
      title: tab.name,
      table: target,
      tableRef: { schema: tab.schema, name: tab.name, kind: tab.objectKind },
      database: tab.database,
    });
  const openValue = (column: string, value: string) => {
    const condition = equalsCondition(connection.driver, quoteIdent(column, connection.driver), value);
    if (condition !== null) return openRows(condition);
    void navigator.clipboard.writeText(value).then(() => setNote(t("Copied to the clipboard: this value cannot be written as a filter.")));
  };

  const columns = useMemo(() => matchingColumns(profile?.columns ?? [], filter), [profile, filter]);
  return (
    <div className="object-tab table-profile-tab" style={{ display: active ? "flex" : "none" }}>
      <header className="object-header">
        <span className="object-icon">
          <Icon name="activity" size={20} />
        </span>
        <div className="object-title">
          <strong>{t("Profile of {table}", { table: tab.name })}</strong>
          <span className="muted small tp-status" role="status">
            {tab.schema}
            {tab.database ? ` · ${tab.database}` : ""}
            {busy
              ? ` · ${t("Reading the rows…")}`
              : profile &&
                ` · ${profile.cut === "rows" ? plural(profile.rows, "first {n} row", "first {n} rows") : plural(profile.rows, "{n} row read", "{n} rows read")}`}
          </span>
        </div>
        <span className="spacer" />
        <input
          className="tp-filter"
          type="search"
          placeholder={t("Find a column…")}
          aria-label={t("Find a column")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <select
          className="tp-sample"
          aria-label={t("Sample")}
          value={String(tab.sample)}
          onChange={(e) => onChange({ sample: e.target.value === "null" ? null : Number(e.target.value) })}
        >
          {SAMPLE_OPTIONS.map(([size, label]) => (
            <option key={String(size)} value={String(size)}>
              {t(label)}
            </option>
          ))}
        </select>
        <button type="button" className="tp-refresh" disabled={busy} onClick={() => setVersion((v) => v + 1)}>
          <Icon name="refresh" /> {t("Refresh")}
        </button>
      </header>
      {error && <div className="notice error tp-error">{error}</div>}
      {profile?.cut === "rows" && (
        <div className="notice warn tp-cut">
          {plural(
            profile.rows,
            "Only the first {n} row was read: the table has more. Choose a larger sample to profile more of it.",
            "Only the first {n} rows were read: the table has more. Choose a larger sample to profile more of it.",
          )}
        </div>
      )}
      {profile?.cut === "memory" && (
        <div className="notice warn tp-cut">
          {plural(
            profile.rows,
            "Reading stopped at the memory limit after {n} row: the profile covers only that row.",
            "Reading stopped at the memory limit after {n} rows: the profile covers only those rows.",
          )}
        </div>
      )}
      {profile && profile.rows === 0 && <EmptyState compact className="tp-empty" title={t("The table has no rows.")} />}
      {note && <div className="notice ok tp-note">{note}</div>}
      <div className="object-body tp-body">
        {profile && columns.length === 0 && filter.trim() !== "" && <div className="muted tp-none">{t("No column matches.")}</div>}
        {profile && profile.rows > 0 && <Kpis profile={profile} duplicates={duplicates} />}
        {profile && columns.length > 0 && <div className="tp-section">{t("Columns")}</div>}
        <div className="tp-cards">
          {columns.map((c) => (
            <ColumnCard
              key={c.name}
              column={c}
              hide={!!hide?.({ name: c.name, db_type: c.db_type ?? c.data_type })}
              onValue={(v) => openValue(c.name, v)}
              onOutliers={(low, high) => openRows(outlierCondition(quoteIdent(c.name, connection.driver), low, high))}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

/** The figures of the whole table, above its columns. */
function Kpis({ profile, duplicates }: { profile: TableProfile; duplicates: Duplicates | null }) {
  const cells = profile.columns.reduce((n, c) => n + c.rows, 0);
  const nulls = profile.columns.reduce((n, c) => n + c.nulls, 0);
  const cards: [string, string, string | undefined][] = [
    [t("Rows"), count(profile.rows), undefined],
    [t("Columns"), count(profile.columns.length), undefined],
    [t("NULL values"), percent(cells > 0 ? nulls / cells : 0), undefined],
    [t("Duplicates"), duplicates?.value ?? "…", duplicates ? (duplicates.why ?? t("Rows identical to another row")) : t("Counting identical rows…")],
  ];
  return (
    <div className="tp-kpis">
      {cards.map(([label, value, hint]) => (
        <div key={label} className="tp-kpi" title={hint} data-kpi={label}>
          <span className="tp-kpi-label">{label}</span>
          <span className="tp-kpi-value">{value}</span>
        </div>
      ))}
    </div>
  );
}

interface CardProps {
  column: TableColumnProfile;
  /** The column's values show as dots. */
  hide: boolean;
  onValue: (value: string) => void;
  onOutliers: (low: number, high: number) => void;
}

function ColumnCard({ column: c, hide, onValue, onOutliers }: CardProps) {
  const share = (n: number) => percent(c.rows > 0 ? n / c.rows : 0);
  return (
    <section className="tp-card" data-column={c.name} aria-label={c.name}>
      <div className="column-profile-head">
        <strong>{c.name}</strong>
        <span className="muted">{c.db_type ?? c.data_type}</span>
        <span className="muted tp-kind">{t(KINDS[c.kind])}</span>
      </div>
      <dl>
        <dt>{t("NULL values")}</dt>
        <dd className="tp-nulls">{`${count(c.nulls)} (${share(c.nulls)})`}</dd>
        {c.kind === "text" && (
          <>
            <dt>{t("Empty strings")}</dt>
            <dd className="tp-empty-strings">{`${count(c.empty)} (${share(c.empty)})`}</dd>
          </>
        )}
        <dt>{t("Distinct")}</dt>
        <dd className="tp-distinct">{count(c.distinct)}</dd>
        {c.min !== null && (
          <>
            <dt>{t("Minimum")}</dt>
            <dd className="tp-min" title={masked(c.min, hide) ?? undefined}>{masked(c.min, hide)}</dd>
            <dt>{t("Maximum")}</dt>
            <dd className="tp-max" title={masked(c.max, hide) ?? undefined}>{masked(c.max, hide)}</dd>
          </>
        )}
        {c.numbers && (
          <>
            <dt>{t("Mean")}</dt>
            <dd>{figure(c.numbers.mean)}</dd>
            {c.numbers.stddev !== null && (
              <>
                <dt>{t("Standard deviation")}</dt>
                <dd>{figure(c.numbers.stddev)}</dd>
              </>
            )}
            <dt>{t("Quartiles")}</dt>
            <dd className="tp-quartiles">{[c.numbers.q1, c.numbers.median, c.numbers.q3].map(figure).join(" · ")}</dd>
          </>
        )}
        {c.lengths && (
          <>
            <dt>{t("Length (min · avg · max)")}</dt>
            <dd className="tp-lengths">{`${c.lengths.min} · ${figure(c.lengths.avg)} · ${c.lengths.max}`}</dd>
          </>
        )}
      </dl>
      {c.histogram.length > 0 && <Histogram bins={c.histogram} hide={hide} />}
      {c.top.length > 0 && (
        <>
          <div className="column-profile-label muted">{t("Most frequent")}</div>
          <div className="column-profile-top tp-top">
            {c.top.map((v) => (
              <TopValue key={v.value} value={v.value} count={v.count} hide={hide} onValue={onValue} />
            ))}
          </div>
        </>
      )}
      {c.patterns && c.patterns.shares.length > 0 && <Patterns column={c} hide={hide} onValue={onValue} />}
      {c.outliers && (
        <div className="tp-outliers">
          <div className="column-profile-label muted">{t("Outliers (IQR rule)")}</div>
          <div className="tp-outlier-summary">
            {plural(c.outliers.count, "{n} value ({share})", "{n} values ({share})", { share: percent(c.outliers.share) })}
            {" · "}
            {t("fences {low} to {high}", { low: figure(c.outliers.low_fence), high: figure(c.outliers.high_fence) })}
          </div>
          <div className="tp-chips">
            {c.outliers.examples.map((e) => (
              <button
                key={e}
                type="button"
                className="link tp-outlier"
                disabled={hide}
                title={t("Open the rows with outliers")}
                onClick={() => c.outliers && onOutliers(c.outliers.low_fence, c.outliers.high_fence)}
              >
                {masked(e, hide)}
              </button>
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

function TopValue({ value, count: n, hide, onValue }: { value: string; count: number; hide: boolean; onValue: (v: string) => void }) {
  return (
    <>
      <button type="button" className="link tp-top-value" disabled={hide} title={t("Open the rows with this value")} onClick={() => onValue(value)}>
        {value === "" ? t("(empty)") : masked(value, hide)}
      </button>
      <span>{count(n)}</span>
    </>
  );
}

function Patterns({ column: c, hide, onValue }: { column: TableColumnProfile; hide: boolean; onValue: (v: string) => void }) {
  const p = c.patterns;
  if (!p) return null;
  return (
    <div className="tp-patterns">
      <div className="column-profile-label muted">{t("Patterns")}</div>
      {p.dominant && (
        <div className="tp-dominant">
          {t("Mostly {pattern}", { pattern: t(PATTERNS[p.dominant]) })}
        </div>
      )}
      <ul className="tp-pattern-list">
        {p.shares.map((s) => (
          <li key={s.pattern} className="tp-pattern" data-pattern={s.pattern}>
            <span>{t(PATTERNS[s.pattern])}</span>
            <span>{`${count(s.count)} (${percent(s.share)})`}</span>
          </li>
        ))}
      </ul>
      {p.exceptions.length > 0 && (
        <>
          <div className="muted tp-exceptions-label">{t("Values that do not match")}</div>
          <div className="tp-chips">
            {p.exceptions.map((e) => (
              <button key={e} type="button" className="link tp-exception" disabled={hide} title={t("Open the rows with this value")} onClick={() => onValue(e)}>
                {masked(e, hide)}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
