/**
 * Where the parts of the window go: the navigator, the results of the SQL editor, the tab bar
 * and the side panels. Each choice is an attribute on the root (`data-navigator`,
 * `data-results`, `data-tabbar`, `data-panels`) that the styles follow, so the React trees stay
 * as they are; the handles that resize a panel read the choice to keep widening the panel they
 * are dragged towards. A missing key is the default.
 */
import { type SettingDef, type WindowPart, defineSetting, getSetting, onSettingsChanged, useSetting } from "../settings";
import { msg } from "../i18n";

const SECTION = msg("Appearance");

const nav = (row: string): WindowPart => ({ part: "nav", row });

export const navigatorSideSetting = defineSetting<string>({
  key: "layout.navigator_side",
  section: SECTION,
  label: "Navigator",
  description: "The tree of the connections, on the left or on the right.",
  default: "left",
  control: {
    type: "pictures",
    options: [
      { value: "left", label: "Left", picture: { columns: "28px 1fr", rows: "8px 1fr", parts: [nav("1 / 3"), { part: "tabs" }, { part: "editor" }] } },
      { value: "right", label: "Right", picture: { columns: "1fr 28px", rows: "8px 1fr", parts: [{ part: "tabs" }, nav("1 / 3"), { part: "editor" }] } },
    ],
  },
});

export const resultsPositionSetting = defineSetting<string>({
  key: "layout.results_position",
  section: SECTION,
  label: "Results",
  description: "Below the SQL editor, or beside it for wide screens.",
  default: "below",
  control: {
    type: "pictures",
    options: [
      {
        value: "below",
        label: "Below",
        picture: { columns: "20px 1fr", rows: "8px 1fr 1fr", parts: [nav("1 / 4"), { part: "tabs" }, { part: "editor" }, { part: "results" }] },
      },
      {
        value: "beside",
        label: "Beside",
        picture: {
          columns: "20px 1fr 1fr",
          rows: "8px 1fr",
          parts: [nav("1 / 3"), { part: "tabs", column: "2 / 4" }, { part: "editor" }, { part: "results" }],
        },
      },
    ],
  },
});

export const tabBarPositionSetting = defineSetting<string>({
  key: "layout.tab_bar_position",
  section: SECTION,
  label: "Tab bar",
  description: "The open tabs, at the top or at the bottom of the window.",
  default: "top",
  control: {
    type: "pictures",
    options: [
      { value: "top", label: "At the top", picture: { columns: "20px 1fr", rows: "8px 1fr", parts: [nav("1 / 3"), { part: "tabs" }, { part: "editor" }] } },
      { value: "bottom", label: "At the bottom", picture: { columns: "20px 1fr", rows: "1fr 8px", parts: [nav("1 / 3"), { part: "editor" }, { part: "tabs" }] } },
    ],
  },
});

export const sidePanelsSideSetting = defineSetting<string>({
  key: "layout.side_panels_side",
  section: SECTION,
  label: "Side panels",
  description: "History, variables, the value and metadata of a cell: on the right or on the left of the work.",
  default: "right",
  control: {
    type: "pictures",
    options: [
      {
        value: "right",
        label: "On the right",
        picture: {
          columns: "20px 1fr 22px",
          rows: "8px 1fr",
          parts: [nav("1 / 3"), { part: "tabs" }, { part: "panel", row: "1 / 3" }, { part: "editor" }],
        },
      },
      {
        value: "left",
        label: "On the left",
        picture: {
          columns: "20px 22px 1fr",
          rows: "8px 1fr",
          parts: [nav("1 / 3"), { part: "panel", row: "1 / 3" }, { part: "tabs" }, { part: "editor" }],
        },
      },
    ],
  },
});

/** The root's attribute for each choice, with the value it takes. */
const ATTRIBUTES: [string, SettingDef<string>][] = [
  ["navigator", navigatorSideSetting],
  ["results", resultsPositionSetting],
  ["tabbar", tabBarPositionSetting],
  ["panels", sidePanelsSideSetting],
];

/** A stored value that is not one of the choices (edited by hand) counts as the default. */
function choice(def: SettingDef<string>): string {
  const value = getSetting(def);
  const c = def.control;
  return c.type === "pictures" && c.options.some((o) => o.value === value) ? value : def.default;
}

function applyLayout() {
  const root = document.documentElement;
  // Any setting change calls this: the styles are worked out again only on a new value.
  for (const [name, def] of ATTRIBUTES) {
    const value = choice(def);
    if (root.dataset[name] !== value) root.dataset[name] = value;
  }
}

/** Sets the root's attributes and follows the settings; call once they are loaded. */
export function startLayout() {
  applyLayout();
  onSettingsChanged(applyLayout);
}

/** The navigator is on the right of the work. */
export function useNavigatorRight(): boolean {
  useSetting(navigatorSideSetting);
  return choice(navigatorSideSetting) === "right";
}

/** The results of the SQL editor are beside it, not below. */
export function useResultsBeside(): boolean {
  useSetting(resultsPositionSetting);
  return choice(resultsPositionSetting) === "beside";
}

/** The side panels are on the left of the work. */
export function useSidePanelsLeft(): boolean {
  useSetting(sidePanelsSideSetting);
  return choice(sidePanelsSideSetting) === "left";
}
