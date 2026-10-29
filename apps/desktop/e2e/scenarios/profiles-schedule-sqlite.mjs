// Transfer profiles in scheduled tasks (group H) on SQLite: "Schedule…" from the profiles
// dialog starts a task that runs the profile; the profile cannot be deleted while a task runs
// it (the message names the task), and can once the tasks are gone; "Run now" of a task with a
// folder writes the export into that folder, named by the task and the time of the run, and
// keeps only the newest files, while a task without a folder writes where the profile says.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { palette, setField } from "../plans.mjs";
import { closeProfiles, deleteProfile, listed, openProfiles, rowAction, savedProfiles } from "../profiles-helpers.mjs";
import { button, saveConnections, sqliteBlock } from "../qa-xfer.mjs";

const CONNECTION = "Profiles schedule";
const CONNECTION_ID = "00000000-0000-4000-8000-0000000000c1";
const PROFILE = "Seeded values";

const schedules = `[...document.querySelectorAll('.schedules-dialog')].find(d => d.querySelector('h2')?.textContent === 'Scheduled tasks')`;
const form = `document.querySelector('.schedule-form')`;
const row = (name) =>
  `[...(${schedules}?.querySelectorAll('.schedule-row') ?? [])].find(r => r.querySelector('strong')?.textContent === ${JSON.stringify(name)})`;
const last = (name) => `(${row(name)}?.querySelector('[data-field="last"]')?.textContent ?? '')`;

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "sched.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER); INSERT INTO t (v) VALUES (10), (20), (30);");
  db.close();
  saveConnections(dir, [], sqliteBlock(CONNECTION, join(dir, "sched.db"), CONNECTION_ID));
  mkdirSync(join(dir, "own"));
  // A profile as the export dialog would have saved it.
  writeFileSync(
    join(dir, "transfer-profiles.json"),
    JSON.stringify({
      profiles: [
        {
          id: "00000000-0000-4000-8000-0000000000d1",
          name: PROFILE,
          task: {
            kind: "export_query",
            connection_id: CONNECTION_ID,
            database: null,
            sql: "SELECT id, v FROM t ORDER BY id",
            path: join(dir, "own", "values.csv"),
            options: null,
          },
        },
      ],
    }),
  );
}

/** Sets the form's fields (`data-field` → value) and presses Save; waits for the form to go. */
async function fillAndSave(page, fields) {
  for (const [field, value] of Object.entries(fields)) {
    await page.waitFor(`${form}?.querySelector('[data-field="${field}"]') != null`, 15_000);
    await setField(page, `.schedule-form [data-field="${field}"]`, value);
  }
  await page.eval(`${button(form, "Save")}.click()`);
  await page.waitFor(`${form} === null || ${form}.querySelector('[data-field="error"]') !== null`, 15_000);
}

/** Runs the task `name` now and waits for its last run to be a new one. */
async function runNow(page, name) {
  const before = await page.eval(`${row(name)}.querySelectorAll('[data-field="last"] .admin-badge').length + ':' + ${last(name)}`);
  await page.eval(`${button(row(name), "Run now")}.click()`);
  await page.waitFor(
    `!${row(name)}.querySelector('[data-field="next"] .admin-badge.running') && ${row(name)}.querySelectorAll('[data-field="last"] .admin-badge').length + ':' + ${last(name)} !== ${JSON.stringify(before)}`,
    30_000,
  );
}

/** Deletes the task `name` from the open list, confirming. */
async function deleteTask(page, name) {
  await page.eval(`${button(row(name), "Delete")}.click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${row(name)} === undefined`);
}

export default async function (page, { dir }) {
  const folder = join(dir, "nightly");
  const own = join(dir, "own", "values.csv");

  // Schedule… on the profile: the form is for a profile, with the one picked.
  await openProfiles(page);
  assert.deepEqual(await listed(page), [PROFILE]);
  await rowAction(page, PROFILE, "Schedule…");
  await page.waitFor(`${form} !== null && ${form}.querySelector('[data-field="profile"]') !== null`, 15_000);
  assert.equal(await page.eval(`${form}.querySelector('[data-field="kind"]').value`), "profile");
  await page.waitFor(`${form}.querySelector('[data-field="profile"]').selectedOptions[0]?.textContent === ${JSON.stringify(PROFILE)}`, 10_000);
  // The task follows the profile's connection: it cannot be changed here.
  assert.equal(await page.eval(`${form}.querySelector('[data-field="connection"]').disabled`), true);
  await fillAndSave(page, { name: "Nightly", folder, keep: "1" });
  await page.waitFor(`${form} === null`);

  // The profile is in use: it stays, and the message names the task.
  const refused = await deleteProfile(page, PROFILE);
  assert.match(refused ?? "", /Nightly/);
  assert.match(refused ?? "", /runs this profile/);
  assert.deepEqual(await listed(page), [PROFILE]);
  assert.ok(savedProfiles(dir)[PROFILE], "still in the file");
  await closeProfiles(page);

  // Run now: the export goes into the task's folder, not where the profile says.
  await palette(page, "Scheduled tasks");
  await page.waitFor(`${row("Nightly")} !== undefined`, 15_000);
  assert.match(await page.eval(`${row("Nightly")}.textContent`), /Run a transfer profile/);
  assert.equal(await page.eval(last("Nightly")), "never");
  await runNow(page, "Nightly");
  assert.match(await page.eval(last("Nightly")), /^done/, await page.eval(`${row("Nightly")}.querySelector('[data-field="last"] .admin-badge')?.title`));
  assert.ok(existsSync(folder), "the folder was made");
  let files = readdirSync(folder);
  assert.equal(files.length, 1, `${files}`);
  assert.match(files[0], /^Nightly-\d{8}-\d{6}\.csv$/);
  assert.equal(readFileSync(join(folder, files[0]), "utf8").replaceAll("\r\n", "\n"), "id,v\n1,10\n2,20\n3,30\n");
  assert.equal(existsSync(own), false, "the profile's own file is not written by a task with a folder");
  // A second later: another stamp, and with one file kept the first goes.
  const first = files[0];
  await page.sleep(1100);
  await runNow(page, "Nightly");
  files = readdirSync(folder);
  assert.equal(files.length, 1, `one kept: ${files}`);
  assert.notEqual(files[0], first);

  // A task without a folder writes where the profile says.
  await page.eval(`${button(schedules, "New task…")}.click()`);
  await page.waitFor(`${form} !== null`);
  await setField(page, `.schedule-form [data-field="kind"]`, "profile");
  await page.waitFor(`${form}.querySelector('[data-field="profile"]') !== null`, 10_000);
  await fillAndSave(page, { name: "Direct" });
  await page.waitFor(`${row("Direct")} !== undefined`, 15_000);
  await runNow(page, "Direct");
  assert.match(await page.eval(last("Direct")), /^done/, await page.eval(`${row("Direct")}.querySelector('[data-field="last"] .admin-badge')?.title`));
  assert.equal(readFileSync(own, "utf8").replaceAll("\r\n", "\n"), "id,v\n1,10\n2,20\n3,30\n");
  assert.equal(readdirSync(folder).length, 1, "the other task's folder is untouched");

  // Both tasks gone: the profile can be deleted, and goes from the file.
  await deleteTask(page, "Direct");
  await deleteTask(page, "Nightly");
  await page.eval(`${button(schedules, "Close")}.click()`);
  await page.waitFor(`${schedules} === undefined`);
  await openProfiles(page);
  assert.equal(await deleteProfile(page, PROFILE), null);
  assert.deepEqual(await listed(page), []);
  assert.deepEqual(Object.keys(savedProfiles(dir)), []);
  await closeProfiles(page);
}
