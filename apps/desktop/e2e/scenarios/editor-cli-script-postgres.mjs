// "Run script with the command-line client" (editor context menu) with the real psql, against
// the dev PostgreSQL (psql found on the PATH or in the portable PostgreSQL of the dev machine):
//  - the script is sent to psql whole, with its own commands: \set and :variables, \echo,
//    \copy to a file and \copy from a file (the paths are the machine's), several statements;
//    the output is shown as psql prints it, the status says "psql finished";
//  - an error stops the script (ON_ERROR_STOP): what ran before it stays, what follows does
//    not, the status says "psql failed (exit 3)" and the output quotes the server's message;
//  - a selection runs alone, the rest of the editor does not;
//  - Cancel kills psql within seconds and the server's session goes away;
//  - a read-only connection refuses a script that changes data and runs one that only reads;
//  - a database whose client is not installed (mysql, unless on the PATH) says so and how to fix it.
// Needs PostgreSQL and MySQL from docker/compose.yml and psql (PLATYPUS_E2E_PGBIN names its folder).
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { hasOnPath, pgFolder, withPath } from "../clients.mjs";
import { db, dbQuiet, saveConnections, skipUnless } from "../qa-xfer.mjs";
import { clearStatus, editorMenu, openEditor, setEditor, visibleTab, waitDone } from "../qa-sql.mjs";

export const skip = async () => {
  const down = await skipUnless("pg", "mysql");
  if (down) return down;
  return pgFolder() === null ? "no psql on the PATH (set PLATYPUS_E2E_PGBIN to the folder of psql.exe)" : null;
};

// English messages from psql, whatever the language of the machine.
export const env = () => ({ ...withPath(pgFolder()), LC_ALL: "C", LANG: "C" });

const READ_ONLY = `
[[connections]]
id = "00000000-0000-4000-8000-0000000c0201"
name = "CLI read-only"
driver = "postgres"
host = "127.0.0.1"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
save_password = false
password_command = "echo platypus"
read_only = true
`;

export function setup({ dir }) {
  dbQuiet("pg", "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND pid <> pg_backend_pid()");
  dbQuiet("pg", "DROP TABLE IF EXISTS qa_cli");
  saveConnections(dir, ["pg", "mysql"]);
  const file = join(dir, "connections.toml");
  writeFileSync(file, readFileSync(file, "utf8") + READ_ONLY);
}

const output = `(${visibleTab}.querySelector('pre.message-output')?.textContent ?? '')`;
const slash = (path) => path.replaceAll("\\", "/");

/** Runs `script` (the whole editor) with the client; returns the status line and the output. */
async function viaClient(page, script) {
  await setEditor(page, script);
  return runMenu(page);
}

/** Runs the editor (or its selection) with the client from the editor's context menu. */
async function runMenu(page) {
  await clearStatus(page);
  await editorMenu(page, "Run script with the command-line client");
  const status = await waitDone(page, 60_000);
  return { status, text: await page.eval(output) };
}

const rows = (sql) => db("pg", sql);

export default async function (page, { dir }) {
  try {
    await openEditor(page, "QA PG");
    const out = slash(join(dir, "out.csv"));
    const input = slash(join(dir, "in.csv"));

    // The script as psql reads it: variables, echo, DDL, DML, a SELECT, \copy to a file.
    let r = await viaClient(
      page,
      [
        "\\set n 5",
        "\\echo the value is :n",
        "SELECT :n + 1 AS six, 'x' AS b;",
        "CREATE TABLE qa_cli (id int PRIMARY KEY, v text);",
        "INSERT INTO qa_cli VALUES (1, 'one'), (2, 'two');",
        `\\copy (SELECT * FROM qa_cli ORDER BY id) TO '${out}' CSV HEADER`,
      ].join("\n"),
    );
    assert.match(r.status, /^psql finished in /, `${r.status}\n${r.text}`);
    assert.match(r.text, /the value is 5/);
    assert.match(r.text, /six \| b/);
    assert.match(r.text, /\s6 \| x/);
    assert.match(r.text, /\(1 row\)/);
    assert.match(r.text, /INSERT 0 2/);
    assert.match(r.text, /COPY 2/);
    assert.equal(readFileSync(join(dir, "out.csv"), "utf8").replaceAll("\r\n", "\n"), "id,v\n1,one\n2,two\n");
    assert.equal(rows("SELECT count(*) FROM qa_cli"), "2");

    // \copy from a file of the machine: only a client can do that.
    writeFileSync(join(dir, "in.csv"), "id,v\n3,three\n4,four\n");
    r = await viaClient(page, `\\copy qa_cli FROM '${input}' CSV HEADER`);
    assert.match(r.status, /^psql finished/, r.status + r.text);
    assert.match(r.text, /COPY 2/);
    assert.equal(rows("SELECT string_agg(v, ',' ORDER BY id) FROM qa_cli"), "one,two,three,four");
    // ... and a file that is not there is the client's error, not a crash.
    r = await viaClient(page, `\\copy qa_cli FROM '${slash(join(dir, "missing.csv"))}' CSV`);
    assert.match(r.status, /^psql failed \(exit 3\)/, r.status);
    assert.match(r.text, /missing\.csv/);

    // An error stops the script: the first insert stays, the last never runs.
    r = await viaClient(
      page,
      "INSERT INTO qa_cli VALUES (10, 'before');\nSELECT * FROM no_such_table;\nINSERT INTO qa_cli VALUES (11, 'after');",
    );
    assert.match(r.status, /^psql failed \(exit 3\) in /, r.status);
    assert.match(r.text, /relation "no_such_table" does not exist/);
    assert.equal(rows("SELECT count(*) FROM qa_cli WHERE id = 10"), "1", "autocommit: what ran before stays");
    assert.equal(rows("SELECT count(*) FROM qa_cli WHERE id = 11"), "0", "the script stopped at the error");
    // A syntax error too, with psql's position line.
    r = await viaClient(page, "SELEC 1;");
    assert.match(r.status, /^psql failed/);
    assert.match(r.text, /syntax error at or near "SELEC"/);

    // A script with no output says so.
    r = await viaClient(page, "SET search_path = public;");
    assert.match(r.status, /^psql finished/);
    assert.match(r.text, /SET/);

    // A selection runs alone (the first line: the context menu opens over it, which keeps it).
    await setEditor(page, "INSERT INTO qa_cli VALUES (21, 'selected');\nINSERT INTO qa_cli VALUES (20, 'not selected');");
    await page.eval(`(() => {
      const c = ${visibleTab}.querySelector('.cm-content');
      const view = c.cmTile?.root?.view ?? c.cmView?.view ?? c.cmView?.rootView?.view;
      view.dispatch({ selection: { anchor: 0, head: view.state.doc.line(1).to } });
    })()`);
    r = await runMenu(page);
    assert.match(r.status, /^psql finished/, r.status + r.text);
    assert.equal(rows("SELECT string_agg(id::text, ',' ORDER BY id) FROM qa_cli WHERE id >= 20"), "21");

    // Cancel: a long statement is stopped in seconds, and the server forgets the session.
    await setEditor(page, "SELECT pg_sleep(120) AS qa_cli_sleep;");
    await clearStatus(page);
    await editorMenu(page, "Run script with the command-line client");
    const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
    await page.waitFor(`${cancel}?.disabled === false`, 15_000);
    for (let i = 0; i < 100 && rows("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND pid <> pg_backend_pid()") === "0"; i++) await page.sleep(100);
    assert.equal(rows("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND state = 'active' AND pid <> pg_backend_pid()"), "1", "psql is running it");
    const stopped = Date.now();
    await page.eval(`${cancel}.click()`);
    await page.waitFor(`${cancel}?.disabled === true`, 15_000);
    assert.ok(Date.now() - stopped < 10_000, "stopped within seconds");
    await page.waitFor(`document.body.innerText.includes('psql was stopped')`, 10_000);
    for (let i = 0; i < 100 && rows("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND state = 'active' AND pid <> pg_backend_pid()") !== "0"; i++) await page.sleep(100);
    // Checked at the end, so the rest of the scenario still runs: Cancel kills psql without
    // sending the server a cancel, and a long statement runs on to its end (bug B4).
    const left = rows("SELECT count(*) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND state = 'active' AND pid <> pg_backend_pid()");

    // A read-only connection: a script that reads runs, one that changes data does not.
    await openEditor(page, "CLI read-only");
    r = await viaClient(page, "SELECT count(*) AS counted FROM qa_cli;");
    assert.match(r.status, /^psql finished/, r.status + r.text);
    assert.match(r.text, /counted/);
    const before = rows("SELECT count(*) FROM qa_cli");
    await setEditor(page, "INSERT INTO qa_cli VALUES (30, 'read-only');");
    await clearStatus(page);
    await editorMenu(page, "Run script with the command-line client");
    // Either the refusal alone, or a question that is answered No: nothing is inserted.
    await page.waitFor(`document.querySelector('.toast, .modal, .notice.error') !== null`, 15_000);
    const refusal = await page.eval(`(document.querySelector('.modal, .toast, .notice.error')?.textContent ?? '')`);
    assert.match(refusal, /read-only/, refusal);
    await page.eval(`(() => { const b = [...document.querySelectorAll('.modal button')].find(b => /^(Cancel|No)$/.test(b.textContent.trim())); b?.click(); })()`);
    await page.sleep(500);
    assert.equal(rows("SELECT count(*) FROM qa_cli"), before, "nothing was inserted");

    // Another database whose client is not installed: the message says what to do.
    if (!hasOnPath("mysql")) {
      await openEditor(page, "QA MySQL");
      r = await viaClient(page, "SELECT 1;");
      const message = await page.eval(`${visibleTab}.innerText`);
      assert.match(message, /mysql was not found: install the client and put it on the PATH/, message);
    }
    assert.equal(left, "0", "the server went on with the statement after Cancel");
  } finally {
    dbQuiet("pg", "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE query LIKE '%qa_cli_sleep%' AND pid <> pg_backend_pid()");
    dbQuiet("pg", "DROP TABLE IF EXISTS qa_cli");
  }
}
