// Bug ORA-16: Oracle numbers without a declared precision are handled as text after the
// query: NUMBER columns, INTEGER (NUMBER(38)), and every computed number (COUNT(*), SUM,
// LEVEL, arithmetic) come back as Utf8 (crates/platypus-driver-oracle/src/convert.rs, `kind`:
// Number(0, -127) and Number(p > 18, 0) -> Kind::Text, which keeps them exact) and nothing
// downstream knows they are numbers. Column statistics compare them as strings — for 1..20 the
// Maximum is "9", no histogram — they sit left-aligned in the grid, and the statistics/SQL on
// the result order them as text. The same values cast to NUMBER(5) behave as numbers.
// Steps: SELECT LEVEL n FROM DUAL CONNECT BY LEVEL <= 20; hover the N header (statistics).
// Expected: Minimum 1, Maximum 20, a histogram (as for CAST(LEVEL AS NUMBER(5))). Actual:
// Maximum 9, no bars.
import { assert, needsOracle, oracleEditor, oracleSetup, runOra } from "../qa-ora.mjs";
import { profileOf } from "../qa-ana.mjs";

export const skip = needsOracle();
export const setup = ({ dir }) => oracleSetup(dir);

export default async function (page) {
  await oracleEditor(page);
  const { status } = await runOra(page, "SELECT LEVEL n, CAST(LEVEL AS NUMBER(5)) n5 FROM DUAL CONNECT BY LEVEL <= 20");
  assert.equal(status, "20 rows");
  const typed = await profileOf(page, "N5");
  assert.deepEqual([typed.fields.Minimum, typed.fields.Maximum], ["1", "20"]);
  const plain = await profileOf(page, "N");
  assert.deepEqual([plain.fields.Minimum, plain.fields.Maximum], ["1", "20"], "NUMBER without precision compared as text");
  assert.ok(plain.bars > 0, "no histogram for a NUMBER column");
}
