// Unit tests of the pooler a PostgreSQL host suggests (src/components/pooler.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { suggestedPooler } from "../src/components/pooler.ts";

test("Supabase's pooler: transaction mode on 6543, session mode on 5432", () => {
  assert.deepEqual(suggestedPooler("aws-0-eu-central-1.pooler.supabase.com", 6543), { mode: "transaction", service: "Supabase" });
  assert.deepEqual(suggestedPooler(" AWS-0-EU.pooler.supabase.com ", 5432), { mode: "session", service: "Supabase" });
  assert.equal(suggestedPooler("db.abcdefgh.supabase.co", 5432), null, "the direct connection");
});

test("Neon's pooled host", () => {
  assert.deepEqual(suggestedPooler("ep-cool-darkness-123456-pooler.eu-central-1.aws.neon.tech", null), { mode: "transaction", service: "Neon" });
  assert.equal(suggestedPooler("ep-cool-darkness-123456.eu-central-1.aws.neon.tech", null), null);
});

test("other hosts suggest nothing", () => {
  assert.equal(suggestedPooler("localhost", 5432), null);
  assert.equal(suggestedPooler("", null), null);
});
