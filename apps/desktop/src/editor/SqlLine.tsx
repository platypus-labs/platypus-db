/** A one-line SQL field with the editor's colors and completion: Enter runs, Escape clears,
 *  Ctrl+Enter asks for a larger editor. */
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { syntaxHighlighting } from "@codemirror/language";
import { Annotation, Compartment, EditorState } from "@codemirror/state";
import { EditorView, keymap, placeholder } from "@codemirror/view";
import type { CompletionCatalog, DriverKind } from "../api";
import { highlight, language, theme } from "./SqlEditor";

export interface SqlLineHandle {
  focus(): void;
}

interface Props {
  value: string;
  driver: DriverKind;
  catalog: CompletionCatalog | null;
  placeholder: string;
  label: string;
  onChange: (text: string) => void;
  onRun: () => void;
  onClear: () => void;
  onExpand: () => void;
  /** Takes the focus when it appears, e.g. again after the grid under it changed. */
  autoFocus?: boolean;
}

const lineTheme = EditorView.theme({
  "&": { height: "auto", fontSize: "12px", backgroundColor: "transparent" },
  ".cm-scroller": { lineHeight: "20px", overflow: "hidden" },
  ".cm-content": { padding: "0" },
  ".cm-line": { padding: "0 4px" },
  ".cm-placeholder": { color: "var(--fg-muted)" },
});

/** Marks the text set from `value`, which is not reported back as a change. */
const fromProps = Annotation.define<boolean>();

/** Pasted or typed line breaks become spaces. */
const singleLine = EditorState.transactionFilter.of((tr) => {
  if (!tr.docChanged || tr.newDoc.lines === 1) return tr;
  const text = tr.newDoc.toString().replace(/\r?\n/g, " ");
  return [tr, { changes: { from: 0, to: tr.newDoc.length, insert: text }, sequential: true }];
});

export const SqlLine = forwardRef<SqlLineHandle, Props>(function SqlLine(props, ref) {
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const languageConf = useRef(new Compartment());
  // Keymaps read the latest callbacks without rebuilding the field.
  const callbacks = useRef(props);
  callbacks.current = props;

  useEffect(() => {
    const call = (name: "onRun" | "onClear" | "onExpand") => () => {
      callbacks.current[name]();
      return true;
    };
    const state = EditorState.create({
      doc: props.value,
      extensions: [
        history(),
        closeBrackets(),
        autocompletion(),
        singleLine,
        syntaxHighlighting(highlight),
        languageConf.current.of(language(props.driver, props.catalog)),
        placeholder(props.placeholder),
        EditorView.contentAttributes.of({ "aria-label": props.label }),
        // The completion list takes Enter and Escape first while it is open.
        keymap.of([
          { key: "Enter", run: call("onRun"), preventDefault: true },
          { key: "Mod-Enter", run: call("onExpand"), preventDefault: true },
          { key: "Escape", run: call("onClear"), preventDefault: true },
          ...closeBracketsKeymap,
          ...completionKeymap,
          ...historyKeymap,
          ...defaultKeymap,
        ]),
        EditorView.updateListener.of((u) => {
          if (u.docChanged && !u.transactions.some((tr) => tr.annotation(fromProps))) {
            callbacks.current.onChange(u.state.doc.toString());
          }
        }),
        theme,
        lineTheme,
      ],
    });
    const v = new EditorView({ state, parent: host.current! });
    view.current = v;
    if (props.autoFocus) v.focus();
    return () => {
      v.destroy();
      view.current = null;
    };
    // Created once; later props go through the effects below.
  }, []);

  useEffect(() => {
    const v = view.current;
    if (v && v.state.doc.toString() !== props.value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: props.value }, annotations: fromProps.of(true) });
    }
  }, [props.value]);

  useEffect(() => {
    view.current?.dispatch({ effects: languageConf.current.reconfigure(language(props.driver, props.catalog)) });
  }, [props.driver, props.catalog]);

  useImperativeHandle(ref, () => ({ focus: () => view.current?.focus() }));

  return <div className="sql-line" ref={host} />;
});
