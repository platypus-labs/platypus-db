// Unit tests of the translated guide pages: which page a file is, which page is shown (the
// translation or the English one) and the source line that marks a translation
// (src/help/pages.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { parseMarkdown } from "../src/help/markdown.ts";
import { titleOf, translationOf, withTranslations } from "../src/help/pages.ts";

test("a translated page is named by its language and the key of its English page", () => {
  assert.deepEqual(translationOf("docs/user/translations/it/guide/getting-started.md"), { lang: "it", key: "getting-started" });
  assert.deepEqual(translationOf("../../../../docs/user/translations/de/guide/databases/postgresql.md"), { lang: "de", key: "databases/postgresql" });
  assert.deepEqual(translationOf("docs/user/translations/it/guide/databases/README.md"), { lang: "it", key: "databases/README" });
  assert.equal(translationOf("docs/user/guide/connect.md"), null);
  assert.equal(translationOf("docs/user/translations/it/README.md"), null);
});

test("a translated page replaces the English one; the rest stays English", () => {
  const english = { README: "# Guide", connect: "# Connect", "docs/user/FEATURES.md": "# Features" };
  assert.deepEqual(withTranslations(english, { connect: "# Collegarsi" }), { ...english, connect: "# Collegarsi" });
  // No translation, or a language with none: English as it is.
  assert.deepEqual(withTranslations(english, {}), english);
  // A translation of a page that has no English original is not shown.
  assert.deepEqual(withTranslations(english, { gone: "# Altro" }), english);
});

test("the title of a translated page is its own, and its source line is not shown", () => {
  const page = "<!-- source: 0123456789ab -->\n# Collegarsi a un database\n\nTesto.\n";
  assert.equal(titleOf(page, "connect"), "Collegarsi a un database");
  assert.deepEqual(parseMarkdown(page).map((b) => b.kind), ["heading", "paragraph"]);
});
