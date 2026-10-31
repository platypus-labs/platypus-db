// QA5-S13 (medium). Compare with... on MySQL drops the table options: a table with
// DEFAULT CHARSET=latin1 and ENGINE=MyISAM arrives as an InnoDB table in the database's default
// character set (columns added later get another charset than the rest of the table).
// Steps: a (id, v varchar(10)) ENGINE=MyISAM DEFAULT CHARSET=latin1 COLLATE=latin1_general_ci; compare with an empty schema; run.
// Expected: the target table has the same engine and default collation. Actual: InnoDB, utf8mb4_0900_ai_ci.
// Code: crates/platypus-schema/src/create.rs parse (options are kept for SQLite/PostgreSQL/ClickHouse only).
import assert from "node:assert/strict";
import { DST, ENGINES, PLACES, SRC, needsEngines, openPath, roundTrip, saveEngines } from "../qa3-schema.mjs";

const KEY = "mysql";
export const skip = needsEngines(KEY);
const SRC_SQL = `CREATE TABLE S.a (id int PRIMARY KEY, v varchar(10)) ENGINE=MyISAM DEFAULT CHARSET=latin1 COLLATE=latin1_general_ci;`;

export function setup({ dir }) {
  
  ENGINES[KEY].seed(dir, SRC, SRC_SQL);
  ENGINES[KEY].seed(dir, DST, "");
  saveEngines(dir, [KEY]);
}

const read = (sql) => String(ENGINES[KEY].root(sql)).replace(/\r/g, "").trim();

export default async function (page) {
  const p = PLACES[KEY];
  try {
    await openPath(page, [...p.chain, p.last]);
    const r = await roundTrip(page, p.chain, p.last, p.target, p.targetSchema);
    assert.ok(r.sql, `Compare gave no script: ${JSON.stringify(r.first)}`);
    assert.equal(r.runError, null, r.sql);
    assert.equal(read(`SELECT engine || '|' || table_collation FROM information_schema.tables WHERE table_schema = '${DST}' AND table_name = 'a'`).replace("\n", ""), "MyISAM|latin1_general_ci");
  } finally {
    
    ENGINES[KEY].drop(SRC);
    ENGINES[KEY].drop(DST);
  }
}
