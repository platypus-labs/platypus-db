import { registerAction } from "../actions";
import { parseConfig } from "../analysis/chartConfig";
import { DashboardsDialog } from "../dashboard/DashboardsDialog";
import { DashboardTab } from "../dashboard/DashboardTab";
import { readContent } from "../dashboard/model";
import { t } from "../i18n";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";

registerTabKind({
  kind: "dashboard",
  component: DashboardTab,
  create: (init) => ({ widgets: [], refresh: 0, name: null, ...init }),
  // The name it is saved under, once saved.
  label: (tab) => tab.title || t("Dashboard"),
  // The widgets and the refresh interval; the rows are read again.
  persist: (tab) => ({ widgets: tab.widgets, refresh: tab.refresh, name: tab.name, title: tab.title ?? null }),
  restore: (saved) => {
    const { widgets, refresh } = readContent(saved, parseConfig);
    return {
      widgets,
      refresh,
      name: typeof saved.name === "string" ? saved.name : null,
      title: typeof saved.title === "string" ? saved.title : null,
    };
  },
});

registerAction({
  id: "connection.new_dashboard",
  scope: "navigator.connection",
  title: "New dashboard",
  group: "open",
  order: 14,
  run: ({ connection }) => workbench().openTab("dashboard", connection.id, {}),
});

registerAction({
  id: "connection.open_dashboard",
  scope: "navigator.connection",
  title: "Dashboards…",
  group: "open",
  order: 15,
  run: ({ connection }) => {
    const project = workbench().activeProject()?.id ?? null;
    workbench().openDialog((close) => <DashboardsDialog connection={connection} project={project} onClose={close} />);
  },
});
