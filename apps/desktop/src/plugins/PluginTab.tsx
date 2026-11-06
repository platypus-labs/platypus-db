// A tab of a plugin: its document in a frame, or why it cannot show. Loaded when such a tab opens.
import { useEffect, useState } from "react";
import { t } from "../i18n";
import type { TabProps } from "../tabs/registry";
import type { PluginTabModel } from "../tabs/plugin";
import { EmptyState } from "../ui/State";
import { PluginFrame } from "./PluginFrame";
import { followPluginChanges, installedPlugins, reloadPlugins, usePlugins } from "./store";

export default function PluginTab({ tab, connection }: TabProps<PluginTabModel>) {
  const plugins = usePlugins();
  // A tab restored at start, or moved to a window of its own, may come before the list of plugins.
  const [read, setRead] = useState(() => installedPlugins().length > 0);
  // Turned off or removed in another window, it goes here too.
  useEffect(followPluginChanges, []);
  useEffect(() => {
    if (!read) {
      void reloadPlugins()
        .catch((e: unknown) => console.warn("plugins", e))
        .finally(() => setRead(true));
    }
  }, [read]);
  if (!read) return null;
  const plugin = plugins.find((p) => p.manifest.id === tab.plugin);
  const declared = plugin?.manifest.tabs?.find((d) => d.kind === tab.tabKind);
  if (!plugin?.enabled || !plugin.approved.tabs || !declared) {
    return (
      <EmptyState icon="extension" title={t("This tab's plugin is not available")}>
        {t("It was removed or turned off. Settings › Plugins lists what is installed.")}
      </EmptyState>
    );
  }
  return <PluginFrame plugin={plugin} document={declared.entry} title={declared.title} connection={connection} />;
}
