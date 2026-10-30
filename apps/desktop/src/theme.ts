/**
 * Appearance: the color theme (the system's light or dark one, or a preset), custom colors
 * over it, interface size, grid density and corner radius. Every color is a CSS variable set
 * on `<html>`; the settings live in `settings.toml` with the others, and the last applied
 * variables are cached so the next start paints in the right colors before they load.
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { defineSetting, getSetting, getStored, onSettingsChanged, setSetting } from "./settings";
import { msg } from "./i18n";

/** Fired on `window` when the theme changes, for code that caches colors (the grid). */
export const THEME_EVENT = "platypus-theme";

/** The colors every preset gives. */
type BasePalette = Record<
  | "bg"
  | "bg-sidebar"
  | "bg-input"
  | "fg"
  | "fg-muted"
  | "border"
  | "accent"
  | "accent-fg"
  | "hover"
  | "error"
  | "error-bg"
  | "ok"
  | "ok-bg"
  | "warn-bg"
  | "grid-bg"
  | "grid-bg-alt"
  | "grid-header"
  | "grid-selection"
  | "syn-keyword"
  | "syn-string"
  | "syn-number"
  | "syn-comment"
  | "syn-type"
  | "syn-ident",
  string
>;

/** Colors added with the Platypus themes; presets that do not give them get `derive`'s. */
type ExtraPalette = Record<
  | "bg-rail"
  | "bg-tabbar"
  | "fg-strong"
  | "border-strong"
  | "active"
  | "obj-db"
  | "obj-table"
  | "obj-view"
  | "obj-routine"
  | "obj-trigger"
  | "type-dev"
  | "type-dev-bg"
  | "type-test"
  | "type-test-bg"
  | "type-prod"
  | "type-prod-bg"
  | "warn"
  | "frame-dev"
  | "frame-test"
  | "frame-prod",
  string
>;

type Palette = BasePalette & ExtraPalette;

/** The colors of one variant of a theme file: any of the palette's, the rest from the
 *  Platypus theme of the same lightness. */
export type ThemeColors = Partial<Palette>;

/** A theme imported from a file (see `themes/file.ts`): a light variant, a dark one or both. */
export interface CustomTheme {
  name: string;
  light?: ThemeColors;
  dark?: ThemeColors;
}

/** The theme setting's value for the imported theme, kept under `CUSTOM_THEME_KEY`. */
export const CUSTOM = "custom";
export const CUSTOM_THEME_KEY = "appearance.custom_theme";

interface Preset {
  label: string;
  dark: boolean;
  colors: BasePalette & Partial<ExtraPalette>;
}

const mix = (a: string, pct: number, b: string) => `color-mix(in srgb, ${a} ${pct}%, ${b})`;

/** The extra colors of a preset that predates them, from its base colors. */
function derive(c: BasePalette, dark: boolean): ExtraPalette {
  // The rail and the tab bar sit a step darker than what they frame, on light themes too.
  return {
    "bg-rail": mix(c["bg-sidebar"], dark ? 65 : 94, "#000000"),
    "bg-tabbar": mix(c.bg, dark ? 70 : 96, "#000000"),
    "fg-strong": mix(c.fg, 80, dark ? "#ffffff" : "#000000"),
    "border-strong": mix(c.border, 85, c.fg),
    active: mix(c.accent, 16, c["bg-sidebar"]),
    "obj-db": c.accent,
    "obj-table": c["syn-ident"],
    "obj-view": c["syn-keyword"],
    "obj-routine": c["syn-number"],
    "obj-trigger": c["syn-type"],
    "type-dev": c.ok,
    "type-dev-bg": c["ok-bg"],
    "type-test": c["syn-number"],
    "type-test-bg": c["warn-bg"],
    "type-prod": c.error,
    "type-prod-bg": c["error-bg"],
    warn: c["syn-number"],
    // The window frame takes a tint of the active connection's type.
    "frame-dev": mix(c.ok, 8, mix(c["bg-sidebar"], dark ? 65 : 94, "#000000")),
    "frame-test": mix(c["syn-number"], 8, mix(c["bg-sidebar"], dark ? 65 : 94, "#000000")),
    "frame-prod": mix(c.error, 10, mix(c["bg-sidebar"], dark ? 65 : 94, "#000000")),
  };
}

/** All the colors of a preset: its own, and derived ones for what it does not give. */
function paletteOf(p: Preset): Palette {
  return { ...derive(p.colors, p.dark), ...p.colors };
}

export const PRESETS = {
  platypus_dark: {
    label: "Platypus Dark",
    dark: true,
    colors: {
      bg: "#16191b",
      "bg-sidebar": "#1b1f22",
      "bg-input": "#20262a",
      "bg-rail": "#111416",
      "bg-tabbar": "#1b1f22",
      fg: "#d5dadd",
      "fg-strong": "#f1f3f4",
      "fg-muted": "#8d969b",
      border: "#262c30",
      "border-strong": "#343b40",
      active: "#20262a",
      accent: "#ff8d28",
      "accent-fg": "#141519",
      hover: "rgba(255, 255, 255, 0.045)",
      error: "#ff8a8a",
      "error-bg": "#2e1416",
      ok: "#7ed3a0",
      "ok-bg": "#16281d",
      "warn-bg": "#2a2119",
      warn: "#e9c46a",
      "grid-bg": "#16191b",
      "grid-bg-alt": "#181c1e",
      "grid-header": "#1b1f22",
      "grid-selection": "rgba(255, 141, 40, 0.22)",
      "syn-keyword": "#7fc8db",
      "syn-string": "#7ed3a0",
      "syn-number": "#f2b16d",
      "syn-comment": "#80898f",
      "syn-type": "#c3a6ff",
      "syn-ident": "#d5dadd",
      "obj-db": "#8fb8ff",
      "obj-table": "#8fb8ff",
      "obj-view": "#c3a6ff",
      "obj-routine": "#f2b16d",
      "obj-trigger": "#e9c46a",
      "type-dev": "#7ed3a0",
      "type-dev-bg": "#16281d",
      "type-test": "#8fb8ff",
      "type-test-bg": "#17223a",
      "type-prod": "#ff8a8a",
      "type-prod-bg": "#2e1416",
      "frame-dev": "#0f1513",
      "frame-test": "#0f131b",
      "frame-prod": "#1d1012",
    },
  },
  platypus_light: {
    label: "Platypus Light",
    dark: false,
    colors: {
      bg: "#ffffff",
      "bg-sidebar": "#f6f8f9",
      "bg-input": "#ffffff",
      "bg-rail": "#f1f4f5",
      "bg-tabbar": "#f6f8f9",
      fg: "#2a2e31",
      "fg-strong": "#131416",
      "fg-muted": "#5d6266",
      border: "#e3e6e8",
      "border-strong": "#cdd2d5",
      active: "#e2e8ea",
      accent: "#b85600",
      "accent-fg": "#ffffff",
      hover: "rgba(0, 0, 0, 0.04)",
      error: "#b42318",
      "error-bg": "#fdecea",
      ok: "#1e7b4f",
      "ok-bg": "#e3f4ea",
      "warn-bg": "#fbf0e1",
      warn: "#854d0e",
      "grid-bg": "#ffffff",
      "grid-bg-alt": "#f8fafa",
      "grid-header": "#f6f8f9",
      "grid-selection": "rgba(184, 86, 0, 0.14)",
      "syn-keyword": "#155e75",
      "syn-string": "#1e7b4f",
      "syn-number": "#a15c07",
      "syn-comment": "#6b7176",
      "syn-type": "#6d3fc0",
      "syn-ident": "#2a2e31",
      "obj-db": "#2f5fb3",
      "obj-table": "#2f5fb3",
      "obj-view": "#6d3fc0",
      "obj-routine": "#a15c07",
      "obj-trigger": "#854d0e",
      "type-dev": "#1e7b4f",
      "type-dev-bg": "#e3f4ea",
      "type-test": "#2f5fb3",
      "type-test-bg": "#e6eefa",
      "type-prod": "#b42318",
      "type-prod-bg": "#fde3e3",
      "frame-dev": "#eef4f0",
      "frame-test": "#edf1f8",
      "frame-prod": "#f9ecea",
    },
  },
  // "light" and "dark" keep their keys, so a saved choice still applies.
  light: {
    label: msg("Classic Light"),
    dark: false,
    colors: {
      bg: "#ffffff",
      "bg-sidebar": "#f4f5f7",
      "bg-input": "#ffffff",
      fg: "#1d2129",
      "fg-muted": "#666d7b",
      border: "#e3e5e8",
      accent: "#1971c2",
      "accent-fg": "#ffffff",
      hover: "rgba(0, 0, 0, 0.05)",
      error: "#c62828",
      "error-bg": "#fdecea",
      ok: "#2b8a3e",
      "ok-bg": "#ebfbee",
      "warn-bg": "#fff4e6",
      "grid-bg": "#ffffff",
      "grid-bg-alt": "#f8f9fb",
      "grid-header": "#f1f3f5",
      "grid-selection": "rgba(28, 126, 214, 0.16)",
      "syn-keyword": "#7c3aed",
      "syn-string": "#2b8a3e",
      "syn-number": "#c2410c",
      "syn-comment": "#6e747d",
      "syn-type": "#0b7285",
      "syn-ident": "#1c7ed6",
    },
  },
  dark: {
    label: msg("Classic Dark"),
    dark: true,
    colors: {
      bg: "#1b1d21",
      "bg-sidebar": "#23262b",
      "bg-input": "#15171a",
      fg: "#e6e8eb",
      "fg-muted": "#8b919a",
      border: "#33373d",
      accent: "#4dabf7",
      "accent-fg": "#0b1621",
      hover: "rgba(255, 255, 255, 0.06)",
      error: "#ff8a80",
      "error-bg": "#3b1f1f",
      ok: "#8ce99a",
      "ok-bg": "#1f3324",
      "warn-bg": "#3b2f1a",
      "grid-bg": "#1b1d21",
      "grid-bg-alt": "#202328",
      "grid-header": "#25282d",
      "grid-selection": "rgba(77, 171, 247, 0.22)",
      "syn-keyword": "#c4a7ff",
      "syn-string": "#8ce99a",
      "syn-number": "#ffa94d",
      "syn-comment": "#6c727b",
      "syn-type": "#66d9e8",
      "syn-ident": "#74c0fc",
    },
  },
  nord: {
    label: "Nord",
    dark: true,
    colors: {
      bg: "#2e3440",
      "bg-sidebar": "#3b4252",
      "bg-input": "#272c36",
      fg: "#eceff4",
      "fg-muted": "#a7b1c2",
      border: "#434c5e",
      accent: "#88c0d0",
      "accent-fg": "#2e3440",
      hover: "rgba(255, 255, 255, 0.06)",
      error: "#e08a92",
      "error-bg": "#45313a",
      ok: "#a3be8c",
      "ok-bg": "#334036",
      "warn-bg": "#4a4232",
      "grid-bg": "#2e3440",
      "grid-bg-alt": "#333a47",
      "grid-header": "#3b4252",
      "grid-selection": "rgba(136, 192, 208, 0.25)",
      "syn-keyword": "#81a1c1",
      "syn-string": "#a3be8c",
      "syn-number": "#b793b0",
      "syn-comment": "#959fb2",
      "syn-type": "#8fbcbb",
      "syn-ident": "#88c0d0",
    },
  },
  dracula: {
    label: "Dracula",
    dark: true,
    colors: {
      bg: "#282a36",
      "bg-sidebar": "#21222c",
      "bg-input": "#1e1f29",
      fg: "#f8f8f2",
      "fg-muted": "#a0a6c4",
      border: "#3c3f52",
      accent: "#bd93f9",
      "accent-fg": "#282a36",
      hover: "rgba(255, 255, 255, 0.06)",
      error: "#ff6e6e",
      "error-bg": "#422733",
      ok: "#50fa7b",
      "ok-bg": "#243b2d",
      "warn-bg": "#423c27",
      "grid-bg": "#282a36",
      "grid-bg-alt": "#2d2f3c",
      "grid-header": "#313343",
      "grid-selection": "rgba(189, 147, 249, 0.25)",
      "syn-keyword": "#ff79c6",
      "syn-string": "#f1fa8c",
      "syn-number": "#bd93f9",
      "syn-comment": "#8492be",
      "syn-type": "#8be9fd",
      "syn-ident": "#50fa7b",
    },
  },
  solarized_light: {
    label: "Solarized Light",
    dark: false,
    colors: {
      bg: "#fdf6e3",
      "bg-sidebar": "#eee8d5",
      "bg-input": "#fffbf0",
      fg: "#2f4750",
      "fg-muted": "#576a70",
      border: "#e0d8c0",
      accent: "#2075b3",
      "accent-fg": "#ffffff",
      hover: "rgba(0, 0, 0, 0.05)",
      error: "#c32321",
      "error-bg": "#f9e0d6",
      ok: "#617000",
      "ok-bg": "#eef0d5",
      "warn-bg": "#f8ecd0",
      "grid-bg": "#fdf6e3",
      "grid-bg-alt": "#f7f0dc",
      "grid-header": "#eee8d5",
      "grid-selection": "rgba(38, 139, 210, 0.18)",
      "syn-keyword": "#667500",
      "syn-string": "#207b74",
      "syn-number": "#cb2c79",
      "syn-comment": "#5f7277",
      "syn-type": "#8c6a00",
      "syn-ident": "#268bd2",
    },
  },
  high_contrast: {
    label: msg("High contrast"),
    dark: true,
    colors: {
      bg: "#000000",
      "bg-sidebar": "#0d0d0d",
      "bg-input": "#000000",
      fg: "#ffffff",
      "fg-muted": "#cfcfcf",
      border: "#8a8a8a",
      accent: "#ffd400",
      "accent-fg": "#000000",
      hover: "rgba(255, 255, 255, 0.14)",
      error: "#ff7b7b",
      "error-bg": "#3a0000",
      ok: "#6bff8e",
      "ok-bg": "#003a12",
      "warn-bg": "#3a3000",
      "grid-bg": "#000000",
      "grid-bg-alt": "#121212",
      "grid-header": "#1a1a1a",
      "grid-selection": "rgba(255, 212, 0, 0.3)",
      "syn-keyword": "#ff9de2",
      "syn-string": "#9dff9d",
      "syn-number": "#ffc66b",
      "syn-comment": "#b8b8b8",
      "syn-type": "#6be4ff",
      "syn-ident": "#8ec8ff",
    },
  },
} satisfies Record<string, Preset>;

export type PresetName = keyof typeof PRESETS;
export type ThemeName = "system" | typeof CUSTOM | PresetName;

/** Every color a theme sets, as its variable is named without `--`. */
export const PALETTE_KEYS = Object.keys(paletteOf(PRESETS.platypus_dark)) as (keyof Palette)[];

const SECTION = msg("Appearance");

export const themeSetting = defineSetting<string>({
  key: "appearance.theme",
  section: SECTION,
  label: "Theme",
  default: "system",
  control: {
    type: "swatches",
    options: [
      {
        value: "system",
        label: "System",
        colors: [PRESETS.platypus_light.colors.bg, PRESETS.platypus_dark.colors.bg, PRESETS.platypus_dark.colors.accent],
      },
      ...(Object.entries(PRESETS) as [PresetName, Preset][]).map(([value, p]) => ({
        value,
        label: p.label,
        colors: [p.colors.bg, p.colors["bg-sidebar"], p.colors.accent, p.colors["syn-keyword"]],
      })),
    ],
  },
});

const color = (key: string, label: string, description: string) =>
  defineSetting<string>({ key, section: SECTION, label, description, default: "", control: { type: "color" } });

export const accentSetting = color("appearance.accent", msg("Accent color"), msg("Buttons, selection and focus."));
export const backgroundSetting = color("appearance.background", msg("Background"), msg("Editor and results; Reset keeps the theme's."));
export const sidebarSetting = color("appearance.sidebar", msg("Sidebar and panels"), msg("The connections tree and menus."));
export const textSetting = color("appearance.text", msg("Text"), msg("Muted text follows it."));

/** Read by the backend too, to open the window with or without the system's bar (lib.rs). */
export const titleBarSetting = defineSetting<string>({
  key: "appearance.title_bar",
  section: SECTION,
  label: "Title bar",
  description: "In the top bar, as in a browser, or the system's above it. Not on macOS.",
  default: "app",
  control: {
    type: "select",
    options: [
      { value: "app", label: "In the top bar" },
      { value: "system", label: "System" },
    ],
  },
});

const ZOOM_STEPS = ["80", "90", "100", "110", "125", "150"];

export const zoomSetting = defineSetting<string>({
  key: "appearance.zoom",
  section: SECTION,
  label: "Interface size",
  default: "100",
  control: {
    type: "select",
    options: ZOOM_STEPS.map((v) => ({ value: v, label: `${v}%` })),
  },
});

/** One step bigger (`1`) or smaller (`-1`) on the scale of the setting, or back to 100%
 *  (`0`): Ctrl+= and Ctrl+-, as in a browser. */
export function stepZoom(step: -1 | 0 | 1) {
  const at = ZOOM_STEPS.indexOf(getSetting(zoomSetting));
  const next = step === 0 ? "100" : ZOOM_STEPS[Math.min(ZOOM_STEPS.length - 1, Math.max(0, (at < 0 ? 2 : at) + step))]!;
  void setSetting(zoomSetting, next);
}

export const densitySetting = defineSetting<string>({
  key: "appearance.density",
  section: SECTION,
  label: "Grid rows",
  default: "normal",
  control: {
    type: "select",
    options: [
      { value: "compact", label: "Compact" },
      { value: "normal", label: "Normal" },
      { value: "comfortable", label: "Comfortable" },
    ],
  },
});

export const radiusSetting = defineSetting<string>({
  key: "appearance.radius",
  section: SECTION,
  label: "Corners",
  default: "rounded",
  control: {
    type: "select",
    options: [
      { value: "square", label: "Square" },
      { value: "rounded", label: "Rounded" },
      { value: "round", label: "Round" },
    ],
  },
});

export const editorFontSetting = defineSetting<number>({
  key: "appearance.editor_font_size",
  section: SECTION,
  label: "SQL editor font size",
  default: 13,
  control: { type: "number", min: 9, max: 28, step: 1 },
});

export const gridFontSetting = defineSetting<number>({
  key: "appearance.grid_font_size",
  section: SECTION,
  label: "Result grid font size",
  default: 12,
  control: { type: "number", min: 9, max: 24, step: 1 },
});

const ROW_HEIGHTS: Record<string, number> = { compact: 20, normal: 24, comfortable: 30 };
const RADII: Record<string, [number, number, number, number]> = {
  square: [0, 0, 0, 0],
  rounded: [4, 6, 10, 14],
  round: [7, 10, 14, 20],
};

const HEX = /^#[0-9a-f]{6}$/i;

/** Black or white, whichever reads better on `hex`. */
function readableOn(hex: string): string {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  // Above this luminance black contrasts more than white.
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b! > 0.179 ? "#000000" : "#ffffff";
}

const systemDark = () => window.matchMedia("(prefers-color-scheme: dark)").matches;

/** The preset the "System" theme stands for right now. */
const systemPreset = (dark = systemDark()): PresetName => (dark ? "platypus_dark" : "platypus_light");

/** The imported theme, if any. */
export function customTheme(): CustomTheme | null {
  const stored = getStored(CUSTOM_THEME_KEY);
  if (typeof stored !== "string") return null;
  try {
    const theme = JSON.parse(stored) as CustomTheme;
    return theme.light || theme.dark ? theme : null;
  } catch {
    return null;
  }
}

export function themeName(): ThemeName {
  const name = getSetting(themeSetting);
  if (name === CUSTOM) return customTheme() ? CUSTOM : "system";
  return name === "system" || name in PRESETS ? (name as ThemeName) : "system";
}

/** A variant of `theme` as a preset: its colors over the Platypus theme of its lightness.
 *  With both variants, `dark` (else the system's mode) picks one. */
export function customPreset(theme: CustomTheme, dark = systemDark()): Preset {
  const variantDark = theme.light && theme.dark ? dark : !!theme.dark;
  const colors = (variantDark ? theme.dark : theme.light) ?? {};
  return { label: theme.name, dark: variantDark, colors: { ...PRESETS[systemPreset(variantDark)].colors, ...colors } };
}

/** A theme shown in place of the chosen one, before applying it. */
let preview: CustomTheme | null = null;

/** Shows `theme` without saving it; `null` goes back to the settings' theme. */
export function previewTheme(theme: CustomTheme | null) {
  preview = theme;
  applyAppearance();
}

/** The preset of the theme shown: the previewed one, else the setting's. `dark` forces the
 *  variant of themes that follow the system. */
function shownPreset(dark?: boolean): Preset {
  if (preview) return customPreset(preview, dark);
  const name = themeName();
  if (name === CUSTOM) return customPreset(customTheme()!, dark);
  return PRESETS[name === "system" ? systemPreset(dark) : name];
}

/** The name of the theme in use, as the Settings list it (an imported one by its own). */
export function themeLabel(): string {
  const name = themeName();
  if (name === "system") return "System";
  return name === CUSTOM ? customTheme()!.name : PRESETS[name].label;
}

/** The colors of the theme in use, with the custom colors over them, as a theme file holds
 *  them: both variants when it follows the system, else the one it has. */
export function currentThemeColors(): CustomTheme {
  const name = themeName();
  const label = themeLabel();
  const custom = name === CUSTOM ? customTheme()! : null;
  const both = name === "system" || (!!custom?.light && !!custom.dark);
  const colorsOf = (dark?: boolean) => {
    const { vars } = variables(dark);
    return Object.fromEntries(PALETTE_KEYS.map((k) => [k, vars[`--${k}`]!])) as ThemeColors;
  };
  if (both) return { name: label, light: colorsOf(false), dark: colorsOf(true) };
  return shownPreset().dark ? { name: label, dark: colorsOf() } : { name: label, light: colorsOf() };
}

/** The variables of the current settings: the preset, then the custom colors over it. */
function variables(dark?: boolean): { dark: boolean; vars: Record<string, string> } {
  const preset: Preset = shownPreset(dark);
  const vars: Palette = paletteOf(preset);
  const custom = (def: typeof accentSetting) => {
    const v = getSetting(def);
    return HEX.test(v) ? v : null;
  };
  const accent = custom(accentSetting);
  if (accent) {
    vars.accent = accent;
    vars["accent-fg"] = readableOn(accent);
    vars["grid-selection"] = `color-mix(in srgb, ${accent} 24%, transparent)`;
  }
  const bg = custom(backgroundSetting);
  if (bg) {
    vars.bg = bg;
    vars["grid-bg"] = bg;
    vars["bg-input"] = `color-mix(in srgb, ${bg} 92%, ${readableOn(bg)})`;
    vars["grid-bg-alt"] = `color-mix(in srgb, ${bg} 96%, ${readableOn(bg)})`;
    vars["grid-header"] = `color-mix(in srgb, ${bg} 92%, ${readableOn(bg)})`;
  }
  const sidebar = custom(sidebarSetting);
  if (sidebar) vars["bg-sidebar"] = sidebar;
  const text = custom(textSetting);
  if (text) {
    vars.fg = text;
    vars["fg-muted"] = `color-mix(in srgb, ${text} 62%, ${vars.bg})`;
  }
  // What is mixed from a custom color follows it, as on a preset without its own: the tab bar
  // from the background, the rail from the sidebar, the active row from the sidebar and the
  // accent, strong text from the text. Lighter or darker by the actual background, which a
  // custom color may have turned the other way from the preset.
  if (bg || sidebar || text || accent) {
    const derived = derive(vars, readableOn(vars.bg) === "#ffffff");
    if (bg) vars["bg-tabbar"] = derived["bg-tabbar"];
    if (sidebar) {
      vars["bg-rail"] = derived["bg-rail"];
      vars["frame-dev"] = derived["frame-dev"];
      vars["frame-test"] = derived["frame-test"];
      vars["frame-prod"] = derived["frame-prod"];
    }
    if (sidebar || accent) vars.active = derived.active;
    if (text) vars["fg-strong"] = derived["fg-strong"];
  }
  const radius = RADII[getSetting(radiusSetting)] ?? RADII.rounded!;
  const out: Record<string, string> = Object.fromEntries(Object.entries(vars).map(([k, v]) => [`--${k}`, v]));
  out["--radius"] = `${radius[0]}px`;
  out["--radius-lg"] = `${radius[1]}px`;
  out["--radius-xl"] = `${radius[2]}px`;
  out["--radius-sheet"] = `${radius[3]}px`;
  const gridFont = getSetting(gridFontSetting);
  // A bigger font needs taller rows than the density gives at 12px.
  out["--grid-row-h"] = String((ROW_HEIGHTS[getSetting(densitySetting)] ?? 24) + Math.max(0, gridFont - 12));
  out["--grid-font-size"] = String(gridFont);
  out["--editor-font-size"] = `${getSetting(editorFontSetting)}px`;
  return { dark: preset.dark, vars: out };
}

/** v3 since the frame colors: an older cache lacks their variables, so it is dropped. */
const CACHE = "platypus.appearance.v3";
const OLD_CACHES = ["platypus.appearance.v1", "platypus.appearance.v2"];

function paint(dark: boolean, vars: Record<string, string>) {
  const root = document.documentElement;
  for (const [k, v] of Object.entries(vars)) root.style.setProperty(k, v);
  root.style.colorScheme = dark ? "dark" : "light";
  root.dataset.theme = dark ? "dark" : "light";
  window.dispatchEvent(new Event(THEME_EVENT));
}

let zoom = 0;
let painted = "";

/** Applies the appearance settings, and keeps them for the next start. */
export function applyAppearance() {
  const scale = Number(getSetting(zoomSetting)) / 100;
  if (scale > 0 && scale !== zoom) {
    zoom = scale;
    getCurrentWebview()
      .setZoom(scale)
      .catch((e: unknown) => console.warn("zoom", e));
  }
  const { dark, vars } = variables();
  // Any setting change calls this: repaint only when the appearance changed.
  const signature = JSON.stringify([dark, vars]);
  if (signature === painted) return;
  painted = signature;
  paint(dark, vars);
  // A preview is not kept for the next start.
  if (preview) return;
  try {
    localStorage.setItem(CACHE, JSON.stringify({ dark, vars }));
  } catch {
    // The theme still applies for this run.
  }
}

/** Paints the last applied appearance before the settings load, so the app never flashes. */
export function paintCached() {
  try {
    for (const old of OLD_CACHES) localStorage.removeItem(old);
    const cached = JSON.parse(localStorage.getItem(CACHE) ?? "null") as { dark: boolean; vars: Record<string, string> } | null;
    if (cached) {
      document.documentElement.dataset.firstPaint = "cache";
      return paint(cached.dark, cached.vars);
    }
  } catch {
    // Fall through to the system's colors.
  }
  document.documentElement.dataset.firstPaint = "default";
  const preset = PRESETS[systemPreset()];
  paint(preset.dark, Object.fromEntries(Object.entries(paletteOf(preset)).map(([k, v]) => [`--${k}`, v])));
}

/** Follows the settings and, for the system theme, the system's light or dark mode. Call
 *  once the settings are loaded. */
export function startAppearance() {
  // 0.1 kept a light/dark choice in the browser storage; it becomes the setting.
  try {
    const old = localStorage.getItem("platypus.theme");
    if ((old === "light" || old === "dark") && getSetting(themeSetting) === "system") void setSetting(themeSetting, old);
    localStorage.removeItem("platypus.theme");
  } catch {
    // Nothing to carry over.
  }
  applyAppearance();
  onSettingsChanged(applyAppearance);
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () => {
    const name = themeName();
    if (name === "system" || name === CUSTOM || preview) applyAppearance();
  });
}

/** The quick toggle in the sidebar: system, light, dark, and back to system. */
export function cycleTheme() {
  const next: Record<string, ThemeName> = {
    system: "platypus_light",
    platypus_light: "platypus_dark",
    platypus_dark: "system",
  };
  void setSetting(themeSetting, next[themeName()] ?? "system");
}
