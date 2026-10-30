/** The admin tab's dashboard: tiles for the server's connections, transactions and statements
 *  per second, cache hit ratio and uptime, each with a sparkline of its last readings; the
 *  size of each database and the replication. What the server does not tell shows "—", and
 *  the reason in its tooltip. */
import type { ReactNode } from "react";
import type { Metric, ReplicationRow, ServerMetrics } from "../api";
import { formatBytes } from "../format";
import { formatCount, msg, t } from "../i18n";
import {
  connectionShare,
  gaugeSeries,
  hitRatio,
  latest,
  ratioSeries,
  rateSeries,
  rateText,
  type Sample,
  uptimeText,
} from "./dashboardModel";
import { Sparkline } from "./Sparkline";
import { LoadingState } from "../ui/State";

const ROLES: Record<ReplicationRow["role"], string> = {
  standby: msg("Replica"),
  primary: msg("Source"),
  group: msg("Availability group"),
  table: msg("Replicated table"),
};

/** The tooltip of a tile with no number: the server's reason, else that it needs a second
 *  reading (a rate is the change between two) or that its counter went back. */
function why(metrics: Metric[], samples: number): string {
  const reason = metrics.find((m) => m.unavailable)?.unavailable;
  if (reason) return reason;
  return samples < 2
    ? t("A rate needs two readings: wait for the next refresh.")
    : t("The counter went back: the server restarted.");
}

function Tile({
  id,
  title,
  value,
  note,
  reason,
  children,
}: {
  id: string;
  title: string;
  /** `null` shows "—". */
  value: string | null;
  note?: ReactNode;
  reason: string;
  children?: ReactNode;
}) {
  return (
    <div className="dash-tile" data-tile={id}>
      <div className="dash-title">{title}</div>
      <div className="dash-value" data-field="value" title={value === null ? reason : undefined}>
        {value ?? "—"}
      </div>
      {note && <div className="dash-note muted small">{note}</div>}
      {children}
    </div>
  );
}

export function Dashboard({ samples }: { samples: Sample[] }) {
  const cur = samples[samples.length - 1];
  if (!cur) return <LoadingState compact title={t("Reading the server…")} />;
  const m = cur.metrics;
  const n = samples.length;
  const transactions = rateSeries(samples, (x) => x.transactions);
  const statements = rateSeries(samples, (x) => x.statements);
  const ratios = ratioSeries(samples);
  const ratio = hitRatio(samples[n - 2] ?? null, cur);
  const tps = latest(transactions);
  const qps = latest(statements);
  const share = connectionShare(m);
  const active = m.connections_active.value;
  const idle = m.connections_idle.value;
  const max = m.connections_max.value;
  return (
    <div className="dash" data-field="dashboard">
      <div className="dash-tiles">
        <Tile
          id="connections"
          title={t("Connections")}
          value={active === null ? null : formatCount(active)}
          reason={why([m.connections_active], n)}
          note={
            <>
              {t("running")}
              {idle !== null && ` · ${t("{n} idle", { n: formatCount(idle) })}`}
              {max !== null && ` · ${t("max {n}", { n: formatCount(max) })}`}
              {share !== null && ` (${Math.round(share * 100)}%)`}
            </>
          }
        >
          <Sparkline values={gaugeSeries(samples, (x) => x.connections_active)} label={t("Running connections")} />
        </Tile>
        <Tile
          id="transactions"
          title={t("Transactions per second")}
          value={tps === null ? null : rateText(tps)}
          reason={why([m.transactions], n)}
        >
          <Sparkline values={transactions} label={t("Transactions per second")} />
        </Tile>
        <Tile
          id="statements"
          title={t("Statements per second")}
          value={qps === null ? null : rateText(qps)}
          reason={why([m.statements], n)}
        >
          <Sparkline values={statements} label={t("Statements per second")} />
        </Tile>
        <Tile
          id="cache"
          title={t("Cache hit ratio")}
          value={ratio === null ? null : `${(ratio * 100).toFixed(1)}%`}
          reason={why([m.cache_hits, m.cache_reads], 2)}
          note={t("pages found in memory")}
        >
          <Sparkline values={ratios} domain={[0, 1]} label={t("Cache hit ratio")} />
        </Tile>
        <Tile
          id="uptime"
          title={t("Uptime")}
          value={m.uptime_seconds.value === null ? null : uptimeText(m.uptime_seconds.value)}
          reason={why([m.uptime_seconds], 2)}
        />
      </div>
      <Databases metrics={m} />
      <Replication metrics={m} />
    </div>
  );
}

function Databases({ metrics }: { metrics: ServerMetrics }) {
  const biggest = Math.max(1, ...metrics.databases.map((d) => d.bytes));
  return (
    <section className="dash-section" data-section="databases">
      <h3>{t("Databases")}</h3>
      {metrics.databases_unavailable && (
        <p className="muted small" data-field="databases-unavailable">
          {t("The sizes could not be read: {reason}", { reason: metrics.databases_unavailable })}
        </p>
      )}
      {metrics.databases.length > 0 && (
        <table className="meta-table admin-table">
          <thead>
            <tr>
              <th>{t("Database")}</th>
              <th className="num">{t("Size")}</th>
              <th className="dash-bar-cell" />
            </tr>
          </thead>
          <tbody>
            {metrics.databases.map((d) => (
              <tr key={d.name} data-name={d.name}>
                <td>{d.name}</td>
                <td className="num">{formatBytes(d.bytes)}</td>
                <td className="dash-bar-cell">
                  <span className="dash-bar" style={{ width: `${Math.max(1, (d.bytes / biggest) * 100)}%` }} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

/** How far behind a link is: seconds, bytes of log, or both. */
function lag(row: ReplicationRow): string {
  const parts: string[] = [];
  if (row.lag_seconds !== null) parts.push(t("{n} s", { n: row.lag_seconds.toFixed(row.lag_seconds < 10 ? 1 : 0) }));
  if (row.lag_bytes !== null) parts.push(formatBytes(row.lag_bytes));
  return parts.join(" · ");
}

function Replication({ metrics }: { metrics: ServerMetrics }) {
  return (
    <section className="dash-section" data-section="replication">
      <h3>{t("Replication")}</h3>
      {metrics.replication_unavailable && (
        <p className="muted small" data-field="replication-unavailable">
          {t("The replication could not be read: {reason}", { reason: metrics.replication_unavailable })}
        </p>
      )}
      {metrics.replication.length === 0 && !metrics.replication_unavailable && (
        <p className="muted small" data-field="replication-none">
          {t("This server neither follows nor feeds another.")}
        </p>
      )}
      {metrics.replication.length > 0 && (
        <table className="meta-table admin-table">
          <thead>
            <tr>
              <th>{t("Link")}</th>
              <th>{t("Name")}</th>
              <th>{t("State")}</th>
              <th className="num">{t("Behind")}</th>
              <th>{t("Note")}</th>
            </tr>
          </thead>
          <tbody>
            {metrics.replication.map((r, i) => (
              <tr key={i}>
                <td>{t(ROLES[r.role])}</td>
                <td>{r.name}</td>
                <td>{r.state}</td>
                <td className="num">{lag(r)}</td>
                <td className="admin-query" title={r.detail ?? undefined}>
                  {r.detail}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
