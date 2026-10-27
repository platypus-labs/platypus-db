// JRN-08 (low, cosmetic). While a connection loads (e.g. its password prompt is open) the
// navigator shows its placeholder row as "loading… loading…": the row's label and, beside
// it, the same word again as its detail; the connection row's own detail says it a third
// time, cut ("carica…" in Italian). An error row repeats its message the same way.
// Steps: expand a PostgreSQL connection that asks for its password; look at the tree.
// Expected: "loading…" once. Actual: twice in the row.
// Suspected: apps/desktop/src/components/Navigator.tsx:646 (the status row gets
// `label: s.detail` and `...s`, whose `detail` is shown again).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { needsPostgres, pgConnection, treeRow } from "../qa-jrn.mjs";

export const skip = needsPostgres;
const NAME = "QA jrn loading";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000a8004", NAME)}`);
}

export default async function (page) {
  await page.eval(`${treeRow(NAME)}.querySelector('.twisty').click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.waitFor(`${treeRow("loading…")} !== undefined`);
  const row = await page.eval(`${treeRow("loading…")}.textContent`);
  await page.key("Escape", "Escape", 27);
  assert.equal(row.trim(), "loading…");
}
