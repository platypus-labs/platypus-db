/**
 * Appearance: the color theme (the system's light or dark one, or a preset), custom colors
 * over it, interface size, grid density and corner radius. Every color is a CSS variable set
 * on `<html>`; the settings live in `settings.toml` with the others, and the last applied
 * variables are cached so the next start paints in the right colors before they load.
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { defineSetting, getSetting, onSettingsChanged, setSetting } from "./settings";

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
  | "type-prod-bg",
  string
>;

type Palette = BasePalette & ExtraPalette;

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
      bg: "#141519",
      "bg-sidebar": "#17181d",
      "bg-input": "#1d1f25",
      "bg-rail": "#0d0e11",
      "bg-tabbar": "#0f1013",
      fg: "#c9ccd3",
      "fg-strong": "#f2f3f5",
      "fg-muted": "#6f7480",
      border: "#23252c",
      "border-strong": "#2b2e36",
      active: "#24262e",
      accent: "#6c63ff",
      "accent-fg": "#ffffff",
      hover: "rgba(255, 255, 255, 0.04)",
      error: "#ff7b7b",
      "error-bg": "#3a1518",
      ok: "#7ed3a0",
      "ok-bg": "#173322",
      "warn-bg": "#3a2a12",
      "grid-bg": "#141519",
      "grid-bg-alt": "#17181d",
      "grid-header": "#17181d",
      "grid-selection": "rgba(108, 99, 255, 0.22)",
      "syn-keyword": "#b69cff",
      "syn-string": "#7ed3a0",
      "syn-number": "#f2b16d",
      "syn-comment": "#5c616c",
      "syn-type": "#6fd3e8",
      "syn-ident": "#8ea2ff",
      "obj-db": "#8ea2ff",
      "obj-table": "#8ea2ff",
      "obj-view": "#b69cff",
      "obj-routine": "#f2b16d",
      "obj-trigger": "#f5d76e",
      "type-dev": "#7ed3a0",
      "type-dev-bg": "#173322",
      "type-test": "#f2b16d",
      "type-test-bg": "#3a2a12",
      "type-prod": "#ff8a8a",
      "type-prod-bg": "#3a1518",
    },
  },
  platypus_light: {
    label: "Platypus Light",
    dark: false,
    colors: {
      bg: "#ffffff",
      "bg-sidebar": "#f6f7f9",
      "bg-input": "#ffffff",
      "bg-rail": "#eceef2",
      "bg-tabbar": "#f1f2f5",
      fg: "#2b2f36",
      "fg-strong": "#111318",
      "fg-muted": "#868c96",
      border: "#e6e8eb",
      "border-strong": "#dcdfe3",
      active: "#ebeafd",
      accent: "#5b52f0",
      "accent-fg": "#ffffff",
      hover: "rgba(0, 0, 0, 0.04)",
      error: "#c62828",
      "error-bg": "#fdecea",
      ok: "#2b8a3e",
      "ok-bg": "#e6f6ea",
      "warn-bg": "#fff3e0",
      "grid-bg": "#ffffff",
      "grid-bg-alt": "#f9fafb",
      "grid-header": "#f6f7f9",
      "grid-selection": "rgba(91, 82, 240, 0.14)",
      "syn-keyword": "#7c3aed",
      "syn-string": "#2b8a3e",
      "syn-number": "#c2410c",
      "syn-comment": "#8b919a",
      "syn-type": "#0b7285",
      "syn-ident": "#4f46e5",
      "obj-db": "#4f46e5",
      "obj-table": "#4f46e5",
      "obj-view": "#7c3aed",
      "obj-routine": "#c2410c",
      "obj-trigger": "#b7791f",
      "type-dev": "#2f7a3a",
      "type-dev-bg": "#e3f4e6",
      "type-test": "#a15c07",
      "type-test-bg": "#fdf0d9",
      "type-prod": "#b42318",
      "type-prod-bg": "#fde3e3",
    },
  },
  // "light" and "dark" keep their keys, so a saved choice still applies.
  light: {
    label: "Classic Light",
    dark: false,
    colors: {
      bg: "#ffffff",
      "bg-sidebar": "#f4f5f7",
      "bg-input": "#ffffff",
      fg: "#1d2129",
      "fg-muted": "#6b7280",
      border: "#e3e5e8",
      accent: "#1c7ed6",
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
      "syn-comment": "#8b919a",
      "syn-type": "#0b7285",
      "syn-ident": "#1c7ed6",
    },
  },
  dark: {
    label: "Classic Dark",
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
      "fg-muted": "#9aa5b8",
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
      "syn-number": "#b48ead",
      "syn-comment": "#6f7c96",
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
      "syn-comment": "#7282b4",
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
      "fg-muted": "#768689",
      border: "#e0d8c0",
      accent: "#268bd2",
      "accent-fg": "#ffffff",
      hover: "rgba(0, 0, 0, 0.05)",
      error: "#dc322f",
      "error-bg": "#f9e0d6",
      ok: "#6c7d00",
      "ok-bg": "#eef0d5",
      "warn-bg": "#f8ecd0",
      "grid-bg": "#fdf6e3",
      "grid-bg-alt": "#f7f0dc",
      "grid-header": "#eee8d5",
      "grid-selection": "rgba(38, 139, 210, 0.18)",
      "syn-keyword": "#859900",
      "syn-string": "#2aa198",
      "syn-number": "#d33682",
      "syn-comment": "#93a1a1",
      "syn-type": "#b58900",
      "syn-ident": "#268bd2",
    },
  },
  high_contrast: {
    label: "High contrast",
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
export type ThemeName = "system" | PresetName;

const SECTION = "Appearance";

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

export const accentSetting = color("appearance.accent", "Accent color", "Buttons, selection and focus.");
export const backgroundSetting = color("appearance.background", "Background", "Editor and results; Reset keeps the theme's.");
export const sidebarSetting = color("appearance.sidebar", "Sidebar and panels", "The connections tree and menus.");
export const textSetting = color("appearance.text", "Text", "Muted text follows it.");

export const zoomSetting = defineSetting<string>({
  key: "appearance.zoom",
  section: SECTION,
  label: "Interface size",
  default: "100",
  control: {
    type: "select",
    options: ["80", "90", "100", "110", "125", "150"].map((v) => ({ value: v, label: `${v}%` })),
  },
});

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

const ROW_HEIGHTS: Record<string, number> = { compact: 20, normal: 24, comfortable: 30 };
const RADII: Record<string, [number, number, number]> = {
  square: [0, 0, 0],
  rounded: [4, 6, 8],
  round: [7, 10, 14],
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
const systemPreset = (): PresetName => (systemDark() ? "platypus_dark" : "platypus_light");

export function themeName(): ThemeName {
  const name = getSetting(themeSetting);
  return name === "system" || name in PRESETS ? (name as ThemeName) : "system";
}

/** The variables of the current settings: the preset, then the custom colors over it. */
function variables(): { dark: boolean; vars: Record<string, string> } {
  const name = themeName();
  const preset: Preset = PRESETS[name === "system" ? systemPreset() : name];
  const vars: Record<string, string> = paletteOf(preset);
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
  // What frames the custom colors follows them, as on a preset without its own.
  if (bg || sidebar || text || accent) {
    const derived = derive(vars as BasePalette, preset.dark);
    if (bg) vars["bg-tabbar"] = derived["bg-tabbar"];
    if (bg || sidebar) vars["bg-rail"] = derived["bg-rail"];
    if (bg || sidebar || accent) vars.active = derived.active;
    if (text) vars["fg-strong"] = derived["fg-strong"];
  }
  const radius = RADII[getSetting(radiusSetting)] ?? RADII.rounded!;
  const out: Record<string, string> = Object.fromEntries(Object.entries(vars).map(([k, v]) => [`--${k}`, v]));
  out["--radius"] = `${radius[0]}px`;
  out["--radius-lg"] = `${radius[1]}px`;
  out["--radius-xl"] = `${radius[2]}px`;
  out["--grid-row-h"] = String(ROW_HEIGHTS[getSetting(densitySetting)] ?? 24);
  out["--editor-font-size"] = `${getSetting(editorFontSetting)}px`;
  return { dark: preset.dark, vars: out };
}

/** v2 since the Platypus themes: a v1 cache lacks their variables, so it is dropped. */
const CACHE = "platypus.appearance.v2";
const OLD_CACHE = "platypus.appearance.v1";

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
  try {
    localStorage.setItem(CACHE, JSON.stringify({ dark, vars }));
  } catch {
    // The theme still applies for this run.
  }
}

/** Paints the last applied appearance before the settings load, so the app never flashes. */
export function paintCached() {
  try {
    localStorage.removeItem(OLD_CACHE);
    const cached = JSON.parse(localStorage.getItem(CACHE) ?? "null") as { dark: boolean; vars: Record<string, string> } | null;
    if (cached) return paint(cached.dark, cached.vars);
  } catch {
    // Fall through to the system's colors.
  }
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
    if (themeName() === "system") applyAppearance();
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
