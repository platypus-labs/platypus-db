// Running an SQL file in a database, and dumping a schema (or some of its objects) to one.
import { type ReactNode, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, type SavedConnection } from "../api";
import { GuardDialog } from "../guards/GuardDialog";
import { type GuardRequest, needsProductionConfirm } from "../guards/guards";
import { requestNavigatorRefresh } from "../navigatorEvents";
import { answerFor } from "../testAnswers";
import { workbench } from "../workbench";
import { JobFooter, useJob } from "./job";
import { t, plural, msg } from "../i18n";

/** The extensions of an SQL file, plain or compressed. */
const SQL_FILES = [{ name: "SQL", extensions: ["sql", "gz", "zst"] }];

/** A file's path, typed or chosen with `choose`. */
function FileField(props: {
  path: string;
  disabled: boolean;
  onChange: (path: string) => void;
  choose: () => Promise<unknown>;
}) {
  async function choose() {
    const path = await answerFor("file", props.choose);
    if (typeof path === "string") props.onChange(path);
  }
  return (
    <div className="field">
      <span>{t("File")}</span>
      <div className="row tight">
        <input
          aria-label={t("File")}
          value={props.path}
          disabled={props.disabled}
          spellCheck={false}
          onChange={(e) => props.onChange(e.target.value)}
        />
        <button
          type="button"
          disabled={props.disabled}
          onClick={() => void choose()}
        >
          {t("Choose file…")}
        </button>
      </div>
    </div>
  );
}

function Frame(props: {
  title: string;
  running: boolean;
  onClose: () => void;
  children: ReactNode;
}) {
  const close = () => !props.running && props.onClose();
  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => e.target === e.currentTarget && close()}
    >
      <div
        className="modal small sql-file"
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        <h2>{props.title}</h2>
        {props.children}
      </div>
    </div>
  );
}

/** Runs an SQL file (`.gz` and `.zst` too) in a database, a statement at a time, by default
 *  in one transaction. */
export function RunSqlFileDialog({
  connection,
  database,
  onClose,
}: {
  connection: SavedConnection;
  database: string | null;
  onClose: () => void;
}) {
  const [path, setPath] = useState("");
  const [transaction, setTransaction] = useState(true);
  const [confirm, setConfirm] = useState<GuardRequest | null>(null);
  const { status, start } = useJob();
  const running = status.state === "running";
  const name = path.split(/[\\/]/).pop() ?? path;

  function request() {
    if (!needsProductionConfirm(connection)) return void run();
    setConfirm({
      connection,
      unfiltered: [],
      production: [t("Every statement of {file}", { file: name })],
      blocked: false,
    });
  }

  async function run() {
    const sid = await workbench().sideSession(connection, database);
    if (sid === null) return;
    // What ran may have changed objects, whether the file ran to its end or not.
    const refresh = () =>
      requestNavigatorRefresh({ connectionId: connection.id, database });
    await start(
      (onEvent) => api.runSqlFile(sid, path.trim(), transaction, onEvent),
      refresh,
      refresh,
    );
  }

  const target =
    database === null ? connection.name : `${connection.name} · ${database}`;
  return (
    <Frame title={t("Run an SQL file")} running={running} onClose={onClose}>
      <p className="muted small">
        {t(
          "Its statements run in order in {target}, in the connection's current schema.",
          { target },
        )}
      </p>
      <FileField
        path={path}
        disabled={running}
        onChange={setPath}
        choose={() =>
          open({
            multiple: false,
            directory: false,
            title: t("Run an SQL file"),
            filters: [
              ...SQL_FILES,
              { name: t("All files"), extensions: ["*"] },
            ],
          })
        }
      />
      <label className="check">
        <input
          type="checkbox"
          checked={transaction}
          disabled={running}
          onChange={(e) => setTransaction(e.target.checked)}
        />
        {t("In one transaction")}
      </label>
      <p className="muted small">
        {transaction
          ? t(
              "A failure or a stop rolls back the whole file, where the database can roll back DDL.",
            )
          : t("A failure or a stop keeps what ran before it.")}
      </p>
      <JobFooter
        status={status}
        messages={[msg("Running… {n} statements"), msg("Ran {n} statements.")]}
        // Nothing to undo when the first statement fails, or the file cannot be read.
        failed={(rows) =>
          rows === 0
            ? ""
            : `${transaction ? t("Rolled back.") : plural(rows, "{n} statement ran before.", "{n} statements ran before.")} `
        }
        stopped={
          transaction
            ? msg("Stopped and rolled back.")
            : msg("Stopped after {n} statements.")
        }
        stop={t("Stop")}
        onClose={onClose}
      >
        <button
          type="button"
          className="primary"
          disabled={running || !path.trim()}
          onClick={request}
        >
          {t("Run")}
        </button>
      </JobFooter>
      {confirm && (
        <GuardDialog
          request={confirm}
          onAnswer={(go) => {
            setConfirm(null);
            if (go) void run();
          }}
        />
      )}
    </Frame>
  );
}

interface DumpProps {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** The objects to dump; `null` for the whole schema. */
  objects: string[] | null;
  onClose: () => void;
}

/** Writes a schema's DDL, and by default its rows, to an SQL file that runs again elsewhere. */
export function DumpSqlDialog({
  connection,
  database,
  schema,
  objects,
  onClose,
}: DumpProps) {
  const [path, setPath] = useState("");
  const [data, setData] = useState(true);
  const { status, start } = useJob();
  const running = status.state === "running";

  async function run() {
    const sid = await workbench().sideSession(connection, database);
    if (sid === null) return;
    await start((onEvent) =>
      api.dumpSql(sid, schema, path.trim(), data, objects, onEvent),
    );
  }

  const fileName =
    (objects?.length === 1 ? objects[0]! : schema).replace(
      /[<>:"/\\|?*]/g,
      "_",
    ) || "dump";
  return (
    <Frame
      title={
        objects
          ? plural(
              objects.length,
              "Dump {n} object to an SQL file",
              "Dump {n} objects to an SQL file",
            )
          : t("Dump {schema} to an SQL file", { schema })
      }
      running={running}
      onClose={onClose}
    >
      <p className="muted small">
        {objects
          ? t(
              "The file creates {names} again, in an order that keeps their references.",
              { names: objects.join(", ") },
            )
          : t(
              "The file creates the schema's sequences, tables, views and routines again, in an order that keeps their references.",
            )}
      </p>
      <label className="field">
        <span>{t("Contents")}</span>
        <select
          value={data ? "data" : "ddl"}
          disabled={running}
          onChange={(e) => setData(e.target.value === "data")}
        >
          <option value="data">{t("Structure and data")}</option>
          <option value="ddl">{t("Structure only")}</option>
        </select>
      </label>
      <FileField
        path={path}
        disabled={running}
        onChange={setPath}
        choose={() =>
          save({ defaultPath: `${fileName}.sql`, filters: SQL_FILES })
        }
      />
      <p className="muted small">
        {t("A name ending in .gz or .zst compresses the file.")}
      </p>
      <JobFooter
        status={status}
        messages={[msg("Dumping… {n} rows"), msg("Dumped {n} rows.")]}
        stop={t("Stop")}
        onClose={onClose}
      >
        <button
          type="button"
          className="primary"
          disabled={running || !path.trim()}
          onClick={() => void run()}
        >
          {t("Dump")}
        </button>
      </JobFooter>
    </Frame>
  );
}
