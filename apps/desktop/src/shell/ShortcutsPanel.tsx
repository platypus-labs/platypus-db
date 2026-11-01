import { useEffect, useState } from "react";
import {
  type Action,
  type ActionScope,
  allActions,
  bindableActions,
  isMac,
  keyCapture,
  setShortcut,
  shortcutChanged,
  shortcutLabel,
  shortcutOf,
  shortcutsOf,
} from "../actions";
import { toCommandError } from "../api";
import { useSettingsVersion } from "../settings";
import { altGraphText, removesShortcut, typesText } from "../shortcutKeys";
import { msg, t } from "../i18n";
import { fold } from "../ui/textSearch";

const SCOPES: Record<string, string> = {
  global: msg("Everywhere"),
  grid: msg("Results grid"),
  editor: msg("SQL editor"),
  tab: msg("Tab"),
  "navigator.list": msg("Navigator: connection list"),
  "navigator.connection": msg("Navigator: connection"),
  "navigator.schema": msg("Navigator: schema"),
  "navigator.object": msg("Navigator: table or routine"),
  "navigator.column": msg("Navigator: column"),
  "navigator.index": msg("Navigator: index"),
  "navigator.database": msg("Navigator: database"),
  "navigator.folder": msg("Navigator: folder"),
  project: msg("Project"),
  "project.folder": msg("Project folder"),
  "project.add": msg("Project rail: add"),
  script: msg("Saved script"),
  "dashboard.widget": msg("Dashboard widget"),
};

const scopeLabel = (scope: ActionScope) => t(SCOPES[scope] ?? scope);

/** Whether an action's title (in English or translated) or id contains `needle`, case and
 *  accents ignored. */
export function actionMatches(a: Action, needle: string): boolean {
  const n = fold(needle);
  return !n || fold(t(a.title)).includes(n) || fold(a.title).includes(n) || a.id.includes(n);
}

/** Every action with its shortcut: click one, press the new keys; Backspace removes it.
 *  `search`, lowercase, is the Settings' own search, applied with the panel's filter. */
export function ShortcutsPanel({ search = "" }: { search?: string }) {
  useSettingsVersion();
  const [recording, setRecording] = useState<string | null>(null);
  useEffect(() => {
    keyCapture.active = recording !== null;
    return () => {
      keyCapture.active = false;
    };
  }, [recording]);
  const [filter, setFilter] = useState("");
  const [error, setError] = useState<{ id: string; text: string } | null>(null);
  const needle = filter.trim();
  const actions = bindableActions()
    .filter((a) => actionMatches(a, needle) && actionMatches(a, search))
    .sort((a, b) => a.scope.localeCompare(b.scope) || t(a.title).localeCompare(t(b.title)));

  /** Other actions that answer to `shortcut` where `action` does: of the same scope, or global
   *  ones, whose keys are taken before any other scope sees them. */
  const clashes = (action: Action, shortcut: string) =>
    allActions().filter(
      (a) =>
        a.id !== action.id &&
        (a.scope === action.scope || a.scope === "global" || action.scope === "global") &&
        shortcutsOf(a).some((s) => s.toLowerCase() === shortcut.toLowerCase()),
    );

  function save(action: Action, shortcut: string | null) {
    setError(null);
    setShortcut(action, shortcut).catch((e: unknown) => setError({ id: action.id, text: toCommandError(e).message }));
  }

  function record(action: Action, e: React.KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") return setRecording(null);
    if (removesShortcut(e)) {
      save(action, "");
      return setRecording(null);
    }
    // AltGr types the character: as a shortcut it would never run.
    if (altGraphText(e.nativeEvent, isMac)) return setError({ id: action.id, text: t("A key typed with AltGr is a character, not a shortcut.") });
    const shortcut = shortcutOf(e.nativeEvent);
    if (!shortcut) return;
    // A letter alone would run the action instead of being typed: still recording.
    if (typesText(shortcut)) return setError({ id: action.id, text: t("A key that types a character needs Ctrl or Alt.") });
    save(action, shortcut);
    setRecording(null);
  }

  let scope: string | null = null;
  return (
    <div className="shortcuts">
      <input
        className="shortcuts-filter"
        placeholder={t("Filter actions…")}
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
      />
      <p className="muted small">{t("Click a shortcut, then press the new keys. Backspace removes it, Esc cancels.")}</p>
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
                <td title={a.id}>{t(a.title)}</td>
                <td>
                  <button
                    type="button"
                    className={`shortcut-key${recording === a.id ? " recording" : ""}`}
                    onClick={() => {
                      setError(null);
                      setRecording(a.id);
                    }}
                    onKeyDown={(e) => recording === a.id && record(a, e)}
                    onBlur={() => recording === a.id && setRecording(null)}
                    title={t("Click, then press the keys; Backspace removes the shortcut")}
                  >
                    {recording === a.id ? t("Press keys…") : (label ?? "—")}
                  </button>
                  {error?.id === a.id && (
                    <div className="error-text small" role="alert" data-field="shortcut-error">
                      {error.text}
                    </div>
                  )}
                  {clash.length > 0 && (
                    <span className="error-text small" title={clash.map((c) => t(c.title)).join(", ")}>
                      {" "}
                      {t("also {action}", { action: t(clash[0]!.title) })}
                    </span>
                  )}
                </td>
                <td>
                  {shortcutChanged(a) && (
                    <button type="button" className="link" onClick={() => save(a, null)} title={t("Back to the default")}>
                      {t("Reset")}
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
