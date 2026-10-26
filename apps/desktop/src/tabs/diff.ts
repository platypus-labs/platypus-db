import { DiffTab, type DiffTabModel } from "../analysis/DiffTab";
import { registerTabKind } from "./registry";

registerTabKind({
  kind: "diff",
  component: DiffTab,
  create: (init) => ({ ...init, diff: init.diff ?? null }),
  label: (tab: DiffTabModel) => tab.title ?? "",
  // The rows compared are gone after a restart.
  persist: () => null,
  restore: () => ({}),
});
