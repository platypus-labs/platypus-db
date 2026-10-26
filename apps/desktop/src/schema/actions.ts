import { createElement } from "react";
import { registerAction } from "../actions";
import { workbench } from "../workbench";
import { CompareDialog } from "./CompareDialog";
import { LintDialog } from "./LintDialog";

registerAction({
  id: "schema.lint",
  scope: "navigator.schema",
  title: "Check schema…",
  group: "open",
  order: 36,
  when: ({ connection }) => connection.driver !== "mongo",
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(LintDialog, { connection, database, schema: schema.name, onClose: close })),
});

registerAction({
  id: "schema.compare",
  scope: "navigator.schema",
  title: "Compare with…",
  group: "open",
  order: 37,
  when: ({ connection }) => connection.driver !== "mongo",
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(CompareDialog, { connection, database, schema: schema.name, onClose: close })),
});
