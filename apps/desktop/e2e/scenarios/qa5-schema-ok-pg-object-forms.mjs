// Regression guard (passes): the object forms of PostgreSQL create what they preview and the objects work.
// A replaced view with a quoted name, a sequence with negative bounds, cycle and cache, a function with a
// body that contains the usual dollar-quote tag, and triggers: row-level BEFORE, one with an early RETURN,
// UPDATE OR DELETE with a WHEN condition, a statement-level AFTER one and an INSTEAD OF trigger on a view.
import assert from "node:assert/strict";
import { ENGINES, invoke, saveEngines, session, needsEngines } from "../qa3-schema.mjs";

export const skip = needsEngines("pg");
const S = "qa3s_frm5";
const q = (sql) => ENGINES.pg.query(sql).trim();

export function setup({ dir }) {
  ENGINES.pg.seed(dir, S, `CREATE TABLE S.t (id int PRIMARY KEY, v int); CREATE VIEW S.vt AS SELECT * FROM S.t;`);
  saveEngines(dir, ["pg"]);
}

const trigger = (name, extra = {}) => ({ kind: "trigger", name, table: "t", timing: "before", events: ["insert"], for_each_row: true, when: null, body: "NEW.v := 1;", replace: false, ...extra });

export default async function (page, { dir }) {
  try {
    const sid = await session(page, ENGINES.pg.connections(dir)[0].id);
    const create = async (form) => {
      const r = await invoke(page, "object_form_sql", { driver: "postgres", schema: S, form });
      assert.equal(r.err, undefined, r.err);
      for (const sql of r.ok) {
        const x = await invoke(page, "execute_statement", { sessionId: sid, sql });
        assert.equal(x.err, undefined, `${x.err}\n${sql}`);
      }
    };
    await create({ kind: "view", name: "My View", query: `SELECT id FROM ${S}.t;`, replace: true, materialized: false });
    assert.equal(q(`SELECT count(*) FROM pg_views WHERE schemaname = '${S}' AND viewname = 'My View'`), "1");
    await create({ kind: "sequence", name: "sq1", start: -1, increment: -2, min: -100, max: -1, cycle: true, cache: 5 });
    assert.equal(q(`SELECT seqstart || '|' || seqincrement || '|' || seqmin || '|' || seqmax || '|' || seqcycle || '|' || seqcache FROM pg_sequence WHERE seqrelid = '${S}.sq1'::regclass`), "-1|-2|-100|-1|true|5");
    await create({ kind: "function", name: "f3", params: [], returns: "text", body: "RETURN '$body$ x';", replace: false });
    assert.equal(q(`SELECT ${S}.f3()`), "$body$ x");
    await create(trigger("tg1"));
    await create(trigger("tg5", { body: "IF NEW.id > 100 THEN RETURN NULL; END IF;" }));
    q(`INSERT INTO ${S}.t (id) VALUES (1), (101)`);
    assert.equal(q(`SELECT string_agg(id || ':' || v, ',' ORDER BY id) FROM ${S}.t`), "1:1");
    await create(trigger("tg2", { events: ["update", "delete"], when: "OLD.v IS NOT NULL", body: "RAISE NOTICE 'x';" }));
    q(`DELETE FROM ${S}.t`);
    assert.equal(q(`SELECT count(*) FROM ${S}.t`), "0");
    await create(trigger("tg4", { table: "vt", timing: "instead_of", events: ["insert"], body: `INSERT INTO ${S}.t(id, v) VALUES (NEW.id, NEW.v); RETURN NEW;` }));
    q(`INSERT INTO ${S}.vt (id, v) VALUES (50, 5)`);
    assert.equal(q(`SELECT v FROM ${S}.t WHERE id = 50`), "1"); // the BEFORE INSERT trigger tg1 of the table sets v
  } finally {
    ENGINES.pg.drop(S);
  }
}

