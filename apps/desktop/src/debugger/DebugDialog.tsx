/** "Debug…" of a PL/pgSQL routine in the navigator: whether the database can debug (with the
 *  extension to create when only it is missing), then the values of the call's arguments.
 *  The call starts here, so that a value the database refuses is fixed in place; once it
 *  stops at its first statement, the debugger tab opens. */
import { useEffect, useState } from "react";
import { api, type DebugArgument, type DebugInfo, type SavedConnection, toCommandError } from "../api";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { initialArguments, inputParameters, isTrigger, misplacedDefault, parameterName } from "./model";

interface Props {
  connection: SavedConnection;
  database: string | null;
  schema: string;
  /** As the navigator lists it, with its argument types: `add(integer, integer)`. */
  name: string;
  onClose: () => void;
}

type Load =
  | { step: "loading" }
  | { step: "failed"; message: string }
  /** The connection's driver has no debugger (e.g. behind a pooler in transaction mode). */
  | { step: "unsupported" }
  | { step: "ready"; sid: number; info: DebugInfo };

export function DebugDialog({ connection, database, schema, name, onClose }: Props) {
  const [load, setLoad] = useState<Load>({ step: "loading" });
  const [args, setArgs] = useState<DebugArgument[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function read() {
    try {
      const sid = await workbench().sideSession(connection, database);
      if (sid === null) return onClose();
      if (!workbench().capabilities(sid)?.debugger) return setLoad({ step: "unsupported" });
      const info = await api.debugRoutine(sid, schema, name);
      setArgs(initialArguments(info.routine));
      setLoad({ step: "ready", sid, info });
    } catch (e) {
      setLoad({ step: "failed", message: toCommandError(e).message });
    }
  }

  useEffect(() => {
    void read();
    // Read once, for the routine the dialog opened on.
  }, []);

  async function install(sid: number) {
    setBusy(true);
    setError(null);
    try {
      await api.debugInstall(sid);
      await read();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function start(sid: number, info: DebugInfo) {
    setBusy(true);
    setError(null);
    try {
      const started = await api.debugStart(sid, info.routine.id, args);
      onClose();
      workbench().openTab("debugger", connection.id, {
        database,
        title: t("Debug {routine}", { routine: `${info.routine.name}(${info.routine.signature})` }),
        started,
      });
    } catch (e) {
      setError(toCommandError(e).message);
      setBusy(false);
    }
  }

  const close = () => !busy && onClose();
  const ready = load.step === "ready" ? load : null;
  const routine = ready?.info.routine ?? null;
  const why = ready ? obstacle(ready.info, connection) : null;
  const misplaced = misplacedDefault(args);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div className="modal debug-dialog" role="dialog" onKeyDown={(e) => e.key === "Escape" && close()}>
        <h2>{t("Debug {routine}", { routine: `${schema}.${name}` })}</h2>
        {load.step === "loading" && <p className="muted small">{t("Reading the routine…")}</p>}
        {load.step === "failed" && (
          <div className="notice error small" data-field="debug-error">
            {load.message}
          </div>
        )}
        {load.step === "unsupported" && (
          <div className="notice small" data-field="debug-unavailable">
            {t("This connection cannot debug routines: the debugger needs a session of its own on PostgreSQL, not through a pooler in transaction mode.")}
          </div>
        )}
        {ready && why && (
          <div className="notice small" data-field="debug-unavailable">
            {why}
          </div>
        )}
        {ready?.info.availability === "missing_extension" && (
          <div className="row tight">
            <button
              type="button"
              data-field="debug-install"
              disabled={busy || connection.read_only}
              title={connection.read_only ? t("The connection is read-only") : undefined}
              onClick={() => void install(ready.sid)}
            >
              {t("Create the pldbgapi extension")}
            </button>
          </div>
        )}
        {routine && !why && (
          <>
            <p className="muted small">
              {inputParameters(routine).length === 0
                ? t("The routine takes no arguments.")
                : t("The values are read as the parameters' types; the call stops at its first statement.")}
            </p>
            <div className="debug-arguments" data-field="debug-arguments">
              {inputParameters(routine).map((p, i) => (
                <ArgumentField
                  key={i}
                  label={parameterName(p, i)}
                  type={p.type_name}
                  mode={p.mode}
                  hasDefault={p.has_default}
                  value={args[i] ?? { kind: "value", value: "" }}
                  disabled={busy}
                  wrong={misplaced === i}
                  onChange={(a) => setArgs((all) => all.map((old, j) => (j === i ? a : old)))}
                />
              ))}
            </div>
            {misplaced !== null && (
              <div className="notice error small" data-field="debug-misplaced">
                {t("A default value can be kept only if every later argument keeps its default too.")}
              </div>
            )}
          </>
        )}
        {error && (
          <div className="notice error small" data-field="debug-error">
            {error}
          </div>
        )}
        <div className="actions">
          <span className="spacer" />
          <button type="button" disabled={busy} onClick={onClose}>
            {t("Cancel")}
          </button>
          <button
            type="button"
            className="primary"
            data-field="debug-start"
            autoFocus
            disabled={!ready || !!why || busy || misplaced !== null || connection.read_only}
            title={connection.read_only ? t("The connection is read-only") : undefined}
            onClick={() => ready && void start(ready.sid, ready.info)}
          >
            {busy ? t("Starting…") : t("Debug")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Why the routine cannot be debugged on this database; `null` when it can. */
function obstacle({ availability, routine }: DebugInfo, connection: SavedConnection): string | null {
  switch (availability) {
    case "not_installed":
      return t("The server has no debugger: its administrator must install pldebugger (the postgresql-NN-pldebugger package) and add plugin_debugger to shared_preload_libraries.");
    case "not_loaded":
      return t("The server has the debugger but does not load it: its administrator must add plugin_debugger to shared_preload_libraries and restart the server.");
    case "missing_extension":
      return connection.read_only
        ? t("The database lacks the pldbgapi extension of the debugger, and the connection is read-only.")
        : t("The database lacks the pldbgapi extension of the debugger: create it to go on (it needs a superuser).");
    case "ready":
      break;
  }
  if (isTrigger(routine)) return t("A trigger function runs only when its trigger fires: it cannot be called here.");
  if (routine.language !== "plpgsql") {
    return t("Only PL/pgSQL routines can be debugged; {routine} is written in {language}.", {
      routine: routine.name,
      language: routine.language,
    });
  }
  return null;
}

function ArgumentField({
  label,
  type,
  mode,
  hasDefault,
  value,
  disabled,
  wrong,
  onChange,
}: {
  label: string;
  type: string;
  mode: string;
  hasDefault: boolean;
  value: DebugArgument;
  disabled: boolean;
  wrong: boolean;
  onChange: (value: DebugArgument) => void;
}) {
  return (
    <div className={`debug-argument${wrong ? " wrong" : ""}`} data-param={label}>
      <span className="debug-argument-name">
        <strong>{label}</strong> <span className="muted small">{mode === "in" ? type : `${mode.toUpperCase()} ${type}`}</span>
      </span>
      <input
        aria-label={label}
        data-field="debug-value"
        spellCheck={false}
        disabled={disabled || value.kind !== "value"}
        value={value.kind === "value" ? value.value : ""}
        placeholder={value.kind === "null" ? "NULL" : value.kind === "default" ? t("Default value") : ""}
        onChange={(e) => onChange({ kind: "value", value: e.target.value })}
      />
      <select
        aria-label={t("Value of {parameter}", { parameter: label })}
        data-field="debug-kind"
        disabled={disabled}
        value={value.kind}
        onChange={(e) => {
          const kind = e.target.value as DebugArgument["kind"];
          onChange(kind === "value" ? { kind, value: "" } : { kind });
        }}
      >
        <option value="value">{t("Value")}</option>
        <option value="null">NULL</option>
        {hasDefault && <option value="default">{t("Default value")}</option>}
      </select>
    </div>
  );
}
