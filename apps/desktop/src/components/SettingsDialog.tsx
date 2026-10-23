import { useState } from "react";
import { toCommandError } from "../api";
import { type SettingDef, getSetting, setSetting, settingDefs, useSettingsVersion } from "../settings";

type Value = boolean | number | string;

/** Every declared setting, by section. Changes apply and save at once. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  useSettingsVersion();
  const [error, setError] = useState<string | null>(null);
  const sections = new Map<string, SettingDef<Value>[]>();
  for (const def of settingDefs()) {
    sections.set(def.section, [...(sections.get(def.section) ?? []), def]);
  }
  const save = (def: SettingDef<Value>, value: Value | null) => {
    setError(null);
    setSetting(def, value).catch((e: unknown) => setError(`${def.label}: ${toCommandError(e).message}`));
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal settings"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2>Settings</h2>
        {error && <div className="notice error small">{error}</div>}
        {[...sections].map(([section, defs]) => (
          <section key={section}>
            <h3>{section}</h3>
            {defs.map((def) => (
              <SettingRow key={def.key} def={def} onSave={(v) => save(def, v)} />
            ))}
          </section>
        ))}
        <div className="actions">
          <span className="muted small">Saved in settings.toml</span>
          <span className="spacer" />
          <button className="primary" autoFocus onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}

function SettingRow({ def, onSave }: { def: SettingDef<Value>; onSave: (value: Value | null) => void }) {
  const value = getSetting(def);
  const changed = value !== def.default;
  const c = def.control;
  let input: React.ReactNode;
  switch (c.type) {
    case "boolean":
      input = <input type="checkbox" checked={value as boolean} onChange={(e) => onSave(e.target.checked)} />;
      break;
    case "number":
      input = (
        <NumberInput
          value={value as number}
          min={c.min}
          max={c.max}
          step={c.step}
          onCommit={(n) => onSave(n)}
        />
      );
      break;
    case "text":
      input = (
        <input
          defaultValue={value as string}
          placeholder={c.placeholder}
          onBlur={(e) => e.target.value !== value && onSave(e.target.value)}
        />
      );
      break;
    case "select":
      input = (
        <select value={value as string} onChange={(e) => onSave(e.target.value)}>
          {c.options.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      );
      break;
  }
  return (
    <label className="setting">
      <span>
        {def.label}
        {def.description && <span className="muted small">{def.description}</span>}
      </span>
      {input}
      <button
        type="button"
        className="link"
        style={{ visibility: changed ? "visible" : "hidden" }}
        title={`Default: ${String(def.default)}`}
        onClick={() => onSave(null)}
      >
        Reset
      </button>
    </label>
  );
}

/** Saves on blur or Enter, and only valid numbers within bounds. */
function NumberInput(props: { value: number; min?: number; max?: number; step?: number; onCommit: (n: number) => void }) {
  const [text, setText] = useState(String(props.value));
  const commit = () => {
    const n = Number(text);
    const valid =
      text.trim() !== "" &&
      Number.isFinite(n) &&
      (props.min === undefined || n >= props.min) &&
      (props.max === undefined || n <= props.max);
    if (valid && n !== props.value) props.onCommit(n);
    else setText(String(props.value));
  };
  return (
    <input
      type="number"
      value={text}
      min={props.min}
      max={props.max}
      step={props.step}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}
