// Unit tests of the summary of a comparison script (src/schema/scriptSummary.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { scriptSummary } from "../src/schema/scriptSummary.ts";

test("the summary counts created, altered and dropped tables, and every DROP", () => {
  const s = scriptSummary([
    "CREATE TABLE a (id int);",
    "  create table if not exists b (id int);",
    "ALTER TABLE c ADD COLUMN x int;",
    "ALTER TABLE c DROP COLUMN y;",
    "ALTER TABLE e ALTER COLUMN z DROP DEFAULT;",
    "ALTER TABLE e ALTER COLUMN z DROP NOT NULL;",
    "DROP TABLE d;",
    "CREATE INDEX i ON a (id);",
  ]);
  // c and e are altered by two statements each; dropping a default or a NOT NULL loses nothing.
  assert.deepEqual(s, { created: 2, altered: 2, dropped: 1, drops: 2 });
});

test("an empty script does nothing", () => {
  assert.deepEqual(scriptSummary([]), { created: 0, altered: 0, dropped: 0, drops: 0 });
});
