// XFER-02 (high). "Export all tables" overwrites files of tables whose names collide.
// Steps: a PostgreSQL schema with tables "Mix" and mix (names differing by case) and "a/b"
// and a_b; navigator › schema › "Export all tables…", CSV, choose a folder.
// Expected: four files, one per table (e.g. a suffix for a taken name), or a refusal.
// Actual: "Exported 4 rows." but only two files: Mix.csv and mix.csv are the same file on
// Windows, and "a/b" becomes a_b.csv, the name of the other table; the last table written
// wins and the others' rows are lost without a word.
// Suspected: apps/desktop/src-tauri/src/commands/export.rs:94-130 (file_name() and no check
// for names already used in the folder or in the same run).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { mkdirSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { closeModal, db, dbQuiet, DBS, expand, exportTablesDialog, menu, row, runJob, saveConnections, skipUnless } from "../transfer-helpers.mjs";

const SCHEMA = "qa_xfer_exp";

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db(
    "pg",
    `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE; CREATE SCHEMA ${SCHEMA};
     CREATE TABLE ${SCHEMA}."Mix" (v text); INSERT INTO ${SCHEMA}."Mix" VALUES ('upper');
     CREATE TABLE ${SCHEMA}.mix (v text); INSERT INTO ${SCHEMA}.mix VALUES ('lower');
     CREATE TABLE ${SCHEMA}."a/b" (v text); INSERT INTO ${SCHEMA}."a/b" VALUES ('slash');
     CREATE TABLE ${SCHEMA}.a_b (v text); INSERT INTO ${SCHEMA}.a_b VALUES ('underscore');`,
  );
}

export default async function (page, { dir }) {
  try {
    const out = join(dir, "out");
    mkdirSync(out);
    await expand(page, DBS.pg.name, null);
    await page.waitFor(`${row(SCHEMA, DBS.pg.name)} !== undefined`, 20_000);
    await page.eval(`window.__platypus.answer('folder', ${JSON.stringify(out)})`);
    await menu(page, SCHEMA, "Export all tables", exportTablesDialog, DBS.pg.name);
    await page.waitFor(`${exportTablesDialog}.textContent.includes('a_b, mix')`, 15_000);
    assert.match(await runJob(page, exportTablesDialog, "Export to folder…"), /Exported 4 rows/);
    await closeModal(page, exportTablesDialog);
    const values = readdirSync(out)
      .flatMap((f) => readFileSync(join(out, f), "utf8").split(/\r?\n/).slice(1))
      .filter(Boolean)
      .sort();
    assert.deepEqual(values, ["lower", "slash", "underscore", "upper"], `files: ${readdirSync(out).join(", ")}`);
  } finally {
    dbQuiet("pg", `DROP SCHEMA IF EXISTS ${SCHEMA} CASCADE`);
  }
}
