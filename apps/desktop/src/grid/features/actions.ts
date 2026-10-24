// Search, the value panel, the record view and counts by value, on the grid's shortcuts and
// menu.
import { registerAction } from "../../actions";

registerAction({
  id: "grid.find",
  scope: "grid",
  title: "Find…",
  shortcut: "Mod+F",
  group: "find",
  order: 60,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.find(),
});

registerAction({
  id: "grid.value_panel",
  scope: "grid",
  title: "Value panel",
  shortcut: "F7",
  group: "find",
  order: 61,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleValuePanel(),
});

registerAction({
  id: "grid.record_view",
  scope: "grid",
  title: "Record view",
  shortcut: "Mod+Alt+R",
  group: "view",
  order: 62,
  when: ({ source, grid }) => source?.features !== undefined && grid.rowCount() > 0,
  run: ({ source }) => source!.features!.toggleRecordView(),
});

registerAction({
  id: "grid.group_by",
  scope: "grid",
  title: "Count by value",
  group: "view",
  order: 63,
  when: ({ source, grid }) => source?.features !== undefined && grid.selection() !== null,
  run: ({ source, grid }) => source!.features!.groupBy(grid.selection()!.cols),
});
