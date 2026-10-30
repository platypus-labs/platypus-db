/** The compatibility check after a downloaded driver changes version: each saved connection
 *  that uses it is opened with a small probe (server version, schemas, `SELECT 1`) and the
 *  dialog says how each went. When one fails, it offers to go back to the previous version. */
import { useEffect, useRef, useState } from "react";
import { api, type DriverKind, type SavedConnection, toCommandError } from "../api";
import { pool } from "../editor/multiRun";
import { formatCount, plural, t } from "../i18n";
import { type ProbeOutcome, compatTally } from "./compat";
import { DRIVER_NAMES } from "./names";

/** Connections probed at the same time. */
const PARALLEL = 3;

interface Props {
  driver: DriverKind;
  /** The version now active. */
  to: string;
  /** The version that was active before, which a failure offers to go back to. */
  from: string | null;
  /** Connections to probe, and those that ask for a password each time, which are not. */
  run: SavedConnection[];
  skipped: SavedConnection[];
  /** Ask before connecting: the change came by itself (auto-update), not from a click. */
  ask: boolean;
  /** The active version changed (a way back was taken). */
  onChanged: () => void;
  onClose: () => void;
}

export function CompatDialog({ driver, to, from, run, skipped, ask, onChanged, onClose }: Props) {
  const [started, setStarted] = useState(!ask);
  const [outcomes, setOutcomes] = useState<Map<string, ProbeOutcome>>(new Map());
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** The dialog went: the probes still queued are not started. */
  const closed = useRef(false);
  const name = DRIVER_NAMES[driver];

  const setOutcome = (id: string, o: ProbeOutcome) => {
    if (!closed.current) setOutcomes((all) => new Map(all).set(id, o));
  };

  async function probeAll() {
    setStarted(true);
    setRunning(true);
    const initial = new Map<string, ProbeOutcome>();
    for (const c of run) initial.set(c.id, { state: "queued" });
    for (const c of skipped) initial.set(c.id, { state: "skipped" });
    setOutcomes(initial);
    await pool(
      run,
      PARALLEL,
      async (c) => {
        setOutcome(c.id, { state: "running" });
        const at = performance.now();
        try {
          const probe = await api.probeConnection(c);
          const summary = `${probe.product} ${probe.version} · ${plural(probe.schemas, "{n} schema", "{n} schemas")} · SELECT 1`;
          setOutcome(c.id, { state: "done", summary, ms: Math.round(performance.now() - at) });
        } catch (e) {
          setOutcome(c.id, { state: "failed", message: toCommandError(e).message });
        }
      },
      () => closed.current,
    );
    if (!closed.current) setRunning(false);
  }

  /** StrictMode mounts the dialog twice in development, which would probe each connection twice. */
  const once = useRef(false);
  useEffect(() => {
    closed.current = false;
    if (!ask && !once.current) {
      once.current = true;
      void probeAll();
    }
    return () => void (closed.current = true);
  }, []);

  async function goBack() {
    if (!from) return;
    setError(null);
    try {
      await api.activateDriverVersion(driver, from);
      onChanged();
      onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const counts = compatTally(outcomes.values());
  const finished = started && !running && outcomes.size > 0;
  const rows = [...run, ...skipped];
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !running && onClose()}>
      <div className="modal multi-run driver-compat" role="dialog" aria-label={t("Driver compatibility")} data-driver={driver} onKeyDown={(e) => e.key === "Escape" && !running && onClose()}>
        <h2>{t("{name} driver {version}: compatibility", { name, version: to })}</h2>
        {!started ? (
          <p className="small" data-field="ask">
            {plural(
              run.length,
              "The {name} driver is now at {version}. Connect to the {n} saved connection that uses it to check that it still works?",
              "The {name} driver is now at {version}. Connect to the {n} saved connections that use it to check that they still work?",
              { name, version: to },
            )}
          </p>
        ) : rows.length === 0 ? (
          <p className="muted small">{t("No saved connection uses this driver.")}</p>
        ) : (
          <div className="multi-run-list" role="list">
            {rows.map((c) => (
              <ProbeRow key={c.id} connection={c} outcome={outcomes.get(c.id) ?? { state: "queued" }} />
            ))}
          </div>
        )}
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          {finished && (
            <span className="muted small" data-field="summary">
              {[
                t("Working: {n}", { n: formatCount(counts.done) }),
                counts.failed > 0 && t("Failed: {n}", { n: formatCount(counts.failed) }),
                counts.skipped > 0 && t("Not tried: {n}", { n: formatCount(counts.skipped) }),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          )}
          <span className="spacer" />
          {finished && counts.failed > 0 && from && (
            <button type="button" data-action="rollback" onClick={() => void goBack()}>
              {t("Go back to {version}", { version: from })}
            </button>
          )}
          {!started && (
            <button type="button" className="primary" data-action="check" onClick={() => void probeAll()}>
              {t("Check connections")}
            </button>
          )}
          <button type="button" className={started ? "primary" : undefined} data-action="close" onClick={onClose} disabled={running}>
            {started ? t("Close") : t("Not now")}
          </button>
        </div>
      </div>
    </div>
  );
}

function ProbeRow({ connection, outcome }: { connection: SavedConnection; outcome: ProbeOutcome }) {
  let text: string;
  switch (outcome.state) {
    case "queued":
      text = t("Waiting");
      break;
    case "running":
      text = t("Connecting…");
      break;
    case "done":
      text = `${outcome.summary} · ${outcome.ms} ms`;
      break;
    case "failed":
      text = outcome.message;
      break;
    case "skipped":
      text = t("Asks for its password: not tried");
      break;
  }
  return (
    <div className={`multi-run-row outcome ${outcome.state}`} role="listitem" data-id={connection.id} data-state={outcome.state}>
      <span className="multi-run-state" aria-hidden />
      <span className="multi-run-name">{connection.name}</span>
      <span className="multi-run-outcome" title={text}>
        {text}
      </span>
    </div>
  );
}
