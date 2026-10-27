import { ProfileFields } from "../drivers/ProfileFields";
import { PROFILES } from "../drivers/profiles";
import { withDriver } from "../drivers/DriverPopup";
import { VendorDriverState } from "../drivers/VendorDrivers";
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  DATA_FILE_EXTENSIONS,
  isEmbedded,
  toCommandError,
  type ConnectionType,
  type DriverKind,
  type NetworkProfile,
  type ParseNote,
  type ParsedConnection,
  type PasswordInput,
  type Project,
  type SavedConnection,
  type SavedInit,
  type ServerInfo,
} from "../api";
import { AgentAccessField, withEffectiveAgentAccess } from "../agents/access";
import { CONNECTION_TYPES, noteText } from "../connections";
import { applyUrlEdit, looksLikeConnection, splitLogin, urlModeDriver, validPort, withParsed, type ConnectBy } from "../connectionUrl";
import { hasSessionTimeZone, withoutUnusableInit } from "../connectionEdit";
import { toggled } from "../sets";
import { basename } from "../workspace";
import { DriverIcon, DriverPicker, driverName } from "./DriverPicker";
import { NetworkFields, sshSecretIds, sshSecretsOf } from "./NetworkFields";
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
  /** The picker's tile (MariaDB rather than MySQL), for the header's logo. */
  const [tile, setTile] = useState<string>();
  /** The address is entered as host, port and database, or as one URL. */
  const [connectBy, setConnectBy] = useState<ConnectBy>("host");
  /** The URL as the form's settings print it, without the password. */
  const [printedUrl, setPrintedUrl] = useState("");
  /** The URL being typed, while it is: the field shows it as it is, not as printed. */
  const [urlDraft, setUrlDraft] = useState<string | null>(null);
  const [urlError, setUrlError] = useState<string | null>(null);
  /** Counts the URL edits, so that a slow answer for an old text is ignored. */
  const urlEdits = useRef(0);
  /** What a pasted URL could not carry over. */
  const [notes, setNotes] = useState<ParseNote[]>([]);
  /** A URL pasted in a new connection's form that is for another database. */
  const [otherDriver, setOtherDriver] = useState<{ driver: DriverKind; text: string } | null>(null);
  const [savedSsh] = useState(() => sshSecretIds(existing ? initial.ssh : null));
  const [profiles, setProfiles] = useState<NetworkProfile[]>([]);
  /** The network profile being edited in its own dialog, and whether it is saved. */
  const [editing, setEditing] = useState<{ profile: NetworkProfile; existing: boolean } | null>(null);
  const [busy, setBusy] = useState(false);
  /** A password is saved for this connection, for the "(saved)" placeholder. */
  const [passwordSaved, setPasswordSaved] = useState(false);
  const [result, setResult] = useState<
    { ok: true; server: ServerInfo } | { ok: false; message: string } | null
  >(null);

  useEffect(() => {
    api.listNetworkProfiles().then(setProfiles, (e) => setResult({ ok: false, message: toCommandError(e).message }));
    // Not knowing, the placeholder says nothing is saved: the field then asks for it.
    if (existing && initial.save_password) api.connectionHasPassword(initial.id).then(setPasswordSaved, () => setPasswordSaved(false));
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
  /** The address is a URL field instead of the host, port and database ones. */
  const urlMode = connectBy === "url" && server && !adbcProfile && urlModeDriver(conn.driver);

  // The URL the settings print, for the URL field.
  useEffect(() => {
    if (!urlMode) return;
    let current = true;
    api.formConnectionUrl(conn).then(
      (url) => current && setPrintedUrl(url ?? ""),
      () => current && setPrintedUrl(""),
    );
    return () => {
      current = false;
    };
  }, [urlMode, conn]);
  const secret = t(adbcProfile?.secret ?? (libsql ? "Auth token" : "Password"));

  // An empty password field keeps the stored one when editing.
  const passwordInput = (typed = password): PasswordInput =>
    typed !== ""
      ? { action: "set", value: typed }
      : existing
        ? { action: "keep" }
        : { action: "clear" };
  const savedPlaceholder = existing && conn.save_password && passwordSaved ? t("•••••• (saved)") : "";

  // A profile's SSH servers are saved with the profile, not here.
  const sshSecrets = () => sshSecretsOf(conn.network_profile ? null : conn.ssh, sshTyped, savedSsh);

  // The fields of the network page go to the profile when one is chosen; Init settings the
  // driver cannot apply are left out.
  const ownNetwork = (c: SavedConnection): SavedConnection =>
    withoutUnusableInit(c.network_profile ? { ...c, ssh: null } : c);

  /** A login typed in the host field (`user:password@host:port`) moved to its fields, so
   *  that the password goes where passwords go, never into the saved host. */
  function takeLogin(): { c: SavedConnection; typed: string } {
    const login = server && !adbcProfile && conn.driver !== "mongo" ? splitLogin(conn.host) : null;
    if (!login) return { c: { ...conn, host: conn.host.trim() }, typed: password };
    const c = { ...conn, host: login.host, port: login.port ?? conn.port, user: login.user || conn.user };
    const typed = login.password ?? password;
    setConn(c);
    setPassword(typed);
    return { c, typed };
  }

  /** The form's problem that the server would answer less clearly, or `null`. */
  const formProblem = (c: SavedConnection) => (server && !validPort(c.port) ? t("The port must be a number from 1 to 65535.") : null);

  async function test() {
    const { c, typed } = takeLogin();
    const problem = formProblem(c);
    if (problem) return setResult({ ok: false, message: problem });
    setBusy(true);
    setResult(null);
    try {
      const server = await withDriver(() => api.testConnection(ownNetwork(c), passwordInput(typed), sshSecrets()));
      setResult({ ok: true, server });
    } catch (e) {
      setResult({ ok: false, message: toCommandError(e).message });
    } finally {
      setBusy(false);
    }
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    const { c, typed } = takeLogin();
    const problem = formProblem(c);
    if (problem) return setResult({ ok: false, message: problem });
    setBusy(true);
    try {
      // Saved as agents get it: a production connection keeps no read-write access for later.
      const saved = withEffectiveAgentAccess(ownNetwork(c));
      onSaved(await api.saveConnection(saved, passwordInput(typed), sshSecrets(), [...inProjects]));
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

  /** The form of a URL read by the picker or pasted here. */
  function fillFrom(parsed: ParsedConnection) {
    setConn((c) => withParsed(c, parsed));
    // A password typed for another server is not sent to this one.
    setPassword(parsed.password ?? "");
    setNotes(parsed.notes);
    setOtherDriver(null);
    setResult(null);
  }

  // A URL pasted in the name or host field fills the form. A new connection keeps the
  // database it was opened for: a URL for another one offers to go back to the picker.
  function onPaste(e: React.ClipboardEvent<HTMLInputElement>) {
    const text = e.clipboardData.getData("text").trim();
    if (!looksLikeConnection(text)) return;
    e.preventDefault();
    // The form's database reads a string that names none (`Server=…;Uid=…`) as its own.
    api.parseConnectionUrl(text, conn.driver).then(
      (parsed) => (!existing && parsed.driver !== conn.driver ? setOtherDriver({ driver: parsed.driver, text }) : fillFrom(parsed)),
      (err) => setResult({ ok: false, message: toCommandError(err).message }),
    );
  }

  /** The URL field edited: read into the settings; one that does not parse changes nothing. */
  function editUrl(text: string) {
    setUrlDraft(text);
    const edit = ++urlEdits.current;
    if (!text.trim()) {
      setUrlError(null);
      return;
    }
    api.parseConnectionUrl(text, conn.driver).then(
      (parsed) => {
        if (edit !== urlEdits.current) return;
        if (parsed.driver !== conn.driver) {
          setUrlError(t("This URL is for {driver}: go back to choose it.", { driver: driverName(parsed.driver) }));
          return;
        }
        const next = applyUrlEdit(conn, password, text, parsed);
        setUrlError(null);
        setConn(next.conn);
        setPassword(next.password);
        setUrlDraft(next.draft);
        setNotes(parsed.notes);
        setResult(null);
      },
      (err) => edit === urlEdits.current && setUrlError(toCommandError(err).message),
    );
  }

  /** Back to the choice of the database, with nothing of this form kept. */
  function back() {
    setConn(initial);
    setPassword("");
    setSshTyped({});
    setResult(null);
    setNotes([]);
    setOtherDriver(null);
    setPage("general");
    setTile(undefined);
    setPicking(true);
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
        onPick={(driver, preset, label) => {
          changeDriver(driver);
          setConn((c) => ({ ...c, ...preset }));
          setTile(label);
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
          <div className="dialog-header">
            <h2>{t(existing ? "Edit connection" : "New connection")}</h2>
            <DriverIcon driver={conn.driver} label={tile} />
          </div>
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
                placeholder={t("My database")}
              />
            </label>

            <div className="row">
              {/* A new connection's database is chosen in the picker: Back changes it. */}
              {existing && (
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
              )}
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
                        placeholder={savedPlaceholder}
                        onChange={(e) => setPassword(e.target.value)}
                        autoComplete="off"
                      />
                    </label>
                  )
                }
              />
            ) : (
              <>
                {server && urlModeDriver(conn.driver) && (
                  <div className="connect-by" role="radiogroup" aria-label={t("Connect by")}>
                    <span>{t("Connect by")}</span>
                    <div className="connect-by-options">
                      {(["host", "url"] as const).map((mode) => (
                        <button
                          type="button"
                          role="radio"
                          key={mode}
                          aria-checked={connectBy === mode}
                          data-field={`connect_by_${mode}`}
                          onClick={() => {
                            setConnectBy(mode);
                            setUrlDraft(null);
                            setUrlError(null);
                          }}
                        >
                          {t(mode === "host" ? "Host" : "URL")}
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {urlMode && (
                  <div className="field">
                    <span>{t("URL")}</span>
                    <input
                      value={urlDraft ?? printedUrl}
                      onChange={(e) => editUrl(e.target.value)}
                      onBlur={() => {
                        urlEdits.current++;
                        setUrlDraft(null);
                        setUrlError(null);
                      }}
                      spellCheck={false}
                      autoComplete="off"
                      aria-invalid={urlError !== null}
                      aria-describedby={urlError ? "conn-url-error" : undefined}
                      title={t("The password goes to the Password field, not in the URL.")}
                      data-field="connection_url"
                    />
                    {urlError && (
                      <div id="conn-url-error" className="error-text small" role="alert">
                        {urlError}
                      </div>
                    )}
                  </div>
                )}

                {!urlMode && (
                <div className="row">
                  <label className="field grow">
                    <span>{t("Host")}</span>
                    <input
                      required
                      value={conn.host}
                      onChange={(e) => set("host", e.target.value)}
                      onPaste={onPaste}
                      onBlur={() => {
                        if (conn.driver !== "mongo" && splitLogin(conn.host)) takeLogin();
                      }}
                    />
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
                )}

                {!urlMode && (
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
                  {conn.driver === "oracle" && (
                    <label className="field">
                      <span>{t("Role")}</span>
                      <select
                        value={conn.options?.role ?? ""}
                        onChange={(e) => {
                          const options = { ...conn.options };
                          if (e.target.value) options.role = e.target.value;
                          else delete options.role;
                          set("options", options);
                        }}
                        data-field="role"
                      >
                        <option value="">{t("No administrative role")}</option>
                        {["SYSDBA", "SYSOPER", "SYSBACKUP", "SYSDG", "SYSKM", "SYSASM"].map((r) => (
                          <option key={r} value={r}>
                            {r}
                          </option>
                        ))}
                      </select>
                    </label>
                  )}
                </div>
                )}
                {conn.driver === "oracle" || conn.driver === "db2" ? <VendorDriverState driver={conn.driver} /> : null}

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
                          placeholder={savedPlaceholder}
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
            {server && !libsql && (conn.driver === "postgres" || hasSessionTimeZone(conn.driver)) && (
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
                {hasSessionTimeZone(conn.driver) && (
                  <label className="field grow">
                    <span>{t("Time zone")}</span>
                    <input
                      value={init.time_zone ?? ""}
                      onChange={(e) => setInit("time_zone", e.target.value || null)}
                      placeholder={conn.driver === "mysql" ? t("+01:00 or Europe/Rome") : "Europe/Rome"}
                      data-field="init_time_zone"
                    />
                  </label>
                )}
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

          {otherDriver && (
            <div className="notice warn url-other" role="status">
              <span>{t("This URL is for {driver}: go back to choose it.", { driver: driverName(otherDriver.driver) })}</span>
              <button type="button" onClick={() => back()}>
                {t("Back")}
              </button>
            </div>
          )}
          {notes.length > 0 && (
            <div className="notice warn url-notes" role="status">
              {notes.map((note) => (
                <div key={note.kind}>{noteText(note)}</div>
              ))}
            </div>
          )}

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
            {!existing && (
              <button type="button" onClick={() => back()} disabled={busy} title={t("Choose another database; this form is cleared")}>
                {t("Back")}
              </button>
            )}
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
