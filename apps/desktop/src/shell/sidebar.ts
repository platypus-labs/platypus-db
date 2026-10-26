/**
 * Whether the navigator column is shown. Stored with the other preferences (`settings.toml`)
 * without a Settings entry: the buttons of the top bar and of the navigator, and `Ctrl+B`, are
 * its controls. Its width is kept by the handle (`SidebarResizer.tsx`).
 */
import { getStored, setStored, useSettingsVersion } from "../settings";
import { registerAction, shortcutLabel } from "../actions";
import { t } from "../i18n";

const KEY = "layout.sidebar_collapsed";
/** Where earlier builds kept it: still read, removed on the next change. */
const OLD_KEY = "ui.sidebar_collapsed";

const isCollapsed = () => (getStored(KEY) ?? getStored(OLD_KEY)) === true;

export function useSidebarCollapsed(): boolean {
  useSettingsVersion();
  return isCollapsed();
}

const warn = (e: unknown) => console.warn("sidebar", e);

/** The top bar's button that shows and hides the navigator. */
export const SIDEBAR_TOGGLE_ID = "sidebar-toggle";

export function toggleSidebar() {
  const collapse = !isCollapsed();
  // Hidden under the focus, the navigator column (App.tsx) would drop it to the page: the top
  // bar's button, which shows the navigator again, takes it.
  if (collapse && document.activeElement?.closest(".sidebar-column")) document.getElementById(SIDEBAR_TOGGLE_ID)?.focus();
  // Both changes are made in memory at once (a second toggle already sees them); each write
  // sets its own key, so their order does not matter.
  if (getStored(OLD_KEY) !== undefined) setStored(OLD_KEY, null).catch(warn);
  setStored(KEY, collapse ? true : null).catch(warn);
}

const action = { id: "app.toggleSidebar", shortcut: "Mod+B" };

registerAction({
  ...action,
  scope: "global",
  title: "Show or hide the navigator",
  run: toggleSidebar,
});

/** The name of the buttons that show and hide the navigator, with its current shortcut. */
export function toggleSidebarTitle(): string {
  const keys = shortcutLabel(action);
  return keys ? `${t("Show or hide the navigator")} (${keys})` : t("Show or hide the navigator");
}
