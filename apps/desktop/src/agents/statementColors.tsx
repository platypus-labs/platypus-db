/**
 * A statement in the colors of the editor, for the dialog that asks about an agent's change.
 * Only how it looks: the text is the same text, whatever the parser makes of it.
 */
import { LanguageSupport } from "@codemirror/language";
import type { Extension } from "@codemirror/state";
import { classHighlighter, highlightCode } from "@lezer/highlight";
import type { ReactNode } from "react";
import type { DriverKind } from "../api";
import { language } from "../editor/SqlEditor";

/** The language inside what the editor configures for a driver, alone or with its completion. */
function supportIn(extension: Extension): LanguageSupport | null {
  if (extension instanceof LanguageSupport) return extension;
  if (!Array.isArray(extension)) return null;
  for (const inner of extension as readonly Extension[]) {
    const found = supportIn(inner);
    if (found) return found;
  }
  return null;
}

const supports = new Map<DriverKind, LanguageSupport | null>();

function supportOf(driver: DriverKind): LanguageSupport | null {
  if (!supports.has(driver)) supports.set(driver, supportIn(language(driver, null)));
  return supports.get(driver) ?? null;
}

/** `text` as spans of the classes of its tokens; as plain text where no language is known. */
export function colored(text: string, driver: DriverKind | null): ReactNode {
  const support = driver && supportOf(driver);
  if (!support) return text;
  const out: ReactNode[] = [];
  highlightCode(
    text,
    support.language.parser.parse(text),
    classHighlighter,
    (piece, classes) => {
      out.push(classes ? <span key={out.length} className={classes}>{piece}</span> : piece);
    },
    () => out.push("\n"),
  );
  return out;
}
