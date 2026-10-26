/**
 * Whether the navigator column is shown. Stored with the other preferences (`settings.toml`)
 * without a Settings entry: the top bar's button and `Ctrl+B` are its controls. Its width is
 * kept by the handle (`SidebarResizer.tsx`).
 */
import { getStored, setStored, useSettingsVersion } from "../settings";
import { registerAction } from "../actions";

const KEY = "layout.sidebar_collapsed";
/** Where earlier builds kept it: still read, removed on the next change. */
const OLD_KEY = "ui.sidebar_collapsed";

const isCollapsed = () => (getStored(KEY) ?? getStored(OLD_KEY)) === true;

export function useSidebarCollapsed(): boolean {
  useSettingsVersion();
  return isCollapsed();
}

const warn = (e: unknown) => console.warn("sidebar", e);

export function toggleSidebar() {
  const collapse = !isCollapsed();
  // One write after the other: each saves the whole file.
  void (async () => {
    if (getStored(OLD_KEY) !== undefined) await setStored(OLD_KEY, null);
    await setStored(KEY, collapse ? true : null);
  })().catch(warn);
}

registerAction({
  id: "app.toggleSidebar",
  scope: "global",
  title: "Toggle sidebar",
  shortcut: "Mod+B",
  run: toggleSidebar,
});
