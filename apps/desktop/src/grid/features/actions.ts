// Search and the value panel, on the grid's shortcuts and menu.
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
