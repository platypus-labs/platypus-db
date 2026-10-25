/** Which buttons the SQL editor's toolbar shows, and whether with words or icons only. */
import type { ReactNode } from "react";
import { defineSetting, useSetting } from "../settings";
import { Icon, type IconName } from "../ui/Icon";
import { msg } from "../i18n";

const show = (key: string, label: string) =>
  defineSetting<boolean>({ key: `toolbar.${key}`, section: "Toolbar", label, default: true, control: { type: "boolean" } });

export const toolbarSettings = {
  script: show("show_script", msg("Run script button")),
  cancel: show("show_cancel", msg("Cancel button")),
  transactions: show("show_transactions", msg("Auto-commit, Commit and Rollback")),
  files: show("show_files", msg("Open and Save script")),
  history: show("show_history", msg("History")),
  refresh: show("show_refresh", msg("Auto-refresh")),
  panels: show("show_panels", msg("Variables, Outline and Terminal")),
};

export const toolbarLabels = defineSetting<string>({
  key: "toolbar.labels",
  section: "Toolbar",
  label: "Buttons",
  default: "icons",
  control: {
    type: "select",
    options: [
      { value: "text", label: "Icons and words" },
      { value: "icons", label: "Icons only" },
    ],
  },
});

/** The toolbar's visible buttons and how to label them. */
export function useToolbar() {
  const visible = {
    script: useSetting(toolbarSettings.script),
    cancel: useSetting(toolbarSettings.cancel),
    transactions: useSetting(toolbarSettings.transactions),
    files: useSetting(toolbarSettings.files),
    history: useSetting(toolbarSettings.history),
    refresh: useSetting(toolbarSettings.refresh),
    panels: useSetting(toolbarSettings.panels),
  };
  const iconsOnly = useSetting(toolbarLabels) === "icons";
  /** The icon and the words (already translated), or the icon alone: the button's
   *  `aria-label` and tooltip keep the words. Run is not labelled here: it keeps its word
   *  either way. */
  const label = (icon: IconName, text: string): ReactNode =>
    iconsOnly ? (
      <Icon name={icon} />
    ) : (
      <>
        <Icon name={icon} />
        {text}
      </>
    );
  return { visible, iconsOnly, label };
}
