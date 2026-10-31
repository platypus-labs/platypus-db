// Passwords from a .pgpass file (Connection > Password from > .pgpass file) against the dev
// PostgreSQL, with the file the way libpq reads it (PGPASSFILE points the app to it): comment
// and blank lines are skipped, a line whose fields do not all match is skipped, `*` matches
// anything, `\:` and `\\` in a password are unescaped, the first matching line wins, and
// the file may have Windows line ends.
//  - the form shows the source, hides the password field, and Test connection signs in with
//    the file's password; a query tab on the connection runs as the right user;
//  - a user whose password has a colon and a backslash signs in (the escapes);
//  - a user with no matching line gets an error that names the file and the host:port:db:user
//    that was looked up, not a server error;
//  - a matching line with the wrong password gives the server's own refusal;
//  - the file is read at every connection: removing it gives the "no line" error, putting it
//    back makes the same connection work again, without restarting the app.
// Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { renameSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { dialog, editConnection, fill, formField, needsPort, openQueryTab, run, saveForm, testConnection, treeRow } from "../conn-helpers.mjs";
import { db, dbQuiet } from "../qa-xfer.mjs";
import { gridTsv } from "../qa-sql.mjs";

export const skip = needsPort(55432, "PostgreSQL");

const connection = (n, name, user) => `
[[connections]]
id = "00000000-0000-4000-8000-0000000c010${n}"
name = "${name}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "${user}"
ssl_mode = "disable"
save_password = false
pgpass = true
`;

const PGPASS = [
  "# localhost:55432:platypus:platypus:the-comment-is-not-a-line",
  "",
  "localhost:55432:otherdb:platypus:wrong-database",
  "otherhost:55432:platypus:platypus:wrong-host",
  "localhost:5433:platypus:platypus:wrong-port",
  "*:*:*:qa_pgpass_colon:p\\:a\\\\ss",
  "localhost:55432:platypus:platypus:platypus",
  "localhost:55432:platypus:platypus:never-used-the-first-match-wins",
  "localhost:55432:platypus:qa_pgpass_wrong:the-wrong-password",
  "",
].join("\r\n");

export function setup({ dir }) {
  dbQuiet("pg", "DROP ROLE IF EXISTS qa_pgpass_colon; DROP ROLE IF EXISTS qa_pgpass_wrong");
  db("pg", "CREATE ROLE qa_pgpass_colon LOGIN PASSWORD 'p:a\\ss'; CREATE ROLE qa_pgpass_wrong LOGIN PASSWORD 'the-right-password'");
  writeFileSync(join(dir, "pgpass.conf"), PGPASS);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${connection(1, "pgpass ok", "platypus")}${connection(2, "pgpass escapes", "qa_pgpass_colon")}` +
      `${connection(3, "pgpass no line", "nobody")}${connection(4, "pgpass wrong", "qa_pgpass_wrong")}`,
  );
}

export const env = ({ dir }) => ({ PGPASSFILE: join(dir, "pgpass.conf") });

/** Opens the form of `name`, tests it, and closes the form without saving. */
async function tested(page, name) {
  await editConnection(page, name);
  const notice = await testConnection(page);
  await page.eval(`[...${dialog}.querySelectorAll('button')].find(b => b.textContent.trim() === 'Cancel').click()`);
  await page.waitFor(`${dialog} === null`);
  return notice;
}

export default async function (page, { dir }) {
  try {
    // The form: the source is the file, and no password is asked for.
    await page.waitFor(`${treeRow("pgpass ok")} !== undefined`);
    await editConnection(page, "pgpass ok");
    assert.equal(await page.eval(`${dialog}.querySelector('[data-field=password_source]').value`), "pgpass");
    assert.equal(await page.eval(`${dialog}.querySelector('input[type=password]') === null`), true, "no password field");
    assert.match(await testConnection(page), /^Connected/);
    // Saving keeps the source (the setting round-trips through the file).
    await page.eval(fill(formField("Name"), "pgpass ok"));
    await saveForm(page);

    // A query tab signs in with the file's password; the first matching line is the one used.
    await openQueryTab(page, "pgpass ok");
    assert.equal(await run(page, "SELECT current_user"), "1 row");
    assert.equal((await gridTsv(page)).trim(), "current_user\nplatypus");

    // The escapes: `p\:a\\ss` is the password p:a\ss.
    assert.match(await tested(page, "pgpass escapes"), /^Connected/);

    // No line for the user: the app says so before asking the server, and names the file.
    const noLine = await tested(page, "pgpass no line");
    assert.match(noLine, /no line of .*pgpass\.conf matches localhost:55432:platypus:nobody/, noLine);
    assert.ok(!/authentication/i.test(noLine), noLine);

    // A line with the wrong password: the server refuses it.
    assert.match(await tested(page, "pgpass wrong"), /password authentication failed/i);

    // The file is read again at every connection.
    renameSync(join(dir, "pgpass.conf"), join(dir, "pgpass.moved"));
    assert.match(await tested(page, "pgpass ok"), /no line of .*pgpass\.conf matches localhost:55432:platypus:platypus/);
    renameSync(join(dir, "pgpass.moved"), join(dir, "pgpass.conf"));
    assert.match(await tested(page, "pgpass ok"), /^Connected/);
    // A password changed in the file is the one used next: the line now holds a wrong one.
    writeFileSync(join(dir, "pgpass.conf"), PGPASS.replace(":platypus:platypus:platypus", ":platypus:platypus:changed-in-the-file"));
    assert.match(await tested(page, "pgpass ok"), /password authentication failed/i);
  } finally {
    dbQuiet("pg", "DROP ROLE IF EXISTS qa_pgpass_colon; DROP ROLE IF EXISTS qa_pgpass_wrong");
  }
}
