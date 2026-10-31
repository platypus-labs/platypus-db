// Regression guard (passes): on CockroachDB the undo script of a deleted row puts back every type
// as it was (INT[] with NULL, STRING[] with quotes, commas, backslashes and 'NULL', ENUM, FLOAT4, BIT,
// VARBIT, TIMETZ, COLLATE, TIMESTAMPTZ, DECIMAL, INTERVAL, BYTES, JSONB with a number above 2^64, INET),
// and a duplicated row leaves computed and identity columns to the database.
// (The connection uses `echo x` as its password command: the harness' `cmd /c echo.` prints nothing and
// the app now refuses an empty password command output, so qa2-dbs-ok-cockroach-edit cannot connect.)
import { join } from "node:path";
import { readFileSync, writeFileSync } from "node:fs";
import { CTRL, assert, button, conns, editBar, editorText, focusCell, needsPorts, openData, openPath, save, sh, statusBar, visibleTab } from "../qa-dbs.mjs";

export const skip = needsPorts(56257);

export function setup({ dir }) {
  sh("cockroach", `DROP SCHEMA IF EXISTS qa_g2 CASCADE; CREATE SCHEMA qa_g2;
CREATE TYPE qa_g2.mood AS ENUM ('sad','ok','happy');
CREATE TABLE qa_g2.t (id INT PRIMARY KEY DEFAULT unique_rowid(), ia INT[], sa STRING[], m qa_g2.mood, f FLOAT8, f4 FLOAT4, u UUID, bt BIT(8), vb VARBIT, ttz TIMETZ, c STRING COLLATE en, tstz TIMESTAMPTZ, dd DECIMAL, iv INTERVAL, by BYTES, j JSONB, ip INET, g INT AS (id * 2) STORED);`);
  sh("cockroach", `INSERT INTO qa_g2.t (id, ia, sa, m, f, f4, u, bt, vb, ttz, c, tstz, dd, iv, by, j, ip) VALUES (1, ARRAY[1,NULL,3], ARRAY['a b','c,"d"','NULL','', 'back\\slash', 'it''s {x}'], 'happy', 0.1, 0.1, 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a12', B'10101010', B'101', '10:00:00+02', 'Zz', '2024-01-02 03:04:05.123456+05:30', 0.1234567890123456789, '1 year 2 mons 3 days 04:05:06.789', '\\xcafe', '{"a": 12345678901234567890, "b": 1.10}', '10.0.0.1/24'),
(2, NULL, NULL, NULL, 'NaN', 'Infinity', NULL, NULL, NULL, NULL, NULL, NULL, 'NaN', NULL, NULL, NULL, NULL);`);
  conns(dir, [["QA CRDB", "cockroach"]]);
  const f = join(dir, "connections.toml");
  writeFileSync(f, readFileSync(f, "utf8").replace("cmd /c echo.", "echo x"));
}

export default async function (page) {
  try {
    const dump = () => sh("cockroach", "SELECT id, ia, sa, m, f, f4, u, bt, vb, ttz, c, tstz, dd, iv, by, j, ip, g FROM qa_g2.t ORDER BY id;");
    await openPath(page, ["QA CRDB", "platypus", "qa_g2", "Tables", "t"]);
    await openData(page, "t");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    // Duplicate row (Ctrl+D): the computed g is not written.
    await focusCell(page, 1, 1);
    await page.key("d", "KeyD", 68, CTRL);
    await page.waitFor(`${editBar}.includes('1 row changed')`, 5_000);
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("cockroach", "SELECT count(*) FROM qa_g2.t;").split("\n")[1], "3");

    // Delete the first row, run its undo script: the table is as it was.
    const before = dump();
    await page.sleep(500);
    await focusCell(page, 0, 1);
    await page.key("Delete", "Delete", 46);
    assert.match(await save(page), /to edit a cell/);
    await page.waitFor(`${statusBar}.includes('Undo script')`, 15_000);
    await page.eval(`${button(visibleTab, "Undo script")}.click()`);
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent.includes('INSERT')`, 15_000);
    sh("cockroach", await page.eval(editorText));
    assert.equal(dump(), before);  } finally {
    sh("cockroach", "DROP SCHEMA IF EXISTS qa_g2 CASCADE;");
  }
}


