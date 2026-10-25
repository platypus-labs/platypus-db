import { runAction, shortcutLabel, useActions } from "../actions";
import type { Project, SavedConnection } from "../api";
import mark from "../assets/brand/platypus-mark.png";
import { CONNECTION_TYPES } from "../connections";
import { useSetting } from "../settings";
import { PRESETS, cycleTheme, themeName, themeSetting } from "../theme";
import { Icon } from "../ui/Icon";
import { workbench } from "../workbench";

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
  return (
    <header className="topbar">
      <img className="topbar-mark" src={mark} alt="PlatypusDB" width={26} height={26} />
      <span className="topbar-project">{project?.name ?? "All connections"}</span>
      {connection && (
        <>
          <span className="topbar-sep" aria-hidden="true">
            /
          </span>
          <span className="topbar-connection">
            <span className="dot" aria-hidden="true" />
            {connection.name}
            {type && <span className="topbar-type">{type.label}</span>}
          </span>
        </>
      )}
      <button
        type="button"
        className="topbar-search"
        title="Search everything"
        onClick={() => palette && runAction(palette, undefined, () => {})}
      >
        <Icon name="search" size={14} />
        <span>Search tables, queries, commands</span>
        {keys && <kbd>{keys}</kbd>}
      </button>
      <ThemeButton />
      <button type="button" className="icon" title="Settings (Ctrl+,)" aria-label="Settings" onClick={() => workbench().openSettings()}>
        <Icon name="settings" />
      </button>
    </header>
  );
}

/** Cycles system, Platypus Light and Platypus Dark; the other themes are in the Settings. */
function ThemeButton() {
  useSetting(themeSetting);
  const name = themeName();
  const label = name === "system" ? "System" : PRESETS[name].label;
  return (
    <button
      type="button"
      className="icon"
      title={`Theme: ${label} (click to change; more in the Settings)`}
      aria-label="Change theme"
      onClick={cycleTheme}
    >
      <Icon name="theme" />
    </button>
  );
}
