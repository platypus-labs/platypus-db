/** The main window's side of the other windows: the events they send (a tab to open, a tab
 *  moved back, a window closed with its tab), the quick query window and the tray icon. */
import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, toCommandError } from "../api";
import { registerAction } from "../actions";
import { defineSetting, useSetting } from "../settings";
import type { PersistedTab } from "../workspace";
import type { TabKindName } from "../tabs/registry";
import { language, t } from "../i18n";
import { logError } from "./activity";

/** Events from the other windows to the main one (see `WindowApp`). */
export const WINDOW_EVENTS = {
  /** Open a tab here: `{ kind, connectionId, init }`. */
  openTab: "platypus://open-tab",
  /** Take this tab back as an open tab. */
  attach: "platypus://attach-tab",
  /** A window closed with this tab: kept to reopen, as a tab closed here. */
  closed: "platypus://window-tab-closed",
} as const;

export interface WindowEvents {
  openTab: (request: { kind: TabKindName; connectionId: string; init?: Record<string, unknown> }) => void;
  attach: (saved: PersistedTab) => void;
  closed: (saved: PersistedTab) => void;
}

/** Listens, in the main window, to what the other windows send. */
export function useWindowEvents(handlers: WindowEvents) {
  const latest = useRef(handlers);
  latest.current = handlers;
  useEffect(() => {
    const stops = [
      listen<Parameters<WindowEvents["openTab"]>[0]>(WINDOW_EVENTS.openTab, (e) => latest.current.openTab(e.payload)),
      listen<PersistedTab>(WINDOW_EVENTS.attach, (e) => latest.current.attach(e.payload)),
      listen<PersistedTab>(WINDOW_EVENTS.closed, (e) => latest.current.closed(e.payload)),
    ];
    return () => stops.forEach((s) => void s.then((stop) => stop()));
  }, []);
}

export const traySetting = defineSetting<boolean>({
  key: "appearance.tray",
  section: "Appearance",
  label: "Show an icon in the system tray, for a quick query",
  description: "A click on it opens a small query window over the others. On Linux it needs AppIndicator.",
  // Linux desktops without AppIndicator have no tray: off unless asked.
  default: !navigator.userAgent.includes("Linux"),
  control: { type: "boolean" },
});

const quickTitle = () => t("Quick query");

/** Shows the tray icon while the setting is on, with its menu in the interface's language. */
export function useTray() {
  const on = useSetting(traySetting);
  const lang = language();
  useEffect(() => {
    const labels: [string, string, string, string] | null = on ? [t("Quick query…"), t("Show PlatypusDB"), t("Quit"), quickTitle()] : null;
    api.setTray(labels).catch((e) => logError(t("The tray icon could not be shown: {message}", { message: toCommandError(e).message })));
  }, [on, lang]);
}

registerAction({
  id: "app.quick_query",
  scope: "global",
  title: "Quick query…",
  run: () => api.openQuickWindow(quickTitle()),
});
