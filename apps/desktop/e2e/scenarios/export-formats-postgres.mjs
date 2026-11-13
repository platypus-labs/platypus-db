// The result export in every format against PostgreSQL, each file read back with nothing but
// node and compared with what the table holds: CSV (quoting of commas, quotes and newlines,
// the delimiters, no header row), JSON, NDJSON, XML, Markdown (pipes escaped, newlines as
// <br>), HTML (markup escaped), SQL INSERT statements (run back into a copy of the table),
// Excel (cell types: numbers, booleans, dates as serials, text inline) and Parquet (the footer
// read for rows, columns, types and codec; the file imported into a new table gives the rows
// back), and the compressions gzip and zstd of the text formats (decompressed with node and
// equal to the plain file).
// Edge cases: a result with no rows in every format (a header only, or an empty array, or a
// Parquet file with the columns and no rows), 50,000 rows (several batches) in CSV, NDJSON,
// Parquet, Excel and zstd, "Rows already loaded" as the source, a target folder that does not
// exist (the failure names the path and no file appears).
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync, zstdDecompressSync } from "node:zlib";
import { parquetInfo } from "../parquet.mjs";
import {
  chooseFile, closeModal, db, dbQuiet, DBS, exportResult, importDialog, menu, openEditor, runJob, runScript, saveConnections, showTable, skipUnless, zipEntry,
} from "../transfer-helpers.mjs";

const TABLE = "qa_expo";
const ALL = `SELECT * FROM ${TABLE} ORDER BY id`;

export const skip = () => skipUnless("pg");

export function setup({ dir }) {
  saveConnections(dir, ["pg"]);
  db(
    "pg",
    `DROP TABLE IF EXISTS ${TABLE}, ${TABLE}_sql, ${TABLE}_pq;
     CREATE TABLE ${TABLE} (id int PRIMARY KEY, name text, amount numeric(10,2), active bool, born date, seen timestamptz, note text);
     INSERT INTO ${TABLE} VALUES
       (1, 'Plain', 10.50, true, '2024-02-29', '2024-06-01 10:00:00+02', 'x'),
       (2, 'Comma, "quote"', -0.05, false, '1999-12-31', '2000-01-01 00:00:00+00', NULL),
       (3, 'Pipe | and <b>&amp; tag', 0, NULL, NULL, NULL, E'multi\\nline'),
       (4, 'Ünïcödé 😀', 123456.78, true, '2024-01-31', '2024-01-31 23:59:59.5+00', '');
     CREATE TABLE ${TABLE}_sql (LIKE ${TABLE} INCLUDING ALL)`,
  );
}

const HEADER = "id,name,amount,active,born,seen,note";
const EXPECTED_CSV =
  `${HEADER}\n` +
  `1,Plain,10.50,true,2024-02-29,2024-06-01T08:00:00Z,x\n` +
  `2,"Comma, ""quote""",-0.05,false,1999-12-31,2000-01-01T00:00:00Z,\n` +
  `3,Pipe | and <b>&amp; tag,0.00,,,,"multi\nline"\n` +
  `4,Ünïcödé 😀,123456.78,true,2024-01-31,2024-01-31T23:59:59.500Z,""\n`;

const ROWS = [
  { id: 1, name: "Plain", amount: "10.50", active: true, born: "2024-02-29", seen: "2024-06-01T08:00:00Z", note: "x" },
  { id: 2, name: 'Comma, "quote"', amount: "-0.05", active: false, born: "1999-12-31", seen: "2000-01-01T00:00:00Z", note: null },
  { id: 3, name: "Pipe | and <b>&amp; tag", amount: "0.00", active: null, born: null, seen: null, note: "multi\nline" },
  { id: 4, name: "Ünïcödé 😀", amount: "123456.78", active: true, born: "2024-01-31", seen: "2024-01-31T23:59:59.500Z", note: "" },
];

/** Exports the shown result as `format` to `name` in `dir`; returns the status line. */
async function exported(page, dir, name, options = {}) {
  const out = await exportResult(page, join(dir, name), options);
  assert.match(out, /^Exported [\d,]+ rows? to /, out);
  return out;
}

const read = (dir, name) => readFileSync(join(dir, name), "utf8");

/** The cells of row `n` of an Excel sheet: `{ A: { type, value } }`. */
function cells(sheet, n) {
  const row = new RegExp(`<row r="${n}"[^>]*>(.*?)</row>`, "s").exec(sheet)?.[1] ?? "";
  const out = {};
  for (const m of row.matchAll(/<c r="([A-Z]+)\d+"([^>]*?)(?:\/>|>(.*?)<\/c>)/gs)) {
    const type = /t="(\w+)"/.exec(m[2])?.[1] ?? "n";
    const style = /s="(\d+)"/.exec(m[2])?.[1];
    const value = type === "inlineStr" ? /<t[^>]*>(.*?)<\/t>/s.exec(m[3])?.[1] : /<v>(.*?)<\/v>/s.exec(m[3] ?? "")?.[1];
    out[m[1]] = { type, style, value };
  }
  return out;
}

const unescapeXml = (s) => s.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&amp;/g, "&");

export default async function (page, { dir }) {
  try {
    await showTable(page, DBS.pg.name, "public", TABLE);
    await openEditor(page, DBS.pg.name);
    assert.equal(await runScript(page, ALL), "4 rows");

    // CSV: quoting, the NULL and the empty string told apart, UTC instants, exact decimals.
    await exported(page, dir, "all.csv");
    assert.equal(read(dir, "all.csv"), EXPECTED_CSV);
    // Rows already loaded give the same file as running the query again.
    await exported(page, dir, "loaded.csv", { source: "loaded" });
    assert.equal(read(dir, "loaded.csv"), EXPECTED_CSV);
    // Delimiters: a field with the delimiter is quoted, the others not; no header row.
    await exported(page, dir, "semi.csv", { delimiter: ";" });
    assert.equal(read(dir, "semi.csv").split("\n")[0], HEADER.replaceAll(",", ";"));
    assert.equal(read(dir, "semi.csv").split("\n")[2], '2;"Comma, ""quote""";-0.05;false;1999-12-31;2000-01-01T00:00:00Z;');
    await exported(page, dir, "tab.csv", { delimiter: "\t" });
    assert.equal(read(dir, "tab.csv").split("\n")[1], "1\tPlain\t10.50\ttrue\t2024-02-29\t2024-06-01T08:00:00Z\tx");
    await exported(page, dir, "pipe.csv", { delimiter: "|", header: false });
    assert.ok(!read(dir, "pipe.csv").startsWith("id"), "no header row");
    assert.ok(read(dir, "pipe.csv").startsWith("1|Plain|10.50|true|2024-02-29|2024-06-01T08:00:00Z|x\n"), read(dir, "pipe.csv"));
    assert.ok(read(dir, "pipe.csv").includes('2|"Comma, ""quote"""|-0.05|false'), "quotes still double, the comma is no reason to quote");

    // JSON and NDJSON: typed values (booleans, null), decimals and instants as exact text.
    await exported(page, dir, "all.json", { format: "json" });
    assert.deepEqual(JSON.parse(read(dir, "all.json")), ROWS);
    await exported(page, dir, "all.ndjson", { format: "ndjson" });
    const lines = read(dir, "all.ndjson").trimEnd().split("\n");
    assert.equal(lines.length, 4);
    assert.deepEqual(lines.map((l) => JSON.parse(l)), ROWS);

    // XML: escaped text, null="true" for NULL, an empty element for the empty string.
    await exported(page, dir, "all.xml", { format: "xml" });
    const xml = read(dir, "all.xml");
    assert.ok(xml.startsWith('<?xml version="1.0" encoding="UTF-8"?>\n<rows>\n'), xml.slice(0, 60));
    assert.equal(xml.match(/<row>/g).length, 4);
    assert.ok(xml.includes('<field name="note" null="true"/>'));
    assert.ok(xml.includes('<field name="note"></field>'));
    assert.ok(xml.includes('<field name="name">Pipe | and &lt;b&gt;&amp;amp; tag</field>'));
    assert.equal(unescapeXml(/<field name="name">(Comma.*?)<\/field>/.exec(xml)[1]), 'Comma, "quote"');
    assert.ok(!xml.includes("<b>"), "markup in a value is escaped");

    // Markdown: the pipe in a value is escaped, a newline is <br>, the markup is kept as text.
    await exported(page, dir, "all.md", { format: "markdown" });
    const md = read(dir, "all.md").split("\n");
    assert.equal(md[0], `| ${HEADER.split(",").join(" | ")} |`);
    assert.equal(md[1], "| --- | --- | --- | --- | --- | --- | --- |");
    assert.equal(md[4], "| 3 | Pipe \\| and <b>&amp; tag | 0.00 |  |  |  | multi<br>line |");
    assert.equal(md.filter((l) => l.startsWith("|")).length, 6);

    // HTML: the table with its head and four rows, the markup escaped.
    await exported(page, dir, "all.html", { format: "html" });
    const html = read(dir, "all.html");
    assert.ok(html.startsWith("<!DOCTYPE html>"));
    assert.equal(html.match(/<tr>/g).length, 5);
    assert.ok(html.includes("<th>seen</th>"));
    assert.ok(html.includes("<td>Pipe | and &lt;b&gt;&amp;amp; tag</td>"));
    assert.ok(!html.includes("<b>"), "markup in a value is escaped");
    assert.ok(html.includes("<td>Comma, &quot;quote&quot;</td>"));

    // SQL: the statements run into a copy of the table give the same rows (and the same text).
    await exported(page, dir, "all.sql", { format: "sql", table: `${TABLE}_sql` });
    const statements = read(dir, "all.sql");
    assert.equal(statements.match(/^INSERT INTO qa_expo_sql \(id, name, amount, active, born, seen, note\) VALUES/gm).length, 4);
    for (const statement of statements.split(";\n").filter((s) => s.trim())) db("pg", statement);
    assert.equal(db("pg", `SELECT count(*) FROM (SELECT * FROM ${TABLE} EXCEPT SELECT * FROM ${TABLE}_sql) x`), "0");
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}_sql`), "4");
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}_sql WHERE note IS NULL`), "1");
    assert.equal(db("pg", `SELECT count(*) FROM ${TABLE}_sql WHERE note = ''`), "1");

    // Excel: a header of text, numbers as numbers, booleans as booleans, dates and instants as
    // serial numbers with a date style, NULL as no cell, text inline.
    await exported(page, dir, "all.xlsx", { format: "xlsx" });
    const sheet = zipEntry(join(dir, "all.xlsx"), "xl/worksheets/sheet1.xml");
    assert.match(sheet, /<dimension ref="A1:G5"\/>/);
    const header = cells(sheet, 1);
    assert.deepEqual(Object.values(header).map((c) => c.value), HEADER.split(","));
    assert.ok(Object.values(header).every((c) => c.type === "inlineStr"));
    const first = cells(sheet, 2);
    assert.deepEqual([first.A.type, first.A.value], ["n", "1"]);
    assert.deepEqual([first.B.type, first.B.value], ["inlineStr", "Plain"]);
    assert.deepEqual([first.C.type, first.C.value], ["n", "10.5"]);
    assert.deepEqual([first.D.type, first.D.value], ["b", "1"]);
    assert.equal(first.E.value, "45351", "2024-02-29 is serial 45351");
    assert.ok(first.E.style !== undefined, "a date has a style");
    assert.equal(Math.floor(Number(first.F.value)), 45444, "2024-06-01 is serial 45444");
    const third = cells(sheet, 4);
    assert.equal(third.D, undefined, "NULL is no cell");
    assert.equal(third.E, undefined);
    assert.equal(unescapeXml(third.B.value), "Pipe | and <b>&amp; tag");
    assert.equal(third.G.value, "multi\nline");
    assert.equal(cells(sheet, 5).B.value, "Ünïcödé 😀");
    assert.ok(zipEntry(join(dir, "all.xlsx"), "xl/workbook.xml")?.includes("<sheet "), "a workbook");

    // Parquet: the footer says rows, columns, types and the zstd codec.
    await exported(page, dir, `${TABLE}_pq.parquet`, { format: "parquet" });
    const info = parquetInfo(join(dir, `${TABLE}_pq.parquet`));
    assert.equal(info.rows, 4);
    assert.deepEqual(info.columns.map((c) => c.name), HEADER.split(","));
    assert.deepEqual(info.columns.map((c) => c.type), ["INT32", "BYTE_ARRAY", "INT64", "BOOLEAN", "INT32", "INT64", "BYTE_ARRAY"]);
    assert.ok(info.columns.every((c) => c.codec === "ZSTD"), JSON.stringify(info.columns));
    // ... and the file read by the app gives the rows back.
    await menu(page, "public", "Import data into a new table", importDialog, DBS.pg.name);
    assert.equal(await chooseFile(page, join(dir, `${TABLE}_pq.parquet`)), null);
    assert.match(await runJob(page, importDialog, "Import"), /Imported 4 rows/);
    await closeModal(page, importDialog);
    assert.equal(
      db(
        "pg",
        `SELECT count(*) FROM ${TABLE} a JOIN ${TABLE}_pq b USING (id) WHERE a.name = b.name AND a.amount = b.amount AND a.active IS NOT DISTINCT FROM b.active AND a.born IS NOT DISTINCT FROM b.born AND a.seen IS NOT DISTINCT FROM b.seen AND a.note IS NOT DISTINCT FROM b.note`,
      ),
      "4",
    );

    // Compression: gzip and zstd of the text formats, and the extension the dialog suggests
    // is the one the file is saved with; decompressed they equal the plain files.
    await exported(page, dir, "all.csv.gz", { compression: "gzip" });
    assert.equal(readFileSync(join(dir, "all.csv.gz")).subarray(0, 2).toString("hex"), "1f8b");
    assert.equal(gunzipSync(readFileSync(join(dir, "all.csv.gz"))).toString("utf8"), EXPECTED_CSV);
    await exported(page, dir, "all.csv.zst", { compression: "zstd" });
    assert.equal(readFileSync(join(dir, "all.csv.zst")).subarray(0, 4).toString("hex"), "28b52ffd");
    assert.equal(zstdDecompressSync(readFileSync(join(dir, "all.csv.zst"))).toString("utf8"), EXPECTED_CSV);
    await exported(page, dir, "all.json.zst", { format: "json", compression: "zstd" });
    assert.deepEqual(JSON.parse(zstdDecompressSync(readFileSync(join(dir, "all.json.zst"))).toString("utf8")), ROWS);
    await exported(page, dir, "all.sql.gz", { format: "sql", table: "t", compression: "gzip" });
    assert.ok(gunzipSync(readFileSync(join(dir, "all.sql.gz"))).toString("utf8").startsWith("INSERT INTO t ("));

    // A result with no rows: the columns are still there, in every format.
    assert.equal(await runScript(page, `SELECT id, name, note FROM ${TABLE} WHERE false`), "0 rows");
    await exported(page, dir, "empty.csv");
    assert.equal(read(dir, "empty.csv"), "id,name,note\n");
    await exported(page, dir, "empty.json", { format: "json" });
    assert.deepEqual(JSON.parse(read(dir, "empty.json")), []);
    await exported(page, dir, "empty.ndjson", { format: "ndjson" });
    assert.equal(read(dir, "empty.ndjson"), "");
    await exported(page, dir, "empty.xml", { format: "xml" });
    assert.equal(read(dir, "empty.xml"), '<?xml version="1.0" encoding="UTF-8"?>\n<rows>\n</rows>\n');
    await exported(page, dir, "empty.md", { format: "markdown" });
    assert.equal(read(dir, "empty.md"), "| id | name | note |\n| --- | --- | --- |\n");
    await exported(page, dir, "empty.html", { format: "html" });
    assert.ok(read(dir, "empty.html").includes("<thead><tr><th>id</th><th>name</th><th>note</th></tr></thead>"));
    assert.equal(read(dir, "empty.html").match(/<tr>/g).length, 1);
    await exported(page, dir, "empty.sql", { format: "sql", table: "t" });
    assert.equal(read(dir, "empty.sql"), "");
    await exported(page, dir, "empty.parquet", { format: "parquet" });
    const none = parquetInfo(join(dir, "empty.parquet"));
    assert.equal(none.rows, 0);
    assert.deepEqual(none.columns.map((c) => c.name), ["id", "name", "note"]);
    await exported(page, dir, "empty.xlsx", { format: "xlsx" });
    const emptySheet = zipEntry(join(dir, "empty.xlsx"), "xl/worksheets/sheet1.xml");
    assert.deepEqual(Object.values(cells(emptySheet, 1)).map((c) => c.value), ["id", "name", "note"]);
    assert.equal(cells(emptySheet, 2).A, undefined);

    // 50,000 rows: more than one batch, every format reads them all back.
    // The grid streams the first rows only; the export runs the query again for all of them.
    assert.match(await runScript(page, "SELECT g AS n, md5(g::text) AS h FROM generate_series(1, 50000) g"), /^1,000 rows loaded/);
    await exported(page, dir, "big.csv", { source: "query" });
    const csv = read(dir, "big.csv").split("\n");
    assert.equal(csv.length, 50_002, "header, rows and the final newline");
    assert.equal(csv[1], "1,c4ca4238a0b923820dcc509a6f75849b");
    assert.ok(csv[50_000].startsWith("50000,"));
    await exported(page, dir, "big.csv.zst", { compression: "zstd" });
    assert.equal(zstdDecompressSync(readFileSync(join(dir, "big.csv.zst"))).toString("utf8"), read(dir, "big.csv"));
    await exported(page, dir, "big.ndjson", { format: "ndjson" });
    assert.equal(read(dir, "big.ndjson").trimEnd().split("\n").length, 50_000);
    await exported(page, dir, "big.parquet", { format: "parquet" });
    const big = parquetInfo(join(dir, "big.parquet"));
    assert.equal(big.rows, 50_000);
    assert.deepEqual(big.columns.map((c) => [c.name, c.type]), [["n", "INT32"], ["h", "BYTE_ARRAY"]]);
    await exported(page, dir, "big.xlsx", { format: "xlsx" });
    const bigSheet = zipEntry(join(dir, "big.xlsx"), "xl/worksheets/sheet1.xml");
    assert.equal(bigSheet.match(/<row /g).length, 50_001);
    assert.match(bigSheet, /<dimension ref="A1:B50001"\/>/);

    // A folder that does not exist: the failure names the path and nothing is written.
    const failed = await exportResult(page, join(dir, "no-such-folder", "x.csv"));
    assert.match(failed, /^Export failed: .*no-such-folder/, failed);
    assert.equal(existsSync(join(dir, "no-such-folder")), false);
    // The dialog is still usable after a failure.
    await exported(page, dir, "after-failure.csv");
    assert.equal(read(dir, "after-failure.csv").split("\n").length, 50_002);
  } finally {
    dbQuiet("pg", `DROP TABLE IF EXISTS ${TABLE}, ${TABLE}_sql, ${TABLE}_pq`);
  }
}
