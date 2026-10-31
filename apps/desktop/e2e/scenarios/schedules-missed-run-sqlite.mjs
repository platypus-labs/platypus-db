// Scheduled tasks whose time passed while the app was closed (schedules.json written before the
// start, with `due` two days ago): at the start each enabled task runs once (a script, an
// export into its folder, a schema backup), not once per missed day; its next time is the
// first one after now; a disabled task and one not yet due do not run; a task whose script
// fails is recorded as failed and waits for its retry (5 minutes later, try 2), and a task
// without a stored time just gets its next time. The list shows the outcomes.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { palette } from "../plans.mjs";

const CONNECTION_ID = "00000000-0000-4000-8000-0000000000e1";
const ID = (n) => `00000000-0000-4000-8000-0000000001${String(n).padStart(2, "0")}`;
const schedules = `[...document.querySelectorAll('.schedules-dialog')].find(d => d.querySelector('h2')?.textContent === 'Scheduled tasks')`;
const row = (name) =>
  `[...(${schedules}?.querySelectorAll('.schedule-row') ?? [])].find(r => r.querySelector('strong')?.textContent === ${JSON.stringify(name)})`;
const lastBadge = (name) => `${row(name)}.querySelector('[data-field="last"] .admin-badge')`;

const daysAgo = (n) => new Date(Date.now() - n * 86_400_000).toISOString();
const daysAhead = (n) => new Date(Date.now() + n * 86_400_000).toISOString();

function task(n, name, kind, extra = {}) {
  return {
    id: ID(n),
    name,
    enabled: true,
    connection_id: CONNECTION_ID,
    task: kind,
    repeat: { every: "day" },
    time: "07:00",
    timezone: "UTC",
    ...extra,
  };
}

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "sched.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v INTEGER); INSERT INTO t (v) VALUES (10), (20); CREATE TABLE ran (n INTEGER);");
  db.close();
  mkdirSync(join(dir, "sql"));
  writeFileSync(join(dir, "sql", "fill.sql"), "INSERT INTO ran VALUES (1);\n");
  writeFileSync(join(dir, "sql", "broken.sql"), "INSERT INTO ran VALUES (2);\nINSERT INTO no_such_table VALUES (1);\n");
  mkdirSync(join(dir, "out-export"));
  mkdirSync(join(dir, "out-backup"));
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7

[[connections]]
id = "${CONNECTION_ID}"
name = "Schedules db"
driver = "sqlite"
database = '${join(dir, "sched.db")}'
save_password = false
`,
  );
  const list = [
    task(1, "Fill", { kind: "script", path: join(dir, "sql", "fill.sql") }),
    task(2, "Export values", { kind: "export", sql: "SELECT id, v FROM t ORDER BY id", format: "csv" }, { folder: join(dir, "out-export") }),
    task(3, "Backup schema", { kind: "backup", format: "plain", schema: null }, { folder: join(dir, "out-backup") }),
    task(4, "Disabled", { kind: "script", path: join(dir, "sql", "fill.sql") }, { enabled: false }),
    task(5, "Not yet", { kind: "script", path: join(dir, "sql", "fill.sql") }),
    task(6, "Broken", { kind: "script", path: join(dir, "sql", "broken.sql") }, { retries: 2, retry_minutes: 5 }),
    task(7, "No time yet", { kind: "script", path: join(dir, "sql", "fill.sql") }),
  ];
  writeFileSync(
    join(dir, "schedules.json"),
    JSON.stringify({
      schedules: list,
      runs: {},
      due: {
        [ID(1)]: daysAgo(2),
        [ID(2)]: daysAgo(3),
        [ID(3)]: daysAgo(1),
        [ID(4)]: daysAgo(2),
        [ID(5)]: daysAhead(1),
        [ID(6)]: daysAgo(2),
      },
      retry: {},
    }),
  );
}

export default async function (page, { dir }) {
  await palette(page, "Scheduled tasks");
  await page.waitFor(`${row("Fill")} !== undefined`, 15_000);
  // The tasks run at the start; wait for the last of them to have a run.
  for (const name of ["Fill", "Export values", "Backup schema", "Broken"]) {
    await page.waitFor(`${lastBadge(name)} != null`, 30_000);
  }

  // Once, however many days were missed.
  const check = new DatabaseSync(join(dir, "sched.db"));
  const ran = check.prepare("SELECT n FROM ran ORDER BY n").all().map((r) => r.n);
  check.close();
  // "Fill" ran once; "Broken" ran its first statement (no transaction around a script) or none.
  assert.equal(ran.filter((n) => n === 1).length, 1, `fill ran ${JSON.stringify(ran)}`);

  // The list: done for the three that worked, failed for the broken script.
  for (const name of ["Fill", "Export values", "Backup schema"]) {
    assert.match(await page.eval(`${lastBadge(name)}.textContent`), /^done/, name);
  }
  assert.match(await page.eval(`${lastBadge("Broken")}.textContent`), /^failed/);
  assert.match(await page.eval(`${lastBadge("Broken")}.title`), /no_such_table/);

  // The ones that were not due did not run.
  for (const name of ["Disabled", "Not yet", "No time yet"]) {
    assert.equal(await page.eval(`${row(name)}.querySelector('[data-field="last"]').textContent`), "never", name);
  }

  // The export is in the task's folder, named by task and time.
  const exported = readdirSync(join(dir, "out-export"));
  assert.equal(exported.length, 1, `${exported}`);
  assert.match(exported[0], /^Export_values-\d{8}-\d{6}\.csv$/);
  assert.equal(readFileSync(join(dir, "out-export", exported[0]), "utf8").replaceAll("\r\n", "\n"), "id,v\n1,10\n2,20\n");
  const backups = readdirSync(join(dir, "out-backup"));
  assert.equal(backups.length, 1, `${backups}`);
  assert.match(readFileSync(join(dir, "out-backup", backups[0]), "utf8"), /CREATE TABLE/i);

  // What the file keeps: one run for each, the next time in the future, the retry of the broken
  // one 5 minutes after it ended and marked as the second try.
  const saved = JSON.parse(readFileSync(join(dir, "schedules.json"), "utf8"));
  for (const n of [1, 2, 3, 6]) assert.equal(saved.runs[ID(n)].length, 1, `run of ${n}`);
  for (const n of [1, 2, 3, 6, 7]) assert.ok(new Date(saved.due[ID(n)]) > new Date(), `next time of ${n} is ahead`);
  assert.equal(saved.runs[ID(6)][0].ok, false);
  assert.equal(saved.runs[ID(6)][0].attempt, 1);
  const [retryAt, attempt] = saved.retry[ID(6)];
  assert.equal(attempt, 2);
  const gap = new Date(retryAt) - new Date(saved.runs[ID(6)][0].finished);
  assert.ok(Math.abs(gap - 5 * 60_000) < 2000, `retry ${gap} ms after the failure`);
  assert.equal(saved.runs[ID(4)], undefined, "disabled: no run");
  assert.equal(saved.runs[ID(5)], undefined, "not yet: no run");
  assert.equal(saved.runs[ID(7)], undefined, "no time yet: no run");
  assert.equal(existsSync(join(dir, "schedules.json.unreadable")), false);

  // The list says when the broken one tries again: its next time is the retry, 5 minutes away.
  const nextText = await page.eval(`${row("Broken")}.querySelector('[data-field="next"]').textContent`);
  assert.ok(nextText.length > 0 && nextText !== "off", nextText);
}
