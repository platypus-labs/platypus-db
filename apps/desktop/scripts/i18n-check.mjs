// Checks the translations (src/i18n.ts): every text given to `t`, `tNodes`, `msg` or
// `plural`, and every action title and setting label, has its entry in each dictionary, and
// no entry is left over. With `--untranslated` it also lists the text shown in JSX without `t`
// and the `label` properties written as plain text.
import { readdirSync, readFileSync } from "node:fs";
import { join, relative, sep } from "node:path";
import { parseAst } from "rolldown/parseAst";

const src = join(import.meta.dirname, "..", "src");
const showRaw = process.argv.includes("--untranslated");

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? files(join(dir, e.name)) : /\.tsx?$/.test(e.name) && !e.name.endsWith(".d.ts") ? [join(dir, e.name)] : [],
  );
}

/** Calls whose object argument holds text shown by the menus, the palette and the Settings. */
const DECLARING = new Set(["registerAction", "defineSetting"]);
/** Their fields that are shown. */
const SHOWN = new Set(["title", "label", "description", "section", "submenu"]);
/** JSX attributes that are read or shown. */
const ATTRS = new Set(["title", "placeholder", "aria-label", "alt"]);

/** A string literal, or a template without substitutions. */
function literal(n) {
  if (!n) return null;
  if (n.type === "Literal" && typeof n.value === "string") return n.value;
  if (n.type === "TemplateLiteral" && n.expressions.length === 0) return n.quasis[0].value.cooked;
  return null;
}

const keyName = (n) => (n.type === "Identifier" ? n.name : literal(n));

/** Every node below `node`, with whether a declaring call holds it. */
function walk(node, visit, declaring = false) {
  if (!node || typeof node.type !== "string") return;
  const inside = declaring || (node.type === "CallExpression" && node.callee.type === "Identifier" && DECLARING.has(node.callee.name));
  visit(node, declaring);
  for (const value of Object.values(node)) {
    if (Array.isArray(value)) value.forEach((v) => v && typeof v === "object" && walk(v, visit, inside));
    else if (value && typeof value === "object" && typeof value.type === "string") walk(value, visit, inside);
  }
}

const texts = new Map(); // text -> first place
const raw = [];

for (const file of files(src)) {
  if (relative(src, file).startsWith(`i18n${sep}`)) continue;
  const code = readFileSync(file, "utf8");
  const ast = parseAst(code, { lang: file.endsWith("x") ? "tsx" : "ts" });
  const line = (n) => code.slice(0, n.start).split("\n").length;
  const where = (n) => `${relative(src, file)}:${line(n)}`;
  /** The text of a literal, or of each branch of `a ? "x" : "y"`. */
  const add = (node, n) => {
    if (node?.type === "ConditionalExpression") return [node.consequent, node.alternate].forEach((b) => add(b, n));
    const text = literal(node);
    if (text && !texts.has(text)) texts.set(text, where(n));
  };
  walk(ast, (n, declaring) => {
    if (n.type === "CallExpression" && n.callee.type === "Identifier") {
      if (n.callee.name === "t" || n.callee.name === "msg" || n.callee.name === "tNodes") add(n.arguments[0], n);
      if (n.callee.name === "plural") [1, 2].forEach((i) => add(n.arguments[i], n));
    }
    if (declaring && n.type === "Property" && SHOWN.has(keyName(n.key))) {
      add(n.value, n);
      // Text from a variable: its literal must be marked with `msg` where it is written.
      if (showRaw && literal(n.value) === null) raw.push(`${where(n)}  ${keyName(n.key)}: (not a literal)`);
    }
    if (!showRaw) return;
    // A `label` written as plain text outside the declaring calls (as the navigator's folders
    // were) is shown as it is, unless it is a name (a driver, a format) that needs none.
    if (!declaring && n.type === "Property" && keyName(n.key) === "label" && /[A-Za-z]{2}/.test(literal(n.value) ?? "")) {
      raw.push(`${where(n)}  label: ${JSON.stringify(literal(n.value))}`);
    }
    if (n.type === "JSXText" && /[A-Za-z]{2}/.test(n.value)) raw.push(`${where(n)}  ${n.value.trim()}`);
    if (n.type === "JSXAttribute" && ATTRS.has(n.name.name) && /[A-Za-z]{2}/.test(literal(n.value) ?? "")) {
      raw.push(`${where(n)}  ${n.name.name}="${literal(n.value)}"`);
    }
  });
}

let errors = 0;
for (const name of readdirSync(join(src, "i18n")).filter((f) => f.endsWith(".ts"))) {
  const code = readFileSync(join(src, "i18n", name), "utf8");
  const keys = new Set();
  walk(parseAst(code, { lang: "ts" }), (n) => {
    if (n.type !== "Property") return;
    const key = keyName(n.key);
    if (keys.has(key)) {
      console.error(`${name}: "${key}" twice`);
      errors++;
    }
    keys.add(key);
  });
  for (const [text, at] of texts) {
    if (!keys.has(text)) {
      console.error(`${name}: missing "${text}" (${at})`);
      errors++;
    }
  }
  for (const key of keys) {
    if (!texts.has(key)) {
      console.error(`${name}: "${key}" is not used`);
      errors++;
    }
  }
}
if (showRaw) console.log(raw.join("\n"));
if (errors > 0) {
  console.error(`${errors} translation problems`);
  process.exit(1);
}
console.log(`i18n: ${texts.size} texts translated`);
