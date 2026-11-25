// Every translated page of the guide has an English original and a well-formed source line, and
// the check tells a stale page from a current one (scripts/guide-translations.mjs).
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { classifyLink } from "../apps/desktop/src/help/pages.ts";
import { parseMarkdown } from "../apps/desktop/src/help/markdown.ts";
import { languagesIn, report, sourceHash } from "./guide-translations.mjs";

test("every translated page has an original and a source line", () => {
  for (const lang of languagesIn()) {
    const r = report(lang);
    assert.deepEqual(r.orphans, [], `${lang}: pages with no English original`);
    assert.deepEqual(r.malformed, [], `${lang}: pages without a "<!-- source: <12 hex> -->" first line`);
  }
});

/** Every `link` node below `value` (blocks and inline nodes nest in lists, quotes and tables). */
const linksIn = (value) => (Array.isArray(value) ? value.flatMap(linksIn) : value && typeof value === "object" ? [...(value.kind === "link" ? [value] : []), ...Object.values(value).flatMap(linksIn)] : []);
const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8");
const anchorsOf = (text) => parseMarkdown(text).filter((b) => b.kind === "heading").map((b) => b.anchor);

test("every link of a translated page leads to a page and a heading that exist", () => {
  for (const lang of languagesIn()) {
    const dir = `docs/user/translations/${lang}/guide/`;
    for (const file of readdirSync(new URL(`../${dir}`, import.meta.url), { recursive: true }).filter((f) => String(f).endsWith(".md"))) {
      const page = String(file).replaceAll("\\", "/");
      const key = page.slice(0, -3);
      for (const { href } of linksIn(parseMarkdown(read(dir + page)))) {
        const link = classifyLink(key, href);
        if (link.kind === "none" || link.kind === "external" || (link.kind === "page" && link.key === null)) continue;
        const target = link.kind === "anchor" ? key : link.key;
        const path = (existsSync(new URL(`../docs/user/translations/${lang}/guide/${target}.md`, import.meta.url)) ? `docs/user/translations/${lang}/guide/` : "docs/user/guide/") + `${target}.md`;
        assert.ok(existsSync(new URL(`../${path}`, import.meta.url)), `${lang}/${page}: ${href} leads to no page`);
        if (link.anchor) assert.ok(anchorsOf(read(path)).includes(link.anchor), `${lang}/${page}: ${href} leads to no heading of ${path}`);
      }
    }
  }
});

test("a page is missing, current, stale, malformed or an orphan", () => {
  const dir = mkdtempSync(join(tmpdir(), "guide-tr-"));
  try {
    const write = (path, text) => {
      mkdirSync(join(dir, path, ".."), { recursive: true });
      writeFileSync(join(dir, path), text);
    };
    for (const p of ["a", "b", "c", "d"]) write(`docs/user/guide/${p}.md`, `# ${p}\r\n`);
    write("docs/user/translations/it/guide/a.md", `<!-- source: ${sourceHash("# a\n")} -->\n# a\n`);
    write("docs/user/translations/it/guide/b.md", "<!-- source: 000000000000 -->\n# b\n");
    write("docs/user/translations/it/guide/c.md", "# c\n");
    write("docs/user/translations/it/guide/gone.md", "<!-- source: 000000000000 -->\n");
    assert.deepEqual(report("it", dir), { missing: ["d.md"], stale: ["b.md"], orphans: ["gone.md"], malformed: ["c.md"] });
    assert.deepEqual(languagesIn(dir), ["it"]);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
