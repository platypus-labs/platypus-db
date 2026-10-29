// Arranging the grid's columns from its menu: hide, pin, the Columns dialog, the computed
// columns and a reset.
// Headers also drag to move columns, and their borders to resize them (see DataGrid).
import { registerAction } from "../../actions";
import type { GridActionContext } from "../DataGrid";
import { EMPTY_LAYOUT, hideColumn, pinColumn } from "../layout";

/** The focused column's name and the layout, when the grid can be arranged. */
function target({ grid }: GridActionContext) {
  const layout = grid.layout();
  const focused = grid.focused();
  const name = focused ? grid.columns()[focused.col]?.name : undefined;
  return layout && name !== undefined ? { layout, name } : null;
}

const pinned = (ctx: GridActionContext) => {
  const t = target(ctx);
  return t ? t.layout.pinned.includes(t.name) : null;
};

registerAction({
  id: "grid.hide_column",
  scope: "grid",
  title: "Hide column",
  submenu: "Columns",
  group: "columns",
  order: 80,
  when: (ctx) => target(ctx) !== null && ctx.grid.visibleColumns(true).length > 1,
  run: ({ grid }) => {
    const t = target({ grid, source: null })!;
    grid.setLayout(hideColumn(t.layout, t.name));
  },
});

registerAction({
  id: "grid.pin_column",
  scope: "grid",
  title: "Pin column",
  submenu: "Columns",
  group: "columns",
  order: 81,
  when: (ctx) => pinned(ctx) === false,
  run: ({ grid }) => {
    const t = target({ grid, source: null })!;
    grid.setLayout(pinColumn(t.layout, t.name, true));
  },
});

registerAction({
  id: "grid.unpin_column",
  scope: "grid",
  title: "Unpin column",
  submenu: "Columns",
  group: "columns",
  order: 81,
  when: (ctx) => pinned(ctx) === true,
  run: ({ grid }) => {
    const t = target({ grid, source: null })!;
    grid.setLayout(pinColumn(t.layout, t.name, false));
  },
});

registerAction({
  id: "grid.columns",
  scope: "grid",
  title: "Columns…",
  submenu: "Columns",
  group: "columns",
  order: 82,
  when: ({ grid, source }) => grid.layout() !== null && source?.features !== undefined,
  run: ({ source }) => source!.features!.openColumns(),
});

registerAction({
  id: "grid.computed_columns",
  scope: "grid",
  title: "Computed columns…",
  submenu: "Columns",
  group: "columns",
  order: 82.5,
  when: ({ source }) => source?.features !== undefined,
  run: ({ source }) => source!.features!.openComputed(),
});

registerAction({
  id: "grid.reset_columns",
  scope: "grid",
  title: "Reset columns",
  submenu: "Columns",
  group: "columns",
  order: 83,
  when: ({ grid }) => {
    const l = grid.layout();
    return !!l && (l.order.length > 0 || l.hidden.length > 0 || l.pinned.length > 0 || Object.keys(l.widths).length > 0);
  },
  run: ({ grid }) => grid.setLayout(EMPTY_LAYOUT),
});
