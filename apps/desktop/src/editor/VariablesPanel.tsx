import { useState } from "react";
import { t, tNodes } from "../i18n";
import { EmptyState } from "../ui/State";
import { Icon } from "../ui/Icon";

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
        <strong>{t("Variables")}</strong>
        <span className="spacer" />
        <button className="icon" title={t("Close")} aria-label={t("Close")} onClick={onClose}>
          <Icon name="close" />
        </button>
      </div>
      <p className="muted small side-panel-hint">
        {tNodes("{raw} goes in as typed, {literal} as a literal. {set} sets one from a script.", {
          raw: <code>{"${name}"}</code>,
          literal: <code>:name</code>,
          set: <code>@set name = value</code>,
        })}
      </p>
      <ul className="history-list">
        {entries.map(([key, value]) => (
          <li key={key} className="variable-row">
            <span className="variable-name" title={key}>
              {key}
            </span>
            <input aria-label={t("Value of {name}", { name: key })} value={value} onChange={(e) => set(key, e.target.value)} />
            <button className="icon" title={t("Delete")} aria-label={t("Delete")} onClick={() => set(key, null)}>
              <Icon name="close" />
            </button>
          </li>
        ))}
        {entries.length === 0 && <li className="history-empty"><EmptyState compact title={t("No variables yet")} /></li>}
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
        <input placeholder={t("New variable")} aria-label={t("New variable")} value={name} onChange={(e) => setName(e.target.value)} />
        <button type="submit">{t("Add")}</button>
      </form>
    </aside>
  );
}
