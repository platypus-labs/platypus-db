// The data editor's grid actions: in the grid's context menu and on its shortcuts, for
// results of one table that can be edited.
import { registerAction } from "../../actions";
import type { GridActionContext } from "../DataGrid";

const editable = ({ source }: GridActionContext) => source?.edit?.editable === true;
const withSelection = (ctx: GridActionContext) => editable(ctx) && ctx.grid.selection() !== null;
const dirty = ({ source }: GridActionContext) => source?.edit?.dirty === true;

registerAction({
  id: "grid.edit_cell",
  scope: "grid",
  title: "Edit cell",
  shortcut: ["F2", "Enter"],
  group: "edit",
  order: 20,
  when: (ctx) => editable(ctx) && ctx.grid.focused() !== null,
  run: ({ grid, source }) => source!.edit!.editCell(grid.focused()!),
});

registerAction({
  id: "grid.set_null",
  scope: "grid",
  title: "Set to NULL",
  shortcut: "Shift+Delete",
  group: "edit",
  order: 21,
  when: withSelection,
  run: ({ source }) => source!.edit!.setSelection({ kind: "null" }),
});

registerAction({
  id: "grid.set_default",
  scope: "grid",
  title: "Set to default",
  group: "edit",
  order: 22,
  when: withSelection,
  run: ({ source }) => source!.edit!.setSelection({ kind: "default" }),
});

registerAction({
  id: "grid.paste",
  scope: "grid",
  title: "Paste",
  shortcut: "Mod+V",
  group: "edit",
  order: 23,
  when: (ctx) => editable(ctx) && ctx.grid.focused() !== null,
  run: ({ source }) => source!.edit!.paste(false),
});

registerAction({
  id: "grid.paste_rows",
  scope: "grid",
  title: "Paste as new rows",
  shortcut: "Mod+Shift+V",
  group: "rows",
  order: 33,
  when: editable,
  run: ({ source }) => source!.edit!.paste(true),
});

registerAction({
  id: "grid.add_row",
  scope: "grid",
  title: "Add row",
  shortcut: "Insert",
  group: "rows",
  order: 30,
  when: editable,
  run: ({ source }) => source!.edit!.addRow(),
});

registerAction({
  id: "grid.duplicate_row",
  scope: "grid",
  title: "Duplicate row",
  shortcut: "Mod+D",
  group: "rows",
  order: 31,
  when: withSelection,
  run: ({ source }) => source!.edit!.duplicateSelection(),
});

registerAction({
  id: "grid.delete_row",
  scope: "grid",
  title: "Delete row",
  shortcut: "Delete",
  group: "rows",
  order: 32,
  when: withSelection,
  run: ({ source }) => source!.edit!.deleteSelection(),
});

registerAction({
  id: "grid.save_changes",
  scope: "grid",
  title: "Save changes",
  shortcut: "Mod+S",
  group: "save",
  order: 40,
  when: (ctx) => editable(ctx) && dirty(ctx),
  run: ({ source }) => source!.edit!.save(),
});

registerAction({
  id: "grid.preview_changes",
  scope: "grid",
  title: "Preview SQL",
  group: "save",
  order: 41,
  when: dirty,
  run: ({ source }) => source!.edit!.preview(),
});

registerAction({
  id: "grid.undo_change",
  scope: "grid",
  title: "Undo change",
  shortcut: "Mod+Z",
  group: "save",
  order: 42,
  when: ({ source }) => source?.edit?.canUndo === true,
  run: ({ source }) => source!.edit!.undo(),
});

registerAction({
  id: "grid.discard_changes",
  scope: "grid",
  title: "Discard changes",
  group: "save",
  order: 43,
  when: dirty,
  run: ({ source }) => source!.edit!.discard(),
});
