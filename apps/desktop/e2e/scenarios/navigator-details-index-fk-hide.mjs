// QA5-NAV-9 (low). The object details page of a PostgreSQL table lists its indexes and foreign
// keys without what makes them behave: a partial unique index `(pa) WHERE pb IS NULL` shows as a
// plain "Unique: yes" on `pa`; `lower(note), amount DESC NULLS LAST INCLUDE (pa)` shows as
// "lower(note), amount"; a foreign key shows no ON DELETE / ON UPDATE / DEFERRABLE. The DDL
// tab has all of it, so the data is read; the lists hide it.
// Steps: table with a partial unique index and a FK ... ON DELETE SET NULL; right-click >
//   Open details; look at Indexes, then Foreign keys.
// Expected: the predicate (or a "partial" mark) in Indexes, the actions in Foreign keys.
// Actual: neither.
// Code: apps/desktop/src/objects (details page) / the catalog rows it shows: index columns only,
//   foreign key columns and target only.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, saveConnections } from "../navigator-helpers.mjs";

export const skip = needs("postgres");

const SCHEMA = "qa5_det";

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA};
    CREATE TABLE ${SCHEMA}.parent (id int PRIMARY KEY);
    CREATE TABLE ${SCHEMA}.child (id int PRIMARY KEY, pa int REFERENCES ${SCHEMA}.parent (id) ON DELETE SET NULL ON UPDATE CASCADE, pb int);
    CREATE UNIQUE INDEX ix_u ON ${SCHEMA}.child (pa) WHERE pb IS NULL;`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a5e01"));
}

const section = async (page, name) => {
  await page.eval(`(() => { const b = [...document.querySelectorAll('button, [role=tab], a, li')].find(e => e.textContent.trim().startsWith(${JSON.stringify(name)}) && e.children.length <= 3); b?.click(); })()`);
  await page.sleep(800);
  return page.eval(`[...document.querySelectorAll('table')].filter(t => t.offsetParent !== null).map(t => t.innerText).join(' / ')`);
};

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, SCHEMA, "child");
    await menu(page, "child", "Open details");
    await page.waitFor(`document.body.innerText.includes('Foreign keys')`, 15_000);
    const indexes = await section(page, "Indexes");
    const keys = await section(page, "Foreign keys");
    assert.match(indexes, /IS NULL|partial|WHERE/i, `Indexes hides the predicate:\n${indexes}`);
    assert.match(keys, /SET NULL|CASCADE/i, `Foreign keys hides the actions:\n${keys}`);
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE;`);
  }
}
