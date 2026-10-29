// Unit tests of the scheduled tasks dialog's defaults, choices and order (src/admin/schedules.ts).
import assert from "node:assert/strict";
import { test } from "node:test";
import { cleaned, newProfileSchedule, newSchedule, newSnapshotSchedule, repeatOf, sortSchedules, taskKinds, taskOf, taskOnConnection, writesFiles } from "../src/admin/schedules.ts";

test("backups with the database's tools, or as a SQL dump", () => {
  assert.deepEqual(taskKinds("postgres"), ["script", "export", "backup", "snapshot", "profile"]);
  assert.deepEqual(taskKinds("mysql"), ["script", "export", "backup", "snapshot", "profile"]);
  assert.deepEqual(taskKinds("sqlite"), ["script", "export", "backup", "snapshot", "profile"]);
  assert.deepEqual(taskKinds("mongo"), ["script", "export", "profile"]);
  assert.deepEqual(taskOf("backup", "postgres"), { kind: "backup", format: "custom", schema: null });
  assert.deepEqual(taskOf("backup", "mysql").format, "plain");
  assert.ok(writesFiles(taskOf("export", "sqlite")));
  assert.ok(!writesFiles(taskOf("script", "sqlite")));
});

test("a snapshot task needs a schema and writes no files", () => {
  assert.deepEqual(taskOf("snapshot", "postgres"), { kind: "snapshot", schema: "" });
  assert.ok(!writesFiles(taskOf("snapshot", "postgres")));
  const s = newSnapshotSchedule("c1", "shop", "public", "Snapshots of public", "id1");
  assert.deepEqual(s.task, { kind: "snapshot", schema: "public" });
  assert.equal(s.database, "shop");
  assert.equal(s.name, "Snapshots of public");
  assert.equal(cleaned({ ...s, folder: "/out" }).folder, null);
  const moved = taskOnConnection(s.task, "postgres", "mysql");
  assert.deepEqual(moved, { kind: "snapshot", schema: "" }, "the schema belongs to the old connection");
  assert.deepEqual(taskOnConnection(s.task, "postgres", "mongo"), { kind: "script", path: "" });
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
  assert.deepEqual(taskOnConnection(backup, "postgres", "sqlite"), { kind: "backup", format: "plain", schema: null });
  assert.deepEqual(taskOnConnection(backup, "postgres", "mongo"), { kind: "script", path: "" }, "no backup there");
  const exp = { kind: "export", sql: "SELECT 1", format: "csv" };
  assert.deepEqual(taskOnConnection(exp, "postgres", "sqlite"), exp);
});

test("a profile runs on any database, and a new task takes its name and first connection", () => {
  for (const driver of ["postgres", "sqlite", "mongo", "redis"]) assert.ok(taskKinds(driver).includes("profile"), driver);
  assert.deepEqual(taskOf("profile", "mongo"), { kind: "profile", profile_id: "" });
  assert.ok(writesFiles(taskOf("profile", "sqlite")), "a scheduled export is sent to the task's folder");
  const profile = { id: "p1", name: "Orders", task: { kind: "migrate" } };
  const s = newProfileSchedule(profile, "c1", "id1");
  assert.deepEqual(s.task, { kind: "profile", profile_id: "p1" });
  assert.equal(s.name, "Orders");
  assert.equal(s.connection_id, "c1");
  assert.deepEqual(s.repeat, { every: "day" });
});

test("a profile task stays when its connection changes to a driver that runs it", () => {
  const task = { kind: "profile", profile_id: "p1" };
  assert.equal(taskOnConnection(task, "postgres", "mongo"), task);
});
