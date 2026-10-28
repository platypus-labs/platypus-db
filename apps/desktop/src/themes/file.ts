/**
 * Theme files: a theme's colors as JSON, to share it or keep it. A file has a light variant,
 * a dark one or both, each with any of the app's and the editor's colors; the colors it
 * leaves out come from the Platypus theme of the same lightness.
 *
 *   { "platypus_theme": 1, "name": "Sea", "dark": { "bg": "#0b1d26", "accent": "#3fc1c9" } }
 *
 * Kept apart from `theme.ts`, which paints: this only reads and writes the text.
 */

/** The format's version; a newer file is refused rather than half read. */
export const THEME_FORMAT = 1;

/** Longest theme name kept. */
const NAME_CHARS = 60;

export interface ThemeVariants {
  name: string;
  light?: Record<string, string>;
  dark?: Record<string, string>;
}

/** What reading a file gave: the theme, and what was left out of it and why. */
export interface ReadTheme {
  theme: ThemeVariants;
  /** Keys and values skipped, e.g. `dark.bg: not a color`. */
  skipped: string[];
}

/** The file's text for `theme`, with the format's version first. */
export function themeFileText(theme: ThemeVariants): string {
  const doc: Record<string, unknown> = { platypus_theme: THEME_FORMAT, name: theme.name };
  if (theme.light) doc.light = theme.light;
  if (theme.dark) doc.dark = theme.dark;
  return `${JSON.stringify(doc, null, 2)}\n`;
}

/** A name for the file of `theme`: its name, lower case, without what file names refuse. */
export function themeFileName(theme: ThemeVariants): string {
  const base = theme.name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
  return `${base || "theme"}.platypus-theme.json`;
}

/**
 * Reads a theme file. `keys` are the colors a theme may set and `isColor` tells a valid CSS
 * color; unknown keys and invalid values are skipped and listed. Throws, with a message for
 * the user, when the text is not a theme file at all.
 */
export function readThemeFile(text: string, keys: readonly string[], isColor: (value: string) => boolean): ReadTheme {
  let doc: unknown;
  try {
    doc = JSON.parse(text);
  } catch {
    throw new Error("The file is not JSON.");
  }
  if (typeof doc !== "object" || doc === null || Array.isArray(doc)) throw new Error("The file is not a theme.");
  const root = doc as Record<string, unknown>;
  const version = root.platypus_theme;
  if (typeof version !== "number") throw new Error("The file is not a PlatypusDB theme: platypus_theme is missing.");
  if (version > THEME_FORMAT) throw new Error("The theme is from a newer PlatypusDB.");
  const skipped: string[] = [];
  for (const key of Object.keys(root)) {
    if (!["platypus_theme", "name", "light", "dark"].includes(key)) skipped.push(`${key}: unknown`);
  }
  const known = new Set(keys);
  const variant = (which: "light" | "dark") => {
    const value = root[which];
    if (value === undefined) return undefined;
    if (typeof value !== "object" || value === null || Array.isArray(value)) {
      skipped.push(`${which}: not a set of colors`);
      return undefined;
    }
    const colors: Record<string, string> = {};
    for (const [key, color] of Object.entries(value as Record<string, unknown>)) {
      if (!known.has(key)) skipped.push(`${which}.${key}: unknown`);
      else if (typeof color !== "string" || !isColor(color.trim())) skipped.push(`${which}.${key}: not a color`);
      else colors[key] = color.trim();
    }
    return colors;
  };
  const light = variant("light");
  const dark = variant("dark");
  if (!light && !dark) throw new Error("The theme has neither light nor dark colors.");
  const name =
    typeof root.name === "string"
      ? root.name
          .replace(/\p{Cc}/gu, "")
          .trim()
          .slice(0, NAME_CHARS)
      : "";
  const theme: ThemeVariants = { name: name || "Imported theme" };
  if (light) theme.light = light;
  if (dark) theme.dark = dark;
  return { theme, skipped };
}
