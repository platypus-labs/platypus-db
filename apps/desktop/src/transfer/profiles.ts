// What the dialogs keep in a transfer profile and take back from one: which connections it
// uses, and how a saved import plan fills the columns again. Types only from the app: the
// unit tests load this module without it.
import type { ImportPlan, ProfileTask, TransferProfile } from "../api";

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** The connections a task uses, the first being where its data is read or written. */
export function profileConnections(task: ProfileTask): string[] {
  return task.kind === "transfer_table" || task.kind === "migrate"
    ? [task.source_connection_id, task.target_connection_id]
    : [task.connection_id];
}

/** The connections of `task` that are not among the `known` ones. */
export const missingConnections = (task: ProfileTask, known: string[]) =>
  profileConnections(task).filter((id) => !known.includes(id));

/** Whether `profile` reads or writes through connection `id`. */
export const usesConnection = (profile: TransferProfile, id: string) => profileConnections(profile.task).includes(id);

/** Whether the profile writes a file or a folder, which a scheduled run can send elsewhere. */
export const exportsFiles = (task: ProfileTask) => task.kind === "export_query" || task.kind === "export_tables";

/** The profiles for a list: those of connection `connectionId`, or all; by name. */
export function profilesOf(all: TransferProfile[], connectionId?: string): TransferProfile[] {
  return all
    .filter((p) => !connectionId || usesConnection(p, connectionId))
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** For each of the table's `targets` columns, the source column the plan feeds it from, or
 *  "" when the plan leaves it out. */
export function mappingFor(targets: string[], plan: ImportPlan): string[] {
  return targets.map((name) => plan.columns.find((c) => same(c.target, name))?.source ?? "");
}

/** A source column of a new table: whether it is copied, and its name and type there. */
export interface NewColumn {
  include: boolean;
  target: string;
  type: string;
}

/** The columns of a new table for the source columns `sources`: those the plan creates take
 *  its names and types, the others are left out. `defaults` is how they start without a plan. */
export function newColumnsFor(sources: string[], plan: ImportPlan, defaults: NewColumn[]): NewColumn[] {
  if (!plan.create) return defaults;
  return defaults.map((fallback, i) => {
    const kept = plan.columns.find((c) => c.source === sources[i]);
    return kept ? { include: true, target: kept.target, type: kept.data_type ?? fallback.type } : { ...fallback, include: false };
  });
}

/** What the plan names that is not there any more: source columns the file or table lacks,
 *  and, when it fills an existing table, columns that table lacks. */
export function missingColumns(plan: ImportPlan, sources: string[], targets: string[] | null) {
  return {
    sources: plan.columns.filter((c) => !sources.some((s) => s === c.source)).map((c) => c.source),
    targets: plan.create || !targets ? [] : plan.columns.filter((c) => !targets.some((n) => same(n, c.target))).map((c) => c.target),
  };
}

/** The keys an upsert matches, when they are no longer the table's primary key. */
export const changedKeys = (plan: ImportPlan, keys: string[]) =>
  plan.mode === "upsert" && (plan.keys ?? []).join("\u0000") !== keys.join("\u0000");

/** `tables` split into those found among `existing` names and those gone. */
export function splitTables<T extends { name: string }>(tables: T[], existing: string[]) {
  return {
    kept: tables.filter((x) => existing.includes(x.name)),
    gone: tables.filter((x) => !existing.includes(x.name)),
  };
}

/** The profile as the dialog saves it: new with `id`, or `current` changed. */
export function profileOf(name: string, task: ProfileTask, id: string): TransferProfile {
  return { id, name: name.trim(), task };
}
