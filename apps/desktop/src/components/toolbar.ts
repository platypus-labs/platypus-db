/** Which buttons the SQL editor's toolbar shows, and whether with words or icons only. */
import { defineSetting, useSetting } from "../settings";

const show = (key: string, label: string) =>
  defineSetting<boolean>({ key: `toolbar.${key}`, section: "Toolbar", label, default: true, control: { type: "boolean" } });

export const toolbarSettings = {
  script: show("show_script", "Run script button"),
  cancel: show("show_cancel", "Cancel button"),
  transactions: show("show_transactions", "Auto-commit, Commit and Rollback"),
  files: show("show_files", "Open and Save script"),
  history: show("show_history", "History"),
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
  };
  const iconsOnly = useSetting(toolbarLabels) === "icons";
  /** `icon text`, or the icon alone (the words move to the tooltip). */
  const label = (icon: string, text: string) => (iconsOnly ? icon : icon ? `${icon} ${text}` : text);
  return { visible, iconsOnly, label };
}
