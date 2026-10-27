// XFER-26 (low). A cancelled import says nothing.
// Steps: import a large CSV (500,000 rows) into a new SQLite table; while "Copying… n rows"
// shows, click "Cancel import".
// Expected: a notice that the import was cancelled and nothing was imported (the table is
// rolled back, rightly), as the result grid's export says "Export cancelled" and MongoDB's
// fake data "Stopped after n documents".
// Actual: the progress line disappears and the dialog looks as if nothing had been started;
// the same happens when "Stop" is pressed while generating fake rows on a SQL database.
// Suspected: apps/desktop/src/transfer/ImportDataDialog.tsx:236-237 ("cancelled" sets the
// status back to idle) and apps/desktop/src/transfer/job.tsx:43-60 (no notice without
// `stopped`).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, chooseFile, expand, importDialog, menu, sqliteOnly } from "../qa-xfer.mjs";

export async function setup({ dir }) {
  await sqliteOnly(dir);
  const lines = ["id,name"];
  for (let i = 0; i < 500_000; i++) lines.push(`${i},name ${i}`);
  writeFileSync(join(dir, "big.csv"), `${lines.join("\n")}\n`);
}

export default async function (page, { dir }) {
  await expand(page, "QA SQLite", "main");
  await menu(page, "main", "Import data into a new table", importDialog, "QA SQLite");
  assert.equal(await chooseFile(page, join(dir, "big.csv")), null);
  await page.eval(`${button(importDialog, "Import")}.click()`);
  const stop = button(importDialog, "Cancel import");
  await page.waitFor(`${stop} && !${stop}.disabled && /Copying… [1-9]/.test(${importDialog}.textContent)`, 30_000);
  await page.eval(`${stop}.click()`);
  await page.waitFor(`${stop} === undefined`, 30_000);
  const notices = await page.eval(`[...${importDialog}.querySelectorAll('.notice')].map(n => n.textContent).join(' | ')`);
  assert.match(notices, /cancel|stopp/i, `notices after cancelling: ${JSON.stringify(notices)}`);
}
