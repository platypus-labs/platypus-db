/**
 * Spell checking of comments (and, if chosen, strings) with the web view's own checker:
 * the editor turns it off for its text and back on for those ranges only, so keywords and
 * names are never underlined.
 */
import { syntaxTree } from "@codemirror/language";
import { type Extension, RangeSetBuilder } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { defineSetting } from "../settings";

export const spellcheckSetting = defineSetting<string>({
  key: "editor.spellcheck",
  section: "SQL editor",
  label: "Spell check",
  default: "comments",
  control: {
    type: "select",
    options: [
      { value: "off", label: "Off" },
      { value: "comments", label: "Comments" },
      { value: "all", label: "Comments and strings" },
    ],
  },
});

const mark = Decoration.mark({ attributes: { spellcheck: "true" }, class: "cm-spellcheck" });

function marks(view: EditorView, strings: boolean): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        if (name === "LineComment" || name === "BlockComment" || (strings && name === "String")) {
          builder.add(node.from, node.to, mark);
        }
      },
    });
  }
  return builder.finish();
}

/** The extension for a `spellcheckSetting` value. */
export function spellcheck(mode: string): Extension {
  if (mode === "off") return [];
  const strings = mode === "all";
  return [
    EditorView.contentAttributes.of({ spellcheck: "false" }),
    ViewPlugin.fromClass(
      class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
          this.decorations = marks(view, strings);
        }
        update(u: ViewUpdate) {
          if (u.docChanged || u.viewportChanged || syntaxTree(u.startState) !== syntaxTree(u.state)) {
            this.decorations = marks(u.view, strings);
          }
        }
      },
      { decorations: (p) => p.decorations },
    ),
  ];
}
