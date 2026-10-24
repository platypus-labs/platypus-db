/** Which buttons the SQL editor's toolbar shows, and whether with words or icons only. */
import type { ReactNode } from "react";
import { defineSetting, useSetting } from "../settings";
import { Icon, type IconName } from "../ui/Icon";

const show = (key: string, label: string) =>
  defineSetting<boolean>({ key: `toolbar.${key}`, section: "Toolbar", label, default: true, control: { type: "boolean" } });

export const toolbarSettings = {
  script: show("show_script", "Run script button"),
  cancel: show("show_cancel", "Cancel button"),
  transactions: show("show_transactions", "Auto-commit, Commit and Rollback"),
  files: show("show_files", "Open and Save script"),
  history: show("show_history", "History"),
  refresh: show("show_refresh", "Auto-refresh"),
};

export const toolbarLabels = defineSetting<string>({
  key: "toolbar.labels",
  section: "Toolbar",
  label: "Buttons",
  default: "text",
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
  };
  const iconsOnly = useSetting(toolbarLabels) === "icons";
  /** The icon and the words, or the icon alone: the button's `aria-label` and tooltip keep
   *  the words. */
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
