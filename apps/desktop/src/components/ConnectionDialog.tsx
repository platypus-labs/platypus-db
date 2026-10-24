import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  isEmbedded,
  toCommandError,
  type ConnectionType,
  type DriverKind,
  type NetworkProfile,
  type PasswordInput,
  type Project,
  type SavedConnection,
  type SavedInit,
  type ServerInfo,
  type SslMode,
} from "../api";
import { CONNECTION_TYPES } from "../connections";
import { toggled } from "../sets";
import { basename } from "../workspace";
import { DriverPicker } from "./DriverPicker";
import { NetworkFields, SSL_MODES, sshSecretIds, sshSecretsOf } from "./NetworkFields";
import { NetworkProfileDialog, newNetworkProfile } from "./NetworkProfileDialog";

interface Props {
  initial: SavedConnection;
  /** True when editing a connection that already exists in the store. */
  existing: boolean;
  /** Every project, to pick the connection's. */
  projects: Project[];
  /** The projects the connection is in, or (new) goes into. */
  memberOf: string[];
  onSaved: (all: SavedConnection[]) => void;
  onClose: () => void;
}

type Page = "general" | "network" | "init";

/** Where the password comes from; stored as `save_password`, `pgpass` and `password_command`. */
type PasswordSource = "keychain" | "ask" | "pgpass" | "command";

const COLORS = ["", "#2f9e44", "#1c7ed6", "#f08c00", "#d6336c", "#c92a2a"];

const DEFAULT_PORTS: Record<DriverKind, number | null> = {
  postgres: 5432,
  mysql: 3306,
  sqlite: null,
  duckdb: null,
  sqlserver: 1433,
};

/** The database files of the embedded drivers: file picker filter and example path. */
const FILES: Record<"sqlite" | "duckdb", { name: string; extensions: string[]; example: string }> = {
  sqlite: { name: "SQLite database", extensions: ["db", "sqlite", "sqlite3", "db3"], example: "C:\\data\\app.db" },
  duckdb: { name: "DuckDB database", extensions: ["duckdb", "ddb", "db"], example: "C:\\data\\app.duckdb" },
};

const EMPTY_INIT: SavedInit = { schema: null, time_zone: null, statements: "" };

const SCHEMES: Record<string, DriverKind> = {
  "postgres:": "postgres",
  "postgresql:": "postgres",
  "mysql:": "mysql",
  "mariadb:": "mysql",
  "sqlserver:": "sqlserver",
  "mssql:": "sqlserver",
};

/** Turns a `postgres://`, `mysql://` or `sqlserver://` URL into form fields. */
function applyUrl(conn: SavedConnection, input: string): SavedConnection | null {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    return null;
  }
  const driver = SCHEMES[url.protocol];
  if (!driver) return null;
  const ssl = (url.searchParams.get("sslmode") ?? url.searchParams.get("ssl-mode")) as SslMode | null;
  return {
    ...conn,
    driver,
    host: decodeURIComponent(url.hostname.replace(/^\[|\]$/g, "")) || conn.host,
    port: url.port ? Number(url.port) : DEFAULT_PORTS[driver],
    database: decodeURIComponent(url.pathname.replace(/^\//, "")),
    user: decodeURIComponent(url.username),
    ssl_mode: ssl && SSL_MODES.some((m) => m.value === ssl) ? ssl : conn.ssl_mode,
  };
}

function passwordSource(c: SavedConnection): PasswordSource {
  if (c.password_command != null) return "command";
  if (c.pgpass) return "pgpass";
  return c.save_password ? "keychain" : "ask";
}

function withPasswordSource(c: SavedConnection, source: PasswordSource): SavedConnection {
  return {
    ...c,
    save_password: source === "keychain",
    pgpass: source === "pgpass",
    password_command: source === "command" ? (c.password_command ?? "") : null,
  };
}

export function ConnectionDialog({ initial, existing, projects, memberOf, onSaved, onClose }: Props) {
  const [conn, setConn] = useState(initial);
  const [inProjects, setInProjects] = useState(() => new Set(memberOf));
  const [password, setPassword] = useState("");
  /** SSH passwords and passphrases typed, by `secret_id`. */
  const [sshTyped, setSshTyped] = useState<Record<string, string>>({});
  const [page, setPage] = useState<Page>("general");
  /** A new connection starts from the choice of the database. */
  const [picking, setPicking] = useState(!existing);
  const [savedSsh] = useState(() => sshSecretIds(existing ? initial.ssh : null));
  const [profiles, setProfiles] = useState<NetworkProfile[]>([]);
  /** The network profile being edited in its own dialog, and whether it is saved. */
  const [editing, setEditing] = useState<{ profile: NetworkProfile; existing: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<
    { ok: true; server: ServerInfo } | { ok: false; message: string } | null
  >(null);

  useEffect(() => {
    api.listNetworkProfiles().then(setProfiles, (e) => setResult({ ok: false, message: toCommandError(e).message }));
  }, []);

  const set = <K extends keyof SavedConnection>(key: K, value: SavedConnection[K]) =>
    setConn((c) => ({ ...c, [key]: value }));
  const init = conn.init ?? EMPTY_INIT;
  const setInit = <K extends keyof SavedInit>(key: K, value: SavedInit[K]) => set("init", { ...init, [key]: value });
  const source = passwordSource(conn);
  const profile = profiles.find((p) => p.id === conn.network_profile) ?? null;
  const server = !isEmbedded(conn.driver);

  // An empty password field keeps the stored one when editing.
  const passwordInput = (): PasswordInput =>
    password !== ""
      ? { action: "set", value: password }
      : existing
        ? { action: "keep" }
        : { action: "clear" };

  // A profile's SSH servers are saved with the profile, not here.
  const sshSecrets = () => sshSecretsOf(conn.network_profile ? null : conn.ssh, sshTyped, savedSsh);

  // The fields of the network page go to the profile when one is chosen.
  const ownNetwork = (c: SavedConnection): SavedConnection =>
    c.network_profile ? { ...c, ssh: null } : c;

  async function test() {
    setBusy(true);
    setResult(null);
    try {
      setResult({ ok: true, server: await api.testConnection(ownNetwork(conn), passwordInput(), sshSecrets()) });
    } catch (e) {
      setResult({ ok: false, message: toCommandError(e).message });
    } finally {
      setBusy(false);
    }
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      onSaved(await api.saveConnection(ownNetwork(conn), passwordInput(), sshSecrets(), [...inProjects]));
    } catch (err) {
      setResult({ ok: false, message: toCommandError(err).message });
      setBusy(false);
    }
  }

  function changeDriver(driver: DriverKind) {
    setConn((c) => {
      const next = {
        ...c,
        driver,
        // Keep a custom port, but follow the default one when switching drivers.
        port: c.port === DEFAULT_PORTS[c.driver] ? DEFAULT_PORTS[driver] : c.port,
        database: isEmbedded(c.driver) || isEmbedded(driver) ? "" : c.database,
      };
      // `.pgpass` is PostgreSQL's.
      return driver !== "postgres" && c.pgpass ? withPasswordSource(next, "keychain") : next;
    });
    if (isEmbedded(driver) && page === "network") setPage("general");
  }

  async function browseFile() {
    const file = FILES[conn.driver === "duckdb" ? "duckdb" : "sqlite"];
    const path = await open({
      multiple: false,
      filters: [
        { name: file.name, extensions: file.extensions },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (typeof path !== "string") return;
    setConn((c) => ({ ...c, database: path, name: c.name || basename(path) }));
  }

  function onPaste(e: React.ClipboardEvent<HTMLInputElement>) {
    const parsed = applyUrl(conn, e.clipboardData.getData("text"));
    if (parsed) {
      e.preventDefault();
      setConn({ ...parsed, name: conn.name || parsed.database || parsed.host });
      const pw = new URL(e.clipboardData.getData("text").trim()).password;
      if (pw) setPassword(decodeURIComponent(pw));
    }
  }

  const tab = (id: Page, label: string) => (
    <button type="button" role="tab" aria-selected={page === id} onClick={() => setPage(id)}>
      {label}
    </button>
  );
  const networkSet = conn.network_profile || conn.ssh || conn.ssl_client_cert;
  const initSet = !!(init.schema || init.time_zone || init.statements.trim());

  if (picking) {
    return (
      <DriverPicker
        onPick={(driver) => {
          changeDriver(driver);
          setPicking(false);
        }}
        onClose={onClose}
      />
    );
  }

  return (
    <>
      <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
        <form
          className="modal"
          onSubmit={save}
          onKeyDown={(e) => e.key === "Escape" && onClose()}
          // A required field left empty on another tab: show that tab.
          onInvalidCapture={(e) => {
            const tab = (e.target as HTMLElement).closest<HTMLElement>("[data-page]")?.dataset.page;
            if (tab === "general" || tab === "network" || tab === "init") setPage(tab);
          }}
        >
          <h2>{existing ? "Edit connection" : "New connection"}</h2>
          <div className="dialog-tabs" role="tablist">
            {tab("general", "General")}
            {server && tab("network", `Network${networkSet ? " ●" : ""}`)}
            {tab("init", `Init${initSet ? " ●" : ""}`)}
          </div>

          <div data-page="general" hidden={page !== "general"} className="dialog-page">
            <label className="field">
              <span>Name</span>
              <input
                autoFocus
                required
                value={conn.name}
                onChange={(e) => set("name", e.target.value)}
                onPaste={onPaste}
                placeholder="My database — or paste a postgres:// or mysql:// URL"
              />
            </label>

            <div className="row">
              <label className="field">
                <span>Driver</span>
                <select value={conn.driver} onChange={(e) => changeDriver(e.target.value as DriverKind)}>
                  <option value="postgres">PostgreSQL</option>
                  <option value="mysql">MySQL / MariaDB</option>
                  <option value="sqlite">SQLite</option>
                  <option value="duckdb">DuckDB</option>
                  <option value="sqlserver">SQL Server</option>
                </select>
              </label>
              <label className="field">
                <span>Type</span>
                <select
                  value={conn.connection_type ?? ""}
                  onChange={(e) => set("connection_type", (e.target.value || null) as ConnectionType | null)}
                  title="Production asks before changes and starts in manual commit"
                >
                  <option value="">None</option>
                  {CONNECTION_TYPES.map((t) => (
                    <option key={t.value} value={t.value}>
                      {t.label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>Color</span>
                <div className="swatches">
                  {COLORS.map((c) => (
                    <button
                      type="button"
                      key={c || "none"}
                      className={`swatch${(conn.color ?? "") === c ? " selected" : ""}`}
                      style={{ background: c || "transparent" }}
                      title={c || "No color"}
                      onClick={() => set("color", c || null)}
                    />
                  ))}
                </div>
              </label>
            </div>

            {!server ? (
              <label className="field">
                <span>Database file</span>
                <div className="row tight">
                  <input
                    required
                    value={conn.database}
                    onChange={(e) => set("database", e.target.value)}
                    placeholder={FILES[conn.driver === "duckdb" ? "duckdb" : "sqlite"].example}
                  />
                  <button type="button" onClick={() => void browseFile()}>
                    Browse…
                  </button>
                </div>
              </label>
            ) : (
              <>
                <div className="row">
                  <label className="field grow">
                    <span>Host</span>
                    <input required value={conn.host} onChange={(e) => set("host", e.target.value)} />
                  </label>
                  <label className="field port">
                    <span>Port</span>
                    <input
                      type="number"
                      min={1}
                      max={65535}
                      value={conn.port ?? ""}
                      onChange={(e) => set("port", e.target.value ? Number(e.target.value) : null)}
                    />
                  </label>
                </div>

                <label className="field">
                  <span>Database{conn.driver === "mysql" || conn.driver === "sqlserver" ? " (optional)" : ""}</span>
                  <input value={conn.database} onChange={(e) => set("database", e.target.value)} />
                </label>

                <div className="row">
                  <label className="field grow">
                    <span>User</span>
                    <input value={conn.user} onChange={(e) => set("user", e.target.value)} />
                  </label>
                  <label className="field grow">
                    <span>Password from</span>
                    <select
                      value={source}
                      onChange={(e) => setConn((c) => withPasswordSource(c, e.target.value as PasswordSource))}
                      data-field="password_source"
                    >
                      <option value="keychain">Saved in the keychain</option>
                      <option value="ask">Ask every time</option>
                      {conn.driver === "postgres" && <option value="pgpass">.pgpass file</option>}
                      <option value="command">Shell command</option>
                    </select>
                  </label>
                </div>

                {source === "command" ? (
                  <label className="field">
                    <span>Command that prints the password</span>
                    <input
                      required
                      value={conn.password_command ?? ""}
                      onChange={(e) => set("password_command", e.target.value)}
                      placeholder="op read op://Work/prod-db/password"
                      data-field="password_command"
                    />
                  </label>
                ) : (
                  source !== "pgpass" && (
                    <label className="field">
                      <span>{source === "ask" ? "Password (only to test the connection)" : "Password"}</span>
                      <input
                        type="password"
                        value={password}
                        placeholder={existing && conn.save_password ? "•••••• (saved)" : ""}
                        onChange={(e) => setPassword(e.target.value)}
                        autoComplete="off"
                      />
                    </label>
                  )
                )}
              </>
            )}

            <label className="check">
              <input type="checkbox" checked={conn.read_only} onChange={(e) => set("read_only", e.target.checked)} />
              Read-only (warn before modifying data)
            </label>

            {projects.length > 0 && (
              <div className="field">
                <span>Projects</span>
                <div className="project-chips">
                  {projects.map((p) => {
                    const on = inProjects.has(p.id);
                    return (
                      <button
                        type="button"
                        key={p.id}
                        className={`project-chip${on ? " selected" : ""}`}
                        aria-pressed={on}
                        onClick={() => setInProjects((all) => toggled(all, p.id))}
                      >
                        <span className="dot" style={{ background: p.color }} />
                        {p.name}
                      </button>
                    );
                  })}
                </div>
              </div>
            )}
          </div>

          {server && (
            <div data-page="network" hidden={page !== "network"} className="dialog-page">
              <label className="field">
                <span>Network profile</span>
                <div className="row tight">
                  <select
                    value={conn.network_profile ?? ""}
                    onChange={(e) => set("network_profile", e.target.value || null)}
                    data-field="network_profile"
                  >
                    <option value="">None — this connection's own settings</option>
                    {profiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  {profile && (
                    <button type="button" onClick={() => setEditing({ profile, existing: true })}>
                      Edit…
                    </button>
                  )}
                  <button type="button" onClick={() => setEditing({ profile: newNetworkProfile(), existing: false })}>
                    New…
                  </button>
                </div>
              </label>
              {profile ? (
                <p className="muted small" data-field="profile_summary">
                  {profile.ssh ? `SSH through ${profile.ssh.user ? `${profile.ssh.user}@` : ""}${profile.ssh.host}` : "No SSH tunnel"}
                  {" · SSL "}
                  {profile.ssl_mode}
                  {profile.ssl_client_cert ? " with a client certificate" : ""}
                </p>
              ) : (
                <NetworkFields
                  value={conn}
                  onChange={setConn}
                  driver={conn.driver}
                  typed={sshTyped}
                  onTyped={(id, v) => setSshTyped((t) => ({ ...t, [id]: v }))}
                  saved={savedSsh}
                />
              )}
            </div>
          )}

          <div data-page="init" hidden={page !== "init"} className="dialog-page">
            <p className="muted small">Runs after every connection, and again when a lost connection is reopened.</p>
            {server && (
              <div className="row">
                {conn.driver === "postgres" && (
                  <label className="field grow">
                    <span>Default schema</span>
                    <input
                      value={init.schema ?? ""}
                      onChange={(e) => setInit("schema", e.target.value || null)}
                      placeholder="public"
                      data-field="init_schema"
                    />
                  </label>
                )}
                <label className="field grow">
                  <span>Time zone</span>
                  <input
                    value={init.time_zone ?? ""}
                    onChange={(e) => setInit("time_zone", e.target.value || null)}
                    placeholder={conn.driver === "mysql" ? "+01:00 or Europe/Rome" : "Europe/Rome"}
                    data-field="init_time_zone"
                  />
                </label>
              </div>
            )}
            <label className="field">
              <span>Bootstrap statements</span>
              <textarea
                className="init-script"
                rows={6}
                value={init.statements}
                onChange={(e) => setInit("statements", e.target.value)}
                placeholder={server ? "SET statement_timeout = '30s';" : "PRAGMA foreign_keys = ON;"}
                spellCheck={false}
                data-field="init_statements"
              />
            </label>
          </div>

          {result && (
            <div className={result.ok ? "notice ok" : "notice error"}>
              {result.ok
                ? `Connected: ${result.server.product} ${result.server.version}${
                    !server ? "" : result.server.encrypted ? " · TLS" : " · not encrypted"
                  }`
                : result.message}
            </div>
          )}

          <div className="actions">
            <button type="button" onClick={test} disabled={busy}>
              Test connection
            </button>
            <span className="spacer" />
            <button type="button" onClick={onClose} disabled={busy}>
              Cancel
            </button>
            <button type="submit" className="primary" disabled={busy}>
              Save
            </button>
          </div>
        </form>
      </div>
      {editing && (
        <NetworkProfileDialog
          initial={editing.profile}
          existing={editing.existing}
          driver={conn.driver}
          onSaved={(all, saved) => {
            setProfiles(all);
            // A new profile is chosen; a deleted one is no longer.
            if (saved && !editing.existing) set("network_profile", saved.id);
            if (!saved) set("network_profile", null);
            setEditing(null);
          }}
          onClose={() => setEditing(null)}
        />
      )}
    </>
  );
}
