import { msg, t } from "../i18n";

/** The warnings of a table or schema script (`Script.warnings`, written in English by
 *  `platypus-schema`), with `{name}` where they name a table, an index or a database. */
const TEMPLATES = [
  msg("{database} runs each statement by itself: if one fails, the ones before it stay applied."),
  msg("{database} cannot make these changes in place: the table is created again and its rows copied."),
  msg("Index {index} is on an expression: create it again by hand."),
  msg("Its triggers are created again as they were: check those that use a renamed or dropped column."),
  msg("Its triggers are created again."),
  msg("The primary key has no known name: drop it by hand."),
  msg("SQLite has no table comments: it is not saved."),
  msg("SQLite has no column comments: they are not saved."),
  msg("Drops the table {table}: its rows are lost."),
  msg("Creates the sequence {sequence}, which starts at 1: if its columns have values already, move it past them (setval)."),
];

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

const PATTERNS = TEMPLATES.map((template) => {
  const names: string[] = [];
  const source = template
    .split(/(\{\w+\})/)
    .map((part) => {
      const name = /^\{(\w+)\}$/.exec(part)?.[1];
      if (!name) return escape(part);
      names.push(name);
      return "(.+?)";
    })
    .join("");
  return { template, names, re: new RegExp(`^${source}$`, "s") };
});

/** A script warning in the interface language; one it does not know stays as it is. */
export function translateWarning(warning: string): string {
  for (const { template, names, re } of PATTERNS) {
    const m = re.exec(warning);
    if (m) return t(template, Object.fromEntries(names.map((n, i) => [n, m[i + 1]!])));
  }
  return warning;
}
