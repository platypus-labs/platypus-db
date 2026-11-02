import { useState } from "react";
import { api, type ExportEvent, type ProfileTask, toCommandError, type TransferProfile } from "../api";
import { track } from "../shell/activity";
import { askName } from "../ui/NameDialog";
import { t } from "../i18n";
import { splitTables } from "./profiles";
import { afterEvent, type JobStatus } from "./jobStatus";

/** "Save as profile…" for a transfer dialog; with a `profile` the dialog was opened from,
 *  also "Save profile", which replaces its choices. `build` gives the task of the dialog's
 *  state (asking for the file or folder when it must), or `null` to save nothing. */
export function ProfileButtons(props: {
  profile?: TransferProfile;
  build: () => Promise<ProfileTask | null>;
  disabled?: boolean;
}) {
  const { build, disabled } = props;
  const [current, setCurrent] = useState(props.profile);
  const [message, setMessage] = useState<string | null>(null);

  const kept = (saved: TransferProfile) => {
    setCurrent(saved);
    setMessage(t("Saved as profile {name}.", { name: saved.name }));
  };

  async function replace() {
    const task = await build();
    if (!task || !current) return;
    try {
      kept(await api.saveTransferProfile({ ...current, task }));
    } catch (e) {
      setMessage(toCommandError(e).message);
    }
  }

  async function saveNew() {
    const task = await build();
    if (!task) return;
    askName(t("Save as profile"), "", async (name) => kept(await api.saveTransferProfile({ id: crypto.randomUUID(), name, task })));
  }

  return (
    <>
      {message && <span className="muted small">{message}</span>}
      {current && (
        <button type="button" disabled={disabled} onClick={() => void replace()}>
          {t("Save profile")}
        </button>
      )}
      <button type="button" disabled={disabled} title={disabled ? t("Choose what to transfer first") : undefined} onClick={() => void saveNew()}>
        {t(current ? "Save as new profile…" : "Save as profile…")}
      </button>
    </>
  );
}

/** The `tables` that still exist, each in its schema (the session's own when `null`), with the names of those that are gone. */
export async function existingTables<T extends { schema: string | null; name: string }>(
  sessionId: number,
  tables: T[],
): Promise<{ kept: T[]; gone: string[] }> {
  const names = new Map<string | null, string[]>();
  for (const schema of new Set(tables.map((x) => x.schema))) {
    const own = schema ?? (await api.listSchemas(sessionId)).find((s) => s.current)?.name ?? "";
    const objects = await api.listObjects(sessionId, own).catch(() => []);
    names.set(schema, objects.filter((o) => o.kind === "table").map((o) => o.name));
  }
  const kept: T[] = [];
  const gone: string[] = [];
  for (const [schema, existing] of names) {
    const split = splitTables(
      tables.filter((x) => x.schema === schema),
      existing,
    );
    kept.push(...split.kept);
    gone.push(...split.gone.map((x) => x.name));
  }
  return { kept, gone };
}

/** Runs the job `begin` starts, following its events into `onStatus`, and logs it in the
 *  activity list as `label`; fails if the job does. */
export function runTracked(
  label: string,
  begin: (onEvent: (e: ExportEvent) => void) => Promise<number>,
  onStatus: (status: JobStatus) => void = () => {},
): Promise<void> {
  let status: JobStatus = { state: "running", id: null, rows: 0 };
  const update = (next: JobStatus) => onStatus((status = next));
  update(status);
  const finished = new Promise<void>((resolve, reject) => {
    const fail = (message: string) => {
      update({ state: "failed", message, rows: status.state === "running" ? status.rows : 0 });
      reject(new Error(message));
    };
    begin((event) => {
      update(afterEvent(status, event));
      if (event.type === "done") resolve();
      else if (event.type === "failed") reject(new Error(event.error.message));
      else if (event.type === "cancelled") reject(new Error(t("Cancelled")));
    }).then(
      (id) => status.state === "running" && update({ ...status, id }),
      (e: unknown) => fail(toCommandError(e).message),
    );
  });
  return track(label, finished);
}
