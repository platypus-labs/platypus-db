import { createElement } from "react";
import { registerAction } from "../actions";
import { isNoSql } from "../api";
import { workbench } from "../workbench";
import { CompareDialog } from "./CompareDialog";
import { DataCompareDialog } from "./DataCompareDialog";
import { LintDialog } from "./LintDialog";

registerAction({
  id: "schema.lint",
  scope: "navigator.schema",
  title: "Check schema…",
  group: "open",
  order: 36,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(LintDialog, { connection, database, schema: schema.name, onClose: close })),
});

registerAction({
  id: "schema.compare",
  scope: "navigator.schema",
  title: "Compare with…",
  group: "open",
  order: 37,
  when: ({ connection }) => !isNoSql(connection.driver),
  run: ({ connection, database, schema }) =>
    workbench().openDialog((close) => createElement(CompareDialog, { connection, database, schema: schema.name, onClose: close })),
});

registerAction({
  id: "schema.compare_data",
  scope: "navigator.object",
  title: "Compare data with…",
  group: "open",
  order: 38,
  when: ({ connection, object }) => !isNoSql(connection.driver) && object.kind === "table",
  run: ({ connection, database, schema, object }) =>
    workbench().openDialog((close) =>
      createElement(DataCompareDialog, { connection, database, schema: schema.name, table: object.name, onClose: close }),
    ),
});
