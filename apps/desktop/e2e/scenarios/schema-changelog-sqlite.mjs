// Group I: the schema comparison written as a Flyway migration or a Liquibase changelog. Two
// SQLite databases: the source (customers.email TEXT NOT NULL, a new table orders) and a target
// whose customers.email is an INTEGER, so that SQLite rebuilds the table between two
// `PRAGMA foreign_keys` statements. A third database equals the source.
// Checked: the Output choice, the Flyway file (header, statements, no BEGIN/COMMIT), the
// Liquibase file (changesets numbered from 1, the PRAGMAs with runInTransaction:false, one
// `--comment`), the editor output still working, a cancelled save dialog writing nothing and
// leaving the dialog open, and equal schemas showing the notice and writing nothing.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { answerSave, fill } from "../qa-xfer.mjs";
import { DST, ENGINES, SRC, buttonIn, compareDialog, compareUi, invoke, openPath, rowMenu, saveEngines } from "../qa3-schema.mjs";

const SAME = "qa3s_same";
const SAME_NAME = "QA3 SQLite same";

export function setup({ dir }) {
  ENGINES.sqlite.seed(dir, SRC, `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));`);
  ENGINES.sqlite.seed(dir, DST, "CREATE TABLE customers (id INTEGER PRIMARY KEY, email INTEGER NOT NULL);");
  ENGINES.sqlite.seed(
    dir,
    SAME,
    `CREATE TABLE customers (id INTEGER PRIMARY KEY, email TEXT NOT NULL);
CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers(id));`,
  );
  saveEngines(dir, ["sqlite"], [
    { id: "00000000-0000-4000-8000-0000000c3009", name: SAME_NAME, body: `driver = "sqlite"\ndatabase = '${join(dir, `${SAME}.db`)}'` },
  ]);
  mkdirSync(join(dir, "out"));
}

const outputSelect = `${compareDialog}.querySelectorAll('select')[2]`;
const compareButton = buttonIn(compareDialog, "Compar");

/** Opens Compare with… on the source and picks the target connection and the output. */
async function openCompare(page, targetConnection, output) {
  await rowMenu(page, ["QA3 SQLite src"], "main", "Compare with");
  await page.waitFor(`${compareDialog} !== null`);
  const [conn, schema] = [0, 1].map((i) => `${compareDialog}.querySelectorAll('select')[${i}]`);
  await page.eval(fill(conn, (await page.eval(`[...${conn}.options].find(o => o.textContent === ${JSON.stringify(targetConnection)})?.value`)) ?? ""));
  await page.waitFor(`[...${schema}.options].some(o => o.textContent === 'main')`, 30_000);
  await page.eval(fill(schema, "main"));
  await page.waitFor(`${schema}.value === 'main'`);
  await page.eval(fill(`${outputSelect}`, output));
  await page.waitFor(`${outputSelect}.value === ${JSON.stringify(output)}`);
}

/** Clicks Compare, once the save dialog has been answered, and waits for the answer to be used. */
async function compareAndWait(page) {
  await page.eval(`${compareButton}.click()`);
  await page.waitFor(`window.__qaAnswers?.['plugin:dialog|save'] === undefined`, 30_000);
}

const closeDialog = async (page) => {
  await page.eval(`${buttonIn(compareDialog, "Close")}.click()`);
  await page.waitFor(`${compareDialog} === null`);
};

export default async function (page, { dir }) {
  const out = join(dir, "out");
  await openPath(page, ["QA3 SQLite src", "main"]);

  // The dialog offers three outputs, the SQL editor first.
  await rowMenu(page, ["QA3 SQLite src"], "main", "Compare with");
  await page.waitFor(`${compareDialog} !== null`);
  assert.deepEqual(await page.eval(`[...${outputSelect}.options].map(o => o.textContent)`), ["SQL editor", "Flyway migration…", "Liquibase changelog…"]);
  assert.equal(await page.eval(`${outputSelect}.value`), "editor", "the SQL editor is the default");
  await closeDialog(page);

  // The default output is unchanged: the script opens in the editor of the target.
  const editor = await compareUi(page, ["QA3 SQLite src"], "main", "QA3 SQLite dst", "main");
  assert.ok(editor.sql?.includes("orders"), `the editor shows the script: ${JSON.stringify(editor)}`);
  assert.deepEqual(readdirSync(out), [], "the editor output writes no file");

  // The default file name of a Flyway migration.
  const name = await invoke(page, "flyway_file_name", { version: "20260102030405", description: "make main like main" });
  assert.equal(name.ok, "V20260102030405__make_main_like_main.sql");

  // Flyway.
  const flyway = join(out, "V1__changes.sql");
  await openCompare(page, "QA3 SQLite dst", "flyway");
  await answerSave(page, flyway);
  await compareAndWait(page);
  await page.waitFor(`${compareDialog} === null`, 30_000);
  assert.ok(existsSync(flyway), "the Flyway file is written");
  const f = readFileSync(flyway, "utf8");
  const fLines = f.trimEnd().split("\n");
  assert.equal(fLines[0], "-- make main like main");
  const fSql = fLines.filter((l) => !l.startsWith("--"));
  assert.equal(fSql[0], "PRAGMA foreign_keys = OFF;", f);
  assert.equal(fSql.at(-1), "PRAGMA foreign_keys = ON;", f);
  assert.ok(/CREATE TABLE (main\.)?"?orders"?/i.test(f), `the new table is created: ${f}`);
  assert.ok(/customers/.test(f), f);
  assert.ok(!/^(BEGIN|COMMIT)\b/im.test(f), `no transaction wrapper in a Flyway migration: ${f}`);
  // The PRAGMAs work only outside Flyway's transaction: its script configuration says so.
  assert.equal(readFileSync(`${flyway}.conf`, "utf8"), "executeInTransaction=false\n");

  // Liquibase.
  const liquibase = join(out, "changelog.sql");
  await openCompare(page, "QA3 SQLite dst", "liquibase");
  await answerSave(page, liquibase);
  await compareAndWait(page);
  await page.waitFor(`${compareDialog} === null`, 30_000);
  assert.ok(existsSync(liquibase), "the Liquibase file is written");
  const l = readFileSync(liquibase, "utf8");
  assert.ok(l.startsWith("-- liquibase formatted sql\n"), l);
  const sets = [...l.matchAll(/^-- changeset (\S+):(\d{14})-(\d+)( .*)?$/gm)];
  assert.ok(sets.length >= 4, `a changeset per statement (two PRAGMAs and the DDL): ${l}`);
  assert.ok(sets.every((s) => s[1] === "platypusdb" && s[2] === sets[0][2]), "author platypusdb, one id for all");
  assert.deepEqual(sets.map((s) => Number(s[3])), sets.map((_, i) => i + 1), "numbered from 1");
  const bodies = l.split(/^-- changeset .*$/m).slice(1);
  const outside = (body) => /^PRAGMA foreign_keys = (OFF|ON);$/m.test(body);
  bodies.forEach((body, i) => {
    assert.equal(/runInTransaction:false/.test(sets[i][4] ?? ""), outside(body), `changeset ${i + 1}: ${sets[i][0]} / ${body}`);
  });
  assert.ok(outside(bodies[0]) && outside(bodies.at(-1)), "the PRAGMAs open and close the changelog");
  assert.equal(l.match(/^--comment: make main like main$/gm)?.length, 1, "one comment, on the first changeset");
  assert.ok(bodies[0].includes("--comment"), "the comment is on the first changeset");
  assert.ok(!/^(BEGIN|COMMIT)\b/im.test(l), `no transaction wrapper in a changelog: ${l}`);

  // A cancelled save dialog writes nothing and leaves the dialog open and usable.
  const before = readdirSync(out).sort();
  await openCompare(page, "QA3 SQLite dst", "flyway");
  await answerSave(page, null);
  await compareAndWait(page);
  await page.waitFor(`!${compareButton}.disabled && ${compareButton}.textContent.trim() === 'Compare'`, 15_000);
  assert.ok(await page.eval(`${compareDialog} !== null`), "the dialog stays open");
  assert.equal(await page.eval(`${compareDialog}.querySelector('.notice')`), null, "no notice or error after a cancel");
  assert.deepEqual(readdirSync(out).sort(), before, "nothing is written");
  await closeDialog(page);

  // The same schemas: the notice, and the save dialog is not even asked.
  await openCompare(page, SAME_NAME, "liquibase");
  await answerSave(page, join(out, "same.sql"));
  await page.eval(`${compareButton}.click()`);
  await page.waitFor(`${compareDialog}?.querySelector('.notice') !== null`, 30_000);
  assert.ok(await page.eval(`${compareDialog}.querySelector('.notice').textContent.includes('nothing to change')`));
  assert.equal(await page.eval(`window.__qaAnswers['plugin:dialog|save'] !== undefined`), true, "the save dialog was not opened");
  assert.ok(!existsSync(join(out, "same.sql")), "no file for equal schemas");
  assert.deepEqual(readdirSync(out).sort(), before, "nothing is written");
  await closeDialog(page);
}
