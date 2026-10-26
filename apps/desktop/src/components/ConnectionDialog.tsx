import { ProfileFields } from "../drivers/ProfileFields";
import { PROFILES } from "../drivers/profiles";
import { withDriver } from "../drivers/DriverPopup";
import { VendorDriverState } from "../drivers/VendorDrivers";
import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  DATA_FILE_EXTENSIONS,
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
  VENDOR_DRIVERS,
} from "../api";
import { AgentAccessField, withEffectiveAgentAccess } from "../agents/access";
import { CONNECTION_TYPES } from "../connections";
import { toggled } from "../sets";
import { basename } from "../workspace";
import { DriverPicker } from "./DriverPicker";
import { NetworkFields, SSL_MODES, sshSecretIds, sshSecretsOf } from "./NetworkFields";
import { NetworkProfileDialog, newNetworkProfile } from "./NetworkProfileDialog";
import { t, msg } from "../i18n";

interface Props {
  initial: SavedConnection;
  /** True when editing a connection that already exists in the store. */
  existing: boolean;
  /** Every project, to pick the connection's. */
  projects: Project[];
  /** The projects the connection is in, or (new) goes into. */
  memberOf: string[];
  /** A new connection whose driver is decided, e.g. a dropped data file: no driver picker. */
  driverChosen?: boolean;
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
  // The HTTP interface; 8443 with TLS.
  clickhouse: 8123,
  // The scheme's: 443, or 80 without TLS.
  libsql: null,
  snowflake: null,
  bigquery: null,
  databricks: null,
  trino: 8080,
  adbc: null,
  oracle: 1521,
  db2: 50000,
  odbc: null,
  mongo: 27017,
};

/** The database files of the embedded drivers: file picker filter and example path. */
const FILES: Record<"sqlite" | "duckdb", { name: string; extensions: string[]; example: string }> = {
  sqlite: { name: msg("SQLite database"), extensions: ["db", "sqlite", "sqlite3", "db3"], example: "C:\\data\\app.db" },
  // A data file (CSV, Parquet, Excel...) opens as an in-memory database that reads it.
  duckdb: {
    name: msg("DuckDB database or data file"),
    extensions: ["duckdb", "ddb", "db", ...DATA_FILE_EXTENSIONS],
    example: "C:\\data\\app.duckdb",
  },
};

const EMPTY_INIT: SavedInit = { schema: null, time_zone: null, statements: "" };

const SCHEMES: Record<string, DriverKind> = {
  "postgres:": "postgres",
  "postgresql:": "postgres",
  "mysql:": "mysql",
  "mariadb:": "mysql",
  "sqlserver:": "sqlserver",
  "mssql:": "sqlserver",
  "clickhouse:": "clickhouse",
  "libsql:": "libsql",
};

/** The form fields of a pasted URL, and the password it carries. */
type ParsedUrl = { conn: SavedConnection; password: string };

/** Turns a `mongodb://` or `mongodb+srv://` connection string into form fields, as
 *  `ConnectionConfig::from_mongo_url` does: several hosts stay in the host field, and the
 *  options follow the database after a `?`. */
function applyMongoUrl(conn: SavedConnection, input: string): ParsedUrl | null {
  const match = /^mongodb(\+srv)?:\/\/([^/?]*)\/?([^?]*)\??(.*)$/.exec(input.trim());
  if (!match) return null;
  const [, srv, authority = "", path = "", query = ""] = match;
  const at = authority.lastIndexOf("@");
  const userinfo = at < 0 ? "" : authority.slice(0, at);
  const hosts = authority.slice(at + 1);
  if (!hosts) return null;
  const colon = userinfo.indexOf(":");
  const user = decodeURIComponent(colon < 0 ? userinfo : userinfo.slice(0, colon));
  const password = colon < 0 ? "" : decodeURIComponent(userinfo.slice(colon + 1));
  const single = /^(\[[^\]]+\]|[^:,]+):(\d+)$/.exec(hosts);
  const options = srv ? ["srv=true"] : [];
  let ssl_mode = conn.ssl_mode;
  for (const pair of query.split("&").filter(Boolean)) {
    if (pair === "tls=true" || pair === "ssl=true") ssl_mode = "verify-full";
    else if (pair === "tls=false" || pair === "ssl=false") ssl_mode = "disable";
    else options.push(pair);
  }
  const database = decodeURIComponent(path);
  if (database && user && !query.includes("authSource=")) options.push(`authSource=${database}`);
  return {
    conn: {
      ...conn,
      driver: "mongo",
      host: (single ? single[1]! : hosts).replace(/^\[|\]$/g, ""),
      port: single ? Number(single[2]) : DEFAULT_PORTS.mongo,
      database: options.length ? `${database}?${options.join("&")}` : database,
      user,
      ssl_mode,
    },
    password,
  };
}

/** Turns a `postgres://`, `mysql://`, `sqlserver://`, `clickhouse://`, `libsql://` or `mongodb://` URL
 *  into form fields. */
function applyUrl(conn: SavedConnection, input: string): ParsedUrl | null {
  const mongo = applyMongoUrl(conn, input);
  if (mongo) return mongo;
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
    conn: {
      ...conn,
      driver,
      host: decodeURIComponent(url.hostname.replace(/^\[|\]$/g, "")) || conn.host,
      port: url.port ? Number(url.port) : DEFAULT_PORTS[driver],
      database: decodeURIComponent(url.pathname.replace(/^\//, "")),
      user: decodeURIComponent(url.username),
      ssl_mode: ssl && SSL_MODES.some((m) => m.value === ssl) ? ssl : conn.ssl_mode,
    },
    // Turso's URLs carry the token as a parameter.
    password: decodeURIComponent(url.password) || (url.searchParams.get("authToken") ?? ""),
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

export function ConnectionDialog({ initial, existing, projects, memberOf, driverChosen, onSaved, onClose }: Props) {
  const [conn, setConn] = useState(initial);
  const [inProjects, setInProjects] = useState(() => new Set(memberOf));
  const [password, setPassword] = useState("");
  /** SSH passwords and passphrases typed, by `secret_id`. */
  const [sshTyped, setSshTyped] = useState<Record<string, string>>({});
  const [page, setPage] = useState<Page>("general");
  /** A new connection starts from the choice of the database. */
  const [picking, setPicking] = useState(!existing && !driverChosen);
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
  const file = FILES[conn.driver === "duckdb" ? "duckdb" : "sqlite"];
  /** A libSQL server has no database or user to choose: the token picks the database. */
  const libsql = conn.driver === "libsql";
  /** The fields of a database reached through an ADBC driver. */
  const adbcProfile = PROFILES[conn.driver];
  const secret = t(adbcProfile?.secret ?? (libsql ? "Auth token" : "Password"));

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
      const server = await withDriver(() => api.testConnection(ownNetwork(conn), passwordInput(), sshSecrets()));
      setResult({ ok: true, server });
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
      // Saved as agents get it: a production connection keeps no read-write access for later.
      const saved = withEffectiveAgentAccess(ownNetwork(conn));
      onSaved(await api.saveConnection(saved, passwordInput(), sshSecrets(), [...inProjects]));
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
    const path = await open({
      multiple: false,
      filters: [
        { name: t(file.name), extensions: file.extensions },
        { name: t("All files"), extensions: ["*"] },
      ],
    });
    if (typeof path !== "string") return;
    setConn((c) => ({ ...c, database: path, name: c.name || basename(path) }));
  }

  function onPaste(e: React.ClipboardEvent<HTMLInputElement>) {
    const parsed = applyUrl(conn, e.clipboardData.getData("text"));
    if (parsed) {
      e.preventDefault();
      const next = parsed.conn;
      setConn({ ...next, name: conn.name || next.database.split("?")[0] || next.host });
      if (parsed.password) setPassword(parsed.password);
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
        onPick={(driver, preset) => {
          changeDriver(driver);
          setConn((c) => ({ ...c, ...preset }));
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
          className="modal connection-dialog"
          onSubmit={save}
          onKeyDown={(e) => e.key === "Escape" && onClose()}
          // A required field left empty on another tab: show that tab.
          onInvalidCapture={(e) => {
            const tab = (e.target as HTMLElement).closest<HTMLElement>("[data-page]")?.dataset.page;
            if (tab === "general" || tab === "network" || tab === "init") setPage(tab);
          }}
        >
          <h2>{t(existing ? "Edit connection" : "New connection")}</h2>
          <div className="dialog-tabs" role="tablist">
            {tab("general", t("General"))}
            {server && !adbcProfile && tab("network", `${t("Network")}${networkSet ? " ●" : ""}`)}
            {tab("init", `${t("Init")}${initSet ? " ●" : ""}`)}
          </div>

          <div data-page="general" hidden={page !== "general"} className="dialog-page">
            <label className="field">
              <span>{t("Name")}</span>
              <input
                autoFocus
                required
                value={conn.name}
                onChange={(e) => set("name", e.target.value)}
                onPaste={onPaste}
                placeholder={t("My database — or paste a postgres://, mysql:// or mongodb:// URL")}
              />
            </label>

            <div className="row">
              <label className="field">
                <span>{t("Driver")}</span>
                <select value={conn.driver} onChange={(e) => changeDriver(e.target.value as DriverKind)}>
                  <option value="postgres">PostgreSQL</option>
                  <option value="mysql">MySQL / MariaDB</option>
                  <option value="sqlite">SQLite</option>
                  <option value="duckdb">DuckDB</option>
                  <option value="sqlserver">SQL Server</option>
                  <option value="clickhouse">ClickHouse</option>
                  <option value="libsql">libSQL / Turso</option>
                  <option value="mongo">MongoDB</option>
                  <option value="snowflake">Snowflake</option>
                  <option value="bigquery">BigQuery</option>
                  <option value="databricks">Databricks</option>
                  <option value="trino">Trino</option>
                  <option value="adbc">{t("Other ADBC driver")}</option>
                  <option value="oracle">Oracle</option>
                  <option value="db2">IBM Db2</option>
                  <option value="odbc">{t("Other ODBC driver")}</option>
                </select>
              </label>
              <label className="field">
                <span>{t("Type")}</span>
                <select
                  value={conn.connection_type ?? ""}
                  onChange={(e) => set("connection_type", (e.target.value || null) as ConnectionType | null)}
                  title={t("Production asks before changes and starts in manual commit")}
                >
                  <option value="">{t("None")}</option>
                  {CONNECTION_TYPES.map((type) => (
                    <option key={type.value} value={type.value}>
                      {t(type.label)}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>{t("Color")}</span>
                <div className="swatches">
                  {COLORS.map((c) => (
                    <button
                      type="button"
                      key={c || "none"}
                      className={`swatch${(conn.color ?? "") === c ? " selected" : ""}`}
                      style={{ background: c || "transparent" }}
                      title={c || t("No color")}
                      onClick={() => set("color", c || null)}
                    />
                  ))}
                </div>
              </label>
            </div>

            {!server ? (
              <label className="field">
                <span>{t("Database file")}</span>
                <div className="row tight">
                  <input
                    required
                    value={conn.database}
                    onChange={(e) => set("database", e.target.value)}
                    placeholder={file.example}
                  />
                  <button type="button" onClick={() => void browseFile()}>
                    {t("Browse…")}
                  </button>
                </div>
              </label>
            ) : adbcProfile ? (
              <ProfileFields
                conn={conn}
                profile={adbcProfile}
                onChange={setConn}
                password={
                  adbcProfile.secret === null ? null : (
                    <label className="field">
                      <span>{t(adbcProfile.secret)}</span>
                      <input
                        type="password"
                        value={password}
                        placeholder={existing && conn.save_password ? t("•••••• (saved)") : ""}
                        onChange={(e) => setPassword(e.target.value)}
                        autoComplete="off"
                      />
                    </label>
                  )
                }
              />
            ) : (
              <>
                <div className="row">
                  <label className="field grow">
                    <span>{t("Host")}</span>
                    <input required value={conn.host} onChange={(e) => set("host", e.target.value)} />
                  </label>
                  <label className="field port">
                    <span>{t("Port")}</span>
                    <input
                      type="number"
                      min={1}
                      max={65535}
                      value={conn.port ?? ""}
                      onChange={(e) => set("port", e.target.value ? Number(e.target.value) : null)}
                    />
                  </label>
                </div>

                <div className="field-grid">
                  {!libsql && (
                    <label className="field">
                      <span>
                        {t(
                          conn.driver === "mysql" || conn.driver === "sqlserver" || conn.driver === "clickhouse" || conn.driver === "mongo"
                            ? "Database (optional)"
                            : conn.driver === "oracle"
                              ? "Service name"
                              : "Database",
                        )}
                      </span>
                      <input
                        value={conn.database}
                        onChange={(e) => set("database", e.target.value)}
                        placeholder={
                          conn.driver === "mongo"
                            ? "test?authSource=admin&replicaSet=rs0"
                            : conn.driver === "oracle"
                              ? "FREEPDB1"
                              : undefined
                        }
                        title={
                          conn.driver === "mongo"
                            ? t("The default database, then connection string options after ?; srv=true for mongodb+srv. Users authenticate against admin unless authSource says otherwise.")
                            : conn.driver === "oracle"
                              ? t("The service name; with an empty host, a net service name (tnsnames.ora) or a whole connect descriptor.")
                              : undefined
                        }
                      />
                    </label>
                  )}
                  {!libsql && (
                    <label className="field">
                      <span>{t("User")}</span>
                      <input value={conn.user} onChange={(e) => set("user", e.target.value)} />
                    </label>
                  )}
                </div>
                {VENDOR_DRIVERS.includes(conn.driver) ? <VendorDriverState driver={conn.driver} /> : null}

                <div className="field-grid">
                  <label className="field">
                    <span>{t("{secret} from", { secret })}</span>
                    <select
                      value={source}
                      onChange={(e) => setConn((c) => withPasswordSource(c, e.target.value as PasswordSource))}
                      data-field="password_source"
                    >
                      <option value="keychain">{t("Saved in the keychain")}</option>
                      <option value="ask">{t("Ask every time")}</option>
                      {conn.driver === "postgres" && <option value="pgpass">{t(".pgpass file")}</option>}
                      <option value="command">{t("Shell command")}</option>
                    </select>
                  </label>
                  {source === "command" ? (
                    <label className="field">
                      <span>{t("Command that prints the {secret}", { secret: secret.toLowerCase() })}</span>
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
                        <span>{source === "ask" ? t("{secret} (only to test the connection)", { secret }) : secret}</span>
                        <input
                          type="password"
                          value={password}
                          placeholder={existing && conn.save_password ? t("•••••• (saved)") : ""}
                          onChange={(e) => setPassword(e.target.value)}
                          autoComplete="off"
                        />
                      </label>
                    )
                  )}
                </div>
              </>
            )}

            <label className="check">
              <input type="checkbox" checked={conn.read_only} onChange={(e) => set("read_only", e.target.checked)} />
              {t("Read-only (warn before modifying data)")}
            </label>

            <AgentAccessField conn={conn} onChange={(access) => set("agent_access", access)} />

            {projects.length > 0 && (
              <div className="field">
                <span>{t("Projects")}</span>
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
                <span>{t("Network profile")}</span>
                <div className="row tight">
                  <select
                    value={conn.network_profile ?? ""}
                    onChange={(e) => set("network_profile", e.target.value || null)}
                    data-field="network_profile"
                  >
                    <option value="">{t("None — this connection's own settings")}</option>
                    {profiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  {profile && (
                    <button type="button" onClick={() => setEditing({ profile, existing: true })}>
                      {t("Edit…")}
                    </button>
                  )}
                  <button type="button" onClick={() => setEditing({ profile: newNetworkProfile(), existing: false })}>
                    {t("New…")}
                  </button>
                </div>
              </label>
              {profile ? (
                <p className="muted small" data-field="profile_summary">
                  {profile.ssh ? t("SSH through {host}", { host: `${profile.ssh.user ? `${profile.ssh.user}@` : ""}${profile.ssh.host}` }) : t("No SSH tunnel")}
                  {" · SSL "}
                  {profile.ssl_mode}
                  {profile.ssl_client_cert ? ` ${t("with a client certificate")}` : ""}
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
            <p className="muted small">{t("Runs after every connection, and again when a lost connection is reopened.")}</p>
            {server && !libsql && (
              <div className="row">
                {conn.driver === "postgres" && (
                  <label className="field grow">
                    <span>{t("Default schema")}</span>
                    <input
                      value={init.schema ?? ""}
                      onChange={(e) => setInit("schema", e.target.value || null)}
                      placeholder="public"
                      data-field="init_schema"
                    />
                  </label>
                )}
                <label className="field grow">
                  <span>{t("Time zone")}</span>
                  <input
                    value={init.time_zone ?? ""}
                    onChange={(e) => setInit("time_zone", e.target.value || null)}
                    placeholder={conn.driver === "mysql" ? t("+01:00 or Europe/Rome") : "Europe/Rome"}
                    data-field="init_time_zone"
                  />
                </label>
              </div>
            )}
            <label className="field">
              <span>{t("Bootstrap statements")}</span>
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
                ? `${t("Connected: {server}", { server: `${result.server.product} ${result.server.version}` })}${
                    !server ? "" : result.server.encrypted ? " · TLS" : ` · ${t("not encrypted")}`
                  }`
                : result.message}
            </div>
          )}

          <div className="actions">
            <button type="button" onClick={test} disabled={busy}>
              {t("Test connection")}
            </button>
            <span className="spacer" />
            <button type="button" onClick={onClose} disabled={busy}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary" disabled={busy}>
              {t("Save")}
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
