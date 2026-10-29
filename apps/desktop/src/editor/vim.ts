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
  description:
    "Vim's keys in the SQL editor: Esc for normal mode, :w saves the script. While the editor has the focus, Ctrl+W, Ctrl+O, Ctrl+F and Vim's other Ctrl keys are Vim's.",
  default: false,
  control: { type: "boolean" },
});

/** The Ctrl keys Vim uses: in an editor in Vim mode they are Vim's, not the app's shortcuts
 *  (Ctrl+W deletes a word instead of closing the tab). */
const VIM_CTRL_KEYS = new Set(["b", "d", "e", "f", "o", "r", "u", "v", "w", "y", "["]);

/** Whether `e` is a Ctrl key that Vim takes, in an editor in Vim mode. */
export function vimTakes(e: KeyboardEvent): boolean {
  return (
    e.ctrlKey &&
    !e.altKey &&
    !e.metaKey &&
    !e.shiftKey &&
    VIM_CTRL_KEYS.has(e.key.toLowerCase()) &&
    e.target instanceof Element &&
    // The mode line under the text: there in every mode, unlike `.cm-vimMode`.
    e.target.closest(".cm-editor")?.querySelector(".cm-vim-panel") != null
  );
}

/** What `:w` does in each editor. */
const saves = new WeakMap<EditorView, () => void>();

Vim.defineEx("write", "w", (cm) => saves.get(cm.cm6)?.());

/** The extension for `on`; first among the editor's, so Vim sees the keys before the others. */
export const vimMode = (on: boolean): Extension => (on ? vim({ status: true }) : []);

/** Makes `:w` in `view` call `save`. */
export function onVimWrite(view: EditorView, save: () => void) {
  saves.set(view, save);
}
