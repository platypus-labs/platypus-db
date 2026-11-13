// Regression guard (passes): MariaDB-specific types round-trip through the data editor (UUID,
// INET6, INET4, JSON, BIGINT UNSIGNED above 2^63, BIT), a column defaulting to NEXT VALUE FOR
// a sequence fills in, and an edit of a system-versioned table keeps its history; Generate DDL
// keeps WITH SYSTEM VERSIONING.
import assert from "node:assert/strict";
import { conns, editBar, editorText, focusCell, needsPorts, openData, openPath, rows, save, setCell, sh, stubClipboard, treeMenu } from "../drivers-helpers.mjs";

export const skip = needsPorts(53307);

export function setup({ dir }) {
  sh("mariadb", `DROP TABLE IF EXISTS qa_dbs_m, qa_dbs_sv; DROP SEQUENCE IF EXISTS qa_dbs_seq;
CREATE SEQUENCE qa_dbs_seq START WITH 100 INCREMENT BY 5;
CREATE TABLE qa_dbs_m (id INT PRIMARY KEY DEFAULT (NEXT VALUE FOR qa_dbs_seq), u UUID, ip6 INET6, ip4 INET4, j JSON, big BIGINT UNSIGNED, bits BIT(8));
INSERT INTO qa_dbs_m (u) VALUES (NULL);
CREATE TABLE qa_dbs_sv (id INT PRIMARY KEY, v VARCHAR(10)) WITH SYSTEM VERSIONING;
INSERT INTO qa_dbs_sv VALUES (1, 'a');`);
  conns(dir, [["QA Maria", "mariadb"]]);
}

export default async function (page) {
  try {
    await openPath(page, ["QA Maria", "platypus", "Tables", "qa_dbs_m"]);
    await stubClipboard(page);
    await openData(page, "qa_dbs_m");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    assert.equal((await rows(page))[0][0], "100");
    const values = [[1, "c0eebc99-9c0b-4ef8-bb6d-6bb9bd380a13"], [2, "::ffff:1.2.3.4"], [3, "192.168.0.1"], [4, '{"b": "q\'s"}'], [5, "18446744073709551614"], [6, "3"]];
    for (const [col, text] of values) {
      await focusCell(page, 0, col);
      await setCell(page, text);
    }
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("mariadb", "SELECT u, ip6, ip4, j, big, bin(bits) FROM qa_dbs_m"),
      'c0eebc99-9c0b-4ef8-bb6d-6bb9bd380a13\t::ffff:1.2.3.4\t192.168.0.1\t{"b": "q\'s"}\t18446744073709551614\t11');

    await openData(page, "qa_dbs_sv");
    await page.waitFor(`${editBar}.includes('to edit a cell')`, 15_000);
    await focusCell(page, 0, 1);
    await setCell(page, "b");
    assert.match(await save(page), /to edit a cell/);
    assert.equal(sh("mariadb", "SELECT group_concat(v ORDER BY row_end) FROM qa_dbs_sv FOR SYSTEM_TIME ALL"), "a,b");
    await treeMenu(page, "qa_dbs_sv", "Generate DDL");
    await page.waitFor(`(() => { try { return ${editorText}.includes('CREATE TABLE'); } catch { return false; } })()`, 15_000);
    assert.match(await page.eval(editorText), /WITH SYSTEM VERSIONING/);
  } finally {
    sh("mariadb", "DROP TABLE IF EXISTS qa_dbs_m, qa_dbs_sv; DROP SEQUENCE IF EXISTS qa_dbs_seq;");
  }
}
