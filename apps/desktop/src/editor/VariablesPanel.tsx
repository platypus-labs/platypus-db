import { useState } from "react";

interface Props {
  variables: Record<string, string>;
  onChange: (variables: Record<string, string>) => void;
  onClose: () => void;
}

/** The tab's variables: values of `${name}` and `:name`, set here, by `@set` or by the
 *  parameters dialog. */
export function VariablesPanel({ variables, onChange, onClose }: Props) {
  const [name, setName] = useState("");
  const entries = Object.entries(variables).sort(([a], [b]) => a.localeCompare(b));
  const set = (key: string, value: string | null) => {
    const next = { ...variables };
    if (value === null) delete next[key];
    else next[key] = value;
    onChange(next);
  };
  return (
    <aside className="history side-panel variables-panel">
      <div className="history-header">
        <strong>Variables</strong>
        <span className="spacer" />
        <button className="icon" title="Close" onClick={onClose}>
          ×
        </button>
      </div>
      <p className="muted small side-panel-hint">
        <code>{"${name}"}</code> goes in as typed, <code>:name</code> as a literal. <code>@set name = value</code> sets
        one from a script.
      </p>
      <ul className="history-list">
        {entries.map(([key, value]) => (
          <li key={key} className="variable-row">
            <span className="variable-name" title={key}>
              {key}
            </span>
            <input aria-label={`Value of ${key}`} value={value} onChange={(e) => set(key, e.target.value)} />
            <button className="icon" title="Delete" onClick={() => set(key, null)}>
              ×
            </button>
          </li>
        ))}
        {entries.length === 0 && <li className="muted history-empty">No variables yet</li>}
      </ul>
      <form
        className="variable-add"
        onSubmit={(e) => {
          e.preventDefault();
          const key = name.trim();
          if (!/^[\w.]+$/.test(key)) return;
          set(key, variables[key] ?? "");
          setName("");
        }}
      >
        <input placeholder="New variable" value={name} onChange={(e) => setName(e.target.value)} />
        <button type="submit">Add</button>
      </form>
    </aside>
  );
}
