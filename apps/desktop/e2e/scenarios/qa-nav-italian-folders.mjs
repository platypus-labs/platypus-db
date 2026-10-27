// Bug NAV (low): with the interface in Italian, the navigator's object folders stay in English
// — "Tables", "Views", "Triggers", "Sequences", "Functions"… — while the table's own sections
// below them are translated ("Colonne", "Indici", "Chiavi esterne").
//
// Cause: `FOLDERS` in apps/desktop/src/components/Navigator.tsx has plain labels and
// `schemaRows` shows `folder.label` without t(); apps/desktop/src/i18n/it.ts has no entries for
// them ("Views" exists, for another use).
//
// Steps: language = "it"; expand a SQLite connection with a table, a view and a trigger.
// Expected: "Tabelle", "Viste", "Trigger". Actual: "Tables", "Views", "Triggers".
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { row, saveConnections, treeLabels } from "../qa-nav-helpers.mjs";

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "it.db"));
  db.exec(`CREATE TABLE ordini (id INTEGER PRIMARY KEY);
           CREATE VIEW v AS SELECT * FROM ordini;
           CREATE TRIGGER trg AFTER INSERT ON ordini BEGIN SELECT 1; END;`);
  db.close();
  saveConnections(dir, `[[connections]]
id = "00000000-0000-4000-8000-0000000a0048"
name = "QA Lite"
driver = "sqlite"
database = '${join(dir, "it.db")}'
save_password = false
`);
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.eval(`${row("QA Lite")}.querySelector('.twisty').click()`);
  await page.waitFor(`${row("ordini")} !== undefined`, 15_000);
  const labels = await page.eval(treeLabels);
  for (const english of ["Tables", "Views", "Triggers"]) {
    assert.ok(!labels.includes(english), `untranslated folder ${english}: ${labels.join(", ")}`);
  }
}
