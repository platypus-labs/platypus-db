// JRN-02 (critical). On a production connection, Drop… / Truncate… / Rename… of the
// navigator run their statement on the navigator's hidden session, which production starts in
// manual commit (commands/connections.rs `start_session`), and nothing commits it. The dialog
// closes as if done and the tree no longer shows the table, but the change is not committed:
// - the DROP keeps an ACCESS EXCLUSIVE lock on the production table, so every other client
//   (the application in production) waits on it until PlatypusDB disconnects;
// - closing the window asks nothing and rolls the change back silently.
// No Commit button exists for that session; the user has no way to see or end it.
// Steps: PostgreSQL connection of type Production; navigator > table > Drop… > Drop; from
// another client, read the table with a lock timeout.
// Expected: the drop committed (or the dialog says it is waiting for Commit and offers it):
// the table is gone for the other client. Actual: the other client's read times out on the
// lock; after the window is closed the table is back with its rows.
// Suspected: apps/desktop/src/objects/SqlActionDialog.tsx `run` (executeStatement on the side
// session with no commit) with commands/connections.rs `start_session` (production ⇒
// set_autocommit(false) for every session, the navigator's too). Same for the Rename… and
// Truncate… dialogs (seen on SQLite too: the DELETE is never written to the file).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { closeApp, connectWith, needsPostgres, openPath, pgConnection, psql, psqlTry, reopen } from "../qa-jrn.mjs";
import { menu } from "../qa-nav-helpers.mjs";

export const skip = needsPostgres;
const NAME = "Shop PG PROD";

export function setup({ dir }) {
  psql("DROP TABLE IF EXISTS qa_jrn_lock; CREATE TABLE qa_jrn_lock (id int primary key); INSERT INTO qa_jrn_lock VALUES (1), (2);");
  writeFileSync(join(dir, "connections.toml"), `version = 7\n${pgConnection("00000000-0000-4000-8000-0000000a1002", NAME, 'connection_type = "prod"')}`);
}

export default async function (page, ctx) {
  try {
    await connectWith(page, NAME);
    await openPath(page, [NAME, "platypus", "public", "Tables"], "qa_jrn_lock");
    await menu(page, "qa_jrn_lock", "Drop…");
    await page.waitFor(`document.querySelector('.modal.sql-action button[type=submit]')?.disabled === false`, 10_000);
    await page.eval(`document.querySelector('.modal.sql-action button[type=submit]').id = 'jrn-act'`);
    await page.clickSel("#jrn-act");
    await page.waitFor(`document.querySelector('.modal.sql-action') === null`, 15_000);
    // The tree shows it gone: the app believes it is done.
    await page.waitFor(`![...document.querySelectorAll('.tree-label')].some(l => l.textContent === 'qa_jrn_lock')`, 10_000);

    const other = psqlTry("SET lock_timeout = '2s'; SELECT count(*) FROM qa_jrn_lock");
    assert.doesNotMatch(other.out, /lock timeout/, "another client is blocked by the uncommitted DROP on production");
    assert.match(other.out, /does not exist/, `the drop is committed for other clients: ${other.out}`);

    assert.equal(await closeApp(page), null);
    await reopen(page, ctx, `document.querySelector('.tree-row') !== null`);
    const after = psqlTry("SELECT count(*) FROM qa_jrn_lock");
    assert.match(after.out, /does not exist/, `after the window closed the table is back: ${after.out}`);
  } finally {
    // The app's session still holds the lock when the test fails: end it, then clean up.
    psqlTry(
      "SELECT pg_terminate_backend(l.pid) FROM pg_locks l JOIN pg_class c ON c.oid = l.relation WHERE c.relname = 'qa_jrn_lock' AND l.pid <> pg_backend_pid()",
    );
    psqlTry("SET lock_timeout = '10s'; DROP TABLE IF EXISTS qa_jrn_lock");
  }
}
