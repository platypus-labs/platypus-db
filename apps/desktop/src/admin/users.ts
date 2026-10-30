// Pure helpers of the admin tab's Users and roles view: the list's filter, the privileges
// grouped by object, and the changes the forms build. Types only from the app: the unit
// tests load this module without it.
import type {
  GrantKind,
  GrantTarget,
  Principal,
  PrincipalFlag,
  PrincipalKind,
  PrincipalSpec,
  PrivilegeGrant,
  SecurityOptions,
  TargetClass,
} from "../api";

/** Roles every database has, which a user can belong to though the list does not show them. */
export const BUILT_IN_ROLES: Record<string, string[]> = {
  postgres: ["pg_read_all_data", "pg_write_all_data", "pg_monitor", "pg_read_all_stats", "pg_signal_backend"],
  sqlserver: [
    "db_owner",
    "db_datareader",
    "db_datawriter",
    "db_ddladmin",
    "db_securityadmin",
    "db_accessadmin",
    "db_backupoperator",
    "db_denydatareader",
    "db_denydatawriter",
  ],
};

/** What identifies a user or role in the list: MySQL has the same name on several hosts. */
export const principalKey = (p: Pick<Principal, "name" | "host">) => `${p.name}@${p.host ?? ""}`;

/** The name as the server writes an account: `name@host` on MySQL. */
export const principalLabel = (p: Pick<Principal, "name" | "host">) => (p.host ? `${p.name}@${p.host}` : p.name);

/** Whether `p` matches the filter text: its name, host or the roles it belongs to. */
export function matchesPrincipal(p: Principal, filter: string): boolean {
  const needle = filter.trim().toLowerCase();
  if (!needle) return true;
  return [p.name, p.host ?? "", p.authentication ?? "", ...p.member_of].some((text) => text.toLowerCase().includes(needle));
}

/** The users and roles that belong to the role `role`. */
export const membersOf = (principals: Principal[], role: Principal) =>
  principals.filter((p) => p.member_of.includes(role.name) && principalKey(p) !== principalKey(role));

/** Roles first, or users first: a stable order by kind and name. */
export function sortPrincipals(principals: Principal[]): Principal[] {
  return [...principals].sort(
    (a, b) => a.name.localeCompare(b.name) || (a.host ?? "").localeCompare(b.host ?? "") || a.kind.localeCompare(b.kind),
  );
}

/** The privileges on one object, the columns of a table among them. */
export interface PrivilegeGroup {
  key: string;
  kind: GrantKind;
  /** What the object is called: `public.orders`, or `*.*` for the server. */
  label: string;
  /** The schema (or database) of the object, and the object in it. */
  schema: string | null;
  object: string | null;
  /** `SELECT`, or `SELECT (id)` for a column; `grantable` ones can be passed on. */
  privileges: { name: string; grantable: boolean }[];
}

const KIND_ORDER: GrantKind[] = ["server", "database", "schema", "table", "routine", "column"];

/** The object a grant is on, written as its name. */
export function objectLabel(g: PrivilegeGrant): string {
  switch (g.kind) {
    case "server":
      return "*.*";
    case "database":
    case "schema":
      return g.schema ?? "";
    default:
      return [g.schema, g.object].filter(Boolean).join(".");
  }
}

/** Groups grants by object, server first, then databases, schemas, tables and routines; a
 *  column's privileges join the table's group. */
export function groupPrivileges(grants: PrivilegeGrant[]): PrivilegeGroup[] {
  const groups = new Map<string, PrivilegeGroup>();
  for (const g of grants) {
    // A column belongs with its table.
    const kind: GrantKind = g.kind === "column" ? "table" : g.kind;
    const label = objectLabel(g);
    const key = `${kind}:${label}`;
    let group = groups.get(key);
    if (!group) {
      group = { key, kind, label, schema: g.schema, object: g.object, privileges: [] };
      groups.set(key, group);
    }
    const name = g.column ? `${g.privilege} (${g.column})` : g.privilege;
    if (!group.privileges.some((p) => p.name === name)) group.privileges.push({ name, grantable: g.grantable });
  }
  return [...groups.values()].sort(
    (a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind) || a.label.localeCompare(b.label),
  );
}

/** The kinds of object a privilege can be on, in the order to offer them. */
export const TARGET_ORDER: TargetClass[] = ["server", "database", "schema", "table", "all_tables", "all_routines"];

/** The kinds of object the database has privileges for, in the order to offer them. */
export function offeredTargets(options: SecurityOptions): TargetClass[] {
  return TARGET_ORDER.filter(
    (c) => (options.targets.find((t) => t.target === c)?.privileges.length ?? 0) > 0,
  );
}

/** The privileges a grant on `target` can name. */
export const privilegesOn = (options: SecurityOptions, target: TargetClass): string[] =>
  options.targets.find((t) => t.target === target)?.privileges ?? [];

/** The choice of object that takes back what `group` holds; `null` for a routine, which
 *  needs its signature. */
export function choiceOf(group: PrivilegeGroup): TargetChoice | null {
  const schema = group.schema ?? "";
  switch (group.kind) {
    case "server":
      return { class: "server", schema: "", table: "" };
    case "database":
      return { class: "database", schema, table: "" };
    case "schema":
      return { class: "schema", schema, table: "" };
    case "table":
      return group.object ? { class: "table", schema, table: group.object } : null;
    default:
      return null;
  }
}

/** What the user picked for the object of a grant. */
export interface TargetChoice {
  class: TargetClass;
  /** A database (or MySQL schema), or the schema of the others. */
  schema: string;
  /** The table. */
  table: string;
}

/** The target a choice names; `null` while the choice is incomplete. */
export function grantTarget(choice: TargetChoice): GrantTarget | null {
  const { schema, table } = choice;
  switch (choice.class) {
    case "server":
      return { kind: "server" };
    case "database":
      return { kind: "database", name: schema.trim() };
    case "schema":
      return schema.trim() ? { kind: "schema", name: schema.trim() } : null;
    case "table":
      return schema.trim() && table.trim() ? { kind: "table", schema: schema.trim(), name: table.trim() } : null;
    case "all_tables":
      return schema.trim() ? { kind: "all_tables", schema: schema.trim() } : null;
    case "all_routines":
      return schema.trim() ? { kind: "all_routines", schema: schema.trim() } : null;
  }
}

/** Reads a number the user typed: `null` for empty, `NaN` for what is not a whole number of
 *  at least zero. */
export function parseLimit(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) return null;
  return /^\d+$/.test(trimmed) ? Number(trimmed) : Number.NaN;
}

/** What the form of a principal holds. */
export interface PrincipalForm {
  name: string;
  host: string;
  flags: PrincipalFlag[];
  limit: string;
  validUntil: string;
  password: string;
}

/** The attributes of `now` that were off in `before`, and those that were on and are off
 *  now, among `known`. */
export function flagChanges(before: PrincipalFlag[], now: PrincipalFlag[], known: PrincipalFlag[]) {
  return {
    on: known.filter((f) => now.includes(f) && !before.includes(f)),
    off: known.filter((f) => !now.includes(f) && before.includes(f)),
  };
}

/** The user or role to create from the form. */
export function createSpec(kind: PrincipalKind, form: PrincipalForm, known: PrincipalFlag[]): PrincipalSpec {
  const limit = parseLimit(form.limit);
  return {
    name: form.name.trim(),
    host: kind === "user" && form.host.trim() ? form.host.trim() : null,
    kind,
    // What is unchecked is stated too: a database's default may be on.
    flags_on: known.filter((f) => form.flags.includes(f)),
    flags_off: known.filter((f) => !form.flags.includes(f)),
    connection_limit: limit === null || Number.isNaN(limit) ? null : limit,
    valid_until: form.validUntil.trim() || null,
    password: form.password || null,
  };
}

/** The change from `p` to the form: only what differs. */
export function alterSpec(p: Principal, form: PrincipalForm, known: PrincipalFlag[]): PrincipalSpec {
  const { on, off } = flagChanges(p.flags, form.flags, known);
  const limit = parseLimit(form.limit);
  const limitNow = limit === null || Number.isNaN(limit) ? null : limit;
  const validNow = form.validUntil.trim();
  const name = form.name.trim();
  return {
    name: p.name,
    host: p.host,
    kind: p.kind,
    flags_on: on,
    flags_off: off,
    // Emptying the field removes the limit or the date.
    connection_limit: limitNow === (p.connection_limit ?? null) ? null : (limitNow ?? -1),
    valid_until: validNow === (p.valid_until ?? "") ? null : validNow,
    password: form.password || null,
    rename_to: name && name !== p.name ? name : null,
    login: p.login ?? null,
  };
}

/** What stops the form of a principal from running, if anything. */
export type FormProblem = "name" | "limit" | "password";

/** Why the form cannot be run: it has no name, its limit is no number, or the two passwords
 *  differ. */
export function formProblem(form: PrincipalForm, repeat: string): FormProblem | null {
  if (!form.name.trim()) return "name";
  if (Number.isNaN(parseLimit(form.limit))) return "limit";
  if (form.password !== repeat) return "password";
  return null;
}
