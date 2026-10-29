// Unit tests of the scheduled tasks dialog's defaults, choices and order (src/admin/schedules.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { cleaned, newSchedule, repeatOf, sortSchedules, taskKinds, taskOf, taskOnConnection, writesFiles } from "../src/admin/schedules.ts";

test("backups only where the database's tools are", () => {
  assert.deepEqual(taskKinds("postgres"), ["script", "export", "backup"]);
  assert.deepEqual(taskKinds("mysql"), ["script", "export", "backup"]);
  assert.deepEqual(taskKinds("sqlite"), ["script", "export"]);
  assert.deepEqual(taskOf("backup", "postgres"), { kind: "backup", format: "custom", schema: null });
  assert.deepEqual(taskOf("backup", "mysql").format, "plain");
  assert.ok(writesFiles(taskOf("export", "sqlite")));
  assert.ok(!writesFiles(taskOf("script", "sqlite")));
});

test("a new task runs a script every day at 7", () => {
  const s = newSchedule("c1", "id1");
  assert.equal(s.connection_id, "c1");
  assert.deepEqual(s.repeat, { every: "day" });
  assert.equal(s.time, "07:00");
  assert.equal(s.timezone, "", "the system's");
  assert.ok(s.enabled);
});

test("changing how often keeps the days chosen", () => {
  assert.deepEqual(repeatOf("week", { every: "day" }), { every: "week", days: [1, 2, 3, 4, 5] });
  assert.deepEqual(repeatOf("week", { every: "week", days: [6, 7] }), { every: "week", days: [6, 7] });
  assert.deepEqual(repeatOf("month", { every: "day" }), { every: "month", day: 1 });
  assert.deepEqual(repeatOf("hour", { every: "month", day: 9 }), { every: "hour" });
});

test("running first, then by the next run, never last", () => {
  const view = (name, next, running = false) => ({ ...newSchedule("c", name), name, next, running, runs: [] });
  const sorted = sortSchedules([
    view("off", null),
    view("later", "2026-11-11T07:00:00Z"),
    view("soon", "2026-11-10T07:00:00Z"),
    view("busy", "2026-11-12T07:00:00Z", true),
  ]);
  assert.deepEqual(sorted.map((s) => s.name), ["busy", "soon", "later", "off"]);
});

test("saved without the fields it does not use", () => {
  const s = { ...newSchedule("c", "i"), name: "  Nightly ", folder: "/out", database: " " };
  assert.equal(cleaned(s).name, "Nightly");
  assert.equal(cleaned(s).folder, null, "a script writes no files");
  assert.equal(cleaned(s).database, null);
  const e = { ...s, task: taskOf("export", "sqlite") };
  assert.equal(cleaned(e).folder, "/out");
});

test("a task moved to another database", () => {
  const backup = { kind: "backup", format: "custom", schema: "sales" };
  assert.deepEqual(taskOnConnection(backup, "postgres", "postgres"), backup, "same database: kept");
  assert.deepEqual(taskOnConnection(backup, "postgres", "mysql"), { kind: "backup", format: "plain", schema: null });
  assert.deepEqual(taskOnConnection(backup, "postgres", "sqlite"), { kind: "script", path: "" }, "no tools there");
  const exp = { kind: "export", sql: "SELECT 1", format: "csv" };
  assert.deepEqual(taskOnConnection(exp, "postgres", "sqlite"), exp);
});
