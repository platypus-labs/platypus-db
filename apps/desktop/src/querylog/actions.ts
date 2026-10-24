// The query manager opens from the command palette or its shortcut, wherever the focus is.
import { createElement } from "react";
import { registerAction } from "../actions";
import { workbench } from "../workbench";
import { QueryManager } from "./QueryManager";

registerAction({
  id: "app.query_manager",
  scope: "global",
  title: "Query manager",
  shortcut: "Mod+Shift+M",
  run: () => workbench().openDialog((close) => createElement(QueryManager, { onClose: close })),
});
