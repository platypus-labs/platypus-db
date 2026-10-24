import { useState } from "react";

interface Props {
  /** The statement, shown for context. */
  sql: string;
  /** Keys to ask, in order: names, and `?1`, `?2`… for positional parameters. */
  keys: string[];
  /** Keys whose value goes in as typed (`${name}`); the others become literals. */
  raw: Set<string>;
  initial: Record<string, string>;
  onRun: (values: Record<string, string>) => void;
  onCancel: () => void;
}

/** Asks the values of a statement's parameters before it runs. */
export function ParamsDialog({ sql, keys, raw, initial, onRun, onCancel }: Props) {
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
        <h2>Parameters</h2>
        <pre className="params-sql">{sql}</pre>
        <p className="muted small">
          Numbers, <code>NULL</code>, <code>true</code> and <code>'quoted'</code> text go in as typed, anything else
          as a string. <code>{"${name}"}</code> variables go in as typed.
        </p>
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
            Cancel
          </button>
          <button type="submit" className="primary">
            Run
          </button>
        </div>
      </form>
    </div>
  );
}
