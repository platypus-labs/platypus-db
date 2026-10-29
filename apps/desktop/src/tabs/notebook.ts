import { registerAction } from "../actions";
import { parseConfig } from "../analysis/chartConfig";
import { t } from "../i18n";
import { newCell, readContent } from "../notebook/model";
import { NotebookTab, openNotebook } from "../notebook/NotebookTab";
import { workbench } from "../workbench";
import { registerTabKind } from "./registry";

registerTabKind({
  kind: "notebook",
  component: NotebookTab,
  create: (init) => ({ cells: [], params: {}, file: null, ...init }),
  // The name of its file, once saved.
  label: (tab) => tab.title || t("Notebook"),
  // The cells and the parameters; the results run again.
  persist: (tab) => ({ cells: tab.cells, params: tab.params, file: tab.file, title: tab.title ?? null }),
  restore: (saved) => ({
    ...readContent(saved, parseConfig),
    file: typeof saved.file === "string" ? saved.file : null,
    title: typeof saved.title === "string" ? saved.title : null,
  }),
});

registerAction({
  id: "connection.new_notebook",
  scope: "navigator.connection",
  title: "New notebook",
  group: "open",
  order: 12,
  run: ({ connection }) =>
    workbench().openTab("notebook", connection.id, {
      cells: [newCell("markdown"), newCell("sql")],
    }),
});

registerAction({
  id: "connection.open_notebook",
  scope: "navigator.connection",
  title: "Open notebook…",
  group: "open",
  order: 13,
  run: ({ connection }) => openNotebook(connection),
});
