/**
 * The interface language. Text is written in English, and `t` looks it up in the language's
 * dictionary (`i18n/<lang>.ts`), keyed by the English text itself: a missing entry shows the
 * English. `npm run build` checks that every text has its translation (scripts/i18n-check.mjs).
 *
 * Text is translated where it is shown: action titles by the menus and the palette, setting
 * labels by the Settings dialog, so a feature declares them in English and nothing more.
 */
import { Fragment, type ReactNode, createElement } from "react";
import { defineSetting, getSetting } from "./settings";
import it from "./i18n/it";

export type Language = "en" | "it";

const DICTIONARIES: Record<Exclude<Language, "en">, Record<string, string>> = { it };

export const languageSetting = defineSetting<string>({
  key: "appearance.language",
  section: "Appearance",
  label: "Language",
  description: "The language of the interface. Messages from the databases stay in theirs.",
  default: "system",
  control: {
    type: "select",
    options: [
      { value: "system", label: "System" },
      { value: "en", label: "English" },
      { value: "it", label: "Italiano" },
    ],
  },
});

/** The language shown: the setting's, or the system's when it is one the app speaks. */
export function language(): Language {
  const chosen = getSetting(languageSetting);
  if (chosen === "en" || chosen === "it") return chosen;
  return navigator.language.toLowerCase().startsWith("it") ? "it" : "en";
}

/** `text` in the interface language; `{name}` in it is replaced by `vars.name`. */
export function t(text: string, vars?: Record<string, string | number>): string {
  const lang = language();
  const out = (lang !== "en" && DICTIONARIES[lang][text]) || text;
  return vars ? out.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : out;
}

/** Like `t`, with React nodes in place of `{name}`: a sentence with code samples in it stays
 *  one text to translate. */
export function tNodes(text: string, nodes: Record<string, ReactNode>): ReactNode[] {
  return t(text)
    .split(/\{(\w+)\}/)
    .map((part, i) => createElement(Fragment, { key: i }, i % 2 ? (nodes[part] ?? `{${part}}`) : part));
}

const countFormats = new Map<string, Intl.NumberFormat>();

/** A count grouped as the interface's language writes it ("1,500" in English, "1500" and
 *  "100.000" in Italian), not as the operating system's does. */
export function formatCount(n: number): string {
  const locale = language() === "it" ? "it-IT" : "en-US";
  // One formatter per language: `toLocaleString` builds one at every call, ~40 µs, which a
  // navigator of 10,000 tables pays at every render.
  let format = countFormats.get(locale);
  if (!format) countFormats.set(locale, (format = new Intl.NumberFormat(locale)));
  return format.format(n);
}

/** `one` or `many` by `n`, translated, with `{n}` replaced: `plural(n, "{n} row", "{n} rows")`. */
export function plural(n: number, one: string, many: string, vars?: Record<string, string | number>): string {
  return t(n === 1 ? one : many, { ...vars, n: formatCount(n) });
}

/** Marks text that is translated where it is shown, e.g. a label declared at load time, for
 *  the translation check; it returns the text as it is. */
export const msg = (text: string) => text;
