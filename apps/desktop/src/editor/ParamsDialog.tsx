import { useState } from "react";
import { t, tNodes } from "../i18n";

interface Props {
  /** The statement, shown for context. */
  sql: string;
  /** Keys to ask, in order: names, and `?1`, `?2`… for positional parameters. */
  keys: string[];
  /** Keys whose value goes in as typed (`${name}`); the others become literals. */
  raw: Set<string>;
  initial: Record<string, string>;
  /** Keys whose initial value came with a shared query: the user is told to check them. */
  suggested?: string[];
  onRun: (values: Record<string, string>) => void;
  onCancel: () => void;
}

/** Asks the values of a statement's parameters before it runs. */
export function ParamsDialog({ sql, keys, raw, initial, suggested = [], onRun, onCancel }: Props) {
  const [values, setValues] = useState(() => Object.fromEntries(keys.map((k) => [k, initial[k] ?? ""])));
  return (
    <div className="modal-backdrop" onMouseDown={onCancel}>
      <form
        className="modal params-dialog"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onCancel()}
        onSubmit={(e) => {
          e.preventDefault();
          onRun(values);
        }}
      >
        <h2>{t("Parameters")}</h2>
        <pre className="params-sql">{sql}</pre>
        <p className="muted small">
          {tNodes("Numbers, {null}, {true} and {quoted} text go in as typed, anything else as a string. {raw} variables go in as typed.", {
            null: <code>NULL</code>,
            true: <code>true</code>,
            quoted: <code>'quoted'</code>,
            raw: <code>{"${name}"}</code>,
          })}
        </p>
        {suggested.length > 0 && (
          <p className="notice warn" data-field="suggested_values">
            {tNodes("The shared query came with these values: check them before running. {raw} values go into the SQL as they are.", {
              raw: <code>{"${name}"}</code>,
            })}
          </p>
        )}
        {keys.map((k, i) => (
          <label className="field" key={k}>
            <span>{raw.has(k) ? `\${${k}}` : k.startsWith("?") ? k : `:${k}`}</span>
            <input
              autoFocus={i === 0}
              value={values[k] ?? ""}
              onChange={(e) => setValues((v) => ({ ...v, [k]: e.target.value }))}
            />
          </label>
        ))}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onCancel}>
            {t("Cancel")}
          </button>
          <button type="submit" className="primary">
            {t("Run")}
          </button>
        </div>
      </form>
    </div>
  );
}
