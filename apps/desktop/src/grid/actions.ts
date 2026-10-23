// The grid's built-in actions. Features register more for the "grid" scope from their
// own modules; they appear in the grid's context menu and answer to their shortcuts.
import { registerAction } from "../actions";

registerAction({
  id: "grid.copy",
  scope: "grid",
  title: "Copy",
  shortcut: "Mod+C",
  group: "copy",
  order: 10,
  when: ({ grid }) => grid.selection() !== null,
  run: async ({ grid }) => {
    const sel = grid.selection()!;
    const lines: string[] = [];
    for (let r = sel.r0; r <= sel.r1; r++) {
      const cells: string[] = [];
      for (let c = sel.c0; c <= sel.c1; c++) cells.push(grid.value(r, c) ?? "");
      lines.push(cells.join("\t"));
    }
    await navigator.clipboard.writeText(lines.join("\n"));
  },
});

registerAction({
  id: "grid.select_all",
  scope: "grid",
  title: "Select all",
  shortcut: "Mod+A",
  group: "select",
  order: 50,
  when: ({ grid }) => grid.rowCount() > 0 && grid.columns().length > 0,
  run: ({ grid }) =>
    grid.select({ row: 0, col: 0 }, { row: grid.rowCount() - 1, col: grid.columns().length - 1 }),
});
