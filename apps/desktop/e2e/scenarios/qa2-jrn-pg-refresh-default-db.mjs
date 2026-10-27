// JRN-04 (medium). On PostgreSQL, Refresh (F5 or the menu) on the connection's default
// database row makes the database level disappear: the "platypus" row and every other
// database ("postgres") vanish and the schemas show straight under the connection. Only a
// Refresh of the connection brings the databases back. A developer refreshes after creating a
// schema with a script, and the tree changes shape under them.
// Steps: expand a PostgreSQL connection (databases "platypus" (default) and "postgres");
// right-click "platypus" > Refresh.
// Expected: the same rows, refreshed. Actual: "platypus" and "postgres" gone.
// Suspected: apps/desktop/src/components/Navigator.tsx `database.refresh` (~1380): for the
// default database (`database === null`) the key is `connection.id`, the key of the
// connection's database list, which loadSchemas then overwrites with the schemas.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { connectWith, needsPostgres, pgConnection, treeRow } from "../qa-jrn.mjs";
import { menu, treeLabels } from "../qa-nav-helpers.mjs";

export const skip = needsPostgres;
const NAME = "QA jrn refresh";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000a7001", NAME)}`);
}

export default async function (page) {
  await connectWith(page, NAME);
  await page.waitFor(`${treeRow("postgres")} !== undefined && ${treeRow("public")} !== undefined`, 15_000);
  await menu(page, "platypus", "Refresh");
  // The refresh reloads the schemas of the database: wait for its rows to come back.
  await page.sleep(500);
  await page.waitFor(`${treeRow("public")} !== undefined`, 15_000);
  const labels = await page.eval(treeLabels);
  assert.ok(labels.includes("platypus") && labels.includes("postgres"), `the databases are gone: ${labels.join(", ")}`);
}
