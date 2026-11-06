// The tabs of plugins: one kind for all, since the app restores tabs before it has read the plugins.
// Nothing of the page behind it loads until such a tab opens.
import { Suspense, lazy } from "react";
import { registerTabKind, type TabBase, type TabProps } from "./registry";

export interface PluginTabModel extends TabBase {
  kind: "plugin";
  plugin: string;
  /** The `kind` of the plugin's `[[tabs]]`. */
  tabKind: string;
  /** Its title, as the manifest says it; kept for when the plugin is gone. */
  heading: string;
}

declare module "./registry" {
  interface TabKinds {
    plugin: PluginTabModel;
  }
}

const PluginTab = lazy(() => import("../plugins/PluginTab"));

registerTabKind({
  kind: "plugin",
  component: (props: TabProps<PluginTabModel>) => (
    <Suspense fallback={null}>
      <PluginTab {...props} />
    </Suspense>
  ),
  create: (init) => ({ plugin: "", tabKind: "", heading: "", ...init }),
  label: (tab) => tab.heading,
  persist: ({ plugin, tabKind, heading }) => ({ plugin, tabKind, heading }),
  restore: (saved) => ({
    plugin: String(saved.plugin ?? ""),
    tabKind: String(saved.tabKind ?? ""),
    heading: String(saved.heading ?? ""),
  }),
});
