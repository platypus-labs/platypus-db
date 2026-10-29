// What the scheduled tasks dialog works out: a new task's defaults, which tasks a connection
// can run, and the order of the list. Types only from the app: the unit tests load this
// module without it.
import type { DriverKind, Schedule, ScheduleRepeat, ScheduleTask, ScheduleView } from "../api";

/** Whether the database's own tools back it up and restore it (`commands::backup`). */
export const backsUp = (driver: DriverKind) => driver === "postgres" || driver === "mysql";

/** The tasks a connection's database can run: backups need its own tools. */
export function taskKinds(driver: DriverKind): ScheduleTask["kind"][] {
  return backsUp(driver) ? ["script", "export", "backup"] : ["script", "export"];
}

/** A task's own fields for `kind`, as a new one starts. */
export function taskOf(kind: ScheduleTask["kind"], driver: DriverKind): ScheduleTask {
  if (kind === "script") return { kind, path: "" };
  if (kind === "export") return { kind, sql: "", format: "csv" };
  return { kind, format: driver === "postgres" ? "custom" : "plain", schema: null };
}

/** `task` moved from a connection on `from` to one on `to`: a backup starts again with the
 *  new database's format, and a task the new one cannot run becomes a script. */
export function taskOnConnection(task: ScheduleTask, from: DriverKind | undefined, to: DriverKind): ScheduleTask {
  if (from === to) return task;
  if (!taskKinds(to).includes(task.kind)) return taskOf("script", to);
  return task.kind === "backup" ? taskOf("backup", to) : task;
}

/** A new task on `connectionId`: a script every day at 7, in the system's time zone. */
export function newSchedule(connectionId: string, id: string): Schedule {
  return {
    id,
    name: "",
    enabled: true,
    connection_id: connectionId,
    database: null,
    task: { kind: "script", path: "" },
    repeat: { every: "day" },
    time: "07:00",
    timezone: "",
    folder: null,
    keep: 7,
    retries: 0,
    retry_minutes: 5,
  };
}

/** `repeat` for a new choice of `every`, keeping what it had. */
export function repeatOf(every: ScheduleRepeat["every"], from: ScheduleRepeat): ScheduleRepeat {
  if (every === "week") return { every, days: from.every === "week" ? from.days : [1, 2, 3, 4, 5] };
  if (every === "month") return { every, day: from.every === "month" ? from.day : 1 };
  return { every };
}

/** Whether a task writes files into a folder. */
export const writesFiles = (task: ScheduleTask) => task.kind !== "script";

/** The list's order: those running, then by when they run next, those that do not last. */
export function sortSchedules(all: ScheduleView[]): ScheduleView[] {
  const key = (s: ScheduleView) => (s.running ? -Infinity : s.next ? Date.parse(s.next) : Infinity);
  return [...all].sort((a, b) => key(a) - key(b) || a.name.localeCompare(b.name));
}

/** The task as saved: fields of other kinds and empty texts dropped. */
export function cleaned(schedule: Schedule): Schedule {
  return {
    ...schedule,
    name: schedule.name.trim(),
    database: schedule.database?.trim() || null,
    folder: writesFiles(schedule.task) ? schedule.folder?.trim() || null : null,
    time: schedule.time.trim(),
  };
}
