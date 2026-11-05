// Unit tests of the bundled guide: where a link or an image between pages leads, the page that is
// missing, the Markdown the pages are written in, and the changelog section of a version
// (src/help/pages.ts, src/help/markdown.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { parseInline, parseMarkdown, plainText } from "../src/help/markdown.ts";
import {
  changelogSection,
  classifyLink,
  imagePath,
  pageKeyOf,
  pageSource,
  slugger,
  sortedKeys,
  titleOf,
} from "../src/help/pages.ts";

test("a relative link names a page by its path below the guide", () => {
  assert.deepEqual(classifyLink("README", "connect.md"), { kind: "page", key: "connect", path: "docs/guide/connect.md", anchor: null });
  assert.deepEqual(classifyLink("README", "./connect.md#tls"), { kind: "page", key: "connect", path: "docs/guide/connect.md", anchor: "tls" });
  assert.deepEqual(classifyLink("connect", "databases/postgres.md"), { kind: "page", key: "databases/postgres", path: "docs/guide/databases/postgres.md", anchor: null });
});

test("links climb out of the databases folder and into it", () => {
  assert.equal(classifyLink("databases/postgres", "../connect.md#ssh").key, "connect");
  assert.equal(classifyLink("databases/postgres", "../connect.md#ssh").anchor, "ssh");
  assert.equal(classifyLink("databases/postgres", "mysql.md").key, "databases/mysql");
  assert.equal(classifyLink("databases/postgres", "README.md").key, "databases/README");
  assert.equal(classifyLink("connect", "databases/").key, "databases/README");
  assert.equal(classifyLink("connect", "databases").key, "databases");
});

test("a link to a heading of the same page, with the anchor as GitHub writes it", () => {
  assert.deepEqual(classifyLink("connect", "#SSH-tunnels"), { kind: "anchor", anchor: "ssh-tunnels" });
  assert.deepEqual(classifyLink("connect", "#sp%C3%A4ter"), { kind: "anchor", anchor: "später" });
  assert.deepEqual(classifyLink("connect", "#"), { kind: "none" });
});

test("web and mail addresses open outside; other schemes lead nowhere", () => {
  assert.deepEqual(classifyLink("connect", "https://example.org/a?b=1#c"), { kind: "external", url: "https://example.org/a?b=1#c" });
  assert.equal(classifyLink("connect", "mailto:a@example.org").kind, "external");
  assert.equal(classifyLink("connect", "javascript:alert(1)").kind, "none");
  assert.equal(classifyLink("connect", "file:///C:/x").kind, "none");
  assert.equal(classifyLink("connect", "data:text/html,x").kind, "none");
});

test("a file outside the guide, or one that is not a page, is not a guide page", () => {
  const features = classifyLink("connect", "../FEATURES.md#grid");
  assert.equal(features.kind, "page");
  assert.equal(features.key, null);
  assert.equal(features.path, "docs/FEATURES.md");
  assert.equal(classifyLink("connect", "../../README.md").key, null);
  assert.equal(classifyLink("connect", "../../../../x.md").kind, "none");
  assert.equal(classifyLink("connect", "../screenshots/palette.png").kind, "none");
});

test("the page of a path, and a page the guide does not have", () => {
  assert.equal(pageKeyOf("docs/guide/connect.md"), "connect");
  assert.equal(pageKeyOf("docs/guide/databases/"), "databases/README");
  assert.equal(pageKeyOf("docs/guide/"), "README");
  assert.equal(pageKeyOf("docs/FEATURES.md"), null);
  assert.equal(pageKeyOf("docs/guide/x.png"), null);
  const pages = { README: "# Guide" };
  assert.deepEqual(pageSource(pages, "README"), { text: "# Guide" });
  assert.deepEqual(pageSource(pages, "connect"), { missing: "docs/guide/connect.md" });
  assert.deepEqual(pageSource(pages, "toString"), { missing: "docs/guide/toString.md" });
  assert.deepEqual(pageSource(pages, "docs/FEATURES.md"), { missing: "docs/FEATURES.md" });
});

test("images are found by their path in the repository; addresses are left out", () => {
  assert.equal(imagePath("connect", "../screenshots/palette.png"), "docs/screenshots/palette.png");
  assert.equal(imagePath("databases/postgres", "../../screenshots/a%20b.png?raw=true"), "docs/screenshots/a b.png");
  assert.equal(imagePath("connect", "https://example.org/x.png"), null);
  assert.equal(imagePath("connect", "//example.org/x.png"), null);
});

test("headings get GitHub's anchors, a repeated one numbered", () => {
  const anchor = slugger();
  assert.equal(anchor("Connect with a URL"), "connect-with-a-url");
  assert.equal(anchor("What's new?"), "whats-new");
  assert.equal(anchor("Notes"), "notes");
  assert.equal(anchor("Notes"), "notes-1");
  assert.equal(anchor("Notes"), "notes-2");
});

test("the index lists the pages in reading order, the databases after them", () => {
  const keys = ["databases/sqlite", "troubleshooting", "extra", "databases/README", "connect", "README", "databases/mysql"];
  assert.deepEqual(sortedKeys(keys), ["README", "connect", "troubleshooting", "databases/README", "databases/mysql", "databases/sqlite", "extra"]);
  assert.equal(titleOf("Intro\n\n# Connect `now`\n", "connect"), "Connect now");
  assert.equal(titleOf("```\n# not a title\n```\ntext", "queries"), "queries");
});

test("lists nest, and a code block or a second paragraph stays in its item", () => {
  const [list] = parseMarkdown("1. Open it\n   - a\n   - b\n2. Run:\n\n   ```sql\n   select 1;\n   ```\n\n   Then wait.\n3. Done");
  assert.equal(list.kind, "list");
  assert.equal(list.ordered, true);
  assert.equal(list.items.length, 3);
  assert.deepEqual(list.items[0].map((b) => b.kind), ["paragraph", "list"]);
  assert.equal(list.items[0][1].items.length, 2);
  assert.deepEqual(list.items[1].map((b) => b.kind), ["paragraph", "code", "paragraph"]);
  assert.equal(list.items[1][1].text, "select 1;");
});

test("a table has its header, alignment and cells with a pipe in code", () => {
  const [table] = parseMarkdown("| Name | Port |\n|:--|--:|\n| `a\\|b` | 5432 |\n| x | 3306 |");
  assert.equal(table.kind, "table");
  assert.deepEqual(table.align, ["left", "right"]);
  assert.equal(table.rows.length, 2);
  assert.equal(plainText(table.rows[0][0]), "a|b");
  assert.equal(plainText(table.rows[1][1]), "3306");
});

test("links, images and badges in a line", () => {
  const nodes = parseInline("See [the **form**](connect.md#tls) and ![shot](../screenshots/a.png) or [![badge](b.svg)](https://example.org)");
  assert.equal(nodes[1].kind, "link");
  assert.equal(nodes[1].href, "connect.md#tls");
  assert.equal(nodes[3].kind, "image");
  assert.equal(nodes[3].src, "../screenshots/a.png");
  const badge = nodes.at(-1);
  assert.equal(badge.kind, "link");
  assert.equal(badge.children[0].kind, "image");
});

test("raw HTML never reaches the page: lines and tags are dropped, a script stays text", () => {
  const blocks = parseMarkdown('<p align="center">\n<img src=x onerror=alert(1)>\n</p>\n\nHello <b>there</b> <script>alert(1)</script>\n\n<!-- note -->');
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0].kind, "paragraph");
  const text = plainText(blocks[0].children);
  assert.ok(!text.includes("<"), text);
  assert.ok(text.includes("Hello there"), text);
});

test("headings carry their anchor", () => {
  const blocks = parseMarkdown("# Guide\n\n## Set up\n\n## Set up");
  assert.deepEqual(blocks.map((b) => b.anchor), ["guide", "set-up", "set-up-1"]);
});

const CHANGELOG = "# Changelog\n\n## [Unreleased]\n\n- soon\n\n## [0.4.1] - 2026-11-04\n\n### Added\n\n- thing\n\n## [0.4.0] - 2026-11-02\n\n- old\n";

test("what is new: the section of the running version, else the unreleased one", () => {
  const exact = changelogSection(CHANGELOG, "0.4.1");
  assert.equal(exact.exact, true);
  assert.equal(exact.heading, "[0.4.1] - 2026-11-04");
  assert.ok(exact.body.includes("- thing") && !exact.body.includes("old") && !exact.body.includes("soon"));
  const later = changelogSection(CHANGELOG, "0.5.0");
  assert.equal(later.exact, false);
  assert.equal(later.body, "- soon");
  // 0.4 is not 0.4.1: a version matches its own section only.
  assert.equal(changelogSection(CHANGELOG, "0.4").exact, false);
  assert.equal(changelogSection("# Changelog\n\n## [0.1.0]\n- a", "0.2.0"), null);
});
