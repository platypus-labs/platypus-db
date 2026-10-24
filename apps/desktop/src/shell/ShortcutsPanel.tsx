import { useEffect, useState } from "react";
import {
  type Action,
  type ActionScope,
  allActions,
  keyCapture,
  setShortcut,
  shortcutChanged,
  shortcutLabel,
  shortcutOf,
  shortcutsOf,
} from "../actions";
import { toCommandError } from "../api";
import { useSettingsVersion } from "../settings";

const SCOPES: Record<string, string> = {
  global: "Everywhere",
  grid: "Results grid",
  "navigator.connection": "Navigator: connection",
  "navigator.schema": "Navigator: schema",
  "navigator.object": "Navigator: table or routine",
  "navigator.column": "Navigator: column",
};

const scopeLabel = (scope: ActionScope) => SCOPES[scope] ?? scope;

/** Every action with its shortcut: click one, press the new keys; Backspace removes it. */
export function ShortcutsPanel() {
  useSettingsVersion();
  const [recording, setRecording] = useState<string | null>(null);
  useEffect(() => {
    keyCapture.active = recording !== null;
    return () => {
      keyCapture.active = false;
    };
  }, [recording]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<string | null>(null);
  const needle = filter.trim().toLowerCase();
  const actions = allActions()
    .filter((a) => !needle || a.title.toLowerCase().includes(needle) || a.id.includes(needle))
    .sort((a, b) => a.scope.localeCompare(b.scope) || a.title.localeCompare(b.title));

  /** Other actions of the same scope that answer to `shortcut`. */
  const clashes = (action: Action, shortcut: string) =>
    allActions().filter(
      (a) => a.id !== action.id && a.scope === action.scope && shortcutsOf(a).some((s) => s.toLowerCase() === shortcut.toLowerCase()),
    );

  function save(action: Action, shortcut: string | null) {
    setError(null);
    setShortcut(action, shortcut).catch((e: unknown) => setError(toCommandError(e).message));
  }

  function record(action: Action, e: React.KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return setRecording(null);
    if (e.key === "Backspace" || e.key === "Delete") {
      save(action, "");
      return setRecording(null);
    }
    const shortcut = shortcutOf(e.nativeEvent);
    if (!shortcut) return;
    save(action, shortcut);
    setRecording(null);
  }

  let scope: string | null = null;
  return (
    <div className="shortcuts">
      <input
        className="shortcuts-filter"
        placeholder="Filter actions…"
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      {error && <div className="notice error small">{error}</div>}
      <table>
        <tbody>
          {actions.flatMap((a) => {
            const rows = [];
            if (a.scope !== scope) {
              scope = a.scope;
              rows.push(
                <tr key={`scope:${a.scope}`} className="shortcuts-scope">
                  <th colSpan={3}>{scopeLabel(a.scope)}</th>
                </tr>,
              );
            }
            const label = shortcutLabel(a);
            const clash = label ? clashes(a, shortcutsOf(a)[0]!) : [];
            rows.push(
              <tr key={a.id}>
                <td title={a.id}>{a.title}</td>
                <td>
                  <button
                    type="button"
                    className={`shortcut-key${recording === a.id ? " recording" : ""}`}
                    onClick={() => setRecording(a.id)}
                    onKeyDown={(e) => recording === a.id && record(a, e)}
                    onBlur={() => recording === a.id && setRecording(null)}
                    title="Click, then press the keys; Backspace removes the shortcut"
                  >
                    {recording === a.id ? "Press keys…" : (label ?? "—")}
                  </button>
                  {clash.length > 0 && (
                    <span className="error-text small" title={clash.map((c) => c.title).join(", ")}>
                      {" "}
                      also {clash[0]!.title}
                    </span>
                  )}
                </td>
                <td>
                  {shortcutChanged(a) && (
                    <button type="button" className="link" onClick={() => save(a, null)} title="Back to the default">
                      Reset
                    </button>
                  )}
                </td>
              </tr>,
            );
            return rows;
          })}
        </tbody>
      </table>
    </div>
  );
}
