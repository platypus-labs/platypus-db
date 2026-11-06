// The side panel of a plugin beside the tabs, one at a time; its document stays alive only while
// it is open. Which one is open is kept for the next start.
import { useRef, useSyncExternalStore } from "react";
import type { SavedConnection } from "../api";
import { t } from "../i18n";
import { PanelResizer, useStoredSize } from "../shell/PanelResizer";
import { useSidePanelsLeft } from "../shell/layout";
import { Icon } from "../ui/Icon";
import { PluginFrame } from "./PluginFrame";
import { usePlugins } from "./store";

const KEY = "platypus.plugin_panel.v1";
const listeners = new Set<() => void>();
let open: string | null = readOpen();

function readOpen(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** Opens the panel `<plugin>/<panel>`, or closes it if it is the one open. */
export function togglePanel(plugin: string, panel: string) {
  const key = `${plugin}/${panel}`;
  open = open === key ? null : key;
  try {
    if (open) localStorage.setItem(KEY, open);
    else localStorage.removeItem(KEY);
  } catch {
    // Storage can be unavailable: it stays open for this run.
  }
  listeners.forEach((l) => l());
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function PluginPanels({ connection }: { connection: SavedConnection | null }) {
  const key = useSyncExternalStore(subscribe, () => open);
  const plugins = usePlugins();
  const [width, setWidth] = useStoredSize("layout.plugin_panel_width", 320);
  const left = useSidePanelsLeft();
  const room = useRef<HTMLDivElement>(null);
  const [pluginId, panelId] = key?.split("/") ?? [];
  const plugin = plugins.find((p) => p.manifest.id === pluginId);
  const panel = plugin?.manifest.panels?.find((p) => p.id === panelId);
  if (!plugin?.enabled || !plugin.approved.tabs || !panel) return null;
  return (
    <div className="plugin-panel" ref={room} data-panel={key} style={{ "--plugin-panel-width": `${width}px` } as React.CSSProperties}>
      <PanelResizer
        storeKey="layout.plugin_panel_width"
        size={width}
        fallback={320}
        min={200}
        max={() => window.innerWidth - 360}
        direction="column"
        after={!left}
        onDrag={(w) => room.current?.style.setProperty("--plugin-panel-width", `${w}px`)}
        onResize={setWidth}
        label={t("Resize the plugin's panel")}
      />
      <div className="plugin-panel-body">
        <div className="value-bar">
          <strong className="small plugin-panel-title">{panel.title}</strong>
          <span className="spacer" />
          <button type="button" className="icon" data-action="close-panel" title={t("Close")} aria-label={t("Close")} onClick={() => togglePanel(plugin.manifest.id, panel.id)}>
            <Icon name="close" />
          </button>
        </div>
        <PluginFrame plugin={plugin} document={panel.entry} title={panel.title} connection={connection} />
      </div>
    </div>
  );
}
