// Search, the value panel, the record view, counts by value, column metadata, calculations
// and comparing two rows, on the grid's shortcuts and menu.
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

registerAction({
  id: "grid.metadata",
  scope: "grid",
  title: "Column metadata",
  group: "view",
  order: 64,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleMetadata(),
});

registerAction({
  id: "grid.calc",
  scope: "grid",
  title: "Calculate",
  group: "view",
  order: 65,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.toggleCalc(),
});

registerAction({
  id: "grid.compare_rows",
  scope: "grid",
  title: "Compare the two rows",
  group: "view",
  order: 66,
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
