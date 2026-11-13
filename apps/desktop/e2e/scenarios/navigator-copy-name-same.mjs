// qa4-nav-5 (low). On a table or view, "Copy name" and "Copy qualified name" are two entries of
// the same menu that put the same text on the clipboard (schema-qualified and quoted), so the
// bare name of a table cannot be copied at all; on a routine "Copy name" is bare, so the
// two entries do not mean the same thing across kinds.
// Steps: PostgreSQL, table qa4_nav_cn."Mixed Case"; Copy name; Copy qualified name.
// Expected: Copy name gives the bare name (`Mixed Case`, as it does for a column and a routine),
//   Copy qualified name gives `qa4_nav_cn."Mixed Case"`.
// Actual: both give `qa4_nav_cn."Mixed Case"`.
// Code: apps/desktop/src/components/Navigator.tsx `object.copy_name`: relations copy `target`
//   (the qualified, quoted name), only other kinds copy `object.name`.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, saveConnections } from "../navigator-helpers.mjs";

export const skip = needs("postgres");

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_cn CASCADE; CREATE SCHEMA qa4_nav_cn; CREATE TABLE qa4_nav_cn."Mixed Case" (id int);`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a4005"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, "qa4_nav_cn", "Mixed Case");
    await page.eval(`navigator.clipboard.writeText = async (t) => { window.__copied = t; }`);
    await menu(page, "Mixed Case", "Copy name");
    await page.waitFor(`window.__copied !== undefined`);
    const bare = await page.eval(`window.__copied`);
    await page.eval(`window.__copied = undefined`);
    await menu(page, "Mixed Case", "Copy qualified name");
    await page.waitFor(`window.__copied !== undefined`);
    const qualified = await page.eval(`window.__copied`);
    assert.equal(qualified, 'qa4_nav_cn."Mixed Case"');
    assert.notEqual(bare, qualified, `Copy name is the same as Copy qualified name: ${bare}`);
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS qa4_nav_cn CASCADE;`);
  }
}
