// Regression guard (passes): the table editor on PostgreSQL — a new table with a quoted name,
// a default with a quote, a comment, a unique index, a foreign key with ON DELETE CASCADE and
// a CHECK; then an existing table renamed with a column renamed, retyped, set NOT NULL with a
// default, one dropped and one added, its row kept; and a failing script rolled back whole.
import assert from "node:assert/strict";
import { DBS, connectTo, expand, menu, needs, row, saveConnections } from "../qa-nav-helpers.mjs";
import { addRow, cell, check, choose, confirm, editor, fill, modal, openSave, section } from "../qa-nav-table-editor.mjs";

export const skip = needs("postgres");

const S = "qa_nav_ok_te";
const q = (sql) => DBS.postgres.exec(sql).trim();

export function setup({ dir }) {
  DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${S} CASCADE; CREATE SCHEMA ${S};
    CREATE TABLE ${S}.qa_ref (id int PRIMARY KEY);
    CREATE TABLE ${S}.qa_alter (id int PRIMARY KEY, name varchar(20) NOT NULL, qty int DEFAULT 1, note text);
    INSERT INTO ${S}.qa_alter VALUES (1, 'n', 2, 'x');`);
  saveConnections(dir, DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a0018"));
}

export default async function (page) {
  try {
    await connectTo(page, "QA PG", "platypus");
    await expand(page, S, "qa_ref");

    await menu(page, S, "Create table…");
    await page.waitFor(`${editor}?.querySelector('.table-name input') != null`, 10_000);
    await fill(page, `${editor}.querySelector('.table-name input')`, 'qa New "T"');
    await fill(page, cell(0, 1), "bigint");
    await addRow(page);
    await fill(page, cell(1, 0), "Name Col");
    await fill(page, cell(1, 1), "varchar(50)");
    await fill(page, cell(1, 3), "'it''s'");
    await fill(page, cell(1, 5), "a ' comment");
    await addRow(page);
    await fill(page, cell(2, 0), "ref_id");
    await fill(page, cell(2, 1), "integer");
    await section(page, "Indexes");
    await addRow(page);
    await fill(page, cell(0, 1), "Name Col, ref_id");
    await check(page, cell(0, 2));
    await section(page, "Foreign keys");
    await addRow(page);
    await fill(page, cell(0, 1), "ref_id");
    await fill(page, cell(0, 3), "qa_ref");
    await fill(page, cell(0, 4), "id");
    await choose(page, cell(0, 5), "CASCADE");
    await section(page, "Constraints");
    await addRow(page);
    await fill(page, cell(0, 2), "ref_id > 0");
    await openSave(page);
    assert.equal(await confirm(page), null);
    const table = `'${S}."qa New ""T"""'::regclass`;
    assert.equal(q(`SELECT string_agg(conname || ' ' || pg_get_constraintdef(oid), '; ' ORDER BY conname) FROM pg_constraint WHERE conrelid = ${table}`),
      `qa New "T"_check1 CHECK ((ref_id > 0)); qa New "T"_fk1 FOREIGN KEY (ref_id) REFERENCES ${S}.qa_ref(id) ON DELETE CASCADE; qa New "T"_pkey PRIMARY KEY (id)`);
    assert.match(q(`SELECT indexdef FROM pg_indexes WHERE schemaname = '${S}' AND indexname = 'qa New "T"_idx1'`), /UNIQUE INDEX .* \("Name Col", ref_id\)/);
    assert.equal(q(`SELECT col_description(${table}, 2)`), "a ' comment");
    assert.equal(q(`SELECT column_default FROM information_schema.columns WHERE table_schema = '${S}' AND column_name = 'Name Col'`), "'it''s'::character varying");

    await menu(page, "qa_alter", "Edit table…");
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 4 && document.querySelector('.tab.active')?.textContent.startsWith('qa_alter')`, 10_000);
    await fill(page, cell(1, 0), "full name");
    await fill(page, cell(1, 1), "varchar(100)");
    await fill(page, cell(2, 3), "5");
    await check(page, cell(2, 2));
    await page.eval(`${editor}.querySelectorAll('tbody tr')[3].querySelector('button').click()`);
    await addRow(page);
    await fill(page, cell(3, 0), "added");
    await fill(page, cell(3, 1), "boolean");
    await check(page, cell(3, 2));
    await fill(page, cell(3, 3), "false");
    await fill(page, `${editor}.querySelector('.table-name input')`, "qa_altered");
    await openSave(page);
    assert.equal(await confirm(page), null);
    assert.equal(q(`SELECT string_agg(column_name || ' ' || data_type || ' ' || is_nullable, ', ' ORDER BY ordinal_position) FROM information_schema.columns WHERE table_schema = '${S}' AND table_name = 'qa_altered'`),
      "id integer NO, full name character varying NO, qty integer NO, added boolean NO");
    assert.equal(q(`SELECT * FROM ${S}.qa_altered`), "1|n|2|f");
    await page.waitFor(`${row("qa_altered")} !== undefined && ${row("qa_alter")} === undefined`, 10_000);

    // A failing statement rolls back the whole script.
    await page.waitFor(`${editor}?.querySelectorAll('tbody tr').length === 4 && ${editor}.querySelector('.object-header button.primary').disabled`, 10_000);
    await fill(page, cell(1, 0), "renamed");
    await addRow(page);
    await fill(page, cell(4, 0), "bad");
    await fill(page, cell(4, 1), "notatype");
    await openSave(page);
    assert.match(await confirm(page), /type "notatype" does not exist/);
    await page.eval(`[...${modal}.querySelectorAll('button')].find(b => b.textContent === 'Cancel').click()`);
    assert.equal(q(`SELECT count(*) FROM information_schema.columns WHERE table_schema = '${S}' AND column_name = 'full name'`), "1");
  } finally {
    DBS.postgres.exec(`DROP SCHEMA IF EXISTS ${S} CASCADE;`);
  }
}
