/** "Test connections" (the navigator's list menu, a project's menu): each connection of the
 *  project, or of All connections, connected to and closed again, a few at a time, with the
 *  server's version and the time it took, or why it failed. */
import { useEffect, useRef, useState } from "react";
import { api, type Project, type SavedConnection, toCommandError } from "../api";
import { type TestOutcome, tally, testable } from "../connectionTest";
import { pool } from "../editor/multiRun";
import { formatCount, t } from "../i18n";
import { connectionsOf } from "../projects/state";
import { workbench } from "../workbench";

/** Connections tried at the same time. */
const PARALLEL = 4;

interface Props {
  /** The project's name, `null` for All connections. */
  scope: string | null;
  connections: SavedConnection[];
  onClose: () => void;
}

export function TestConnectionsDialog({ scope, connections, onClose }: Props) {
  const [outcomes, setOutcomes] = useState<Map<string, TestOutcome>>(new Map());
  const [running, setRunning] = useState(false);
  /** The dialog went: the tests still queued are not started. */
  const closed = useRef(false);

  const setOutcome = (id: string, o: TestOutcome) => {
    if (!closed.current) setOutcomes((all) => new Map(all).set(id, o));
  };

  async function run() {
    setRunning(true);
    setOutcomes(new Map(connections.map((c) => [c.id, testable(c) ? { state: "queued" } : { state: "skipped" }])));
    await pool(
      connections.filter(testable),
      PARALLEL,
      async (c) => {
        setOutcome(c.id, { state: "running" });
        const started = performance.now();
        try {
          const server = await api.testConnection(c, { action: "keep" });
          setOutcome(c.id, { state: "done", server, ms: Math.round(performance.now() - started) });
        } catch (e) {
          setOutcome(c.id, { state: "failed", message: toCommandError(e).message });
        }
      },
      () => closed.current,
    );
    if (!closed.current) setRunning(false);
  }

  /** The run started: StrictMode mounts the dialog twice in development, which would test
   *  each connection twice. */
  const started = useRef(false);
  useEffect(() => {
    closed.current = false;
    if (!started.current) {
      started.current = true;
      void run();
    }
    return () => void (closed.current = true);
  }, []);

  const counts = tally(outcomes.values());
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal multi-run test-connections" role="dialog" aria-label={t("Test connections")} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{scope ? t("Test the connections of {project}", { project: scope }) : t("Test connections")}</h2>
        {connections.length === 0 ? (
          <p className="muted small">{t("No connections to test.")}</p>
        ) : (
          <div className="multi-run-list" role="list">
            {connections.map((c) => (
              <TestRow key={c.id} connection={c} outcome={outcomes.get(c.id) ?? { state: "queued" }} />
            ))}
          </div>
        )}
        <div className="actions">
          {!running && outcomes.size > 0 && (
            <span className="muted small" data-field="summary">
              {[
                t("Connected: {n}", { n: formatCount(counts.done) }),
                counts.failed > 0 && t("Failed: {n}", { n: formatCount(counts.failed) }),
                counts.skipped > 0 && t("Not tried: {n}", { n: formatCount(counts.skipped) }),
              ]
                .filter(Boolean)
                .join(" · ")}
            </span>
          )}
          <span className="spacer" />
          <button type="button" data-action="again" onClick={() => void run()} disabled={running || connections.length === 0}>
            {t("Test again")}
          </button>
          <button type="button" className="primary" onClick={onClose} autoFocus>
            {t("Close")}
          </button>
        </div>
      </div>
    </div>
  );
}

function TestRow({ connection, outcome }: { connection: SavedConnection; outcome: TestOutcome }) {
  let text: string;
  switch (outcome.state) {
    case "queued":
      text = t("Waiting");
      break;
    case "running":
      text = t("Connecting…");
      break;
    case "done":
      text = `${outcome.server.product} ${outcome.server.version} · ${outcome.ms} ms`;
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

/** Opens the test of the connections of `project`, or of All connections for `null`. */
export function testConnections(project: Project | null) {
  const connections = connectionsOf(project, workbench().connections());
  workbench().openDialog((close) => <TestConnectionsDialog scope={project?.name ?? null} connections={connections} onClose={close} />);
}
