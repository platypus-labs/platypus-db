import { type ReactNode, useRef, useState } from "react";
import { allActions } from "../actions";
import { AgentsPanel } from "../agents/AgentsPanel";
import { toCommandError } from "../api";
import { type SettingDef, getSetting, setSetting, settingDefs, useSettingsVersion } from "../settings";
import { ShortcutsPanel, actionMatches } from "../shell/ShortcutsPanel";
import { Icon } from "../ui/Icon";
import { DriversPanel } from "../drivers/DriversPanel";
import { MasterPasswordPanel } from "./MasterPassword";
import { msg, t } from "../i18n";

const SHORTCUTS = msg("Keyboard shortcuts");
const SECURITY = msg("Master password");
const AGENTS = msg("Agents");
const DRIVERS = msg("Drivers");

type Value = boolean | number | string;

/** The sections after the declared settings; `keywords` (English) are found by the search
 *  too, and the shortcuts are also found by their actions. */
const PANELS: { title: string; keywords: string; render: (search: string) => ReactNode }[] = [
  { title: SECURITY, keywords: "password encryption credentials keyring", render: () => <MasterPasswordPanel /> },
  { title: AGENTS, keywords: "MCP AI assistant", render: () => <AgentsPanel /> },
  { title: DRIVERS, keywords: "ODBC ADBC Oracle Instant Client download", render: () => <DriversPanel /> },
  { title: SHORTCUTS, keywords: "keys keyboard", render: (search) => <ShortcutsPanel search={search} /> },
];

/** Whether any of `texts`, in English or translated, contains `needle` (lowercase). */
function found(needle: string, ...texts: (string | undefined)[]): boolean {
  return texts.some((s) => !!s && (s.toLowerCase().includes(needle) || t(s).toLowerCase().includes(needle)));
}

function settingFound(needle: string, def: SettingDef<Value>): boolean {
  const c = def.control;
  const options = c.type === "select" || c.type === "swatches" ? c.options.map((o) => o.label) : [];
  return found(needle, def.label, def.description, def.key, ...options);
}

/** Every declared setting, by section, then the keyboard shortcuts; the list on the left
 *  jumps to a section, the search keeps only the settings that match. Changes apply and
 *  save at once. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  useSettingsVersion();
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const needle = search.trim().toLowerCase();
  const body = useRef<HTMLDivElement>(null);
  const sections = new Map<string, SettingDef<Value>[]>();
  for (const def of settingDefs()) {
    if (needle && !found(needle, def.section) && !settingFound(needle, def)) continue;
    sections.set(def.section, [...(sections.get(def.section) ?? []), def]);
  }
  // A panel found by its title shows whole; the shortcuts also by their actions, filtered.
  const panels = PANELS.flatMap((p) => {
    if (!needle || found(needle, p.title, p.keywords)) return [{ ...p, search: "" }];
    const byAction = p.title === SHORTCUTS && allActions().some((a) => actionMatches(a, needle));
    return byAction ? [{ ...p, search: needle }] : [];
  });
  const titles = [...sections.keys(), ...panels.map((p) => p.title)];
  const save = (def: SettingDef<Value>, value: Value | null) => {
    setError(null);
    setSetting(def, value).catch((e: unknown) => setError(`${t(def.label)}: ${toCommandError(e).message}`));
  };

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <div
        className="modal settings"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <div className="settings-header">
          <h2>{t("Settings")}</h2>
          <div className="modal-search">
            <Icon name="search" className="modal-search-icon" />
            <input
              autoFocus
              placeholder={t("Search settings…")}
              aria-label={t("Search settings")}
              value={search}
              onChange={(e) => {
                setSearch(e.target.value);
                body.current?.scrollTo({ top: 0 });
              }}
              onKeyDown={(e) => {
                // Escape clears the search first, then closes.
                if (e.key !== "Escape" || !search) return;
                e.stopPropagation();
                setSearch("");
              }}
            />
          </div>
        </div>
        {error && <div className="notice error small">{error}</div>}
        <div className="settings-layout">
          <nav className="settings-nav">
            {titles.map((section) => (
              <button
                key={section}
                type="button"
                onClick={() =>
                  body.current
                    ?.querySelector(`[data-section="${CSS.escape(section)}"]`)
                    ?.scrollIntoView({ block: "start", behavior: "smooth" })
                }
              >
                {t(section)}
              </button>
            ))}
          </nav>
          <div className="settings-body" ref={body}>
            {[...sections].map(([section, defs]) => (
              <section key={section} data-section={section}>
                <h3>{t(section)}</h3>
                {defs.map((def) => (
                  <SettingRow key={def.key} def={def} onSave={(v) => save(def, v)} />
                ))}
              </section>
            ))}
            {panels.map((p) => (
              <section key={p.title} data-section={p.title}>
                <h3>{t(p.title)}</h3>
                {p.render(p.search)}
              </section>
            ))}
            {titles.length === 0 && <p className="muted settings-none">{t("No settings match.")}</p>}
          </div>
        </div>
        <div className="actions">
          <span className="muted small">{t("Saved in settings.toml")}</span>
          <span className="spacer" />
          <button className="primary" onClick={onClose}>
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
