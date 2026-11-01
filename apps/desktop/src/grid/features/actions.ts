// Search, the value panel, the record view, counts by value, column metadata, calculations,
// the map and comparing two rows, on the grid's shortcuts and menu.
import { registerAction } from "../../actions";
import { msg } from "../../i18n";

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

// Analysis of the shown rows, in a submenu: the first level keeps the main ways to look at them.
const ANALYZE = msg("Analyze");

registerAction({
  id: "grid.group_by",
  scope: "grid",
  title: "Count by value",
  submenu: ANALYZE,
  group: "view",
  order: 63,
  when: ({ source, grid }) => source?.features !== undefined && grid.selection() !== null,
  run: ({ source, grid }) => source!.features!.groupBy(grid.selection()!.cols),
});

registerAction({
  id: "grid.metadata",
  scope: "grid",
  title: "Column metadata",
  submenu: ANALYZE,
  group: "view",
  order: 64,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleMetadata(),
});

registerAction({
  id: "grid.calc",
  scope: "grid",
  title: "Calculate",
  submenu: ANALYZE,
  group: "view",
  order: 65,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleCalc(),
});

registerAction({
  id: "grid.map",
  scope: "grid",
  title: "Show on map",
  submenu: ANALYZE,
  group: "view",
  order: 66,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleMap(),
});

registerAction({
  id: "grid.conditional_formatting",
  scope: "grid",
  title: "Conditional formatting…",
  submenu: ANALYZE,
  group: "view",
  order: 67,
  when: ({ source, grid }) => source?.features !== undefined && grid.columns().length > 0,
  run: ({ source, grid }) => {
    const focused = grid.focused();
    source!.features!.openFormatting(focused ? (grid.columns()[focused.col]?.name ?? null) : null);
  },
});

registerAction({
  id: "grid.compare_rows",
  scope: "grid",
  title: "Compare the two rows",
  submenu: ANALYZE,
  group: "view",
  order: 67,
  when: ({ source, grid }) => {
    const sel = grid.selection();
    return (
      source?.features !== undefined &&
      sel !== null &&
      sel.r1 === sel.r0 + 1 &&
      grid.dataRow(sel.r0) !== null &&
      grid.dataRow(sel.r1) !== null
    );
  },
  run: ({ source, grid }) => {
    const sel = grid.selection()!;
    source!.features!.compareRows([grid.dataRow(sel.r0)!, grid.dataRow(sel.r1)!]);
  },
});
