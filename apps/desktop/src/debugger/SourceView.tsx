/** The source of a debugged routine, read-only: line numbers, a gutter of breakpoints that a
 *  click sets or clears, and the line the call is stopped at. */
import { useEffect, useRef } from "react";
import { PostgreSQL } from "@codemirror/lang-sql";
import { LanguageSupport, syntaxHighlighting } from "@codemirror/language";
import { EditorState, RangeSet, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, GutterMarker, gutter, lineNumbers } from "@codemirror/view";
import { highlight, theme } from "../editor/SqlEditor";

class BreakpointMarker extends GutterMarker {
  override toDOM() {
    const dot = document.createElement("span");
    dot.className = "debug-breakpoint-dot";
    return dot;
  }
}

const marker = new BreakpointMarker();

/** The lines with a breakpoint, and the line stopped at (0 for none). */
const setMarks = StateEffect.define<{ breakpoints: number[]; line: number }>();

const dotsField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(dots, tr) {
    for (const e of tr.effects) {
      if (e.is(setMarks)) {
        const doc = tr.state.doc;
        dots = RangeSet.of(e.value.breakpoints.map((n) => marker.range(doc.line(n).from)));
      }
    }
    return dots;
  },
});

const lineField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    for (const e of tr.effects) {
      if (e.is(setMarks)) {
        deco = e.value.line
          ? Decoration.set([Decoration.line({ class: "debug-current-line" }).range(tr.state.doc.line(e.value.line).from)])
          : Decoration.none;
      }
    }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

interface Props {
  source: string;
  /** The line the call is stopped at, from 1; `null` when it runs or has ended. */
  line: number | null;
  breakpoints: ReadonlySet<number>;
  /** Breakpoints can be changed only while the call is stopped. */
  editable: boolean;
  onToggle: (line: number, on: boolean) => void;
}

export function SourceView({ source, line, breakpoints, editable, onToggle }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  // Read by the gutter's handler, which is made once per source.
  const current = useRef({ breakpoints, editable, onToggle });
  current.current = { breakpoints, editable, onToggle };

  useEffect(() => {
    if (!host.current) return;
    const state = EditorState.create({
      doc: source,
      extensions: [
        dotsField,
        lineField,
        gutter({
          class: "debug-breakpoints",
          markers: (v) => v.state.field(dotsField),
          // Wide enough for a dot on every line, so that each can be clicked.
          initialSpacer: () => marker,
          domEventHandlers: {
            mousedown: (v, block) => {
              const { breakpoints, editable, onToggle } = current.current;
              if (!editable) return false;
              const n = v.state.doc.lineAt(block.from).number;
              onToggle(n, !breakpoints.has(n));
              return true;
            },
          },
        }),
        lineNumbers(),
        EditorState.readOnly.of(true),
        EditorView.editable.of(false),
        new LanguageSupport(PostgreSQL.language),
        syntaxHighlighting(highlight),
        theme,
      ],
    });
    const v = new EditorView({ state, parent: host.current });
    view.current = v;
    return () => {
      v.destroy();
      view.current = null;
    };
  }, [source]);

  useEffect(() => {
    const v = view.current;
    if (!v) return;
    const lines = v.state.doc.lines;
    const inDoc = (n: number) => n >= 1 && n <= lines;
    const here = line !== null && inDoc(line) ? line : 0;
    v.dispatch({
      effects: [
        setMarks.of({ breakpoints: [...breakpoints].filter(inDoc).sort((a, b) => a - b), line: here }),
        ...(here ? [EditorView.scrollIntoView(v.state.doc.line(here).from, { y: "center" })] : []),
      ],
    });
  }, [source, line, breakpoints]);

  return <div className={`debug-source${editable ? " editable" : ""}`} ref={host} data-field="debug-source" />;
}
