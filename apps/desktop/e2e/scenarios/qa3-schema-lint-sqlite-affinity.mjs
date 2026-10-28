// SCH-07 (low). On SQLite, Check schema… reports "Foreign key type mismatch" for a key
// declared INTEGER that references a column declared INT ("user_code is INTEGER but users.code
// is INT"): in SQLite both are the same type (INTEGER affinity), nothing is wrong. Also guards
// what it gets right: a key on the rowid primary key and a key covered by a composite primary
// key are not "without index".
// Steps: users(id INTEGER PRIMARY KEY, code INT UNIQUE), items(user_code INTEGER REFERENCES
// users(code), indexed), profiles(id INTEGER PRIMARY KEY REFERENCES users), lines(PRIMARY KEY
// (order_id, product_id), order_id REFERENCES users); navigator > main > Check schema….
// Expected: "No problems found." Actual: a type mismatch on items.
// Suspected: crates/platypus-schema/src/lint.rs:187 (same_type compares the declared names;
// SQLite should compare affinities: INT, INTEGER, BIGINT, … are one).
import assert from "node:assert/strict";
import { DST, ENGINES, SRC, lintDialog, openPath, rowMenu, saveEngines } from "../qa3-schema.mjs";

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE users (id INTEGER PRIMARY KEY, code INT UNIQUE);
CREATE TABLE items (id INTEGER PRIMARY KEY, user_code INTEGER REFERENCES users(code));
CREATE INDEX items_user_code ON items(user_code);
CREATE TABLE profiles (id INTEGER PRIMARY KEY REFERENCES users(id), bio TEXT);
CREATE TABLE lines (order_id INTEGER REFERENCES users(id), product_id INTEGER, PRIMARY KEY (order_id, product_id));`);
  ENGINES.sqlite.seed(dir, DST, "");
  saveEngines(dir, ["sqlite"]);
}

export default async function (page) {
  await openPath(page, ["QA3 SQLite src", "main"]);
  await rowMenu(page, ["QA3 SQLite src"], "main", "Check schema");
  await page.waitFor(`${lintDialog} !== null && !${lintDialog}.textContent.includes('Reading the tables')`, 15_000);
  const found = await page.eval(`[...${lintDialog}.querySelectorAll('.lint-findings li')].map(li => li.textContent)`);
  assert.deepEqual(found, [], "false findings");
}
