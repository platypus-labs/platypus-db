// "Debug…" in the navigator's menu of a PostgreSQL function or procedure. Whether the
// connection can debug, and the routine's language, are known only once its session answers:
// the dialog says why when it cannot.
import { createElement } from "react";
import { registerAction } from "../actions";
import { workbench } from "../workbench";
import { DebugDialog } from "./DebugDialog";

registerAction({
  id: "object.debug",
  scope: "navigator.object",
  title: "Debug…",
  group: "open",
  order: 22,
  when: ({ connection, object }) =>
    connection.driver === "postgres" && (object.kind === "function" || object.kind === "procedure"),
  run: ({ connection, database, schema, object }) =>
    workbench().openDialog((close) =>
      createElement(DebugDialog, { connection, database, schema: schema.name, name: object.name, onClose: close }),
    ),
});
