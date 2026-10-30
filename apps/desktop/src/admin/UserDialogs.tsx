/** The dialogs of the Users and roles view: create or edit a user or role, drop one, grant or
 *  revoke privileges, and add to a role or take out of it. Each shows the statements the change
 *  runs (a password masked) before it does, and runs them on the navigator's session. */
import { type ReactNode, useEffect, useState } from "react";
import {
  api,
  type DriverKind,
  type GrantTarget,
  type ObjectMeta,
  type Principal,
  type PrincipalFlag,
  type PrincipalKind,
  type SavedConnection,
  type SecurityChange,
  type SecurityOptions,
  type TargetClass,
  toCommandError,
} from "../api";
import { msg, t } from "../i18n";
import { workbench } from "../workbench";
import { FLAGS, TARGETS } from "./usersLabels";
import {
  alterSpec,
  BUILT_IN_ROLES,
  createSpec,
  formProblem,
  type FormProblem,
  grantTarget,
  offeredTargets,
  type PrincipalForm,
  principalLabel,
  privilegesOn,
  type TargetChoice,
} from "./usersModel";

/** What every dialog needs from the view that opens it. */
export interface DialogEnv {
  connection: SavedConnection;
  /** The navigator's session, which the statements run on. */
  sideSession: () => Promise<number | null>;
  options: SecurityOptions;
  principals: Principal[];
  /** Called after the statements ran. */
  onDone: () => void;
  onClose: () => void;
}

/** How long typing pauses before the statements are written again. */
const TYPING_PAUSE_MS = 250;

const PROBLEMS: Record<FormProblem, string> = {
  name: msg("Type a name."),
  limit: msg("The connection limit is a whole number."),
  password: msg("The two passwords differ."),
};

interface SecurityDialogProps {
  env: DialogEnv;
  title: string;
  description: string;
  /** The primary button. */
  confirm: string;
  /** The change destroys something: the primary button says so. */
  danger?: boolean;
  /** The change the form describes; `null` while it is incomplete. */
  change: SecurityChange | null;
  /** Says why `change` is `null`, when the form can tell. */
  problem?: string | null;
  /** The change holds a password, so it must not be handed to the editor. */
  secret?: boolean;
  children: ReactNode;
}

/** The frame of the dialogs: the form, the statements it makes, and the button that runs
 *  them. */
function SecurityDialog({ env, title, description, confirm, danger, change, problem, secret, children }: SecurityDialogProps) {
  const { connection } = env;
  const key = change ? JSON.stringify(change) : null;
  /** The statements written for the change with this key, or why they could not be. */
  const [preview, setPreview] = useState<{ key: string; statements: string[] | null; error: string | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  useEffect(() => {
    if (!change || !key) return;
    let current = true;
    const timer = window.setTimeout(async () => {
      try {
        const sid = await env.sideSession();
        if (sid === null || !current) return;
        const statements = await api.securityPreview(sid, change);
        if (current) setPreview({ key, statements, error: null });
      } catch (e) {
        if (current) setPreview({ key, statements: null, error: toCommandError(e).message });
      }
    }, TYPING_PAUSE_MS);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [key]);

  // Only the statements of what the form holds now run: those of an earlier keystroke do not.
  const fresh = preview && preview.key === key ? preview : null;
  const statements = fresh?.statements ?? null;
  const runnable = !!change && !!statements && !running && !connection.read_only;

  async function run() {
    if (!change || !runnable) return;
    setRunning(true);
    setError(null);
    try {
      const sid = await env.sideSession();
      if (sid === null) return;
      await api.applySecurity(sid, change);
      env.onDone();
      env.onClose();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setRunning(false);
    }
  }

  const shown = statements ? `${statements.join(";\n")};` : (problem ? t(problem) : "…");
  return (
    <div className="modal-backdrop" onMouseDown={env.onClose}>
      <form
        className="modal sql-action security-dialog"
        data-dialog="security"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && env.onClose()}
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <h2>{title}</h2>
        <p className="muted small">{description}</p>
        {children}
        <pre className="sql-preview" data-field="preview">
          {shown}
        </pre>
        {fresh?.error && (
          <div className="notice error small" data-field="preview-error">
            {fresh.error}
          </div>
        )}
        {connection.read_only && (
          <div className="notice warn small">{t("{name} is read-only: this statement cannot be run.", { name: connection.name })}</div>
        )}
        {error && (
          <div className="notice error small" data-field="run-error">
            {error}
          </div>
        )}
        <div className="actions">
          <button
            type="button"
            className="link"
            disabled={!statements || secret}
            title={secret ? t("The statements hold a masked password: they cannot be edited and run.") : undefined}
            onClick={() => {
              if (statements) openInEditor(env, statements);
            }}
          >
            {t("Open in editor")}
          </button>
          <span className="spacer" />
          <button type="button" onClick={env.onClose}>
            {t("Cancel")}
          </button>
          <button type="submit" className={danger ? "primary danger" : "primary"} disabled={!runnable} data-action="run">
            {running ? t("Running…") : confirm}
          </button>
        </div>
      </form>
    </div>
  );
}

function openInEditor(env: DialogEnv, statements: string[]) {
  workbench().openTab("query", env.connection.id, { sql: `${statements.join(";\n")};` });
  env.onClose();
}

/** A text field of a form. */
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="field">
      <span>{label}</span>
      {children}
    </label>
  );
}

const DRIVERS_WITH_LIMIT: DriverKind[] = ["postgres", "mysql"];
const DRIVERS_WITH_EXPIRY: DriverKind[] = ["postgres", "clickhouse"];

/** The attributes the form shows for `kind` on `driver`: roles of MySQL and SQL Server have
 *  none. */
function shownFlags(options: SecurityOptions, driver: DriverKind, kind: PrincipalKind): PrincipalFlag[] {
  return kind === "user" || driver === "postgres" ? options.flags : [];
}

/** The form of a user or role, to create or, with `existing`, to change. */
export function PrincipalDialog({ env, kind, existing }: { env: DialogEnv; kind: PrincipalKind; existing?: Principal }) {
  const driver = env.connection.driver;
  const known = env.options.flags;
  const [form, setForm] = useState<PrincipalForm>(() =>
    existing
      ? {
          name: existing.name,
          host: existing.host ?? "",
          flags: existing.flags,
          limit: existing.connection_limit === null ? "" : String(existing.connection_limit),
          validUntil: existing.valid_until ?? "",
          password: "",
        }
      : {
          name: "",
          host: driver === "mysql" ? "%" : "",
          // A PostgreSQL role inherits unless told not to; a user logs in.
          flags: [...(driver === "postgres" ? (["inherit"] as PrincipalFlag[]) : []), ...(kind === "user" && driver === "postgres" ? (["login"] as PrincipalFlag[]) : [])],
          limit: "",
          validUntil: "",
          password: "",
        },
  );
  const [repeat, setRepeat] = useState("");
  const set = (patch: Partial<PrincipalForm>) => setForm((f) => ({ ...f, ...patch }));
  const toggle = (flag: PrincipalFlag, on: boolean) =>
    set({ flags: on ? [...form.flags, flag] : form.flags.filter((f) => f !== flag) });
  const problem = formProblem(form, repeat);
  const change: SecurityChange | null = problem
    ? null
    : existing
      ? { change: "alter", spec: alterSpec(existing, form, known) }
      : { change: "create", spec: createSpec(kind, form, known) };
  const word = kind === "user" ? t("user") : t("role");
  const editing = !!existing;
  return (
    <SecurityDialog
      env={env}
      title={existing ? t("Edit {name}", { name: principalLabel(existing) }) : kind === "user" ? t("New user") : t("New role")}
      description={
        editing
          ? t("Changes the attributes of the {kind}; only what differs is written.", { kind: word })
          : t("Creates the {kind} on the server.", { kind: word })
      }
      confirm={editing ? t("Save") : t("Create")}
      change={change}
      problem={problem ? PROBLEMS[problem] : null}
      secret={!!form.password}
    >
      <Field label={t("Name")}>
        <input autoFocus value={form.name} data-field="name" onChange={(e) => set({ name: e.target.value })} />
      </Field>
      {driver === "mysql" && kind === "user" && (
        <Field label={t("Host")}>
          <input value={form.host} data-field="host" disabled={editing} onChange={(e) => set({ host: e.target.value })} />
        </Field>
      )}
      {kind === "user" && (
        <div className="row">
          <Field label={editing ? t("New password (empty keeps it)") : t("Password")}>
            <input
              type="password"
              autoComplete="new-password"
              value={form.password}
              data-field="password"
              onChange={(e) => set({ password: e.target.value })}
            />
          </Field>
          <Field label={t("Repeat the password")}>
            <input type="password" autoComplete="new-password" value={repeat} data-field="repeat" onChange={(e) => setRepeat(e.target.value)} />
          </Field>
        </div>
      )}
      <div className="row">
        {DRIVERS_WITH_LIMIT.includes(driver) && (
          <Field label={t("Connection limit")}>
            <input value={form.limit} placeholder={t("No limit")} data-field="limit" onChange={(e) => set({ limit: e.target.value })} />
          </Field>
        )}
        {DRIVERS_WITH_EXPIRY.includes(driver) && kind === "user" && (
          <Field label={t("Valid until")}>
            <input
              value={form.validUntil}
              placeholder="2030-12-31 23:59:59"
              data-field="valid-until"
              onChange={(e) => set({ validUntil: e.target.value })}
            />
          </Field>
        )}
      </div>
      <div className="security-flags" role="group" aria-label={t("Attributes")}>
        {shownFlags(env.options, driver, kind).map((flag) => (
          <label key={flag} className="check" title={t(FLAGS[flag].what)}>
            <input
              type="checkbox"
              checked={form.flags.includes(flag)}
              data-flag={flag}
              onChange={(e) => toggle(flag, e.target.checked)}
            />
            <span>{t(FLAGS[flag].label)}</span>
          </label>
        ))}
      </div>
    </SecurityDialog>
  );
}

/** Drops a user or role, with what PostgreSQL and SQL Server need said first. */
export function DropDialog({ env, principal }: { env: DialogEnv; principal: Principal }) {
  const driver = env.connection.driver;
  const [reassign, setReassign] = useState("");
  const [dropOwned, setDropOwned] = useState(false);
  const [dropLogin, setDropLogin] = useState(driver === "sqlserver" && principal.kind === "user" && !!principal.login);
  const others = env.principals.filter((p) => p.name !== principal.name);
  const change: SecurityChange = {
    change: "drop",
    name: principal.name,
    host: principal.host,
    kind: principal.kind,
    options: { reassign_to: reassign || null, drop_owned: dropOwned, drop_login: dropLogin, login: principal.login ?? null },
  };
  return (
    <SecurityDialog
      env={env}
      title={t("Drop {name}", { name: principalLabel(principal) })}
      description={t("Removes the {kind} and the privileges it was given. It cannot be undone.", {
        kind: principal.kind === "user" ? t("user") : t("role"),
      })}
      confirm={t("Drop")}
      danger
      change={change}
    >
      {driver === "postgres" && (
        <>
          <div className="notice warn small">
            {t("PostgreSQL refuses to drop a role that owns objects or holds privileges in a database. Hand them over first, or drop them with the role.")}
          </div>
          <Field label={t("Give what it owns to")}>
            <select value={reassign} data-field="reassign" onChange={(e) => setReassign(e.target.value)}>
              <option value="">{t("(nobody)")}</option>
              {others.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </select>
          </Field>
          <label className="check">
            <input type="checkbox" checked={dropOwned} data-field="drop-owned" onChange={(e) => setDropOwned(e.target.checked)} />
            <span>{t("Also drop what it owns in this database and its privileges (DROP OWNED)")}</span>
          </label>
        </>
      )}
      {driver === "sqlserver" && principal.kind === "user" && principal.login && (
        <label className="check">
          <input type="checkbox" checked={dropLogin} data-field="drop-login" onChange={(e) => setDropLogin(e.target.checked)} />
          <span>{t("Also drop its server login")}</span>
        </label>
      )}
      {driver !== "postgres" && (
        <p className="muted small">{t("Objects it owns are not touched.")}</p>
      )}
    </SecurityDialog>
  );
}

/** Where a grant or revoke starts: the object and privileges of a grant already given. */
export interface PrivilegePreset {
  action: "grant" | "revoke";
  choice: TargetChoice;
  privileges: string[];
}

const TABLE_KINDS = new Set(["table", "view", "materialized_view", "foreign_table"]);

/** Grants privileges to a user or role, or takes them back. */
export function PrivilegesDialog({ env, principal, preset }: { env: DialogEnv; principal: Principal; preset?: PrivilegePreset }) {
  const { connection, options } = env;
  const driver = connection.driver;
  const classes = offeredTargets(options);
  const [action, setAction] = useState<"grant" | "revoke">(preset?.action ?? "grant");
  const [choice, setChoice] = useState<TargetChoice>(
    preset?.choice ?? { class: classes.includes("table") ? "table" : (classes[0] ?? "table"), schema: "", table: "" },
  );
  const [chosen, setChosen] = useState<string[]>(preset?.privileges ?? []);
  const [grantOption, setGrantOption] = useState(false);
  const [cascade, setCascade] = useState(false);
  const [schemas, setSchemas] = useState<string[]>([]);
  const [tables, setTables] = useState<string[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);

  // The schemas of the catalog, to pick the object from.
  useEffect(() => {
    void (async () => {
      try {
        const sid = await env.sideSession();
        if (sid === null) return;
        const list = await api.listSchemas(sid);
        setSchemas(list.filter((s) => !s.system || s.current).map((s) => s.name));
        setChoice((c) => (c.schema || preset ? c : { ...c, schema: list.find((s) => s.current)?.name ?? list[0]?.name ?? "" }));
      } catch (e) {
        setLoadError(toCommandError(e).message);
      }
    })();
  }, []);

  // The tables of the chosen schema.
  useEffect(() => {
    if (choice.class !== "table" || !choice.schema) return;
    let current = true;
    void (async () => {
      try {
        const sid = await env.sideSession();
        if (sid === null) return;
        const objects: ObjectMeta[] = await api.listObjects(sid, choice.schema);
        if (!current) return;
        const names = objects.filter((o) => TABLE_KINDS.has(o.kind)).map((o) => o.name);
        setTables(names);
        setChoice((c) => (c.table && names.includes(c.table) ? c : { ...c, table: c.table || (names[0] ?? "") }));
      } catch (e) {
        if (current) setLoadError(toCommandError(e).message);
      }
    })();
    return () => {
      current = false;
    };
  }, [choice.class, choice.schema]);

  const available = privilegesOn(options, choice.class);
  const target: GrantTarget | null = grantTarget(choice);
  const picked = chosen.filter((p) => available.includes(p));
  // SQL Server's database is the one the session is in: it needs no name.
  const unnamed = choice.class === "database" && driver !== "sqlserver" && !choice.schema.trim();
  const problem =
    !target || unnamed ? msg("Choose the object.") : picked.length === 0 ? msg("Choose at least one privilege.") : null;
  const change: SecurityChange | null =
    problem || !target
      ? null
      : {
          change: "privileges",
          action,
          grantees: [{ name: principal.name, host: principal.host }],
          target,
          privileges: picked,
          grant_option: grantOption,
          cascade,
        };
  const revoke = action === "revoke";
  const toggle = (name: string, on: boolean) => setChosen(on ? [...chosen, name] : chosen.filter((p) => p !== name));
  // PostgreSQL's database is one name, the connection's to start with.
  const setClass = (c: TargetClass) =>
    setChoice((old) => ({
      ...old,
      class: c,
      table: "",
      schema: c === "database" && driver === "postgres" ? connection.database : old.schema,
    }));

  return (
    <SecurityDialog
      env={env}
      title={t("Privileges of {name}", { name: principalLabel(principal) })}
      description={t("Grant privileges on an object, or take them back. Only what the {kind} was given itself is changed, not what it has through its roles.", {
        kind: principal.kind === "user" ? t("user") : t("role"),
      })}
      confirm={revoke ? t("Revoke") : t("Grant")}
      danger={revoke}
      change={change}
      problem={problem}
    >
      <div className="row">
        <Field label={t("Action")}>
          <select value={action} data-field="action" onChange={(e) => setAction(e.target.value as "grant" | "revoke")}>
            <option value="grant">{t("Grant")}</option>
            <option value="revoke">{t("Revoke")}</option>
          </select>
        </Field>
        <Field label={t("On")}>
          <select value={choice.class} data-field="target" onChange={(e) => setClass(e.target.value as TargetClass)}>
            {classes.map((c) => (
              <option key={c} value={c}>
                {t(TARGETS[c])}
              </option>
            ))}
          </select>
        </Field>
      </div>
      {loadError && <div className="notice error small">{loadError}</div>}
      <TargetPicker
        choice={choice}
        driver={driver}
        schemas={schemas}
        tables={tables}
        onChange={setChoice}
      />
      <div className="security-privileges" role="group" aria-label={t("Privileges")} data-field="privileges">
        {available.map((p) => (
          <label key={p} className="check">
            <input type="checkbox" checked={picked.includes(p)} data-privilege={p} onChange={(e) => toggle(p, e.target.checked)} />
            <span>{p}</span>
          </label>
        ))}
      </div>
      {!(revoke && driver === "clickhouse") && (
        <label className="check">
          <input type="checkbox" checked={grantOption} data-field="grant-option" onChange={(e) => setGrantOption(e.target.checked)} />
          <span>{revoke ? t("Take away only the right to grant them (GRANT OPTION)") : t("With grant option: it may grant them to others")}</span>
        </label>
      )}
      {revoke && (driver === "postgres" || driver === "sqlserver") && (
        <label className="check">
          <input type="checkbox" checked={cascade} data-field="cascade" onChange={(e) => setCascade(e.target.checked)} />
          <span>{t("CASCADE: also revoke what others got through it")}</span>
        </label>
      )}
    </SecurityDialog>
  );
}

/** The schema and table of a grant, picked from the catalog; or a name typed, where the
 *  catalog has none (a PostgreSQL database). */
function TargetPicker({
  choice,
  driver,
  schemas,
  tables,
  onChange,
}: {
  choice: TargetChoice;
  driver: DriverKind;
  schemas: string[];
  tables: string[];
  onChange: (c: TargetChoice) => void;
}) {
  if (choice.class === "server") return null;
  if (choice.class === "database" && driver === "sqlserver") {
    return <p className="muted small">{t("On the database this connection is in.")}</p>;
  }
  const label = choice.class === "database" ? t("Database") : t("Schema");
  // MySQL's and ClickHouse's databases are the catalog's schemas; PostgreSQL's database is one name.
  const typed = choice.class === "database" && driver === "postgres";
  return (
    <div className="row">
      <Field label={label}>
        {typed ? (
          <input value={choice.schema} data-field="schema" onChange={(e) => onChange({ ...choice, schema: e.target.value })} />
        ) : (
          <select value={choice.schema} data-field="schema" onChange={(e) => onChange({ ...choice, schema: e.target.value, table: "" })}>
            {!schemas.includes(choice.schema) && <option value={choice.schema}>{choice.schema}</option>}
            {schemas.map((s) => (
              <option key={s} value={s}>
                {s}
              </option>
            ))}
          </select>
        )}
      </Field>
      {choice.class === "table" && (
        <Field label={t("Table or view")}>
          <select value={choice.table} data-field="table" onChange={(e) => onChange({ ...choice, table: e.target.value })}>
            {!tables.includes(choice.table) && <option value={choice.table}>{choice.table}</option>}
            {tables.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </Field>
      )}
    </div>
  );
}

/** Puts a user or role in a role, or takes it out. */
export function MembershipDialog({
  env,
  principal,
  preset,
}: {
  env: DialogEnv;
  principal: Principal;
  preset?: { action: "grant" | "revoke"; role: string };
}) {
  const driver = env.connection.driver;
  const [action, setAction] = useState<"grant" | "revoke">(preset?.action ?? "grant");
  const [role, setRole] = useState(preset?.role ?? "");
  const [admin, setAdmin] = useState(false);
  const roles = [
    ...new Set([
      ...env.principals.filter((p) => p.kind === "role" && p.name !== principal.name).map((p) => p.name),
      ...(BUILT_IN_ROLES[driver] ?? []),
    ]),
  ].sort();
  // SQL Server has no admin option, and MySQL cannot revoke it alone.
  const adminKnown = driver !== "sqlserver" && !(driver === "mysql" && action === "revoke");
  const change: SecurityChange | null = role.trim()
    ? {
        change: "membership",
        action,
        role: role.trim(),
        members: [{ name: principal.name, host: principal.host }],
        admin_option: adminKnown && admin,
      }
    : null;
  const revoke = action === "revoke";
  return (
    <SecurityDialog
      env={env}
      title={t("Roles of {name}", { name: principalLabel(principal) })}
      description={t("Add it to a role to give it the role's privileges, or take it out of one.")}
      confirm={revoke ? t("Take out") : t("Add")}
      danger={revoke}
      change={change}
      problem={msg("Type the role.")}
    >
      <div className="row">
        <Field label={t("Action")}>
          <select value={action} data-field="action" onChange={(e) => setAction(e.target.value as "grant" | "revoke")}>
            <option value="grant">{t("Add to a role")}</option>
            <option value="revoke">{t("Take out of a role")}</option>
          </select>
        </Field>
        <Field label={t("Role")}>
          <input list="security-roles" autoFocus value={role} data-field="role" onChange={(e) => setRole(e.target.value)} />
          <datalist id="security-roles">
            {(revoke ? principal.member_of : roles).map((r) => (
              <option key={r} value={r} />
            ))}
          </datalist>
        </Field>
      </div>
      {adminKnown && (
        <label className="check">
          <input type="checkbox" checked={admin} data-field="admin-option" onChange={(e) => setAdmin(e.target.checked)} />
          <span>{revoke ? t("Take away only the right to add others (ADMIN OPTION)") : t("With admin option: it may add others to the role")}</span>
        </label>
      )}
    </SecurityDialog>
  );
}
