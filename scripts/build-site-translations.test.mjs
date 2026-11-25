// The site's pages for the guide's translations (scripts/build-site-docs.mjs): where they go,
// their language, the list of languages on each page, and links that stay inside a language.
import assert from "node:assert/strict";
import { test } from "node:test";
import { buildSite, outputOf, translationOf } from "./build-site-docs.mjs";

const docs = {
  "docs/user/guide/README.md": "# User guide\n\nSee [Connect](connect.md).",
  "docs/user/guide/getting-started.md": "# Getting started\n\nSee [Connect](connect.md) and [Queries](queries.md).",
  "docs/user/guide/connect.md": "# Connect\n\n## URL\n\ntext",
  "docs/user/guide/queries.md": "# Queries\n\ntext",
  "docs/user/guide/databases/README.md": "# Databases\n\n| Page | Databases |\n|---|---|\n| [PostgreSQL](postgresql.md) | PostgreSQL |\n",
  "docs/user/guide/databases/postgresql.md": "# PostgreSQL\n\ntext",
  "docs/user/FEATURES.md": "# Features\n\ntext",
  "docs/user/SHORTCUTS.md": "# Shortcuts\n\ntext",
  "docs/user/AGENTS.md": "# Agents\n\ntext",
  "docs/user/PLUGINS.md": "# Plugins\n\ntext",
  "docs/user/CONNECTIONS_FILE.md": "# The connections file\n\ntext",
  "docs/user/translations/it/guide/getting-started.md": "<!-- source: 000000000000 -->\n# Per iniziare\n\nVedi [Connettersi](connect.md) e [Query](queries.md).",
  "docs/user/translations/it/guide/connect.md": "<!-- source: 000000000000 -->\n# Connettersi\n\n## URL\n\ntesto",
};

test("translated sources become pages of their language", () => {
  assert.deepEqual(translationOf("docs/user/translations/it/guide/databases/README.md"), { lang: "it", english: "docs/user/guide/databases/README.md" });
  assert.equal(translationOf("docs/user/guide/connect.md"), null);
  assert.equal(outputOf("docs/user/translations/it/guide/getting-started.md"), "docs/it/guide/getting-started.html");
  assert.equal(outputOf("docs/user/translations/it/guide/connect.md"), "docs/it/guide/connect.html");
  assert.equal(outputOf("docs/user/translations/it/guide/databases/README.md"), "docs/it/guide/databases/index.html");
});

test("the Italian pages have their language, their links and the list of languages", () => {
  const files = buildSite(Object.keys(docs), (s) => docs[s]);
  const start = files["docs/it/guide/getting-started.html"];
  assert.ok(start.includes('<html lang="it">'));
  assert.ok(start.includes("<title>PlatypusDB · Per iniziare</title>"));
  assert.ok(!start.includes("source:"), "the source line is not shown");
  // A translated page is linked to its translation; one without a translation to the English page.
  assert.ok(start.includes('href="connect.html"'), start);
  assert.ok(start.includes('href="../../guide/queries.html"'), start);
  assert.ok(start.includes('<nav class="crumbs" aria-label="Languages"><a href="../../../docs.html" lang="en" hreflang="en">English</a> · <span lang="it">Italiano</span></nav>'), start);
  // The English page lists both, a page with no translation lists none.
  assert.ok(files["docs.html"].includes('<html lang="en">') && files["docs.html"].includes('<a href="docs/it/guide/getting-started.html" lang="it" hreflang="it">Italiano</a>'));
  assert.ok(!files["docs/guide/queries.html"].includes('aria-label="Languages"'));
  assert.ok(!Object.keys(files).some((f) => f.startsWith("docs/it/guide/queries")), "no page for what is not translated");
});
