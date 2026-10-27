// The data editor's grid actions: in the grid's context menu and on its shortcuts, for
// results of one table that can be edited.
import { isMac, registerAction } from "../../actions";
import type { GridActionContext } from "../DataGrid";
import { msg } from "../../i18n";
import { type ColumnRule, isNumberRule, loadsFromFile } from "./massEdit";

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
  id: "grid.load_file",
  scope: "grid",
  title: "Load from file…",
  group: "edit",
  order: 24,
  when: (ctx) => {
    const f = ctx.grid.focused();
    return editable(ctx) && f !== null && loadsFromFile(ctx.source!.edit!.rule(f.col));
  },
  run: async ({ grid, source }) => {
    await source!.edit!.loadFile(grid.focused()!);
  },
});

// Mass edit of the selection, in a submenu. Ctrl+D duplicates rows, so filling down takes
// Ctrl+Alt+D; on macOS Control+Shift+D, as Cmd+Option+D hides the Dock and Option changes
// the key typed.
const SELECTION = msg("Edit selection");

/** Whether a selected column's rule passes `test`. */
const someColumn = ({ grid, source }: GridActionContext, test: (rule: ColumnRule | undefined) => boolean) =>
  (grid.selection()?.cols ?? []).some((c) => test(source!.edit!.rule(c)));

registerAction({
  id: "grid.set_value",
  scope: "grid",
  title: "Set value…",
  submenu: SELECTION,
  group: "edit",
  order: 25,
  when: withSelection,
  run: ({ source }) => source!.edit!.massEdit("set"),
});

registerAction({
  id: "grid.fill_down",
  scope: "grid",
  title: "Fill down",
  shortcut: isMac ? "Ctrl+Shift+D" : "Mod+Alt+D",
  submenu: SELECTION,
  group: "edit",
  order: 26,
  when: withSelection,
  disabled: ({ grid }) => {
    const sel = grid.selection();
    return sel && sel.r1 > sel.r0 ? null : msg("Select two rows or more");
  },
  run: ({ source }) => source!.edit!.fillDown(),
});

registerAction({
  id: "grid.replace_text",
  scope: "grid",
  title: "Replace text…",
  submenu: SELECTION,
  group: "edit",
  order: 27,
  when: withSelection,
  disabled: (ctx) => (someColumn(ctx, (r) => r?.kind === "text") ? null : msg("The selection has no text columns")),
  run: ({ source }) => source!.edit!.massEdit("replace"),
});

registerAction({
  id: "grid.number_sequence",
  scope: "grid",
  title: "Number sequence…",
  submenu: SELECTION,
  group: "edit",
  order: 28,
  when: withSelection,
  disabled: (ctx) => (someColumn(ctx, isNumberRule) ? null : msg("The selection has no number columns")),
  run: ({ source }) => source!.edit!.massEdit("sequence"),
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
  id: "grid.redo_change",
  scope: "grid",
  title: "Redo change",
  shortcut: ["Mod+Y", "Mod+Shift+Z"],
  group: "save",
  order: 42,
  when: ({ source }) => source?.edit?.canRedo === true,
  run: ({ source }) => source!.edit!.redo(),
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

registerAction({
  id: "grid.display_column",
  scope: "grid",
  title: "Show column of the referenced table…",
  group: "edit",
  order: 40,
  when: ({ grid, source }) => {
    const f = grid.focused();
    return f !== null && source?.edit?.foreignKey(f.col) != null;
  },
  run: ({ grid, source }) => source!.edit!.chooseDisplayColumn(grid.focused()!.col),
});