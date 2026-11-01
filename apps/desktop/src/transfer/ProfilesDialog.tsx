/** "Transfer profiles": the exports, imports, copies and migrations saved with their choices,
 *  each to run again, open in its dialog, schedule, rename or delete. */
import { createElement, useEffect, useState } from "react";
import { api, type ProfileTask, type SavedConnection, toCommandError, type TransferProfile } from "../api";
import { SchedulesDialog } from "../admin/SchedulesDialog";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { msg, t } from "../i18n";
import { ask } from "../ui/ask";
import { askName } from "../ui/NameDialog";
import { workbench } from "../workbench";
import { changes } from "./ImportDataDialog";
import type { JobStatus } from "./jobStatus";
import { openProfile } from "./profileActions";
import { profileConnections, profilesOf } from "./profiles";
import { runTracked } from "./ProfileSave";
import { toggleSidebar, useSidebarCollapsed } from "../shell/sidebar";
import { EmptyState } from "../ui/State";

const KINDS: Record<ProfileTask["kind"], string> = {
  export_query: msg("Export a query"),
  export_tables: msg("Export tables"),
  import_file: msg("Import a file"),
  transfer_table: msg("Copy a table"),
  migrate: msg("Copy tables with their structure"),
};

/** The profiles, or with `connectionId` those that use that connection. */
export function ProfilesDialog({ connectionId, onClose }: { connectionId?: string; onClose: () => void }) {
  const [profiles, setProfiles] = useState<TransferProfile[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** The state of each profile run from here. */
  const [runs, setRuns] = useState<Record<string, JobStatus>>({});
  /** The profile waiting for the confirmation a production connection asks before it runs. */
  const [confirm, setConfirm] = useState<{ profile: TransferProfile; request: GuardRequest } | null>(null);
  const collapsed = useSidebarCollapsed();
  const connections = workbench().connections();
  const nameOf = (id: string) => connections.find((c) => c.id === id)?.name ?? t("a deleted connection");
  const fail = (e: unknown) => setError(toCommandError(e).message);

  useEffect(() => {
    api.listTransferProfiles().then(setProfiles, fail);
  }, []);

  /** Runs `p`, once confirmed when it writes to a production connection (exports write files). */
  function request(p: TransferProfile) {
    const request = productionRequest(p.task, connections);
    if (request) setConfirm({ profile: p, request });
    else run(p);
  }

  function run(p: TransferProfile) {
    setError(null);
    const status = (s: JobStatus) => setRuns((all) => ({ ...all, [p.id]: s }));
    runTracked(t("Profile {name}", { name: p.name }), (onEvent) => api.runTransferProfile(p.id, onEvent), status).catch(() => {});
  }

  function open(p: TransferProfile) {
    const problem = openProfile(p);
    if (problem) setError(problem);
    else onClose();
  }

  function rename(p: TransferProfile) {
    askName(t("Rename profile"), p.name, async (name) => {
      const saved = await api.saveTransferProfile({ ...p, name });
      setProfiles((all) => all && all.map((x) => (x.id === saved.id ? saved : x)));
    });
  }

  async function remove(p: TransferProfile) {
    if (!(await ask(t("Delete the profile {name}?", { name: p.name }), { confirm: t("Delete"), danger: true }))) return;
    try {
      setProfiles(await api.deleteTransferProfile(p.id));
      setError(null);
    } catch (e) {
      fail(e);
    }
  }

  const shown = profiles && profilesOf(profiles, connectionId);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal schedules-dialog" role="dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{connectionId ? t("Transfer profiles of {name}", { name: nameOf(connectionId) }) : t("Transfer profiles")}</h2>
        <p className="muted small">{t("Save a profile from the export, import, copy and migration dialogs.")}</p>
        {error && <div className="notice error small">{error}</div>}
        {shown && shown.length === 0 && (
          <EmptyState
            className="schedules-empty"
            title={t("No profiles saved yet.")}
            actions={
              <button
                type="button"
                className="primary"
                onClick={() => {
                  if (collapsed) toggleSidebar();
                  onClose();
                }}
              >
                {t("Go to the navigator")}
              </button>
            }
          >
            {t("A profile is saved from the export, import, copy or migration dialog of a table: right-click a table in the navigator.")}
          </EmptyState>
        )}
        {shown && shown.length > 0 && (
          <div className="schedules-list">
            <table className="meta-table admin-table">
              <tbody>
                {shown.map((p) => (
                  <ProfileRow
                    key={p.id}
                    profile={p}
                    connections={profileConnections(p.task).map(nameOf).join(" → ")}
                    status={runs[p.id]}
                    onRun={() => request(p)}
                    onOpen={() => open(p)}
                    onSchedule={() => workbench().openDialog((close) => createElement(SchedulesDialog, { profileId: p.id, onClose: close }))}
                    onRename={() => rename(p)}
                    onDelete={() => void remove(p)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" className="primary" onClick={onClose}>
            {t("Close")}
          </button>
        </div>
      </div>
      {confirm && (
        <GuardDialog
          request={confirm.request}
          onAnswer={(go) => {
            const { profile } = confirm;
            setConfirm(null);
            if (go) run(profile);
          }}
        />
      )}
    </div>
  );
}

/** The confirmation a production target asks before `task` runs, or `null` when it needs none. */
function productionRequest(task: ProfileTask, connections: SavedConnection[]): GuardRequest | null {
  if (task.kind === "export_query" || task.kind === "export_tables") return null;
  const id = task.kind === "import_file" ? task.connection_id : task.target_connection_id;
  const connection = connections.find((c) => c.id === id);
  if (!connection || !needsProductionConfirm(connection)) return null;
  let production: string[];
  if (task.kind === "migrate") {
    production = (task.tables ?? ["*"]).map(
      (table) => `CREATE TABLE ${task.target_schema}.${table} (...) -- ${t("and every row of {table}", { table: `${task.source_schema}.${table}` })}`,
    );
  } else production = changes(task.plan);
  return { connection, unfiltered: [], production, blocked: false };
}

function ProfileRow(props: {
  profile: TransferProfile;
  connections: string;
  status: JobStatus | undefined;
  onRun: () => void;
  onOpen: () => void;
  onSchedule: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const { profile, connections, status } = props;
  const running = status?.state === "running";
  return (
    <tr className="admin-session schedule-row" data-id={profile.id}>
      <td>
        <strong>{profile.name}</strong>
        <div className="muted small">
          {t(KINDS[profile.task.kind])} · {connections}
        </div>
        {status?.state === "running" && <div className="muted small">{t("Running… {n} rows", { n: status.rows.toLocaleString() })}</div>}
        {status?.state === "done" && (
          <div className="muted small">
            {t("Done: {n} rows.", { n: status.rows.toLocaleString() })}
            {status.warnings.length > 0 && ` ${t("Left out:")} ${status.warnings.join("; ")}`}
          </div>
        )}
        {status?.state === "failed" && <div className="notice error small">{status.message}</div>}
      </td>
      <td className="schedule-actions">
        {status?.state === "running" ? (
          <button type="button" disabled={status.id === null} onClick={() => status.id !== null && void api.cancelExport(status.id)}>
            {t("Stop")}
          </button>
        ) : (
          <button type="button" onClick={props.onRun}>
            {t("Run")}
          </button>
        )}
        <button type="button" disabled={running} onClick={props.onOpen}>
          {t("Open…")}
        </button>
        <button type="button" onClick={props.onSchedule}>
          {t("Schedule…")}
        </button>
        <button type="button" onClick={props.onRename}>
          {t("Rename…")}
        </button>
        <button type="button" className="admin-end" onClick={props.onDelete}>
          {t("Delete")}
        </button>
      </td>
    </tr>
  );
}
