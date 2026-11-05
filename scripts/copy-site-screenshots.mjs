// Copies into site/img/screenshots the pictures of docs/screenshots that the site shows (in its
// pages, styles or scripts), and removes the ones nothing shows any more. Run it after the `readme`
// or `gifs` scenarios redo the pictures:
//
//   node scripts/copy-site-screenshots.mjs
//
// Exits 1 and changes nothing when the site shows a picture that docs/screenshots does not have.
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const site = join(root, "site");
const source = join(root, "docs", "screenshots");
const target = join(site, "img", "screenshots");

/** The files below `dir`, as paths with `/` relative to it. */
const filesBelow = (dir) =>
  readdirSync(dir, { recursive: true })
    .map((f) => String(f).replaceAll("\\", "/"))
    .filter((f) => statSync(join(dir, f)).isFile());

/** The pictures, by their path below img/screenshots, that the given texts show. */
export function picturesIn(texts) {
  const names = new Set();
  for (const text of texts) {
    for (const m of text.matchAll(/img\/screenshots\/([\w./-]+\.[a-z]+)/gi)) names.add(m[1]);
  }
  return [...names].sort();
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const texts = filesBelow(site)
    .filter((f) => /\.(html|css|js)$/.test(f))
    .map((f) => readFileSync(join(site, f), "utf8"));
  const shown = picturesIn(texts);
  const missing = shown.filter((name) => !existsSync(join(source, name)));
  if (missing.length > 0) {
    console.error(`not in docs/screenshots: ${missing.join(", ")}`);
    process.exit(1);
  }
  for (const name of shown) {
    mkdirSync(dirname(join(target, name)), { recursive: true });
    copyFileSync(join(source, name), join(target, name));
  }
  const stale = filesBelow(target).filter((name) => !shown.includes(name));
  for (const name of stale) rmSync(join(target, name));
  console.log(`copied ${shown.length} pictures${stale.length > 0 ? `, removed ${stale.join(", ")}` : ""}`);
}
