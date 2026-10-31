// Unit tests of the shared query's file and link (src/share/shareQuery.ts) and of finding its
// connection (src/share/shareMatch.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  MAX_LINK_CHARS,
  ShareError,
  decodeFile,
  decodeLink,
  encodeFile,
  encodeLink,
  isShareLink,
} from "../src/share/shareQuery.ts";
import { describeTarget, heldReferences, matchTarget, prefillFromTarget } from "../src/share/shareMatch.ts";

const target = { driver: "postgres", host: "db.example.com", port: 5432, database: "shop", schema: "public", name: "Shop" };
const query = (over = {}) => ({ title: "Top orders", sql: "SELECT * FROM orders WHERE total > :min", parameters: {}, target, ...over });

const refused = (fn, code) => assert.throws(fn, (e) => e instanceof ShareError && e.code === code, code);
const refusedAsync = async (promise, code) => assert.rejects(promise, (e) => e instanceof ShareError && e.code === code, code);

test("a file round-trips and states its format and version", () => {
  const q = query({ parameters: { min: "10" } });
  const text = encodeFile(q);
  const doc = JSON.parse(text);
  assert.equal(doc.format, "platypus-query");
  assert.equal(doc.version, 1);
  assert.deepEqual(decodeFile(text), q);
});

test("parameters are written only when there are some", () => {
  assert.equal("parameters" in JSON.parse(encodeFile(query())), false);
  assert.deepEqual(decodeFile(encodeFile(query())).parameters, {});
});

test("the target carries no credentials, whatever the caller passes", () => {
  const dirty = { ...target, user: "root", password: "hunter2", ssh: { key: "k" }, token: "t" };
  const text = encodeFile(query({ target: dirty }));
  assert.deepEqual(Object.keys(JSON.parse(text).target).sort(), ["database", "driver", "host", "name", "port", "schema"]);
  assert.ok(!/hunter2|root|token/.test(text));
});

test("a file with credentials in the target is read without them", () => {
  const doc = JSON.parse(encodeFile(query()));
  doc.target.password = "x";
  doc.target.user = "u";
  assert.deepEqual(decodeFile(JSON.stringify(doc)).target, target);
});

test("a byte order mark is tolerated", () => {
  assert.deepEqual(decodeFile(`﻿${encodeFile(query())}`), query());
});

test("files of another format or version are refused", () => {
  const doc = JSON.parse(encodeFile(query()));
  refused(() => decodeFile("not json"), "format");
  refused(() => decodeFile("[]"), "format");
  refused(() => decodeFile(JSON.stringify({ ...doc, format: "other" })), "format");
  refused(() => decodeFile(JSON.stringify({ ...doc, version: 2 })), "version");
  refused(() => decodeFile(JSON.stringify({ ...doc, version: "1" })), "version");
  const { version, ...none } = doc;
  assert.equal(version, 1);
  refused(() => decodeFile(JSON.stringify(none)), "version");
});

test("wrong shapes are refused", () => {
  const doc = JSON.parse(encodeFile(query()));
  const bad = (patch, code = "shape") => refused(() => decodeFile(JSON.stringify({ ...doc, ...patch })), code);
  bad({ sql: 5 });
  bad({ sql: undefined });
  bad({ title: 3 });
  bad({ target: "db" });
  bad({ target: null });
  bad({ target: { ...target, driver: 1 } });
  bad({ target: { ...target, port: "5432" } });
  bad({ target: { ...target, port: 70000 } });
  bad({ target: { ...target, port: 1.5 } });
  bad({ parameters: [] });
  bad({ parameters: { a: 1 } });
});

test("oversize files and fields are refused", () => {
  const doc = JSON.parse(encodeFile(query()));
  refused(() => decodeFile(" ".repeat(1024 * 1024 + 1)), "too_large");
  refused(() => decodeFile(JSON.stringify({ ...doc, sql: "x".repeat(1024 * 1024 + 1) })), "too_large");
  refused(() => decodeFile(JSON.stringify({ ...doc, title: "t".repeat(600) })), "too_large");
  refused(() => decodeFile(JSON.stringify({ ...doc, target: { ...target, host: "h".repeat(600) } })), "too_large");
});

test("a link round-trips", async () => {
  const q = query({ parameters: { min: "10" }, sql: "SELECT 'è', '日本'" });
  const link = await encodeLink(q);
  assert.ok(link.startsWith("platypus://query?d="), link);
  assert.ok(/^[\x21-\x7e]+$/.test(link) && !/[+/=]/.test(link.slice(19)), "url safe");
  assert.ok(isShareLink(link));
  assert.deepEqual(await decodeLink(link), q);
  assert.deepEqual(await decodeLink(`  ${link}\n`), q);
});

test("a repetitive query is compressed", async () => {
  const q = query({ sql: "SELECT 1 UNION ALL\n".repeat(300) });
  const link = await encodeLink(q);
  assert.ok(link.length < 1000, String(link.length));
  assert.equal((await decodeLink(link)).sql, q.sql);
});

test("a query too big for a link is refused, so the file is offered", async () => {
  // Random-looking text does not compress.
  let seed = 7;
  const noise = Array.from({ length: 12000 }, () => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) % 94) + 33)
    .map((c) => String.fromCharCode(c))
    .join("");
  await refusedAsync(encodeLink(query({ sql: noise })), "too_large");
  assert.ok(MAX_LINK_CHARS >= 8000);
});

test("bad links are refused", async () => {
  await refusedAsync(decodeLink("platypus://other?d=AAAA"), "link");
  await refusedAsync(decodeLink("https://example.com"), "link");
  await refusedAsync(decodeLink("platypus://query"), "link");
  await refusedAsync(decodeLink("platypus://query?x=1"), "link");
  await refusedAsync(decodeLink("platypus://query?d=!!!"), "link");
  await refusedAsync(decodeLink("platypus://query?d=_w"), "link");
  // A valid payload that is not a query.
  const text = Buffer.from(`\0${JSON.stringify({ format: "x" })}`).toString("base64url");
  await refusedAsync(decodeLink(`platypus://query?d=${text}`), "format");
  // Marked as deflated, but not deflate data.
  await refusedAsync(decodeLink(`platypus://query?d=${Buffer.from("\u0001hello world").toString("base64url")}`), "link");
});

test("a link that inflates past the limit is refused", async () => {
  const { deflateRawSync } = await import("node:zlib");
  const bomb = deflateRawSync(Buffer.alloc(8 * 1024 * 1024, 32));
  assert.ok(bomb.length < 20000);
  await refusedAsync(decodeLink(`platypus://query?d=${Buffer.concat([Buffer.from([1]), bomb]).toString("base64url")}`), "too_large");
});

test("a link built by hand with plain JSON is read", async () => {
  const json = JSON.stringify(JSON.parse(encodeFile(query())));
  const link = `platypus://query/?d=${Buffer.concat([Buffer.from([0]), Buffer.from(json)]).toString("base64url")}`;
  assert.deepEqual(await decodeLink(link), query());
});

const saved = (id, over = {}) => ({ id, name: id, driver: "postgres", host: "db.example.com", port: 5432, database: "shop", ...over });

test("describing a connection keeps the place and drops the rest", () => {
  const conn = { ...saved("a", { name: "Shop", init: { schema: "public", time_zone: null, statements: "" } }), user: "me", password: "x" };
  assert.deepEqual(describeTarget(conn), target);
  assert.equal(describeTarget(conn, "other").database, "other");
  assert.equal(describeTarget(saved("b")).schema, null);
});

test("same driver, host, port and database is an exact match", () => {
  const all = [saved("a"), saved("b", { host: "DB.Example.com" }), saved("c", { port: 5433 }), saved("d", { driver: "mysql" })];
  const { exact, candidates } = matchTarget(target, all);
  assert.deepEqual(exact.map((c) => c.id), ["a", "b"]);
  assert.deepEqual(candidates.map((c) => c.id), ["c"]);
});

test("a default port matches an empty one", () => {
  assert.equal(matchTarget({ ...target, port: null }, [saved("a", { port: null })]).exact.length, 1);
});

test("candidates are the same driver, the likeliest first", () => {
  const all = [
    saved("other", { host: "x", database: "y" }),
    saved("host", { host: "db.example.com", database: "y" }),
    saved("db", { host: "x", database: "shop" }),
    saved("named", { host: "x", database: "y", name: "Shop" }),
    saved("mysql", { driver: "mysql", database: "shop" }),
  ];
  const { exact, candidates } = matchTarget(target, all);
  assert.deepEqual(exact, []);
  assert.deepEqual(candidates.map((c) => c.id), ["db", "host", "named", "other"]);
});

test("a file database matches by path", () => {
  const file = { driver: "sqlite", host: "", port: null, database: "/data/a.db", schema: null, name: "A" };
  const { exact, candidates } = matchTarget(file, [saved("a", { driver: "sqlite", host: "", port: null, database: "/data/a.db" }), saved("b", { driver: "sqlite", host: "", port: null, database: "/data/b.db" })]);
  assert.deepEqual(exact.map((c) => c.id), ["a"]);
  assert.deepEqual(candidates.map((c) => c.id), ["b"]);
});

test("a new connection is prefilled without credentials", () => {
  const base = { ...saved("new", { name: "", host: "localhost", port: 5432, database: "", user: "" }), password: "" };
  const filled = prefillFromTarget(base, target);
  assert.deepEqual(
    [filled.id, filled.name, filled.driver, filled.host, filled.port, filled.database, filled.user],
    ["new", "Shop", "postgres", "db.example.com", 5432, "shop", ""],
  );
  assert.equal(filled.init.schema, "public");
  const bare = prefillFromTarget(base, { ...target, host: "", port: null, name: "", schema: null });
  assert.deepEqual([bare.name, bare.host, bare.port, "init" in bare], ["", "localhost", 5432, false]);
});

test("the sender's environment references are held, read only once the receiver edits them", () => {
  const base = { ...saved("new", { name: "", host: "localhost", port: 5432, database: "", user: "" }), password: "" };
  const filled = prefillFromTarget(base, {
    ...target,
    host: "${env:AWS_SECRET_ACCESS_KEY}.attacker.example",
    database: "${env:A}-${env:B}",
    schema: "$${env:KEPT}",
  });
  assert.deepEqual(
    [filled.host, filled.database, filled.init.schema],
    ["$${env:AWS_SECRET_ACCESS_KEY}.attacker.example", "$${env:A}-$${env:B}", "$${env:KEPT}"],
  );
  assert.equal(heldReferences("${${"), "$${$${");
  assert.equal(heldReferences(heldReferences("x${env:A}")), "x$${env:A}");
  assert.equal(heldReferences("plain"), "plain");
});
