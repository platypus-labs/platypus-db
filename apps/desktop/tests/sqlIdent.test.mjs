// Unit tests of identifier quoting in the UI (src/sqlIdent.ts), which mirrors
// `Dialect::quote_ident` in crates/platypus-sql/src/dialect.rs.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import {
  MYSQL_RESERVED,
  ORACLE_RESERVED,
  RESERVED,
  SQLITE_RESERVED,
  TSQL_RESERVED,
  quoteIdent,
  redisArg,
} from "../src/sqlIdent.ts";

const dialect = readFileSync(new URL("../../../crates/platypus-sql/src/dialect.rs", import.meta.url), "utf8");

/** The words of a `const NAME: &[&str]` list of dialect.rs. */
function rustList(name) {
  const body = dialect.match(new RegExp(`const ${name}: &\\[&str\\] = &\\[([\\s\\S]*?)\\];`))?.[1];
  assert.ok(body, `${name} in dialect.rs`);
  return [...body.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

test("the reserved lists are those of dialect.rs", () => {
  const lists = { RESERVED, TSQL_RESERVED, ORACLE_RESERVED, MYSQL_RESERVED, SQLITE_RESERVED };
  for (const [name, set] of Object.entries(lists)) {
    assert.deepEqual([...set].sort(), rustList(name).sort(), name);
  }
});

test("PostgreSQL quotes the SQL value functions it would read in FROM", () => {
  for (const name of ["current_role", "current_schema", "current_catalog", "session_user", "system_user", "localtime", "localtimestamp"]) {
    assert.equal(quoteIdent(name, "postgres"), `"${name}"`);
  }
  assert.equal(quoteIdent("orders", "postgres"), "orders");
  assert.equal(quoteIdent("rank", "postgres"), "rank");
});

test("MySQL quotes its own reserved words", () => {
  for (const name of ["rank", "range", "interval", "groups", "lead", "read", "change", "match", "div"]) {
    assert.equal(quoteIdent(name, "mysql"), `\`${name}\``);
  }
  assert.equal(quoteIdent("orders", "mysql"), "orders");
});

test("SQLite quotes the keywords that cannot be bare names", () => {
  for (const name of ["transaction", "commit", "add", "alter", "escape", "nothing", "autoincrement"]) {
    assert.equal(quoteIdent(name, "sqlite"), `"${name}"`);
    assert.equal(quoteIdent(name, "libsql"), `"${name}"`);
  }
  assert.equal(quoteIdent("rank", "sqlite"), "rank");
});

test("Redis names stay as they are; arguments are quoted as redis-cli reads them", () => {
  assert.equal(quoteIdent("db0", "redis"), "db0");
  assert.equal(quoteIdent("hash", "redis"), "hash");
  assert.equal(redisArg("user:*"), "user:*");
  assert.equal(redisArg("a b"), '"a b"');
  assert.equal(redisArg('say "hi"'), '"say \\"hi\\""');
  assert.equal(redisArg("it's"), `"it's"`);
  assert.equal(redisArg("a\\b c"), '"a\\\\b c"');
  assert.equal(redisArg("a\nb"), '"a\\nb"');
});
