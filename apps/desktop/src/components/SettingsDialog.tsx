import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { AgentsPanel } from "../agents/AgentsPanel";
import { toCommandError } from "../api";
import { type SettingDef, getSetting, getStored, setSetting, setStored, settingDefs, useSettingsVersion } from "../settings";
import { ShortcutsPanel } from "../shell/ShortcutsPanel";
import { DriversPanel } from "../drivers/DriversPanel";
import { MasterPasswordPanel } from "./MasterPassword";
import { msg, t } from "../i18n";

/** The pages after the declared settings' sections, each with its own panel. */
const PANELS: [string, () => ReactNode][] = [
  [msg("Master password"), () => <MasterPasswordPanel />],
  [msg("Agents"), () => <AgentsPanel />],
  [msg("Drivers"), () => <DriversPanel />],
  [msg("Keyboard shortcuts"), () => <ShortcutsPanel />],
];

/** The section shown last, opened again next time; stored without a Settings entry. */
const SECTION_KEY = "ui.settings_section";

type Value = boolean | number | string;

/** Every declared setting, by section, then the other pages: one tab each, chosen in the list
 *  on the left. Changes apply and save at once. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  useSettingsVersion();
  const [error, setError] = useState<string | null>(null);
  const sections = new Map<string, SettingDef<Value>[]>();
  for (const def of settingDefs()) {
    sections.set(def.section, [...(sections.get(def.section) ?? []), def]);
  }
  const names = [...sections.keys(), ...PANELS.map(([name]) => name)];
  const [chosen, setChosen] = useState(() => getStored(SECTION_KEY));
  const current = typeof chosen === "string" && names.includes(chosen) ? chosen : names[0]!;
  // Stored once the dialog closes: a write on each tab change would save the file and
  // refresh the whole app at every arrow key.
  const shown = useRef(current);
  shown.current = current;
  useEffect(
    () => () => {
      if (shown.current === getStored(SECTION_KEY)) return;
      setStored(SECTION_KEY, shown.current).catch((e: unknown) => console.warn("settings section", e));
    },
    [],
  );
  const onTabKey = (e: KeyboardEvent<HTMLElement>) => {
    // From the focused tab, which a click elsewhere may have left apart from the selected one.
    const at = names.indexOf(e.currentTarget.dataset.section ?? current);
    const moves: Record<string, number> = { ArrowDown: at + 1, ArrowUp: at - 1 + names.length, Home: 0, End: names.length - 1 };
    const to = moves[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const next = names[to % names.length]!;
    setChosen(next);
    e.currentTarget.parentElement?.querySelector<HTMLElement>(`[data-section="${CSS.escape(next)}"]`)?.focus();
  };
  const save = (def: SettingDef<Value>, value: Value | null) => {
    setError(null);
    setSetting(def, value).catch((e: unknown) => setError(`${t(def.label)}: ${toCommandError(e).message}`));
  };
  const defs = sections.get(current);
  const panel = PANELS.find(([name]) => name === current)?.[1];

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal settings"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2>{t("Settings")}</h2>
        {error && <div className="notice error small">{error}</div>}
        <div className="settings-layout">
          <div className="settings-nav" role="tablist" aria-orientation="vertical" aria-label={t("Settings")}>
            {names.map((name) => (
              <button
                key={name}
                type="button"
                role="tab"
                id={`settings-tab-${names.indexOf(name)}`}
                data-section={name}
                aria-selected={name === current}
                aria-controls="settings-panel"
                tabIndex={name === current ? 0 : -1}
                onClick={() => setChosen(name)}
                onKeyDown={onTabKey}
              >
                {t(name)}
              </button>
            ))}
          </div>
          <section
            // A new panel for each tab, scrolled to its top.
            key={current}
            id="settings-panel"
            className="settings-body"
            role="tabpanel"
            aria-labelledby={`settings-tab-${names.indexOf(current)}`}
          >
            <h3>{t(current)}</h3>
            {defs?.map((def) => <SettingRow key={def.key} def={def} onSave={(v) => save(def, v)} />)}
            {panel?.()}
          </section>
        </div>
        <div className="actions">
          <span className="muted small">{t("Saved in settings.toml")}</span>
          <span className="spacer" />
          <button className="primary" autoFocus onClick={onClose}>
            {t("Done")}
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
          {!hex && <span className="muted small">{t("Theme's")}</span>}
          <input
            type="color"
            value={hex ?? "#808080"}
            title={hex ?? t("The theme's color")}
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
              {t(o.label)}
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
              {t(o.label)}
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
      title={c.type === "color" ? t("Back to the theme's color") : t("Default: {value}", { value: String(def.default) })}
      onClick={() => onSave(null)}
    >
      {t("Reset")}
    </button>
  );
  const text = (
    <span>
      {t(def.label)}
      {def.description && <span className="muted small">{t(def.description)}</span>}
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
