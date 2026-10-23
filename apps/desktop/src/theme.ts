/** Color theme: the system's, or one the user picked. */

export type ThemePref = "system" | "light" | "dark";

const KEY = "platypus.theme";

/** Fired on `window` when the theme changes, for code that caches colors (the grid). */
export const THEME_EVENT = "platypus-theme";

export function loadTheme(): ThemePref {
  try {
    const saved = localStorage.getItem(KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  } catch {
    return "system";
  }
}

export function applyTheme(pref: ThemePref) {
  const root = document.documentElement;
  if (pref === "system") delete root.dataset.theme;
  else root.dataset.theme = pref;
  try {
    localStorage.setItem(KEY, pref);
  } catch {
    // The choice still applies for this run.
  }
  window.dispatchEvent(new Event(THEME_EVENT));
}

export const nextTheme: Record<ThemePref, ThemePref> = { system: "light", light: "dark", dark: "system" };
