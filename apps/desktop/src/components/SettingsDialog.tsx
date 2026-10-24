import { useRef, useState } from "react";
import { toCommandError } from "../api";
import { type SettingDef, getSetting, setSetting, settingDefs, useSettingsVersion } from "../settings";
import { ShortcutsPanel } from "../shell/ShortcutsPanel";

const SHORTCUTS = "Keyboard shortcuts";

type Value = boolean | number | string;

/** Every declared setting, by section, then the keyboard shortcuts; the list on the left
 *  jumps to a section. Changes apply and save at once. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  useSettingsVersion();
  const [error, setError] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);
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
        <div className="settings-layout">
          <nav className="settings-nav">
            {[...sections.keys(), SHORTCUTS].map((section) => (
              <button
                key={section}
                type="button"
                onClick={() =>
                  body.current
                    ?.querySelector(`[data-section="${CSS.escape(section)}"]`)
                    ?.scrollIntoView({ block: "start", behavior: "smooth" })
                }
              >
                {section}
              </button>
            ))}
          </nav>
          <div className="settings-body" ref={body}>
            {[...sections].map(([section, defs]) => (
              <section key={section} data-section={section}>
                <h3>{section}</h3>
                {defs.map((def) => (
                  <SettingRow key={def.key} def={def} onSave={(v) => save(def, v)} />
                ))}
              </section>
            ))}
            <section data-section={SHORTCUTS}>
              <h3>{SHORTCUTS}</h3>
              <ShortcutsPanel />
            </section>
          </div>
        </div>
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
    case "color": {
      const hex = /^#[0-9a-f]{6}$/i.test(value as string) ? (value as string) : null;
      input = (
        <div className="color-control">
          {!hex && <span className="muted small">Theme's</span>}
          <input
            type="color"
            value={hex ?? "#808080"}
            title={hex ?? "The theme's color"}
            onChange={(e) => onSave(e.target.value)}
          />
        </div>
      );
      break;
    }
    case "swatches":
      input = (
        <div className="swatches-control" role="radiogroup">
          {c.options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={value === o.value}
              className={`theme-card${value === o.value ? " selected" : ""}`}
              onClick={() => onSave(o.value)}
            >
              <span className="theme-card-colors">
                {o.colors.map((color, i) => (
                  <span key={i} style={{ background: color }} />
                ))}
              </span>
              {o.label}
            </button>
          ))}
        </div>
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
  const reset = (
    <button
      type="button"
      className="link"
      style={{ visibility: changed ? "visible" : "hidden", gridColumn: c.type === "swatches" ? 3 : undefined }}
      title={c.type === "color" ? "Back to the theme's color" : `Default: ${String(def.default)}`}
      onClick={() => onSave(null)}
    >
      Reset
    </button>
  );
  const text = (
    <span>
      {def.label}
      {def.description && <span className="muted small">{def.description}</span>}
    </span>
  );
  // Cards take a row of their own, under the label.
  if (c.type === "swatches") {
    return (
      <div className="setting">
        {text}
        {reset}
        {input}
      </div>
    );
  }
  return (
    <label className="setting">
      {text}
      {input}
      {reset}
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
