// SQL-13. When the editor's connection is lost (server restart, idle timeout, a DBA killing
// the session), the next statement is silently run again on a new connection. The session's
// state is gone (search_path, USE, SET, temporary tables, application_name) and nothing
// says so: a statement meant for schema `demo` runs against `public`. (An open transaction
// does get a message: "the open transaction was rolled back".)
// Steps: PostgreSQL, `SET search_path = demo`, end the session with
// `SELECT pg_terminate_backend(pg_backend_pid())`, run `SELECT current_schema()`.
// Expected: the user is told the connection was reopened and its settings lost (or the
// statement does not run silently in another context).
// Actual: current_schema = public, no message.
// Code: apps/desktop/src/components/QueryTab.tsx run(): `status.error.kind === "closed"` ->
// second runOne on a reopened connection.
import assert from "node:assert/strict";
import { DB, gridTsv, openEditor, runStatement, saveConnections, skipUnless } from "../editor-helpers.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);

const messages = `[...document.querySelectorAll('.toast, .notice, .result-error, .result-message, .modal')].map(e => e.textContent).join(' | ')`;

export default async function (page) {
  await openEditor(page, DB.pg.name);
  await runStatement(page, "SET search_path = demo");
  await runStatement(page, "SELECT pg_terminate_backend(pg_backend_pid())");
  await runStatement(page, "SELECT current_schema() AS s");
  const rows = await gridTsv(page);
  const shown = await page.eval(messages);
  assert.ok(rows === "s\ndemo" || /reconnect|reopen|lost/i.test(shown), `ran in another context without a word: ${JSON.stringify(rows)}, messages ${JSON.stringify(shown)}`);
}
