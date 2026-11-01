// JRN-13 (low, wording). In the Italian table menu the pair Import/Export reads unevenly:
// "Importa dati…" next to "Esporta i dati…" (one with the article, one without), while the
// schema menu has "Importa dati in una nuova tabella…" and "Esporta tutte le tabelle…".
// Steps: Italian; right-click a table in the navigator.
// Expected: the same form for both ("Importa i dati…" / "Esporta i dati…", or without the
// article for both). Actual: "Importa dati…" / "Esporta i dati…".
// Suspected: apps/desktop/src/i18n/it.ts:218-220.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { treeRow } from "../qa-jrn.mjs";
import { openMenu } from "../qa-nav-helpers.mjs";
import { submenuItems } from "../menu-sub.mjs";
import { sqliteConnection } from "../query.mjs";

const NAME = "QA jrn wording";

export function setup({ dir }) {
  sqliteConnection(dir, { file: "qa_jrn_wording.db", id: "00000000-0000-4000-8000-0000000a8006", name: NAME, sql: "CREATE TABLE t (id INTEGER PRIMARY KEY);" });
  writeFileSync(join(dir, "settings.toml"), '[appearance]\nlanguage = "it"\n');
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`${treeRow("t")} !== undefined`, 15_000);
  await openMenu(page, "t");
  const items = await submenuItems(page, "Dati");
  const importItem = items.find((i) => i.startsWith("Importa"));
  const exportItem = items.find((i) => i.startsWith("Esporta"));
  assert.equal(importItem.replace("Importa", ""), exportItem.replace("Esporta", ""), `${importItem} / ${exportItem}`);
}
