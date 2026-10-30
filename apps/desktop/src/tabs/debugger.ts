import { DebuggerTab } from "../debugger/DebuggerTab";
import "../debugger/actions";
import { t } from "../i18n";
import { registerTabKind } from "./registry";

registerTabKind({
  kind: "debugger",
  component: DebuggerTab,
  create: (init) => ({ ...init }),
  label: (tab) => tab.title ?? t("Debugger"),
  // The call ends with the app: nothing to open again.
  persist: () => null,
  restore: () => ({}),
});
