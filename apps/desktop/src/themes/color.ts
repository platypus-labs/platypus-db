/**
 * The colors of a theme that comes from a plugin. Kept apart from `theme.ts`, which paints: this
 * only reads values, so that a test can run it. The same grammar as `platypus-plugins`
 * (`theme::is_color`): a color and nothing else, with no way to name a resource or to leave the
 * declaration. The value reaches the page through `style.setProperty`, never as CSS text.
 */
const COLOR = /^(?:transparent|#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})|(?:rgba?|hsla?)\((?=[^)]*\d)[\d.,%/ -]*\))$/;

/** A color a theme of a plugin may set. */
export function isThemeColor(value: unknown): value is string {
  return typeof value === "string" && value.length <= 64 && COLOR.test(value);
}

/** The relative luminance of red, green and blue from 0 to 1, as WCAG defines it. */
export function luminance(r: number, g: number, b: number): number {
  const [lr, lg, lb] = [r, g, b].map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lr! + 0.7152 * lg! + 0.0722 * lb!;
}

export interface ThemeVariantsOf {
  name: string;
  light?: Record<string, string>;
  dark?: Record<string, string>;
}

/** What tells the fonts of a plugin's theme from those it had: a new version may have others under
 *  the same names. `null` without any. */
export function themeFontsKey(id: string, version: string, fonts: string[]): string | null {
  return fonts.length > 0 ? `${id}@${version}` : null;
}

/** What of a plugin's theme can be painted: its known tokens with color values. The crate checked
 *  the same when it read the manifest, so anything dropped here means the file was changed by
 *  something else. A theme left with no variant is no theme. */
export function pluginThemeOf(
  theme: { name: string; light?: Record<string, string>; dark?: Record<string, string> },
  keys: readonly string[],
): ThemeVariantsOf | null {
  const known = new Set(keys);
  const variant = (colors: Record<string, string> | undefined) => {
    if (!colors) return undefined;
    const kept = Object.fromEntries(Object.entries(colors).filter(([key, value]) => known.has(key) && isThemeColor(value)));
    return Object.keys(kept).length > 0 ? kept : undefined;
  };
  const light = variant(theme.light);
  const dark = variant(theme.dark);
  if (!light && !dark) return null;
  return { name: theme.name, ...(light && { light }), ...(dark && { dark }) };
}
