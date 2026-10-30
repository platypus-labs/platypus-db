import { type KeyboardEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { allActions } from "../actions";
import { AgentsPanel } from "../agents/AgentsPanel";
import { ThemesPanel } from "../themes/ThemesPanel";
import { toCommandError } from "../api";
import { type SettingDef, type SettingPicture, getSetting, getStored, setSetting, setStored, settingDefs, useSettingsVersion } from "../settings";
import { ShortcutsPanel, actionMatches } from "../shell/ShortcutsPanel";
import { Icon, type IconName, type IconSet } from "../ui/Icon";
import { fold, startsWord } from "../ui/textSearch";
import { DriversPanel } from "../drivers/DriversPanel";
import { DiagnosticsPanel } from "../diagnostics/Diagnostics";
import { PluginsPanel } from "../plugins/PluginsPanel";
import { MasterPasswordPanel } from "./MasterPassword";
import { SyncPanel } from "../sync/SyncPanel";
import { msg, t } from "../i18n";
import { settingNumber } from "./settingValue";

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
  { title: msg("Themes"), keywords: "theme import export JSON colors appearance file", render: () => <ThemesPanel /> },
  { title: msg("Master password"), keywords: "password encryption credentials keyring", render: () => <MasterPasswordPanel /> },
  { title: msg("Sync"), keywords: "synchronize encrypted folder cloud passphrase backup computers devices share", render: () => <SyncPanel /> },
  { title: msg("Agents"), keywords: "MCP AI assistant", render: () => <AgentsPanel /> },
  { title: msg("Drivers"), keywords: "ODBC ADBC Oracle Instant Client download", render: () => <DriversPanel /> },
  { title: msg("Plugins"), keywords: "plugin WebAssembly WASM extension actions driver install", render: () => <PluginsPanel /> },
  { title: msg("Diagnostics"), keywords: "log logs crash report bug support zip", render: () => <DiagnosticsPanel /> },
  {
    title: msg("Keyboard shortcuts"),
    keywords: "keys keyboard",
    contains: (needle) => allActions().some((a) => actionMatches(a, needle)),
    render: (search) => <ShortcutsPanel search={search} />,
  },
];

/** The groups of the page list, in order; a page in none of them goes to the end of the last. */
const NAV_GROUPS: { label: string; pages: string[] }[] = [
  { label: msg("Interface"), pages: ["Appearance", "Navigator", "SQL editor", "Results", "Toolbar", "Tabs", "Themes", "Keyboard shortcuts"] },
  { label: msg("Data and safety"), pages: ["Safety", "Connections", "Performance", "Master password", "Sync", "Driver updates"] },
  { label: msg("Tools"), pages: ["Agents", "Drivers", "Plugins", "Diagnostics"] },
];

/** The icon and one line about each page, shown above its content. */
const PAGE_HEAD: Record<string, { icon: IconName; about: string }> = {
  "Appearance": { icon: "theme", about: msg("Theme, language, layout and icons.") },
  "Navigator": { icon: "sidebar", about: msg("How the navigator lists your databases and objects.") },
  "Driver updates": { icon: "refresh", about: msg("When PlatypusDB looks for newer versions of the downloaded drivers.") },
  "SQL editor": { icon: "sql", about: msg("Editing, completion and checking of SQL.") },
  "Results": { icon: "table", about: msg("How results are shown, formatted and exported.") },
  "Safety": { icon: "lock", about: msg("What PlatypusDB asks before changes, and what it blocks.") },
  "Connections": { icon: "database", about: msg("Checks on open connections.") },
  "Toolbar": { icon: "settings", about: msg("Which buttons the editor toolbar shows.") },
  "Performance": { icon: "activity", about: msg("When slow queries are marked.") },
  "Tabs": { icon: "split", about: msg("How open tabs are arranged.") },
  "Themes": { icon: "theme", about: msg("Import, export and choose the color themes.") },
  "Master password": { icon: "key", about: msg("Protects saved passwords with one password of yours.") },
  "Sync": { icon: "refresh", about: msg("Keep your settings and connections in step across computers.") },
  "Agents": { icon: "agent", about: msg("Connect assistants and tools that work with your databases.") },
  "Drivers": { icon: "database", about: msg("Database drivers installed and available to download.") },
  "Plugins": { icon: "extension", about: msg("Add-ons that extend PlatypusDB.") },
  "Diagnostics": { icon: "activity", about: msg("Logs and reports to help find and fix problems.") },
  "Keyboard shortcuts": { icon: "terminal", about: msg("Every command and its key combination.") },
};

/** Pages with enough settings to split: their sub-tabs, in order, each with the keys it holds
 *  (and a class for its body, laid out its own way). A setting in none of them goes to the last
 *  sub-tab. Pages not listed have a single body. */
const SUB_TABS: Record<string, { label: string; keys: string[]; className?: string }[]> = {
  "Appearance": [
    { label: msg("Theme"), keys: ["appearance.theme", "appearance.accent", "appearance.background", "appearance.sidebar", "appearance.text"] },
    { label: msg("Language and text"), keys: ["appearance.language", "appearance.editor_font_size", "appearance.grid_font_size"] },
    { label: msg("Density"), keys: ["appearance.zoom", "appearance.density", "appearance.radius"] },
    {
      label: msg("Layout"),
      keys: ["layout.navigator_side", "layout.results_position", "layout.tab_bar_position", "layout.side_panels_side"],
      className: "layout-choices",
    },
    { label: msg("Icons and animations"), keys: ["appearance.icon_set", "appearance.animations"] },
    { label: msg("Window"), keys: ["appearance.title_bar", "appearance.tray"] },
  ],
  "SQL editor": [
    { label: msg("Editing"), keys: ["editor.vim", "editor.spellcheck", "editor.lint"] },
    { label: msg("Formatting"), keys: ["editor.format_keyword_case", "editor.format_indent"] },
    { label: msg("Execution"), keys: ["editor.script_row_limit", "editor.notify_after_seconds"] },
  ],
  "Results": [
    {
      label: msg("Formats"),
      keys: ["results.date_format", "results.timestamp_format", "results.time_zone", "results.number_format", "results.relative_time"],
    },
    { label: msg("Display"), keys: ["grid.value_panel"] },
  ],
  "Safety": [
    {
      label: msg("Before changes"),
      keys: ["safety.unfiltered", "safety.confirm_production", "safety.transaction_warning_minutes", "safety.auto_limit", "agents.confirm_writes"],
    },
    { label: msg("Masking"), keys: ["safety.mask", "safety.mask_names", "safety.mask_types"] },
    { label: msg("Lock and backups"), keys: ["safety.lock_minutes", "safety.snapshots"] },
  ],
};

/** The sub-tabs of a page with its settings in them, or null for a page with a single body. */
function subTabsOf(page: string, defs: SettingDef<Value>[] | undefined) {
  const spec = SUB_TABS[page];
  if (!spec || !defs) return null;
  const groups = spec.map((g) => ({ label: g.label, className: g.className, defs: defs.filter((d) => g.keys.includes(d.key)) }));
  const rest = defs.filter((d) => !spec.some((g) => g.keys.includes(d.key)));
  groups[groups.length - 1]!.defs.push(...rest);
  const shown = groups.filter((g) => g.defs.length > 0);
  return shown.length > 1 ? shown : null;
}

function PageHeader({ page }: { page: string }) {
  const head = PAGE_HEAD[page];
  return (
    <div className="settings-head">
      <span className="settings-head-icon">
        <Icon name={head?.icon ?? "settings"} size={18} />
      </span>
      <div>
        <h3>{t(page)}</h3>
        {head && <p>{t(head.about)}</p>}
      </div>
    </div>
  );
}

/** The section shown last, opened again next time; stored without a Settings entry. */
const SECTION_KEY = "ui.settings_section";

/** Whether `needle` (folded) starts a word of any of `texts`, in English or translated. */
function found(needle: string, ...texts: (string | undefined)[]): boolean {
  return texts.some((s) => !!s && (startsWord(s, needle) || startsWord(t(s), needle)));
}

/** Whether the setting's name, text, key or choices hold `needle`, or the sub-tab that shows
 *  it is named so. */
function settingFound(needle: string, def: SettingDef<Value>): boolean {
  const c = def.control;
  const options = c.type === "select" || c.type === "swatches" || c.type === "pictures" ? c.options.map((o) => o.label) : [];
  const subTab = SUB_TABS[def.section]?.find((g) => g.keys.includes(def.key))?.label;
  return found(needle, def.label, def.description, def.key, subTab, ...options);
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
  const all = [...sections.keys(), ...PANELS.map((p) => p.title)];
  const grouped = NAV_GROUPS.map((g, i) => ({
    label: g.label,
    pages: [
      ...g.pages.filter((n) => all.includes(n)),
      ...(i === NAV_GROUPS.length - 1 ? all.filter((n) => !NAV_GROUPS.some((x) => x.pages.includes(n))) : []),
    ],
  })).filter((g) => g.pages.length > 0);
  // The keyboard moves through the pages in the order the list shows them.
  const names = grouped.flatMap((g) => g.pages);
  const [chosen, setChosen] = useState(() => getStored(SECTION_KEY));
  // The sub-tab of each page, kept while the dialog is open.
  const [subChosen, setSubChosen] = useState<Record<string, string>>({});
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
    e.currentTarget.closest('[role="tablist"]')?.querySelector<HTMLElement>(`[data-section="${CSS.escape(next)}"]`)?.focus();
  };
  const onSubTabKey = (e: KeyboardEvent<HTMLElement>, labels: string[]) => {
    const at = labels.indexOf(e.currentTarget.dataset.subtab ?? "");
    const moves: Record<string, number> = { ArrowRight: at + 1, ArrowLeft: at - 1 + labels.length, Home: 0, End: labels.length - 1 };
    const to = moves[e.key];
    if (to === undefined) return;
    e.preventDefault();
    const next = labels[to % labels.length]!;
    setSubChosen((m) => ({ ...m, [current]: next }));
    e.currentTarget.parentElement?.querySelector<HTMLElement>(`[data-subtab="${CSS.escape(next)}"]`)?.focus();
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
  const subs = subTabsOf(current, sections.get(current));
  const sub = subs?.find((g) => g.label === subChosen[current])?.label ?? subs?.[0]?.label;

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
              {grouped.map((g) => (
                <div key={g.label} className="settings-nav-group">
                  <div className="settings-nav-label">{t(g.label)}</div>
                  {g.pages.map((name) => {
                    const i = names.indexOf(name);
                    return (
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
                        <Icon name={PAGE_HEAD[name]?.icon ?? "settings"} size={14} />
                        <span className="settings-nav-text" title={t(name)}>
                          {t(name)}
                        </span>
                      </button>
                    );
                  })}
                </div>
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
              <PageHeader page={current} />
              {subs ? (
                <>
                  <div className="dialog-tabs" role="tablist" aria-label={t(current)}>
                    {subs.map((g, i) => (
                      <button
                        key={g.label}
                        type="button"
                        role="tab"
                        id={`settings-sub-${i}`}
                        data-subtab={g.label} title={t(g.label)}
                        aria-selected={g.label === sub}
                        aria-controls={`settings-subpage-${i}`}
                        tabIndex={g.label === sub ? 0 : -1}
                        onClick={() => setSubChosen((m) => ({ ...m, [current]: g.label }))}
                        onKeyDown={(e) =>
                          onSubTabKey(
                            e,
                            subs.map((x) => x.label),
                          )
                        }
                      >
                        {t(g.label)}
                      </button>
                    ))}
                  </div>
                  {/* All stay mounted, the others hidden: a setting is found in the page whichever sub-tab shows. */}
                  {subs.map((g, i) => (
                    <div
                      key={g.label}
                      id={`settings-subpage-${i}`}
                      className={`dialog-page${g.className ? ` ${g.className}` : ""}`}
                      role="tabpanel"
                      aria-labelledby={`settings-sub-${i}`}
                      hidden={g.label !== sub}
                    >
                      {g.defs.map(row)}
                    </div>
                  ))}
                </>
              ) : (
                sections.get(current)?.map(row)
              )}
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
              <span className="theme-preview" style={{ background: o.colors[0] }}>
                <i style={{ background: o.colors[1] }} />
                <i style={{ background: o.colors[1] }} />
                <span className="theme-preview-editor">
                  {o.colors.slice(2, 5).map((color, i) => (
                    <b key={i} style={{ background: color }} />
                  ))}
                </span>
              </span>
              <span className="theme-card-name">{t(o.label)}</span>
            </button>
          ))}
        </div>
      );
      break;
    case "pictures":
      input = (
        <div className={`pictures-control${c.options.some((o) => "icons" in o.picture) ? " icons" : ""}`} role="radiogroup" aria-label={t(def.label)}>
          {c.options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="radio"
              data-value={o.value}
              aria-checked={value === o.value}
              className={`picture-choice${value === o.value ? " selected" : ""}`}
              onClick={() => onSave(o.value)}
            >
              <Picture picture={o.picture} />
              <span className="picture-choice-name">{t(o.label)}</span>
              {o.description && <span className="muted small">{t(o.description)}</span>}
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
      className="link setting-reset"
      style={{ visibility: changed ? "visible" : "hidden" }}
      title={c.type === "color" ? t("Back to the theme's color") : t("Default: {value}", { value: defaultText(def) })}
      onClick={() => onSave(null)}
    >
      {t("Reset")}
    </button>
  );
  const text = (
    <span>
      <span className={`setting-title${changed ? " changed" : ""}`}>{t(def.label)}</span>
      {def.description && <span className="muted small">{t(def.description)}</span>}
    </span>
  );
  // Cards take a row of their own, under the label.
  if (c.type === "swatches" || c.type === "pictures") {
    return (
      <div className={`setting${c.type === "pictures" ? " pictures" : ""}`} data-key={def.key}>
        {text}
        {reset}
        {input}
      </div>
    );
  }
  return (
    <label className="setting" data-key={def.key}>
      {text}
      {input}
      {reset}
    </label>
  );
}

/** The default of a setting as its control names it: an option's label, On or Off. */
function defaultText(def: SettingDef<Value>): string {
  const c = def.control;
  if (c.type === "select" || c.type === "swatches" || c.type === "pictures") {
    const option = (c.options as { value: unknown; label: string }[]).find((o) => o.value === def.default);
    if (option) return t(option.label);
  }
  if (c.type === "boolean") return t(def.default ? "Enabled" : "Disabled");
  return String(def.default);
}

/** The icons each family shows in its card. */
const SAMPLE_ICONS: IconName[] = ["database", "table", "folder", "run", "settings"];

/** The picture of a choice: the window drawn with its parts in place, or a row of icons. */
function Picture({ picture }: { picture: SettingPicture }) {
  if ("icons" in picture) {
    return (
      <span className="picture-icons">
        {SAMPLE_ICONS.map((name) => (
          <Icon key={name} name={name} size={16} set={picture.icons as IconSet} />
        ))}
      </span>
    );
  }
  return (
    <span className="picture-window" style={{ gridTemplateColumns: picture.columns, gridTemplateRows: picture.rows }}>
      {picture.parts.map((p, i) => (
        <i key={i} className={p.part} style={{ gridRow: p.row, gridColumn: p.column }} />
      ))}
    </span>
  );
}

/** Saves on blur or Enter, and only valid numbers within bounds (whole ones for a whole
 *  step). */
function NumberInput(props: { value: number; min?: number; max?: number; step?: number; onCommit: (n: number) => void }) {
  const [text, setText] = useState(String(props.value));
  // Follows the value when it changes elsewhere (Reset): blur would save the old one.
  useEffect(() => setText(String(props.value)), [props.value]);
  const commit = () => {
    const n = settingNumber(text, props);
    if (n !== null && n !== props.value) props.onCommit(n);
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
