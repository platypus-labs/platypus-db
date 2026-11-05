// Copies into site/img/screenshots the pictures of docs/screenshots that the site's pages show,
// and removes the ones no page shows any more. Run it after the `readme` or `gifs` scenarios redo
// the pictures:
//
//   node scripts/copy-site-screenshots.mjs
//
// Exits 1 and changes nothing when a page shows a picture that docs/screenshots does not have.
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const site = join(root, "site");
const source = join(root, "docs", "screenshots");
const target = join(site, "img", "screenshots");

/** The picture names that the given HTML pages show from img/screenshots. */
export function picturesIn(pages) {
  const names = new Set();
  for (const html of pages) {
    for (const m of html.matchAll(/img\/screenshots\/([\w.-]+)/g)) names.add(m[1]);
  }
  return [...names].sort();
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const pages = readdirSync(site)
    .filter((f) => f.endsWith(".html"))
    .map((f) => readFileSync(join(site, f), "utf8"));
  const shown = picturesIn(pages);
  const missing = shown.filter((name) => !existsSync(join(source, name)));
  if (missing.length > 0) {
    console.error(`not in docs/screenshots: ${missing.join(", ")}`);
    process.exit(1);
  }
  mkdirSync(target, { recursive: true });
  for (const name of shown) copyFileSync(join(source, name), join(target, name));
  const stale = readdirSync(target).filter((name) => !shown.includes(name));
  for (const name of stale) rmSync(join(target, name));
  console.log(`copied ${shown.length} pictures${stale.length > 0 ? `, removed ${stale.join(", ")}` : ""}`);
}
