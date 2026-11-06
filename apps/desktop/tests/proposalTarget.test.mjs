// Unit tests of the table an agent's proposal names (src/agents/proposalTarget.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { proposedTable } from "../src/agents/proposalTarget.ts";

test("the table of a proposal is read from its JSON, never trusted", () => {
  assert.equal(proposedTable('{"schema":"public","table":"orders","updates":[]}'), "public.orders");
  assert.equal(proposedTable('{"schema":"","table":"orders"}'), "orders");
  assert.equal(proposedTable('{"table":42}'), null);
  assert.equal(proposedTable("not json"), null);
  assert.equal(proposedTable("null"), null);
  assert.equal(proposedTable(JSON.stringify({ table: "t".repeat(200) })).length, 81);
});
