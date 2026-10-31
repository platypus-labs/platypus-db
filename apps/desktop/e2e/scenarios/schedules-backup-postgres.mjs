// Scheduled backups with the database's own tools (pg_dump, mysqldump; found on the PATH or in
// the dev machine's portable server folders) against the dev PostgreSQL and MySQL, and the ways
// a scheduled run goes wrong:
//  - a backup task made in the form (PostgreSQL, SQL statements of one schema) writes a file
//    named by task and time into its folder, with only that schema, and keeps only the newest;
//  - seeded tasks write a pg_dump archive of the whole database (magic PGDMP, every table in
//    its listing) and a MySQL dump (CREATE TABLE and the rows);
//  - failures are shown in the list with the tool's reason, and none leaves a file behind:
//    a folder that cannot be made (its parent is a file), a task without a folder, a wrong
//    password from the password command (PostgreSQL and MySQL), a schema that does not exist;
//  - after the password is fixed "Run now" succeeds, and a task whose time came while the app
//    was closed (retries 2, every minute) fails at the start, is queued as try 2, and the
//    scheduler's next tick runs it again by itself once the password is right;
//  - the time zone moves the next run: 03:00 in Tokyo is 18:00 UTC of the day before.
// Needs PostgreSQL and MySQL from docker/compose.yml and the clients' tools.
import assert from "node:assert/strict";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { mysqlFolder, pgFolder, withPath } from "../clients.mjs";
import { palette, setField } from "../plans.mjs";
import { button, db, dbQuiet, skipUnless } from "../qa-xfer.mjs";

const PG_ID = "00000000-0000-4000-8000-0000000b0001";
const FLAKY_PG_ID = "00000000-0000-4000-8000-0000000b0002";
const MY_ID = "00000000-0000-4000-8000-0000000b0003";
const FLAKY_MY_ID = "00000000-0000-4000-8000-0000000b0004";
const ID = (n) => `00000000-0000-4000-8000-0000000b01${String(n).padStart(2, "0")}`;

const schedules = `[...document.querySelectorAll('.schedules-dialog')].find(d => d.querySelector('h2')?.textContent === 'Scheduled tasks')`;
const form = `document.querySelector('.schedule-form')`;
const row = (name) =>
  `[...(${schedules}?.querySelectorAll('.schedule-row') ?? [])].find(r => r.querySelector('strong')?.textContent === ${JSON.stringify(name)})`;
const lastBadge = (name) => `${row(name)}?.querySelector('[data-field="last"] .admin-badge')`;
const lastText = (name) => `(${lastBadge(name)}?.textContent ?? '')`;
const lastTitle = (name) => `(${lastBadge(name)}?.title ?? '')`;

export const skip = async () => {
  const down = await skipUnless("pg", "mysql");
  if (down) return down;
  if (pgFolder() === null) return "no psql/pg_dump on the PATH (set PLATYPUS_E2E_PGBIN)";
  if (mysqlFolder() === null) return "no mysql/mysqldump on the PATH (set PLATYPUS_E2E_MYSQLBIN)";
  return null;
};

// English messages from the tools, whatever the language of the machine.
export const env = () => ({ ...withPath(pgFolder(), mysqlFolder()), LC_ALL: "C", LANG: "C" });

const connection = (id, name, toml, passwordCommand) => `
[[connections]]
id = "${id}"
name = "${name}"
${toml}
connection_type = "dev"
save_password = false
password_command = '${passwordCommand}'
`;

function task(n, name, connectionId, kind, extra = {}) {
  return {
    id: ID(n),
    name,
    enabled: true,
    connection_id: connectionId,
    task: kind,
    repeat: { every: "day" },
    time: "07:00",
    timezone: "UTC",
    ...extra,
  };
}

const ID_OF = {
  "Whole database": ID(1), "Bad folder": ID(2), "No folder": ID(3), "Missing schema": ID(4), "Wrong password": ID(5),
  "MySQL dump": ID(6), "MySQL wrong password": ID(7), Retried: ID(8), Tokyo: ID(9), UTC: ID(10),
};

export function setup({ dir }) {
  db(
    "pg",
    `DROP SCHEMA IF EXISTS qa_sched CASCADE; DROP TABLE IF EXISTS public.qa_sched_other;
     CREATE SCHEMA qa_sched; CREATE TABLE qa_sched.items (id int PRIMARY KEY, name text);
     INSERT INTO qa_sched.items VALUES (1, 'alpha-row'), (2, 'beta-row');
     CREATE TABLE public.qa_sched_other (id int); INSERT INTO public.qa_sched_other VALUES (7)`,
  );
  db("mysql", "DROP TABLE IF EXISTS qa_sched_my; CREATE TABLE qa_sched_my (id int PRIMARY KEY, name varchar(20)); INSERT INTO qa_sched_my VALUES (1, 'gamma-row'), (2, 'delta-row')");
  // The flaky connections read their password from a file that starts wrong.
  writeFileSync(join(dir, "pg-password.txt"), "not-the-password\n");
  writeFileSync(join(dir, "my-password.txt"), "not-the-password\n");
  const pg = `driver = "postgres"\nhost = "localhost"\nport = 55432\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`;
  const my = `driver = "mysql"\nhost = "127.0.0.1"\nport = 53306\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"`;
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n${connection(PG_ID, "Sched PG", pg, "echo platypus")}${connection(FLAKY_PG_ID, "Sched PG flaky", pg, `type "${join(dir, "pg-password.txt")}"`)}` +
      `${connection(MY_ID, "Sched MySQL", my, "echo platypus")}${connection(FLAKY_MY_ID, "Sched MySQL flaky", my, `type "${join(dir, "my-password.txt")}"`)}`,
  );
  // A file where a folder should be: nothing can be made under it.
  writeFileSync(join(dir, "not-a-folder"), "x");
  const backup = (extra = {}) => ({ kind: "backup", format: "custom", schema: null, ...extra });
  const list = [
    task(1, "Whole database", PG_ID, backup(), { folder: join(dir, "whole") }),
    task(2, "Bad folder", PG_ID, backup(), { folder: join(dir, "not-a-folder", "inside") }),
    task(3, "No folder", PG_ID, backup()),
    task(4, "Missing schema", PG_ID, backup({ format: "plain", schema: "no_such_schema" }), { folder: join(dir, "missing") }),
    task(5, "Wrong password", FLAKY_PG_ID, backup({ format: "plain" }), { folder: join(dir, "flaky") }),
    task(6, "MySQL dump", MY_ID, { kind: "backup", format: "plain", schema: null }, { folder: join(dir, "mysql") }),
    task(7, "MySQL wrong password", FLAKY_MY_ID, { kind: "backup", format: "plain", schema: null }, { folder: join(dir, "mysql-flaky") }),
    // Its time passed while the app was closed: it runs at the start, fails, tries again a minute later.
    task(8, "Retried", FLAKY_PG_ID, backup({ format: "plain" }), { folder: join(dir, "retried"), retries: 2, retry_minutes: 1 }),
    task(9, "Tokyo", PG_ID, backup(), { time: "03:00", timezone: "Asia/Tokyo", folder: join(dir, "tokyo") }),
    task(10, "UTC", PG_ID, backup(), { time: "03:00", timezone: "UTC", folder: join(dir, "tokyo") }),
  ];
  writeFileSync(
    join(dir, "schedules.json"),
    JSON.stringify({ schedules: list, runs: {}, due: { [ID(8)]: new Date(Date.now() - 2 * 86_400_000).toISOString() }, retry: {} }),
  );
}

const saved = (dir) => JSON.parse(readFileSync(join(dir, "schedules.json"), "utf8"));

/** Sets the form's fields (`data-field` → value) and presses Save; waits for the form to go. */
async function fillAndSave(page, fields) {
  for (const [field, value] of Object.entries(fields)) {
    await page.waitFor(`${form}?.querySelector('[data-field="${field}"]') != null`, 15_000);
    await setField(page, `.schedule-form [data-field="${field}"]`, value);
  }
  await page.eval(`${button(form, "Save")}.click()`);
  await page.waitFor(`${form} === null || ${form}.querySelector('[data-field="error"]') !== null`, 15_000);
}

/** Runs the task `name` now and waits for the end of that run (the runs the file keeps grow). */
async function runNow(page, name, dir) {
  const runs = () => saved(dir).runs[ID_OF[name]]?.length ?? 0;
  const before = runs();
  await page.eval(`${button(row(name), "Run now")}.click()`);
  for (let i = 0; i < 600 && runs() === before; i++) await page.sleep(100);
  await page.waitFor(`!${row(name)}.querySelector('[data-field="next"] .admin-badge.running')`, 60_000);
  assert.equal(runs(), before + 1, `${name} ran`);
}

const files = (dir, folder) => (existsSync(join(dir, folder)) ? readdirSync(join(dir, folder)) : []);
const text = (dir, folder, name) => readFileSync(join(dir, folder, name), "utf8");

export default async function (page, { dir }) {
  try {
    await palette(page, "Scheduled tasks");
    await page.waitFor(`${row("Whole database")} !== undefined`, 15_000);
    // The task due at the start ran once and failed on the wrong password; the others waited.
    await page.waitFor(`${lastText("Retried")}.startsWith('failed')`, 60_000);
    const first = saved(dir);
    assert.equal(first.runs[ID(8)].length, 1);
    assert.match(first.runs[ID(8)][0].message, /authentication/i);
    assert.equal(first.retry[ID(8)][1], 2, "queued as try 2");
    for (const name of ["Whole database", "Bad folder", "Wrong password", "Tokyo"]) {
      assert.equal(await page.eval(`${row(name)}.querySelector('[data-field="last"]').textContent`), "never", name);
    }

    // The time zone: 03:00 in Tokyo comes 9 hours before 03:00 UTC.
    const tokyo = new Date(first.due[ID(9)]);
    const utc = new Date(first.due[ID(10)]);
    assert.equal(tokyo.getUTCMinutes(), 0);
    assert.equal(utc.getUTCHours(), 3);
    assert.equal(tokyo.getUTCHours(), 18, `Tokyo ${tokyo.toISOString()}`);
    assert.ok(tokyo > new Date() && utc > new Date());

    // A task made in the form: SQL statements of one schema, the newest file only.
    await page.eval(`${button(schedules, "New task…")}.click()`);
    await page.waitFor(`${form} !== null`);
    await setField(page, `.schedule-form [data-field="connection"]`, PG_ID);
    await setField(page, `.schedule-form [data-field="kind"]`, "backup");
    await page.waitFor(`${form}.querySelector('[data-field="schema"]') !== null`, 10_000);
    await fillAndSave(page, { name: "One schema", format: "plain", schema: "qa_sched", folder: join(dir, "one"), keep: "1" });
    await page.waitFor(`${form} === null && ${row("One schema")} !== undefined`, 15_000);
    ID_OF["One schema"] = saved(dir).schedules.find((s) => s.name === "One schema").id;
    await runNow(page, "One schema", dir);
    assert.match(await page.eval(lastText("One schema")), /^done/, await page.eval(lastTitle("One schema")));
    let made = files(dir, "one");
    assert.equal(made.length, 1, `${made}`);
    assert.match(made[0], /^One_schema-\d{8}-\d{6}\.sql$/);
    let sql = text(dir, "one", made[0]);
    assert.match(sql, /CREATE TABLE qa_sched\.items/);
    assert.match(sql, /alpha-row/);
    assert.ok(!sql.includes("qa_sched_other"), "only the schema asked for");
    // Another stamp a second later: with one file kept, the first goes.
    await page.sleep(1100);
    await runNow(page, "One schema", dir);
    const second = files(dir, "one");
    assert.equal(second.length, 1, `${second}`);
    assert.notEqual(second[0], made[0]);

    // The archive of the whole database: pg_dump's magic, both schemas in its listing.
    await runNow(page, "Whole database", dir);
    assert.match(await page.eval(lastText("Whole database")), /^done/, await page.eval(lastTitle("Whole database")));
    made = files(dir, "whole");
    assert.equal(made.length, 1);
    assert.match(made[0], /^Whole_database-\d{8}-\d{6}\.dump$/);
    const archive = readFileSync(join(dir, "whole", made[0]));
    assert.equal(archive.subarray(0, 5).toString("latin1"), "PGDMP");
    const listing = archive.toString("latin1");
    assert.ok(listing.includes("qa_sched_other") && listing.includes("items"), "both tables in the archive");
    assert.match(await page.eval(lastTitle("Whole database")), /written/);

    // Failures. A folder under a file cannot be made: the reason names it, nothing is written.
    await runNow(page, "Bad folder", dir);
    assert.match(await page.eval(lastText("Bad folder")), /^failed/);
    assert.ok((await page.eval(lastTitle("Bad folder"))).includes("not-a-folder"), await page.eval(lastTitle("Bad folder")));
    assert.equal(existsSync(join(dir, "not-a-folder", "inside")), false);
    // No folder at all: the task says what to choose.
    await runNow(page, "No folder", dir);
    assert.match(await page.eval(lastText("No folder")), /^failed/);
    assert.match(await page.eval(lastTitle("No folder")), /choose the folder/);
    // A schema pg_dump does not know: its message, and no file left in the folder.
    await runNow(page, "Missing schema", dir);
    assert.match(await page.eval(lastText("Missing schema")), /^failed/);
    assert.match(await page.eval(lastTitle("Missing schema")), /no_such_schema|no matching schemas/i);
    assert.deepEqual(files(dir, "missing"), [], "a failed backup leaves no file");
    // The wrong password: the server's own refusal; no file.
    await runNow(page, "Wrong password", dir);
    assert.match(await page.eval(lastText("Wrong password")), /^failed/);
    assert.match(await page.eval(lastTitle("Wrong password")), /password authentication failed/i);
    assert.deepEqual(files(dir, "flaky"), []);
    await runNow(page, "MySQL wrong password", dir);
    assert.match(await page.eval(lastText("MySQL wrong password")), /^failed/);
    assert.match(await page.eval(lastTitle("MySQL wrong password")), /access denied|authentication/i);
    assert.deepEqual(files(dir, "mysql-flaky"), []);

    // MySQL dumps the database: the structure and the rows.
    await runNow(page, "MySQL dump", dir);
    assert.match(await page.eval(lastText("MySQL dump")), /^done/, await page.eval(lastTitle("MySQL dump")));
    made = files(dir, "mysql");
    assert.equal(made.length, 1);
    assert.match(made[0], /^MySQL_dump-\d{8}-\d{6}\.sql$/);
    sql = text(dir, "mysql", made[0]);
    assert.match(sql, /CREATE TABLE `qa_sched_my`/);
    assert.match(sql, /gamma-row/);

    // The passwords are right now: the same tasks, run by hand, work.
    writeFileSync(join(dir, "pg-password.txt"), "platypus\n");
    writeFileSync(join(dir, "my-password.txt"), "platypus\n");
    await runNow(page, "Wrong password", dir);
    assert.match(await page.eval(lastText("Wrong password")), /^done/, await page.eval(lastTitle("Wrong password")));
    assert.equal(files(dir, "flaky").length, 1);
    await runNow(page, "MySQL wrong password", dir);
    assert.match(await page.eval(lastText("MySQL wrong password")), /^done/, await page.eval(lastTitle("MySQL wrong password")));
    assert.match(text(dir, "mysql-flaky", files(dir, "mysql-flaky")[0]), /gamma-row/);

    // The retry: the scheduler tries "Retried" again by itself a minute after the failure
    // (it looks every 20 seconds); try 2 works now and the queue is empty.
    await page.waitFor(`${lastText("Retried")}.startsWith('done')`, 150_000);
    const after = saved(dir);
    assert.equal(after.runs[ID(8)].length, 2);
    assert.equal(after.runs[ID(8)][1].attempt, 1);
    assert.equal(after.runs[ID(8)][1].ok, false);
    assert.equal(after.runs[ID(8)][0].attempt, 2);
    assert.equal(after.runs[ID(8)][0].ok, true);
    assert.equal(after.runs[ID(8)][0].manual ?? false, false);
    assert.equal(after.retry[ID(8)], undefined, "nothing queued after a success");
    assert.equal(files(dir, "retried").length, 1);
    assert.match(text(dir, "retried", files(dir, "retried")[0]), /alpha-row/);
    // The runs by hand never queue a retry.
    for (const n of [2, 3, 4, 5, 7]) assert.equal(after.retry[ID(n)], undefined, `no retry for ${n}`);
  } finally {
    dbQuiet("pg", "DROP SCHEMA IF EXISTS qa_sched CASCADE; DROP TABLE IF EXISTS public.qa_sched_other");
    dbQuiet("mysql", "DROP TABLE IF EXISTS qa_sched_my");
  }
}
