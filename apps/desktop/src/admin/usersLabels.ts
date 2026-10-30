// The names of the attributes and of the objects a privilege is on, for the Users and roles
// view and its forms.
import type { PrincipalFlag, TargetClass } from "../api";
import { msg } from "../i18n";

/** What each attribute is called, and what it lets the user or role do. */
export const FLAGS: Record<PrincipalFlag, { label: string; what: string }> = {
  login: { label: msg("Can log in"), what: msg("The role can open sessions: a user.") },
  superuser: { label: msg("Superuser"), what: msg("Skips every permission check.") },
  create_db: { label: msg("Create databases"), what: msg("May create databases.") },
  create_role: { label: msg("Create roles"), what: msg("May create, change and drop other roles.") },
  inherit: { label: msg("Inherit privileges"), what: msg("Uses the privileges of the roles it belongs to without switching to them.") },
  replication: { label: msg("Replication"), what: msg("May start streaming replication.") },
  bypass_rls: { label: msg("Bypass row security"), what: msg("Skips the row level security policies of tables.") },
  locked: { label: msg("Locked"), what: msg("The account cannot log in.") },
  password_expired: { label: msg("Password expired"), what: msg("The password must be changed at the next login.") },
};

/** What each kind of object is called when a privilege is picked. */
export const TARGETS: Record<TargetClass, string> = {
  server: msg("Whole server"),
  database: msg("Database"),
  schema: msg("Schema"),
  table: msg("Table or view"),
  all_tables: msg("All tables of a schema"),
  all_routines: msg("All routines of a schema"),
};
