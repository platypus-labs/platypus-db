import { msg, t } from "../i18n";

/** The warnings of a table or schema script (`Script.warnings`, written in English by
 *  `platypus-schema`), with `{name}` where they name a table, an index or a database. */
const TEMPLATES = [
  msg("{database} runs each statement by itself: if one fails, the ones before it stay applied."),
  msg("{database} cannot make these changes in place: the table is created again and its rows copied."),
  msg("Index {index} is on an expression: create it again by hand."),
  msg("Index {index} is an XML or spatial index: create it again by hand."),
  msg("Detaches partition {table}: it stays as a table of its own, with its rows; drop it by hand if it is not needed."),
  msg("The table {table} is not created: its engine replicates through the keeper path {path}, which a copy would share with the source table and join its replicas. Create it by hand with a path of its own (one with the uuid or database macro)."),
  msg("Its triggers are created again as they were: check those that use a renamed or dropped column."),
  msg("Its triggers are created again."),
  msg("The primary key has no known name: drop it by hand."),
  msg("SQLite has no table comments: it is not saved."),
  msg("SQLite has no column comments: they are not saved."),
  msg("Drops the table {table}: its rows are lost."),
  msg("Creates the sequence {sequence}, which starts at 1: if its columns have values already, move it past them (setval)."),
  msg("The partitioning of {table} differs and cannot be changed in place: create the table again by hand."),
  msg("The engine clause (ORDER BY, PARTITION BY, TTL) of {table} differs and cannot be changed in place: create the table again by hand."),
  msg("The bound of partition {table} differs: change it by hand (detach it and attach it again)."),
  msg("Column {column}: its identity or computation cannot be changed in place: drop it and add it again by hand."),
  msg("The type {type} differs from the one compared with: change it by hand."),
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
