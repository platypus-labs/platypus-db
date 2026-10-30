/** A PL/pgSQL call being debugged, opened by "Debug…" once it stopped at its first
 *  statement: its source with the line it is stopped at and the breakpoints, the steps, the
 *  call stack and the variables of a frame (a value can be changed), then what it returned.
 *  Closing the tab aborts the call. */
import { listen } from "@tauri-apps/api/event";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import {
  api,
  type DebugFrame,
  type DebugOutcome,
  type DebugStarted,
  type DebugState,
  type DebugStateEvent,
  type DebugStep,
  type DebugVariable,
  toCommandError,
} from "../api";
import { t } from "../i18n";
import type { TabBase, TabProps } from "../tabs/registry";
import { breakpointLines, isStopped, withBreakpoint } from "./model";
import { SourceView } from "./SourceView";

/** Not kept for the next start: the call ends with the app. */
export interface DebuggerTabModel extends TabBase {
  kind: "debugger";
  started?: DebugStarted;
}

declare module "../tabs/registry" {
  interface TabKinds {
    debugger: DebuggerTabModel;
  }
}

/** The keys of the steps, as in most debuggers. */
const KEYS: Record<string, DebugStep | "stop"> = {
  F11: "into",
  F10: "over",
  F5: "continue",
  "Shift+F5": "stop",
};

export function DebuggerTab({ tab, active }: TabProps<DebuggerTabModel>) {
  const started = tab.started;
  if (!started) {
    return (
      <div className="object-tab debugger-tab" style={{ display: active ? "flex" : "none" }}>
        <div className="object-body">
          <div className="notice small">{t("This debugging session is over: start another with Debug… on the routine.")}</div>
        </div>
      </div>
    );
  }
  return <Debugging started={started} active={active} />;
}

function Debugging({ started, active }: { started: DebugStarted; active: boolean }) {
  const id = started.debug_id;
  const [state, setState] = useState<DebugState>(started.state);
  const [sources, setSources] = useState<Record<string, string>>({ [started.routine.id]: started.source });
  const [breakpoints, setBreakpoints] = useState<Set<string>>(() => new Set());
  const [stack, setStack] = useState<DebugFrame[]>([]);
  const [frame, setFrame] = useState(0);
  const [variables, setVariables] = useState<DebugVariable[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const stateNow = useRef(state);
  stateNow.current = state;

  const fail = (e: unknown) => {
    // A command cut short by the end of the call says less than the end itself.
    if (stateNow.current.state !== "finished") setError(toCommandError(e).message);
  };

  useEffect(() => {
    const off = listen<DebugStateEvent>("debug-state", ({ payload }) => {
      if (payload.debug_id === id) setState(payload.state);
    });
    return () => {
      void off.then((unlisten) => unlisten());
      // Closing the tab ends the call, which would otherwise hold its session.
      if (stateNow.current.state !== "finished") void api.debugStop(id).catch(() => undefined);
    };
  }, [id]);

  async function loadSource(routine: string) {
    if (routine in sources) return;
    try {
      const source = await api.debugSource(id, routine);
      setSources((all) => ({ ...all, [routine]: source }));
    } catch (e) {
      fail(e);
    }
  }

  // Each stop, on the same line too (a loop): the stack and the innermost frame's variables,
  // read again.
  useEffect(() => {
    if (!isStopped(state)) return;
    let current = true;
    setFrame(0);
    void (async () => {
      try {
        const frames = await api.debugStack(id);
        const vars = await api.debugVariables(id, 0);
        if (!current) return;
        setStack(frames);
        setVariables(vars);
        await loadSource(state.position.routine);
      } catch (e) {
        if (current) fail(e);
      }
    })();
    return () => {
      current = false;
    };
  }, [state]);

  useEffect(() => {
    if (active) root.current?.focus();
  }, [active]);

  async function step(kind: DebugStep) {
    if (!isStopped(state) || busy) return;
    setBusy(true);
    setError(null);
    try {
      setState(kind === "continue" ? await api.debugContinue(id) : await api.debugStep(id, kind));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    if (state.state === "finished") return;
    try {
      const final = await api.debugStop(id);
      if (final) setState(final);
    } catch (e) {
      fail(e);
    }
  }

  async function selectFrame(f: DebugFrame) {
    setFrame(f.level);
    try {
      setVariables(await api.debugVariables(id, f.level));
      await loadSource(f.position.routine);
    } catch (e) {
      fail(e);
    }
  }

  const stopped = isStopped(state) && !busy;
  const shown = stack.find((f) => f.level === frame)?.position ?? (isStopped(state) ? state.position : null);
  const routine = shown?.routine ?? started.routine.id;

  async function toggle(line: number, on: boolean) {
    try {
      await api.debugSetBreakpoint(id, routine, line, on);
      setBreakpoints((all) => withBreakpoint(all, routine, line, on));
    } catch (e) {
      fail(e);
    }
  }

  async function deposit(v: DebugVariable, value: string | null) {
    try {
      await api.debugSetVariable(id, frame, v.name, v.line, value);
      setError(null);
    } catch (e) {
      fail(e);
    }
    try {
      setVariables(await api.debugVariables(id, frame));
    } catch (e) {
      fail(e);
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    const key = `${e.shiftKey ? "Shift+" : ""}${e.key}`;
    const action = KEYS[key];
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    if (action === "stop") void stop();
    else void step(action);
  }

  return (
    <div
      className="object-tab debugger-tab"
      style={{ display: active ? "flex" : "none" }}
      ref={root}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <header className="object-header">
        <div className="object-title">
          <strong>{t("Debug {routine}", { routine: `${started.routine.schema}.${started.routine.name}` })}</strong>
          <span className="muted small">{started.routine.signature}</span>
        </div>
        <Status state={state} busy={busy} />
        <span className="spacer" />
        <div className="debug-toolbar" role="toolbar" aria-label={t("Debugger")}>
          <button data-field="debug-into" disabled={!stopped} title={t("Step into (F11)")} onClick={() => void step("into")}>
            {t("Step into")}
          </button>
          <button data-field="debug-over" disabled={!stopped} title={t("Step over (F10)")} onClick={() => void step("over")}>
            {t("Step over")}
          </button>
          <button data-field="debug-continue" disabled={!stopped} title={t("Continue (F5)")} onClick={() => void step("continue")}>
            {t("Continue")}
          </button>
          <button
            className="debug-stop"
            data-field="debug-stop"
            disabled={state.state === "finished"}
            title={t("Stop: abort the call (Shift+F5)")}
            onClick={() => void stop()}
          >
            {t("Stop")}
          </button>
        </div>
      </header>
      {error && (
        <div className="notice error small debug-error" data-field="debug-error">
          {error}
        </div>
      )}
      <div className="debug-body">
        <section className="debug-source-pane">
          <div className="debug-pane-title muted small" data-field="debug-routine">
            {shown?.name ?? started.routine.name}
            {shown && ` · ${t("line {line}", { line: shown.line })}`}
          </div>
          {sources[routine] !== undefined ? (
            <SourceView
              source={sources[routine]}
              line={isStopped(state) && shown ? shown.line : null}
              breakpoints={breakpointLines(breakpoints, routine)}
              editable={stopped}
              onToggle={(line, on) => void toggle(line, on)}
            />
          ) : (
            <p className="muted small">{t("Reading the source…")}</p>
          )}
        </section>
        <aside className="debug-side">
          <Stack frames={isStopped(state) ? stack : []} selected={frame} onSelect={(f) => void selectFrame(f)} />
          <Variables
            variables={isStopped(state) ? variables : []}
            editable={stopped}
            onChange={(v, value) => void deposit(v, value)}
          />
        </aside>
      </div>
      {state.state === "finished" && <Outcome outcome={state.outcome} />}
    </div>
  );
}

function Status({ state, busy }: { state: DebugState; busy: boolean }) {
  const [kind, text] =
    state.state === "finished"
      ? state.outcome.aborted
        ? ["aborted", t("Stopped")]
        : state.outcome.error
          ? ["failed", t("Failed")]
          : ["done", t("Finished")]
      : state.state === "running" || busy
        ? ["running", t("Running…")]
        : ["stopped", t("Paused")];
  return (
    <span className={`debug-status ${kind}`} data-field="debug-status" data-state={kind}>
      <span className="admin-dot" />
      {text}
    </span>
  );
}

function Stack({ frames, selected, onSelect }: { frames: DebugFrame[]; selected: number; onSelect: (f: DebugFrame) => void }) {
  return (
    <div className="debug-panel" data-field="debug-stack">
      <h3 className="admin-heading">{t("Call stack")}</h3>
      {frames.length === 0 ? (
        <p className="muted small">{t("Only while the call is paused.")}</p>
      ) : (
        <ul className="debug-frames">
          {frames.map((f) => (
            <li key={f.level}>
              <button
                className={f.level === selected ? "debug-frame selected" : "debug-frame"}
                data-level={f.level}
                onClick={() => onSelect(f)}
              >
                <span className="debug-frame-name">{f.position.name}</span>
                <span className="muted small">{t("line {line}", { line: f.position.line })}</span>
                {f.arguments && <code className="debug-frame-args">{f.arguments}</code>}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Variables({
  variables,
  editable,
  onChange,
}: {
  variables: DebugVariable[];
  editable: boolean;
  onChange: (v: DebugVariable, value: string | null) => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const keyOf = (v: DebugVariable) => `${v.name}:${v.line}`;
  const save = (v: DebugVariable, value: string | null) => {
    setEditing(null);
    onChange(v, value);
  };
  return (
    <div className="debug-panel" data-field="debug-variables">
      <h3 className="admin-heading">{t("Variables")}</h3>
      {variables.length === 0 ? (
        <p className="muted small">{t("Only while the call is paused.")}</p>
      ) : (
        <table className="meta-table debug-variables">
          <thead>
            <tr>
              <th>{t("Name")}</th>
              <th>{t("Type")}</th>
              <th>{t("Value")}</th>
            </tr>
          </thead>
          <tbody>
            {variables.map((v) => (
              <tr key={keyOf(v)} data-name={v.name} className={v.kind}>
                <td title={v.kind === "argument" ? t("Argument") : undefined}>{v.name}</td>
                <td className="muted">{v.type_name}</td>
                <td className="debug-value">
                  {editing === keyOf(v) ? (
                    <span className="row tight">
                      <input
                        autoFocus
                        aria-label={t("New value of {variable}", { variable: v.name })}
                        data-field="debug-variable-input"
                        value={draft}
                        spellCheck={false}
                        onChange={(e) => setDraft(e.target.value)}
                        onKeyDown={(e) => {
                          e.stopPropagation();
                          if (e.key === "Enter") save(v, draft);
                          if (e.key === "Escape") setEditing(null);
                        }}
                      />
                      <button type="button" data-field="debug-variable-null" onClick={() => save(v, null)}>
                        NULL
                      </button>
                    </span>
                  ) : (
                    <button
                      className="link debug-value-text"
                      disabled={!editable || v.constant}
                      title={v.constant ? t("A constant cannot be changed") : t("Change the value")}
                      onClick={() => {
                        setDraft(v.value === "NULL" ? "" : v.value);
                        setEditing(keyOf(v));
                      }}
                    >
                      {v.value}
                    </button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Outcome({ outcome }: { outcome: DebugOutcome }) {
  return (
    <section className="debug-outcome" data-field="debug-outcome">
      {outcome.aborted ? (
        <div className="notice small" data-field="debug-result">
          {t("The call was stopped; what it changed is rolled back.")}
        </div>
      ) : outcome.error ? (
        <div className="notice error small" data-field="debug-result">
          {outcome.error}
        </div>
      ) : (
        <div className="notice ok small" data-field="debug-result">
          {t("The call returned.")}
        </div>
      )}
      {outcome.columns.length > 0 && !outcome.aborted && (
        <table className="meta-table debug-result-rows" data-field="debug-rows">
          <thead>
            <tr>
              {outcome.columns.map((c, i) => (
                <th key={i}>{c}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {outcome.rows.map((row, i) => (
              <tr key={i}>
                {row.map((cell, j) => (
                  <td key={j} className={cell === null ? "muted" : undefined}>
                    {cell ?? "NULL"}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {outcome.truncated && <p className="muted small">{t("Only the first {n} rows are shown.", { n: outcome.rows.length })}</p>}
      {outcome.messages.length > 0 && (
        <pre className="admin-statement debug-messages" data-field="debug-messages">
          {outcome.messages.join("\n")}
        </pre>
      )}
    </section>
  );
}
