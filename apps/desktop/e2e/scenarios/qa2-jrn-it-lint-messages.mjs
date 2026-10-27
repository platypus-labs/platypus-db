// JRN-05 (low). With the interface in Italian, Controlla lo schema… lists its findings with
// an Italian heading ("Chiave esterna senza indice") but the explanation under it in English:
// "The table has no primary key.", "No index on the foreign key columns (customer_id).",
// "Same columns as index …", "… Check first that neither is partial or has included columns."
// Steps: Settings > Language Italiano; navigator > schema main > Controlla lo schema….
// Expected: the explanations in Italian. Actual: in English.
// Suspected: crates/platypus-schema/src/lint.rs builds the message text in English and
// apps/desktop/src/schema/LintDialog.tsx shows `f.message` as it is (the kind label alone goes
// through `t`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { italian, treeRow } from "../qa-jrn.mjs";
import { menu } from "../qa-nav-helpers.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "QA jrn lint";
const IT = italian();
const lint = `document.querySelector('.modal.schema-lint')`;

export function setup({ dir }) {
  sqliteConnection(dir, {
    file: "qa_jrn_lint.db",
    id: "00000000-0000-4000-8000-0000000a8001",
    name: NAME,
    sql: `CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT);
          CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));
          CREATE TABLE logs (msg TEXT);`,
  });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("main")} !== undefined`, 15_000);
  await menu(page, "main", IT["Check schema…"]);
  await page.waitFor(`${lint}?.querySelectorAll('.lint-findings li').length === 2`, 15_000);
  const findings = await page.eval(`[...${lint}.querySelectorAll('.lint-findings li')].map(li => [li.querySelector('.lint-kind').textContent, li.querySelector('.small').textContent])`);
  // The headings are Italian already.
  assert.ok(findings.some(([kind]) => kind === IT["Foreign key without index"]), JSON.stringify(findings));
  for (const [kind, text] of findings) {
    assert.doesNotMatch(text, /\b(The table has|No index on|columns|Check first)\b/, `"${kind}" explained in English: ${text}`);
  }
}
