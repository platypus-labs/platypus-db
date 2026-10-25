/**
 * The sidebar column's width and whether it is shown. Both are stored with the other
 * preferences (`settings.toml`) without a Settings entry: the drag handle and the toggle
 * are their controls.
 */
import { getStored, setStored, useSettingsVersion } from "../settings";
import { registerAction } from "../actions";

export const SIDEBAR_DEFAULT = 260;
export const SIDEBAR_MIN = 180;
const WIDTH_KEY = "ui.sidebar_width";
const COLLAPSED_KEY = "ui.sidebar_collapsed";

/** At most half the window, and never under the minimum. */
export function clampSidebar(width: number): number {
  return Math.round(Math.max(SIDEBAR_MIN, Math.min(width, window.innerWidth / 2)));
}

export function useSidebar(): { width: number; collapsed: boolean } {
  useSettingsVersion();
  const width = getStored(WIDTH_KEY);
  return {
    width: typeof width === "number" ? clampSidebar(width) : SIDEBAR_DEFAULT,
    collapsed: getStored(COLLAPSED_KEY) === true,
  };
}

const warn = (e: unknown) => console.warn("sidebar", e);

/** Stores the width; `null` brings the default back. */
export function setSidebarWidth(width: number | null) {
  setStored(WIDTH_KEY, width === null ? null : clampSidebar(width)).catch(warn);
}

export function toggleSidebar() {
  setStored(COLLAPSED_KEY, getStored(COLLAPSED_KEY) === true ? null : true).catch(warn);
}

registerAction({
  id: "app.toggleSidebar",
  scope: "global",
  title: "Toggle sidebar",
  shortcut: "Mod+B",
  run: toggleSidebar,
});
