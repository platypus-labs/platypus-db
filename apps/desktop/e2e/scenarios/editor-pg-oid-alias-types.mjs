// SQL-18. PostgreSQL OID alias types are shown as bare numbers: pg_typeof(1) shows 23 instead
// of integer, 'pg_class'::regclass 1259 instead of pg_class, 'now'::regproc 1299. pg_typeof()
// is the usual way to ask a value's type, and catalog queries return regclass columns.
// Steps: PostgreSQL, Ctrl+Enter on the SELECT below, copy the grid with headers.
// Expected: integer, pg_class, now (the type output psql and DBeaver show).
// Actual: 23, 1259, 1299.
// Code: crates/platypus-driver-pg decodes reg* types through their binary form (an OID).
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SELECT pg_typeof(1) AS t, 'pg_class'::regclass AS c, 'now'::regproc AS p");
  assert.equal(await gridTsv(page), "t\tc\tp\ninteger\tpg_class\tnow");
}
