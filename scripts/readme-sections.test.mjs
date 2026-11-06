// README.md and README.it.md must have the same sections: as many `##` headings, and in each
// one the same pictures, code blocks, tables and links to files (the text is what differs).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (name) => readFileSync(join(root, name), "utf8").replace(/\r\n/g, "\n");

/** What a README is made of: the part before the first `##` and then each `##` section. */
export function sectionsOf(text) {
  const parts = text.split(/^## /m);
  const shape = (body) => {
    const plain = body.replace(/^(```+)[^\n]*\n[\s\S]*?^\1\s*$/gm, "```");
    return {
      pictures: [...plain.matchAll(/!\[[^\]]*\]\(([^)\s]+)\)|<img [^>]*src="([^"]+)"/g)].map((m) => m[1] ?? m[2]),
      code: (body.match(/^```/gm) ?? []).length / 2,
      tables: (plain.match(/^\|[ :-]*---/gm) ?? []).length,
      links: [...plain.matchAll(/(?<!!)\[[^\]]*\]\((?!https?:|mailto:|#)([^)\s]+)\)/g)].map((m) => m[1]).sort(),
    };
  };
  return { head: shape(parts[0]), titles: parts.slice(1).map((p) => p.split("\n")[0]), sections: parts.slice(1).map(shape) };
}

test("the two READMEs have the same sections", () => {
  const en = sectionsOf(read("README.md"));
  const it = sectionsOf(read("README.it.md"));
  assert.equal(it.titles.length, en.titles.length, `sections: ${en.titles.join(" | ")} against ${it.titles.join(" | ")}`);
  assert.deepEqual(it.head.pictures, en.head.pictures, "pictures before the first section");
  en.sections.forEach((section, i) => {
    const where = `section ${i + 1}: "${en.titles[i]}" / "${it.titles[i]}"`;
    assert.deepEqual(it.sections[i].pictures, section.pictures, `${where}: pictures`);
    assert.equal(it.sections[i].code, section.code, `${where}: code blocks`);
    assert.equal(it.sections[i].tables, section.tables, `${where}: tables`);
    assert.deepEqual(it.sections[i].links, section.links, `${where}: links to files`);
  });
});

test("the shape reads sections, pictures, code, tables and local links only", () => {
  const text = "![hero](a.gif)\n\n## One\n\n![x](b.png) [d](docs/x.md) [w](https://e.com)\n\n```sh\n[no](link.md)\n```\n\n| A |\n|---|\n| 1 |\n\n## Two\n";
  const s = sectionsOf(text);
  assert.deepEqual(s.titles, ["One", "Two"]);
  assert.deepEqual(s.head.pictures, ["a.gif"]);
  assert.deepEqual(s.sections[0], { pictures: ["b.png"], code: 1, tables: 1, links: ["docs/x.md"] });
});
