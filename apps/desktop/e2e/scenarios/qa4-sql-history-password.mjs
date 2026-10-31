// qa4-sql-5 (medium, privacy). The query history keeps every statement as typed, so a
// `CREATE ROLE ... PASSWORD 'secret'` (or ALTER USER / SET PASSWORD / IDENTIFIED BY) is stored in
// clear in the history database and listed in the History panel, where anyone who opens the
// panel (or sees the shared screen, or a copied profile) reads the password. The "Mask sensitive
// data" feature and the connection passwords (kept in the keyring) show the app means to keep
// secrets out of sight.
// Steps: PostgreSQL editor, run `CREATE ROLE qa4_h LOGIN PASSWORD 'sup3rS3cret'`, open History.
// Expected: the entry hides the literal after PASSWORD (e.g. PASSWORD '***'), or is not kept.
// Actual: the entry shows PASSWORD 'sup3rS3cret'.
// Code: apps/desktop/src-tauri/src/commands/query.rs (where the statement is recorded) and
//   commands/history.rs: no redaction of password literals before saving.
import assert from "node:assert/strict";
import { DB, button, openEditor, runStatement, saveConnections, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "DROP ROLE IF EXISTS qa4_h");
  await runStatement(page, "CREATE ROLE qa4_h LOGIN PASSWORD 'sup3rS3cret'");
  await runStatement(page, "DROP ROLE qa4_h");
  await page.eval(`${button("History")}.click()`);
  await page.waitFor(`document.querySelectorAll('.history-entry').length >= 3`, 10_000);
  const shown = await page.eval(`[...document.querySelectorAll('.history-entry')].map(e => e.textContent).join(' | ')`);
  assert.ok(!shown.includes("sup3rS3cret"), `the password is in the history panel:\n${shown}`);
}
