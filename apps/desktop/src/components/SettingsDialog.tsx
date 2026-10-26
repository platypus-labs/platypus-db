import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { allActions } from "../actions";
import { AgentsPanel } from "../agents/AgentsPanel";
import { toCommandError } from "../api";
import { type SettingDef, getSetting, getStored, setSetting, setStored, settingDefs, useSettingsVersion } from "../settings";
import { ShortcutsPanel, actionMatches } from "../shell/ShortcutsPanel";
import { Icon } from "../ui/Icon";
import { fold, startsWord } from "../ui/textSearch";
import { DriversPanel } from "../drivers/DriversPanel";
import { MasterPasswordPanel } from "./MasterPassword";
import { msg, t } from "../i18n";

type Value = boolean | number | string;

/** The pages after the declared settings' sections, each with its own panel; `keywords`
 *  (English) are found by the search too, and a panel with `contains` is also found by its
 *  content, which it then filters. */
const PANELS: {
  title: string;
  keywords: string;
  contains?: (needle: string) => boolean;
  render: (search: string) => ReactNode;
}[] = [
  { title: msg("Master password"), keywords: "password encryption credentials keyring", render: () => <MasterPasswordPanel /> },
  { title: msg("Agents"), keywords: "MCP AI assistant", render: () => <AgentsPanel /> },
  { title: msg("Drivers"), keywords: "ODBC ADBC Oracle Instant Client download", render: () => <DriversPanel /> },
  {
    title: msg("Keyboard shortcuts"),
    keywords: "keys keyboard",
    contains: (needle) => allActions().some((a) => actionMatches(a, needle)),
    render: (search) => <ShortcutsPanel search={search} />,
  },
];

/** The section shown last, opened again next time; stored without a Settings entry. */
const SECTION_KEY = "ui.settings_section";

/** Whether `needle` (folded) starts a word of any of `texts`, in English or translated. */
function found(needle: string, ...texts: (string | undefined)[]): boolean {
  return texts.some((s) => !!s && (startsWord(s, needle) || startsWord(t(s), needle)));
}

function settingFound(needle: string, def: SettingDef<Value>): boolean {
  const c = def.control;
  const options = c.type === "select" || c.type === "swatches" ? c.options.map((o) => o.label) : [];
  return found(needle, def.label, def.description, def.key, ...options);
}

/** Every declared setting, by section, then the other pages: one tab each, chosen in the list
 *  on the left. While the search has text, the matching settings of every section show
 *  instead, grouped by section, and the list jumps to a group. Changes apply and save at once. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  useSettingsVersion();
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const needle = fold(search.trim());
  const results = useRef<HTMLDivElement>(null);
  const sections = new Map<string, SettingDef<Value>[]>();
  for (const def of settingDefs()) {
    sections.set(def.section, [...(sections.get(def.section) ?? []), def]);
  }
  const names = [...sections.keys(), ...PANELS.map((p) => p.title)];
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
  const row = (def: SettingDef<Value>) => <SettingRow key={def.key} def={def} onSave={(v) => save(def, v)} />;

  // The search's groups: a section found by its name shows whole, otherwise its matching
  // settings. A panel found by its title shows whole, one found by its content filtered; the
  // others are hidden, not unmounted, so they keep what was typed in them and don't load again
  // while the query changes. Going from the tabs to the search and back does mount them anew:
  // the tab's panel and the search's are separate views.
  const groups = [...sections]
    .map(([section, defs]) => [section, found(needle, section) ? defs : defs.filter((d) => settingFound(needle, d))] as const)
    .filter(([, defs]) => defs.length > 0);
  const panels = PANELS.map((p) => {
    if (found(needle, p.title, p.keywords)) return { ...p, shown: true, search: "" };
    return { ...p, shown: !!p.contains?.(needle), search: needle };
  });
  const titles = [...groups.map(([section]) => section), ...panels.filter((p) => p.shown).map((p) => p.title)];
  const panel = PANELS.find((p) => p.title === current);

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
                results.current?.scrollTo({ top: 0 });
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
        {needle ? (
          <div className="settings-layout">
            <nav className="settings-nav" aria-label={t("Settings")}>
              {titles.map((section) => (
                <button
                  key={section}
                  type="button"
                  onClick={() =>
                    results.current
                      ?.querySelector(`[data-section="${CSS.escape(section)}"]`)
                      ?.scrollIntoView({ block: "start", behavior: "smooth" })
                  }
                >
                  {t(section)}
                </button>
              ))}
            </nav>
            <div className="settings-body settings-results" ref={results}>
              {groups.map(([section, defs]) => (
                <section key={section} data-section={section}>
                  <h3>{t(section)}</h3>
                  {defs.map(row)}
                </section>
              ))}
              {panels.map((p) => (
                <section key={p.title} data-section={p.title} hidden={!p.shown}>
                  <h3>{t(p.title)}</h3>
                  {p.render(p.search)}
                </section>
              ))}
              {titles.length === 0 && <p className="muted settings-none">{t("No settings match.")}</p>}
            </div>
          </div>
        ) : (
          <div className="settings-layout">
            <div className="settings-nav" role="tablist" aria-orientation="vertical" aria-label={t("Settings")}>
              {names.map((name, i) => (
                <button
                  key={name}
                  type="button"
                  role="tab"
                  id={`settings-tab-${i}`}
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
              // Its first content, the heading, takes no focus: the panel does, after the tabs.
              tabIndex={0}
            >
              <h3>{t(current)}</h3>
              {sections.get(current)?.map(row)}
              {panel?.render("")}
            </section>
          </div>
        )}
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
          // A new field when the value changes elsewhere (Reset): blur would save the old one.
          key={value as string}
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
  // Follows the value when it changes elsewhere (Reset): blur would save the old one.
  useEffect(() => setText(String(props.value)), [props.value]);
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
