// QA ui: the navigator's object folders are not translated in Italian.
//
// Steps: set the language to Italiano, expand a connection.
// Expected: the folders read "Tabelle", "Viste", ... like the rest of the interface.
// Actual: they stay in English: "Tables", "Views", "Sequences", "Functions", "Procedures",
// "Triggers", ... The labels are plain strings, never passed through t() nor marked with
// msg(), so the build's translation check does not see them either.
// Code: components/Navigator.tsx, FOLDERS (`label: "Tables"`) and `label: folder.label`.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const NAME = "QA UI italiano";
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

export function setup({ dir }) {
  const path = join(dir, "qa_ui_it.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE qa_ui_t (id INTEGER PRIMARY KEY); CREATE VIEW qa_ui_v AS SELECT id FROM qa_ui_t;");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-00000000e107"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`,
  );
  // The Italian interface (run.mjs leaves a language set by the scenario).
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.waitFor(`document.documentElement.lang === 'it'`);
  await page.waitFor(`${treeRow(NAME)} !== undefined`);
  await page.eval(`${treeRow(NAME)}.querySelector(".twisty").click()`);
  await page.waitFor(`${treeRow("qa_ui_t")} !== undefined`, 15_000);
  const folders = await page.eval(
    `[...document.querySelectorAll('.tree-row')].map(r => r.querySelector('.tree-label')?.textContent).filter(l => ['Tables', 'Views', 'Tabelle', 'Viste'].includes(l))`,
  );
  assert.deepEqual(folders, ["Tabelle", "Viste"]);
}
