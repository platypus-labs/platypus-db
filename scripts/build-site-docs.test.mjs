// The pure parts of scripts/build-site-docs.mjs: escaping, links, pictures, page names, and a whole
// small site built twice.
import assert from "node:assert/strict";
import { test } from "node:test";
import { parseInline, parseMarkdown } from "../apps/desktop/src/help/markdown.ts";
import { buildSite, describe, esc, outputOf, renderBlocks, renderInline, resolveHref, resolveImage } from "./build-site-docs.mjs";

const pages = new Map([
  ["docs/guide/README.md", "docs/guide/index.html"],
  ["docs/guide/connect.md", "docs/guide/connect.html"],
  ["docs/guide/databases/postgresql.md", "docs/guide/databases/postgresql.html"],
  ["docs/guide/getting-started.md", "docs.html"],
  ["docs/FEATURES.md", "docs/features.html"],
]);

test("source documents become pages", () => {
  assert.equal(outputOf("docs/guide/README.md"), "docs/guide/index.html");
  assert.equal(outputOf("docs/guide/databases/README.md"), "docs/guide/databases/index.html");
  assert.equal(outputOf("docs/guide/databases/db2-odbc.md"), "docs/guide/databases/db2-odbc.html");
  assert.equal(outputOf("docs/guide/getting-started.md"), "docs.html");
  assert.equal(outputOf("docs/CONNECTIONS_FILE.md"), "docs/connections-file.html");
});

test("text and attributes are escaped", () => {
  assert.equal(esc(`<a href="x">&'`), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  const html = renderInline(parseInline("a < b & `<script>` ![x\"><b>](../screenshots/a.png)"), { src: "docs/guide/connect.md", out: "docs/guide/connect.html", pages });
  assert.ok(!html.includes("<script>") && !html.includes("<b>"), html);
  assert.ok(html.includes("&lt;script&gt;"));
});

test("a link between two pages becomes a link between the pages", () => {
  assert.equal(resolveHref("connect.md#tls", "docs/guide/README.md", "docs/guide/index.html", pages), "connect.html#tls");
  assert.equal(resolveHref("../FEATURES.md#sql-editor", "docs/guide/connect.md", "docs/guide/connect.html", pages), "../features.html#sql-editor");
  assert.equal(resolveHref("databases/postgresql.md", "docs/guide/connect.md", "docs/guide/connect.html", pages), "databases/postgresql.html");
  assert.equal(resolveHref("../../guide/connect.md", "docs/guide/databases/postgresql.md", "docs/guide/databases/postgresql.html", pages), "../connect.html");
  assert.equal(resolveHref("getting-started.md", "docs/guide/connect.md", "docs/guide/connect.html", pages), "../../docs.html");
  assert.equal(resolveHref("#top", "docs/guide/connect.md", "docs/guide/connect.html", pages), "#top");
});

test("what the site does not carry goes to GitHub, folders as trees", () => {
  const repo = "https://github.com/PaoloCisly/platypus-db";
  assert.equal(resolveHref("../../README.md", "docs/guide/connect.md", "docs/guide/connect.html", pages), `${repo}/blob/main/README.md`);
  assert.equal(resolveHref("../plugins/codex", "docs/AGENTS.md", "docs/agents.html", pages), `${repo}/tree/main/plugins/codex`);
  assert.equal(resolveHref("https://example.com/a?b=1#c", "docs/AGENTS.md", "docs/agents.html", pages), "https://example.com/a?b=1#c");
  assert.equal(resolveHref("mailto:a@b.c", "docs/AGENTS.md", "docs/agents.html", pages), "mailto:a@b.c");
});

test("hostile links are refused", () => {
  const bad = ["javascript:alert(1)", "JaVaScRiPt:alert(1)", " java\tscript:alert(1)", "java\nscript:alert(1)", "data:text/html,<script>1</script>", "vbscript:x", "file:///etc/passwd", "//evil.example/x", "..\\..\\x", "../../../outside.md", "/etc/passwd", ""];
  for (const href of bad) assert.equal(resolveHref(href, "docs/guide/connect.md", "docs/guide/connect.html", pages), null, JSON.stringify(href));
  const ctx = { src: "docs/guide/connect.md", out: "docs/guide/connect.html", pages };
  const html = renderBlocks(parseMarkdown('[click <me>](javascript:alert(1)) and [ok](https://example.com/"onmouseover="x)\n\n<script>alert(1)</script>\n\n<div onclick="x">raw</div>'), ctx);
  assert.ok(!/javascript:/i.test(html), html);
  assert.ok(!html.includes("<script") && !html.includes("<div"), html);
  assert.ok(!html.includes('"onmouseover') && html.includes("&quot;onmouseover"), html);
  assert.ok(html.startsWith("<p>click "), html);
});

test("pictures come from docs/screenshots only", () => {
  assert.equal(resolveImage("../screenshots/guide/welcome.png", "docs/guide/getting-started.md", "docs.html"), "img/screenshots/guide/welcome.png");
  assert.equal(resolveImage("screenshots/palette.png", "docs/FEATURES.md", "docs/features.html"), "../img/screenshots/palette.png");
  for (const bad of ["https://example.com/x.png", "javascript:alert(1)", "//x/y.png", "../../README.png", "../../../x.png", ""]) assert.equal(resolveImage(bad, "docs/guide/getting-started.md", "docs.html"), null, bad);
  const ctx = { src: "docs/FEATURES.md", out: "docs/features.html", pages };
  assert.equal(renderInline(parseInline("![remote](https://example.com/x.png)"), ctx), "remote");
});

test("blocks: lists, tables, code and the description", () => {
  const ctx = { src: "docs/guide/connect.md", out: "docs/guide/connect.html", pages };
  const blocks = parseMarkdown("[← Guide](README.md)\n\n# T\n\nFirst **text** here.\n\n3. a\n4. b\n\n| A | B |\n|:--|--:|\n| `x<y` | 1 |\n\n```sql\nselect '<b>';\n```");
  const html = renderBlocks(blocks, ctx);
  assert.ok(html.includes('<h1 id="t">T</h1>'));
  assert.ok(html.includes('<ol start="3">'));
  assert.ok(html.includes('<th style="text-align:left">A</th>') && html.includes('<td style="text-align:right">1</td>'));
  assert.ok(html.includes("<code>x&lt;y</code>"));
  assert.ok(html.includes('<pre><code class="language-sql">select &#39;&lt;b&gt;&#39;;</code></pre>'));
  assert.equal(describe(blocks), "First text here.");
  assert.equal(describe(parseMarkdown(`x ${"word ".repeat(60)}`)).length <= 161, true);
});

test("the same documents give the same bytes, with every link inside the site", () => {
  const docs = {
    "docs/guide/README.md": "# User guide\n\nStart at [Getting started](getting-started.md) or [Connect](connect.md).\n\n| Page | Notes |\n|---|---|\n| [Connect](connect.md) | x |\n",
    "docs/guide/getting-started.md": "[← User guide](README.md)\n\n# Getting started\n\n![Welcome](../screenshots/guide/welcome.png)\n\n## Install\n\nSee [features](../FEATURES.md#a).",
    "docs/guide/connect.md": "# Connect\n\n## URL\n\ntext [bad](javascript:alert(1)).",
    "docs/guide/databases/README.md": "# Databases\n\n| Page | Databases |\n|---|---|\n| [PostgreSQL](postgresql.md) | PostgreSQL |\n",
    "docs/guide/databases/postgresql.md": "# PostgreSQL, CockroachDB and more\n\ntext",
    "docs/FEATURES.md": "# Features\n\n## A\n\ntext",
    "docs/SHORTCUTS.md": "# Keyboard shortcuts\n\ntext",
    "docs/AGENTS.md": "# Agents and the CLI\n\ntext",
    "docs/PLUGINS.md": "# Plugins\n\ntext",
    "docs/CONNECTIONS_FILE.md": "# The connections file\n\ntext",
  };
  const sources = Object.keys(docs);
  const first = buildSite(sources, (s) => docs[s]);
  assert.deepEqual(buildSite(sources, (s) => docs[s]), first);
  assert.deepEqual(Object.keys(first).sort(), ["docs.html", "docs/agents.html", "docs/connections-file.html", "docs/features.html", "docs/guide/connect.html", "docs/guide/databases/index.html", "docs/guide/databases/postgresql.html", "docs/guide/index.html", "docs/plugins.html", "docs/shortcuts.html"]);
  const page = first["docs/guide/databases/postgresql.html"];
  assert.ok(page.includes('href="../../../style.css"') && page.includes('href="../connect.html"'), "relative to the page");
  assert.ok(page.includes('class="sub on" aria-current="page"'), "the database pages in the menu of a database page");
  assert.ok(page.includes(">PostgreSQL</a>"), "short name in the menu");
  assert.ok(first["docs.html"].includes('src="img/screenshots/guide/welcome.png"'));
  assert.ok(first["docs.html"].includes('href="docs/features.html#a"'));
  assert.ok(!first["docs/guide/connect.html"].includes("javascript:"));
  assert.throws(() => buildSite(["docs/guide/README.md", "docs/guide/getting-started.md", "docs/guide/connect.md", "docs/FEATURES.md"], (s) => (s.endsWith("connect.md") || s.endsWith("README.md") ? "# Same" : "# Other")), /two pages/);
});
