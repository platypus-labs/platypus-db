// Lists, per language, the pages of the user guide that have no translation yet and those whose
// English original changed since they were translated. A translated page starts with
// `<!-- source: <12 hex> -->`, the first 12 hex digits of the SHA-256 of the English page it
// translates (line ends as LF). It never fails: the English guide stays editable.
//
//   node scripts/guide-translations.mjs          every language
//   node scripts/guide-translations.mjs it       one language
//   node scripts/guide-translations.mjs --hash docs/user/guide/connect.md   the value to write
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const GUIDE = "docs/user/guide/";
const TRANSLATIONS = "docs/user/translations/";
/** The first line of a translated page. */
export const SOURCE_LINE = /^<!-- source: ([0-9a-f]{12}) -->$/;

/** The 12 hex digits that name this text of an English page. */
export const sourceHash = (text) => createHash("sha256").update(text.replaceAll("\r\n", "\n")).digest("hex").slice(0, 12);

/** The Markdown files below `dir`, as paths relative to it with `/`. */
const pagesIn = (dir) =>
  existsSync(dir)
    ? readdirSync(dir, { recursive: true })
        .map((f) => String(f).replaceAll("\\", "/"))
        .filter((f) => f.endsWith(".md"))
        .sort()
    : [];

/** The languages that have a translation folder. */
export const languagesIn = (dir = root) => (existsSync(join(dir, TRANSLATIONS)) ? readdirSync(join(dir, TRANSLATIONS), { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name).sort() : []);

/**
 * What is wrong with the translation into `lang`: `missing` pages (no file), `stale` ones (source line
 * names another version of the original), `orphans` (no English original) and `malformed` ones (no
 * source line).
 */
export function report(lang, dir = root) {
  const english = pagesIn(join(dir, GUIDE));
  const translated = pagesIn(join(dir, TRANSLATIONS, lang, "guide"));
  const out = { missing: [], stale: [], orphans: [], malformed: [] };
  for (const page of english) {
    const file = join(dir, TRANSLATIONS, lang, "guide", page);
    if (!translated.includes(page)) {
      out.missing.push(page);
      continue;
    }
    const first = readFileSync(file, "utf8").replaceAll("\r\n", "\n").split("\n", 1)[0];
    const found = SOURCE_LINE.exec(first);
    if (!found) out.malformed.push(page);
    else if (found[1] !== sourceHash(readFileSync(join(dir, GUIDE, page), "utf8"))) out.stale.push(page);
  }
  out.orphans = translated.filter((p) => !english.includes(p));
  return out;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  if (args[0] === "--hash") {
    for (const file of args.slice(1)) console.log(`<!-- source: ${sourceHash(readFileSync(join(root, file), "utf8"))} -->  ${file}`);
  } else {
    for (const lang of args.length > 0 ? args : languagesIn()) {
      const r = report(lang);
      const total = pagesIn(join(root, GUIDE)).length;
      console.log(`${lang}: ${total - r.missing.length}/${total} pages translated, ${r.stale.length} stale`);
      for (const [name, pages] of Object.entries(r)) for (const p of pages) console.log(`  ${name}: ${p}`);
    }
  }
}
