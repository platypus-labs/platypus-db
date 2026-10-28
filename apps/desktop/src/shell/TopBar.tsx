import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useRef, useState } from "react";
import { runAction, shortcutLabel, titleWithKeys, useActions } from "../actions";
import type { Project, SavedConnection } from "../api";
import mark from "../assets/brand/platypus-mark.png";
import { CONNECTION_TYPES } from "../connections";
import { useSetting } from "../settings";
import { PRESETS, cycleTheme, themeName, themeSetting, titleBarSetting } from "../theme";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";
import { useActivity } from "./activity";
import { DRAG_ATTR, dragHandlers } from "./windowDrag";
import { openActivity } from "./ActivityDialog";
import { SIDEBAR_TOGGLE_ID, toggleSidebar, toggleSidebarTitle, useSidebarCollapsed } from "./sidebar";
import { t, plural } from "../i18n";

interface Props {
  project: Project | null;
  /** The connection of the active tab. */
  connection: SavedConnection | undefined;
}

/** The bar on the window frame: where the work is (project, connection and its type), the
 *  search that opens the palette, the theme and the settings. */
export function TopBar({ project, connection }: Props) {
  const palette = useActions("global", undefined).find((a) => a.id === "app.palette");
  const keys = palette && shortcutLabel(palette);
  const type = CONNECTION_TYPES.find((t) => t.value === connection?.connection_type);
  const ownTitleBar = useSetting(titleBarSetting) !== "system" && !MAC;
  const sidebarCollapsed = useSidebarCollapsed();
  // The window opens with the bar the setting asks for (lib.rs); a change applies at once.
  const opened = useRef(ownTitleBar);
  useEffect(() => {
    if (MAC || ownTitleBar === opened.current) return;
    opened.current = ownTitleBar;
    getCurrentWindow().setDecorations(!ownTitleBar).catch((e: unknown) => console.warn("decorations", e));
  }, [ownTitleBar]);
  // The bar moves the window where nothing else takes the click (data-tauri-drag-region on
  // each element that is not a control); on Linux its own handlers do (windowDrag.ts).
  const drag = !ownTitleBar ? {} : LINUX ? { [DRAG_ATTR]: true } : { "data-tauri-drag-region": true };
  const [linuxDrag] = useState(() => dragHandlers(getCurrentWindow(), (e) => console.warn("window drag", e)));
  const barHandlers = ownTitleBar && LINUX ? linuxDrag : {};
  return (
    <header className={`topbar${ownTitleBar ? " own-title-bar" : ""}`} {...drag} {...barHandlers}>
      <img className="topbar-mark" src={mark} alt="PlatypusDB" width={26} height={26} {...drag} />
      <button
        type="button"
        id={SIDEBAR_TOGGLE_ID}
        className="icon"
        title={toggleSidebarTitle()}
        aria-label={t("Show or hide the navigator")}
        aria-expanded={!sidebarCollapsed}
        onClick={toggleSidebar}
      >
        <Icon name="sidebar" />
      </button>
      <span className="topbar-project" {...drag}>
        {project?.name ?? t("All connections")}
      </span>
      {connection && (
        <>
          <span className="topbar-sep" aria-hidden="true" {...drag}>
            /
          </span>
          <span className="topbar-connection" {...drag}>
            <span className="dot" aria-hidden="true" />
            {connection.name}
            {type && <span className="topbar-type">{t(type.label)}</span>}
          </span>
        </>
      )}
      <button
        type="button"
        className="topbar-search"
        title={t("Search everything")}
        onClick={() => palette && runAction(palette, undefined, () => {})}
      >
        <Icon name="search" size={14} />
        <span>{t("Search tables, queries, commands")}</span>
        {keys && <kbd>{keys}</kbd>}
      </button>
      <ActivityButton />
      <ThemeButton />
      <button type="button" className="icon" title={titleWithKeys(t("Settings"), "app.settings")} aria-label={t("Settings")} onClick={() => workbench().openSettings()}>
        <Icon name="settings" />
      </button>
      {ownTitleBar && <WindowControls />}
    </header>
  );
}

const MAC = navigator.userAgent.includes("Mac");
const LINUX = navigator.userAgent.includes("Linux");

/** Minimize, maximize or restore, and close, as the system's title bar has them. */
function WindowControls() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    const update = () => void win.isMaximized().then(setMaximized, () => {});
    update();
    const off = win.onResized(update);
    return () => void off.then((unlisten) => unlisten());
  }, []);
  const win = getCurrentWindow();
  return (
    <div className="window-controls">
      <button type="button" aria-label={t("Minimize")} title={t("Minimize")} onClick={() => void win.minimize()}>
        <Icon name="minimize" size={15} />
      </button>
      <button
        type="button"
        aria-label={t(maximized ? "Restore" : "Maximize")}
        title={t(maximized ? "Restore" : "Maximize")}
        onClick={() => void win.toggleMaximize()}
      >
        <Icon name={maximized ? "restore" : "maximize"} size={13} />
      </button>
      <button type="button" className="close" aria-label={t("Close")} title={t("Close")} onClick={() => void win.close()}>
        <Icon name="close" size={16} />
      </button>
    </div>
  );
}

/** Opens the Activity log; it counts the errors not seen yet and shows when work runs. */
function ActivityButton() {
  const { entries, unseen } = useActivity();
  const running = entries.filter((e) => e.status === "running").length;
  const label = [unseen > 0 && plural(unseen, "{n} new error", "{n} new errors"), running > 0 && t("{n} running", { n: running })]
    .filter(Boolean)
    .join(", ");
  return (
    <button
      type="button"
      className="icon activity-button"
      title={label ? `${t("Activity and errors")} (${label})` : t("Activity and errors")}
      aria-label={label ? `${t("Activity")}, ${label}` : t("Activity")}
      onClick={openActivity}
    >
      <Icon name="activity" />
      {unseen > 0 ? <span className="badge">{unseen > 99 ? "99+" : unseen}</span> : running > 0 && <span className="busy" aria-hidden="true" />}
    </button>
  );
}

/** Cycles system, Platypus Light and Platypus Dark; the other themes are in the Settings. */
function ThemeButton() {
  useSetting(themeSetting);
  const name = themeName();
  const label = t(name === "system" ? "System" : PRESETS[name].label);
  return (
    <button
      type="button"
      className="icon"
      title={t("Theme: {name} (click to change; more in the Settings)", { name: label })}
      aria-label={t("Change theme")}
      onClick={cycleTheme}
    >
      <Icon name="theme" />
    </button>
  );
}
