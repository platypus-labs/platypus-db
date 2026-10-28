// Vim mode of the SQL editor (@replit/codemirror-vim), a setting: Esc for normal mode, the
// mode shown under the text, and `:w` saving the script as Ctrl+S does.
import type { Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import { Vim, vim } from "@replit/codemirror-vim";
import { defineSetting } from "../settings";

export const vimSetting = defineSetting<boolean>({
  key: "editor.vim",
  section: "SQL editor",
  label: "Vim mode",
  description: "Vim's keys in the SQL editor: Esc for normal mode, :w saves the script.",
  default: false,
  control: { type: "boolean" },
});

/** What `:w` does in each editor. */
const saves = new WeakMap<EditorView, () => void>();

Vim.defineEx("write", "w", (cm) => saves.get(cm.cm6)?.());

/** The extension for `on`; first among the editor's, so Vim sees the keys before the others. */
export const vimMode = (on: boolean): Extension => (on ? vim({ status: true }) : []);

/** Makes `:w` in `view` call `save`. */
export function onVimWrite(view: EditorView, save: () => void) {
  saves.set(view, save);
}
