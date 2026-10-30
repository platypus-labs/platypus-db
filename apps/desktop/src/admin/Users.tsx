/** The admin tab's Users and roles view: the users and roles of the server with their
 *  attributes, memberships and privileges, and the dialogs that create, change and drop them
 *  and grant and revoke privileges. */
import { createElement, type ReactNode, useEffect, useRef, useState } from "react";
import { api, type Principal, type PrivilegeGrant, type SavedConnection, type SecurityOptions, toCommandError } from "../api";
import { msg, t } from "../i18n";
import {
  DropDialog,
  type DialogEnv,
  MembershipDialog,
  PrincipalDialog,
  type PrivilegePreset,
  PrivilegesDialog,
} from "./UserDialogs";
import { FLAGS } from "./usersLabels";
import {
  choiceOf,
  groupPrivileges,
  matchesPrincipal,
  membersOf,
  type PrivilegeGroup,
  principalKey,
  principalLabel,
  sortPrincipals,
} from "./usersModel";

type Kind = "all" | "user" | "role";

/** The dialog that is open. */
type Dialog =
  | { kind: "new"; of: "user" | "role" }
  | { kind: "edit"; principal: Principal }
  | { kind: "drop"; principal: Principal }
  | { kind: "privileges"; principal: Principal; preset?: PrivilegePreset }
  | { kind: "membership"; principal: Principal; preset?: { action: "grant" | "revoke"; role: string } };

interface Props {
  connection: SavedConnection;
  active: boolean;
  /** Counts the refreshes asked from outside: the tab's button. */
  refresh: number;
  sideSession: () => Promise<number | null>;
}

export function Users({ connection, active, refresh, sideSession }: Props) {
  const [principals, setPrincipals] = useState<Principal[] | null>(null);
  const [options, setOptions] = useState<SecurityOptions | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");
  const [only, setOnly] = useState<Kind>("all");
  const [selected, setSelected] = useState<string | null>(null);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  /** Counts the reads of the list this view asked for itself, after a change. */
  const [reads, setReads] = useState(0);
  const chosen = principals?.find((p) => principalKey(p) === selected) ?? null;

  useEffect(() => {
    if (!active) return;
    let current = true;
    setLoading(true);
    void (async () => {
      try {
        const sid = await sideSession();
        if (sid === null || !current) return;
        const [list, offered] = await Promise.all([api.listPrincipals(sid), options ?? api.securityOptions(sid)]);
        if (!current) return;
        setPrincipals(sortPrincipals(list));
        setOptions(offered);
        setError(null);
      } catch (e) {
        if (current) setError(toCommandError(e).message);
      } finally {
        if (current) setLoading(false);
      }
    })();
    return () => {
      current = false;
    };
  }, [active, refresh, reads]);

  const all = principals ?? [];
  const counts = { all: all.length, user: all.filter((p) => p.kind === "user").length, role: all.filter((p) => p.kind === "role").length };
  const shown = all.filter((p) => (only === "all" || p.kind === only) && matchesPrincipal(p, filter));
  const why = connection.read_only ? t("The connection is read-only") : undefined;

  const env: DialogEnv | null = options
    ? {
        connection,
        sideSession,
        options,
        principals: all,
        onDone: () => setReads((n) => n + 1),
        onClose: () => setDialog(null),
      }
    : null;

  return (
    <div className="object-body admin-body admin-users" data-view="users">
      <div className="admin-toolbar">
        <div className="admin-filters" role="group" aria-label={t("Show")}>
          {(
            [
              ["all", t("all")],
              ["user", t("users")],
              ["role", t("roles")],
            ] as const
          ).map(([kind, text]) => (
            <button
              key={kind}
              className={`admin-filter${only === kind ? " active" : ""}`}
              data-kind={kind}
              onClick={() => setOnly(kind)}
            >
              <strong>{counts[kind]}</strong> {text}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <input
          type="search"
          placeholder={t("Filter by name, host or role")}
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button disabled={!options || !!why} title={why} data-action="new-user" onClick={() => setDialog({ kind: "new", of: "user" })}>
          {t("New user…")}
        </button>
        {options?.roles !== false && (
          <button disabled={!options || !!why} title={why} data-action="new-role" onClick={() => setDialog({ kind: "new", of: "role" })}>
            {t("New role…")}
          </button>
        )}
      </div>
      {error && (
        <div className="notice error small" data-field="users-error">
          {error}
        </div>
      )}
      <div className="admin-split">
        <PrincipalTable principals={shown} selected={selected} onSelect={setSelected} loading={loading && !principals} />
        <PrincipalDetails
          principal={chosen}
          principals={all}
          connection={connection}
          sideSession={sideSession}
          readOnly={!!why}
          roles={options?.roles !== false}
          version={reads + refresh}
          onSelect={setSelected}
          onOpen={setDialog}
        />
      </div>
      {env && dialog && <UserDialog dialog={dialog} env={env} />}
    </div>
  );
}

/** Renders the dialog `dialog` is. */
function UserDialog({ dialog, env }: { dialog: Dialog; env: DialogEnv }) {
  switch (dialog.kind) {
    case "new":
      return createElement(PrincipalDialog, { env, kind: dialog.of });
    case "edit":
      return createElement(PrincipalDialog, { env, kind: dialog.principal.kind, existing: dialog.principal });
    case "drop":
      return createElement(DropDialog, { env, principal: dialog.principal });
    case "privileges":
      return createElement(PrivilegesDialog, { env, principal: dialog.principal, preset: dialog.preset });
    case "membership":
      return createElement(MembershipDialog, { env, principal: dialog.principal, preset: dialog.preset });
  }
}

/** The attributes worth showing: a user logs in by definition. */
const visibleFlags = (p: Principal) => p.flags.filter((f) => f !== "login" || p.kind === "role");

function Flags({ principal }: { principal: Principal }) {
  return (
    <>
      {visibleFlags(principal).map((f) => (
        <span
          key={f}
          className={`admin-badge users-flag${f === "superuser" ? " superuser" : ""}${f === "locked" ? " waiting" : ""}`}
          title={t(FLAGS[f].what)}
        >
          {t(FLAGS[f].label)}
        </span>
      ))}
    </>
  );
}

function PrincipalTable({
  principals,
  selected,
  onSelect,
  loading,
}: {
  principals: Principal[];
  selected: string | null;
  onSelect: (key: string) => void;
  loading: boolean;
}) {
  return (
    <div className="admin-list">
      <table className="meta-table admin-table admin-users-table">
        <thead>
          <tr>
            <th>{t("Name")}</th>
            <th>{t("Type")}</th>
            <th>{t("Attributes")}</th>
            <th>{t("Member of")}</th>
          </tr>
        </thead>
        <tbody>
          {principals.map((p) => (
            <tr
              key={principalKey(p)}
              data-name={p.name}
              data-kind={p.kind}
              className={`admin-session admin-principal${principalKey(p) === selected ? " selected" : ""}`}
              onClick={() => onSelect(principalKey(p))}
            >
              <td>
                {p.name}
                {p.host && <span className="muted">@{p.host}</span>}
              </td>
              <td>{p.kind === "user" ? t("user") : t("role")}</td>
              <td>
                <Flags principal={p} />
              </td>
              <td className="admin-query">{p.member_of.join(", ")}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {principals.length === 0 && (
        <p className="muted small admin-empty">{loading ? t("Reading the users and roles…") : t("No users or roles match.")}</p>
      )}
    </div>
  );
}

function PrincipalDetails({
  principal,
  principals,
  connection,
  sideSession,
  readOnly,
  roles,
  version,
  onSelect,
  onOpen,
}: {
  principal: Principal | null;
  principals: Principal[];
  connection: SavedConnection;
  sideSession: () => Promise<number | null>;
  readOnly: boolean;
  /** Whether the server has roles. */
  roles: boolean;
  /** Changes when the list was read again, so the privileges are too. */
  version: number;
  onSelect: (key: string) => void;
  onOpen: (dialog: Dialog) => void;
}) {
  const [grants, setGrants] = useState<PrivilegeGrant[] | null>(null);
  const [grantsError, setGrantsError] = useState<string | null>(null);
  const key = principal ? principalKey(principal) : null;
  const shownKey = useRef<string | null>(null);

  useEffect(() => {
    if (!principal) return;
    let current = true;
    // Another principal's privileges are not these: clear them until read.
    if (shownKey.current !== key) setGrants(null);
    shownKey.current = key;
    void (async () => {
      try {
        const sid = await sideSession();
        if (sid === null || !current) return;
        const read = await api.principalPrivileges(sid, principal.name, principal.host);
        if (!current) return;
        setGrants(read);
        setGrantsError(null);
      } catch (e) {
        if (current) {
          setGrants([]);
          setGrantsError(toCommandError(e).message);
        }
      }
    })();
    return () => {
      current = false;
    };
  }, [key, version]);

  if (!principal) {
    return (
      <aside className="admin-details empty">
        <p className="muted small">{t("Select a user or role to see its attributes, roles and privileges.")}</p>
      </aside>
    );
  }
  const why = readOnly ? t("The connection is read-only") : undefined;
  const fields: [string, string, ReactNode][] = [
    ["host", t("Host"), principal.host],
    ["authentication", t("Authentication"), principal.authentication],
    ["limit", t("Connection limit"), principal.connection_limit],
    ["valid-until", t("Valid until"), principal.valid_until],
  ];
  const members = principal.kind === "role" ? membersOf(principals, principal) : [];
  const groups = groupPrivileges(grants ?? []);
  const find = (name: string) => principals.find((p) => p.name === name && p.kind === "role");
  return (
    <aside className="admin-details" data-principal={principal.name}>
      <div className="admin-details-title">
        <strong>{principalLabel(principal)}</strong>
        <span className="admin-badge">{principal.kind === "user" ? t("user") : t("role")}</span>
      </div>
      <dl className="admin-fields">
        {fields
          .filter(([, , value]) => value !== null && value !== "")
          .map(([k, name, value]) => (
            <div key={k} data-field={k}>
              <dt>{name}</dt>
              <dd>{value}</dd>
            </div>
          ))}
        <div data-field="attributes">
          <dt>{t("Attributes")}</dt>
          <dd className="users-flags">
            {visibleFlags(principal).length === 0 ? <span className="muted">{t("none")}</span> : <Flags principal={principal} />}
          </dd>
        </div>
      </dl>
      <div className="admin-actions">
        <button disabled={!!why} title={why} data-action="edit" onClick={() => onOpen({ kind: "edit", principal })}>
          {t("Edit…")}
        </button>
        <button disabled={!!why} title={why} data-action="privileges" onClick={() => onOpen({ kind: "privileges", principal })}>
          {t("Privileges…")}
        </button>
        {roles && (
          <button disabled={!!why} title={why} data-action="roles" onClick={() => onOpen({ kind: "membership", principal })}>
            {t("Roles…")}
          </button>
        )}
        <button className="admin-end" disabled={!!why} title={why} data-action="drop" onClick={() => onOpen({ kind: "drop", principal })}>
          {t("Drop…")}
        </button>
      </div>
      {roles && (
        <>
          <h3 className="admin-heading">{t("Member of")}</h3>
          <MemberChips
            names={principal.member_of}
            find={find}
            onSelect={onSelect}
            onRemove={
              readOnly
                ? undefined
                : (role) => onOpen({ kind: "membership", principal, preset: { action: "revoke", role } })
            }
            empty={t("It belongs to no role.")}
            field="member-of"
          />
        </>
      )}
      {principal.kind === "role" && (
        <>
          <h3 className="admin-heading">{t("Members")}</h3>
          <MemberChips
            names={members.map((m) => m.name)}
            find={(name) => members.find((m) => m.name === name)}
            onSelect={onSelect}
            empty={t("No user or role belongs to it.")}
            field="members"
          />
        </>
      )}
      <h3 className="admin-heading">{t("Privileges")}</h3>
      {grantsError && (
        <div className="notice error small" data-field="privileges-error">
          {grantsError}
        </div>
      )}
      {grants && groups.length === 0 && !grantsError && <p className="muted small">{t("No privileges of its own.")}</p>}
      <PrivilegeGroups
        groups={groups}
        driver={connection.driver}
        onRevoke={
          readOnly
            ? undefined
            : (group) => {
                const choice = choiceOf(group);
                if (choice)
                  onOpen({
                    kind: "privileges",
                    principal,
                    preset: {
                      action: "revoke",
                      choice,
                      privileges: group.privileges.map((p) => p.name).filter((n) => !n.includes(" (")),
                    },
                  });
              }
        }
      />
    </aside>
  );
}

/** The roles a principal belongs to, or the principals that belong to a role, as buttons that
 *  select them; a role can be taken out with ×. */
function MemberChips({
  names,
  find,
  onSelect,
  onRemove,
  empty,
  field,
}: {
  names: string[];
  find: (name: string) => Principal | undefined;
  onSelect: (key: string) => void;
  onRemove?: (name: string) => void;
  empty: string;
  field: string;
}) {
  if (names.length === 0) return <p className="muted small">{empty}</p>;
  return (
    <div className="users-chips" data-field={field}>
      {names.map((name) => {
        const found = find(name);
        return (
          <span key={name} className="users-chip" data-name={name}>
            {found ? (
              <button className="link" onClick={() => onSelect(principalKey(found))}>
                {name}
              </button>
            ) : (
              <span title={t("Built in: not listed")}>{name}</span>
            )}
            {onRemove && (
              <button
                className="users-chip-remove"
                title={t("Take out of {role}", { role: name })}
                aria-label={t("Take out of {role}", { role: name })}
                onClick={() => onRemove(name)}
              >
                ×
              </button>
            )}
          </span>
        );
      })}
    </div>
  );
}

function PrivilegeGroups({
  groups,
  driver,
  onRevoke,
}: {
  groups: PrivilegeGroup[];
  driver: string;
  onRevoke?: (group: PrivilegeGroup) => void;
}) {
  if (groups.length === 0) return null;
  return (
    <table className="meta-table admin-table users-privileges" data-field="privilege-groups">
      <tbody>
        {groups.map((g) => (
          <tr key={g.key} data-object={g.label}>
            <td className="users-object">
              <span className="muted small">{t(KIND_LABEL[g.kind])}</span>
              <br />
              {g.label || (driver === "sqlserver" ? t("this database") : "")}
            </td>
            <td className="users-grants">
              {g.privileges.map((p) => (
                <span
                  key={p.name}
                  className={`admin-badge users-privilege${p.grantable ? " grantable" : ""}`}
                  title={p.grantable ? t("With grant option") : undefined}
                >
                  {p.name}
                  {p.grantable && " ⚑"}
                </span>
              ))}
            </td>
            <td>
              {onRevoke && choiceOf(g) && (
                <button className="link" data-action="revoke" onClick={() => onRevoke(g)}>
                  {t("Revoke…")}
                </button>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

const KIND_LABEL: Record<PrivilegeGroup["kind"], string> = {
  server: msg("Server"),
  database: msg("Database"),
  schema: msg("Schema"),
  table: msg("Table"),
  column: msg("Column"),
  routine: msg("Routine"),
};
