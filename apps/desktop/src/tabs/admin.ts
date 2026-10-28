import { AdminTab } from "../admin/AdminTab";
import { registerTabKind } from "./registry";
import { t } from "../i18n";

registerTabKind({
  kind: "admin",
  component: AdminTab,
  create: (init) => ({ view: "sessions" as const, interval: 5, ...init }),
  label: () => t("Server activity"),
  persist: (tab) => ({ view: tab.view, interval: tab.interval }),
  restore: (saved) => ({
    view: saved.view === "locks" ? ("locks" as const) : ("sessions" as const),
    interval: typeof saved.interval === "number" ? saved.interval : 5,
  }),
});
