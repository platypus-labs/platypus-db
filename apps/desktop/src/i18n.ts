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

export type Language = "en" | "it" | "es" | "de" | "fr" | "pt" | "zh";

/** The languages, by their code, with the locale their numbers and plurals follow. */
const LOCALES: Record<Language, string> = {
  en: "en-US",
  it: "it-IT",
  es: "es-ES",
  de: "de-DE",
  fr: "fr-FR",
  pt: "pt-BR",
  zh: "zh-CN",
};

type Dictionary = Record<string, string>;

/** Italian is in the bundle; the other dictionaries load when their language is chosen
 *  (`loadLanguage`), and until then the text shows in English. */
const DICTIONARIES: Partial<Record<Language, Dictionary>> = { it };
const LOADERS: Record<Exclude<Language, "en" | "it">, () => Promise<{ default: Dictionary }>> = {
  es: () => import("./i18n/es"),
  de: () => import("./i18n/de"),
  fr: () => import("./i18n/fr"),
  pt: () => import("./i18n/pt"),
  zh: () => import("./i18n/zh"),
};

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
      { value: "es", label: "Español" },
      { value: "de", label: "Deutsch" },
      { value: "fr", label: "Français" },
      { value: "pt", label: "Português" },
      { value: "zh", label: "中文" },
    ],
  },
});

const isLanguage = (code: string): code is Language => code in LOCALES;

/** The language shown: the setting's, or the system's when it is one the app speaks. */
export function language(): Language {
  const chosen = getSetting(languageSetting);
  if (isLanguage(chosen)) return chosen;
  const system = navigator.language.toLowerCase().slice(0, 2);
  return isLanguage(system) ? system : "en";
}

/** The locale of the interface's language, for `Intl` formats. */
export const locale = () => LOCALES[language()];

/** Loads the dictionary of the language shown, if it is not loaded yet. */
export async function loadLanguage(): Promise<void> {
  const lang = language();
  if (lang === "en" || lang === "it" || DICTIONARIES[lang]) return;
  DICTIONARIES[lang] = (await LOADERS[lang]()).default;
}

/** `text` in the interface language; `{name}` in it is replaced by `vars.name`. */
export function t(text: string, vars?: Record<string, string | number>): string {
  const lang = language();
  const out = (lang !== "en" && DICTIONARIES[lang]?.[text]) || text;
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
const pluralRules = new Map<string, Intl.PluralRules>();

/** The `cache`d value for the interface's locale, made by `make` the first time. */
function forLocale<T>(cache: Map<string, T>, make: (locale: string) => T): T {
  const at = locale();
  let value = cache.get(at);
  if (!value) cache.set(at, (value = make(at)));
  return value;
}

/** A count grouped as the interface's language writes it ("1,500" in English, "1500" and
 *  "100.000" in Italian), not as the operating system's does. */
export function formatCount(n: number): string {
  // One formatter per language: `toLocaleString` builds one at every call, ~40 µs, which a
  // navigator of 10,000 tables pays at every render.
  return forLocale(countFormats, (at) => new Intl.NumberFormat(at)).format(n);
}

/** `one` or `many` by `n`, translated, with `{n}` replaced: `plural(n, "{n} row", "{n} rows")`. */
export function plural(n: number, one: string, many: string, vars?: Record<string, string | number>): string {
  // The language's rule: French says "0 ligne", as "1 ligne".
  const rules = forLocale(pluralRules, (at) => new Intl.PluralRules(at));
  return t(rules.select(n) === "one" ? one : many, { ...vars, n: formatCount(n) });
}

/** Marks text that is translated where it is shown, e.g. a label declared at load time, for
 *  the translation check; it returns the text as it is. */
export const msg = (text: string) => text;
