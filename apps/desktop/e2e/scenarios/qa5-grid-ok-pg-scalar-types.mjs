// Regression guard (passes): scalar values typed in the PostgreSQL data editor are stored as typed. Integers at
// the limits (out of range is refused), numeric with 14 digits and an exponent, reals, text with quotes,
// backslashes, emoji and the words NULL and \N, char(n), timestamps with microseconds, a zone
// offset, time 24:00, intervals, uuid in capitals, json kept as typed and jsonb normalised, bytea, inet,
// money, xml, bit, point, ranges, timetz and tsvector: each is read back from the database and
// compared with what the cell should hold.
import { assert, connections, editBar, expand, focusCell, openData, save, setCell, sh, needs } from "../qa-grid.mjs";

export const skip = needs("pg");
const COLS = [
  ["i2", "smallint"], ["i8", "bigint"], ["n", "numeric(20,6)"], ["r", "real"], ["d", "double precision"], ["t", "text"],
  ["c", "char(5)"], ["dt", "date"], ["ts", "timestamp(6)"], ["tz", "timestamptz"], ["tm", "time(6)"], ["iv", "interval"],
  ["u", "uuid"], ["j", "json"], ["jb", "jsonb"], ["by", "bytea"], ["ia", "int[]"], ["ta", "text[]"], ["ip", "inet"],
  ["mo", "money"], ["xm", "xml"], ["bt", "bit(4)"], ["pt", "point"], ["tr", "tsrange"], ["tmz", "timetz"], ["tv", "tsvector"],
];
// [col, input, expected ::text or "ERR"]

const CASES = [
  ["i2", "32767", "32767"], ["i2", "32768", "ERR"], ["i2", "-0", "0"], ["i8", "9223372036854775807", "9223372036854775807"],
  ["i8", "9223372036854775808", "ERR"], ["n", "12345678901234.123456", "12345678901234.123456"], ["n", "1e3", "1000.000000"],
  ["n", "-0.0000001", "0.000000"], ["r", "0.1", "0.1"], ["d", "0.30000000000000004", "0.30000000000000004"], ["d", "1e308", "1e+308"],
  ["t", "a'b\\c\"d", "a'b\\c\"d"], ["t", "ünï 😀 日本", "ünï 😀 日本"], ["t", "NULL", "NULL"], ["t", "\\N", "\\N"],
  ["c", "abcdef", "ERR"],  
  ["ts", "2026-01-01 12:34:56.123456", "2026-01-01 12:34:56.123456"], ["ts", "2026-01-01T01:02:03", "2026-01-01 01:02:03"],
  ["tz", "2026-01-01 12:00:00+05:30", "2026-01-01 06:30:00+00"], ["tm", "23:59:59.999999", "23:59:59.999999"], ["tm", "24:00:00", "24:00:00"],
  ["iv", "1 year 2 mons 3 days 04:05:06", "1 year 2 mons 3 days 04:05:06"], ["iv", "-1 day", "-1 days"],
  ["u", "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11", "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11"], ["u", "nope", "ERR"],
  ["j", "{\"b\": 1,   \"a\":[1,2]}", "{\"b\": 1,   \"a\":[1,2]}"], ["jb", "{\"b\": 1,   \"a\":[1,2]}", "{\"a\": [1, 2], \"b\": 1}"], 
  ["by", "\\xdeadbeef", "\\xdeadbeef"], 
  ["ip", "192.168.0.1/24", "192.168.0.1/24"], ["mo", "$1,234.50", "$1,234.50"], ["xm", "<a>x</a>", "<a>x</a>"], ["bt", "1010", "1010"], ["bt", "101", "ERR"],
  ["pt", "(1,2)", "(1,2)"], ["tr", "[2026-01-01 00:00,2026-02-01 00:00)", "[\"2026-01-01 00:00:00\",\"2026-02-01 00:00:00\")"],
  ["tmz", "10:00:00+02", "10:00:00+02"], ["tv", "'a' 'b'", "'a' 'b'"],
];

export function setup({ dir }) {
  sh("pg", `DROP TABLE IF EXISTS qa5_pg; CREATE TABLE qa5_pg (id int PRIMARY KEY, ${COLS.map(([n, t]) => `${n} ${t}`).join(", ")});
    INSERT INTO qa5_pg (id) VALUES (1);`);
  connections(dir, [["QA PG", "pg"]]);
}

export default async function (page) {
  const out = [];
  try {
    await expand(page, "QA PG", "pg", "qa5_pg");
    await openData(page, "qa5_pg");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    for (const [col, input, expected] of CASES) {
      const ci = COLS.findIndex(([n]) => n === col) + 1;
      let got;
      try {
        await focusCell(page, 0, ci);
        await setCell(page, input);
        const s = await save(page);
        if (!/to edit a cell/.test(s)) {
          got = "ERR";
          await page.eval(`(() => { document.querySelector('.edit-error button')?.click(); })()`);
          // discard pending
          await page.key("z", "KeyZ", 90, 2);
          await page.sleep(300);
        } else got = sh("pg", `SELECT ${col}::text FROM qa5_pg WHERE id = 1`);
      } catch (e) {
        got = `EXC ${String(e.message).slice(0, 80)}`; await page.key("Escape", "Escape", 27); await page.key("Escape", "Escape", 27);
      }
      if (got !== expected) out.push(`${col} <- ${JSON.stringify(input)}: expected ${JSON.stringify(expected)} got ${JSON.stringify(got)}`);
      sh("pg", `UPDATE qa5_pg SET ${col} = NULL WHERE id = 1`);
    }
  } finally {
    sh("pg", "DROP TABLE IF EXISTS qa5_pg");
  }
  assert.equal(out.join("\n"), "");
}






