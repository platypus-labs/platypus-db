import { ProfileFields } from "../drivers/ProfileFields";
import { PluginDriverField } from "../plugins/PluginDriverField";
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
  VENDOR_DRIVERS,
} from "../api";
import { AgentAccessField, withEffectiveAgentAccess } from "../agents/access";
import { initHasSettings, networkHasSettings } from "./connectionPages";
import { asksPassword, CONNECTION_TYPES, noteText } from "../connections";
import { applyUrlEdit, cleanFilePath, hostOptional, looksLikeConnection, splitLogin, splitSqlServerHost, sshMissing, urlModeDriver, validPort, withParsed, type ConnectBy } from "../connectionUrl";
import { PasswordPrompt } from "../App";
import { workbench } from "../workbench";
import { hasSessionTimeZone, withoutUnusableInit } from "../connectionEdit";
import { toggled } from "../sets";
import { basename } from "../workspace";
import { DriverIcon, DriverPicker, driverName } from "./DriverPicker";
import { Icon } from "../ui/Icon";
import { NetworkFields, sshSecretIds, sshSecretsOf } from "./NetworkFields";
import { NetworkProfileDialog, newNetworkProfile } from "./NetworkProfileDialog";
import { t, tNodes, msg } from "../i18n";
import { Busy } from "../ui/State";
import { type ManagerId, MANAGERS, managerById, managerOf } from "../credentialManagers";
import { SIGN_INS, type SignInId, signInOf, signInsFor, withSignIn } from "../signIn";
import { suggestedPooler } from "./pooler";

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

type Page = "general" | "safety" | "network" | "init";

/** Where the password comes from; stored as `save_password`, `pgpass` and `password_command`,
 *  which a password manager's source writes for its CLI. */
type PasswordSource = "keychain" | "ask" | "pgpass" | "command" | ManagerId | SignInId;

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
  // The plugin's database decides.
  plugin: null,
  mongo: 27017,
  redis: 6379,
  elasticsearch: 9200,
  cassandra: 9042,
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

const WINDOWS = navigator.userAgent.includes("Windows");

const EMPTY_INIT: SavedInit = { schema: null, time_zone: null, statements: "" };

function passwordSource(c: SavedConnection): PasswordSource {
  const signIn = signInOf(c);
  if (signIn) return signIn.id;
  if (c.password_command != null) return managerOf(c.password_command)?.manager.id ?? "command";
  if (c.pgpass) return "pgpass";
  return c.save_password ? "keychain" : "ask";
}

/** The reference of the secret in `c`'s password manager, empty without one. */
const secretReference = (c: SavedConnection) => managerOf(c.password_command)?.reference ?? "";

function withPasswordSource(c: SavedConnection, source: PasswordSource): SavedConnection {
  const manager = managerById(source);
  const signIn = SIGN_INS.find((s) => s.id === source);
  return {
    ...withSignIn(c, signIn?.id ?? null),
    save_password: source === "keychain",
    pgpass: source === "pgpass",
    password_command: manager
      ? manager.command(secretReference(c))
      : source === "command"
        ? (c.password_command ?? "")
        : null,
  };
}

/** A text field for `conn`'s option `name`. */
function OptionField({
  conn,
  setOption,
  name,
  label,
  placeholder,
  required = false,
}: {
  conn: SavedConnection;
  setOption: (key: string, value: string) => void;
  name: string;
  label: string;
  placeholder: string;
  required?: boolean;
}) {
  return (
    <label className="field">
      <span>{label}</span>
      <input
        required={required}
        value={conn.options?.[name] ?? ""}
        onChange={(e) => setOption(name, e.target.value)}
        placeholder={placeholder}
        spellCheck={false}
        data-field={name}
      />
    </label>
  );
}

/** The port a cloud's CLI forwards to the server: `kubectl port-forward` or an AWS SSM
 *  session, run by `platypus-net` on a free local port while the connection is open. */
function PortForwardFields({
  conn,
  tunnel,
  setOption,
}: {
  conn: SavedConnection;
  /** It has an SSH tunnel, its own or its network profile's. */
  tunnel: boolean;
  setOption: (key: string, value: string) => void;
}) {
  const option = (key: string) => conn.options?.[key] ?? "";
  const field = (name: string, label: string, placeholder: string, required = false) => (
    <OptionField conn={conn} setOption={setOption} name={name} label={label} placeholder={placeholder} required={required} />
  );
  const kind = option("forward");
  const alongside = kind && (tunnel || option("proxy"));
  return (
    <>
      <label className="field">
        <span>{t("Port forward")}</span>
        <select value={kind} onChange={(e) => setOption("forward", e.target.value)} data-field="forward">
          <option value="">{t("None")}</option>
          <option value="kubernetes">{t("Kubernetes (kubectl port-forward)")}</option>
          <option value="ssm">{t("AWS Systems Manager session")}</option>
        </select>
      </label>
      {kind === "kubernetes" && (
        <div className="field-grid three">
          {field("k8s_target", t("Service or pod"), "svc/postgres", true)}
          {field("k8s_namespace", t("Namespace"), "default")}
          {field("k8s_context", t("Context"), t("The current one"))}
        </div>
      )}
      {kind === "ssm" && (
        <div className="field-grid three">
          {field("ssm_target", t("EC2 instance"), "i-0123456789abcdef0", true)}
          {field("aws_region", t("AWS region"), t("The profile's"))}
          {field("aws_profile", t("AWS profile"), "default")}
        </div>
      )}
      {kind && (
        <p className="muted small" data-field="forward_note">
          {kind === "kubernetes"
            ? tNodes("Runs {command} on a free local port while the connection is open: kubectl must be installed and reach the cluster. The port is the server's.", {
                command: <code>kubectl port-forward</code>,
              })
            : tNodes("Runs {command} to the host and port above, as the instance sees them: the AWS CLI and its Session Manager plugin must be installed and signed in.", {
                command: <code>aws ssm start-session</code>,
              })}
        </p>
      )}
      {alongside && (
        <div className="notice small warn" role="alert" data-field="forward_alone">
          {t("A port forward goes alone: remove the SSH tunnel or the proxy.")}
        </div>
      )}
    </>
  );
}

/** Enter in a single-line field saves the connection, as in every other dialog: not in a text
 *  area, a button or a list, and not while an input method is composing text. */
function enterSaves(e: React.KeyboardEvent): boolean {
  if (e.key !== "Enter" || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey) return false;
  if (e.nativeEvent.isComposing || e.keyCode === 229 || e.defaultPrevented) return false;
  const target = e.target as HTMLElement;
  if (target.tagName !== "INPUT") return false;
  const type = (target as HTMLInputElement).type;
  return !["checkbox", "radio", "button", "submit", "file", "range", "color"].includes(type);
}

export function ConnectionDialog({ initial, existing, projects, memberOf, driverChosen, onSaved, onClose }: Props) {
  const [conn, setConn] = useState(initial);
  const [inProjects, setInProjects] = useState(() => new Set(memberOf));
  const [password, setPassword] = useState("");
  /** "Shell command" was chosen in the password's list. */
  const [handWritten, setHandWritten] = useState(false);
  /** SSH passwords and passphrases typed, by `secret_id`. */
  const [sshTyped, setSshTyped] = useState<Record<string, string>>({});
  const [page, setPage] = useState<Page>("general");
  /** A new connection starts from the choice of the database. */
  const [picking, setPicking] = useState(!existing && !driverChosen);
  /** The picker's tile (MariaDB rather than MySQL), for the header's logo. */
  const [tile, setTile] = useState<string>();
  /** The address is entered as host, port and database, or as one URL. */
  const [connectBy, setConnectBy] = useState<ConnectBy>(initial.connect_by ?? "host");
  /** The URL as the form's settings print it, without the password. */
  const [printedUrl, setPrintedUrl] = useState("");
  /** The URL being typed, while it is: the field shows it as it is, not as printed. */
  const [urlDraft, setUrlDraft] = useState<string | null>(null);
  const [urlError, setUrlError] = useState<string | null>(null);
  /** Counts the URL edits, so that a slow answer for an old text is ignored. */
  const urlEdits = useRef(0);
  // The test in flight; a Cancel bumps it, and the late answer of the older one is ignored.
  const testRun = useRef(0);
  const [testing, setTesting] = useState(false);
  /** The last URL edit is still being read: leaving the field keeps it as typed. */
  const urlReading = useRef(false);
  const urlInput = useRef<HTMLInputElement>(null);
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
  // The pooler the host is, when its mode is not set to it yet.
  const suggested = suggestedPooler(conn.host, conn.port);
  const poolerHint = suggested?.mode === "transaction" && conn.options?.pooler !== "transaction" ? suggested : null;
  /** Sets a driver option; an empty value removes it. */
  const setOption = (key: string, value: string) => {
    const options = { ...conn.options };
    if (value.trim()) options[key] = value;
    else delete options[key];
    set("options", options);
  };
  // "Shell command" chosen over a manager keeps its command, to edit by hand: it would be
  // recognized as the manager's again.
  const source = handWritten && conn.password_command != null ? "command" : passwordSource(conn);
  const manager = managerById(source);
  const signIn = signInOf(conn);
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
  const secret =
    adbcProfile?.secret != null
      ? t(adbcProfile.secret)
      : t(libsql ? "Auth token" : conn.driver === "elasticsearch" ? "Password or API key" : "Password");

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
  function takeLogin(base = conn, typedPassword = password): { c: SavedConnection; typed: string } {
    const login = server && !adbcProfile && base.driver !== "mongo" ? splitLogin(base.host) : null;
    if (!login) return { c: tidyAddress({ ...base, host: base.host.trim() }), typed: typedPassword };
    const c = tidyAddress({ ...base, host: login.host, port: login.port ?? base.port, user: login.user || base.user });
    const typed = login.password ?? typedPassword;
    setConn(c);
    setPassword(typed);
    return { c, typed };
  }

  /** The address fields as the driver reads them: a file path without the quotes "Copy as
   *  path" adds, SQL Server's `host,port` split. */
  function tidyAddress(c: SavedConnection): SavedConnection {
    if (!server) return { ...c, database: cleanFilePath(c.database) };
    const split = c.driver === "sqlserver" ? splitSqlServerHost(c.host) : null;
    return split ? { ...c, host: split.host, port: split.port } : c;
  }

  /** The form's problem that the server would answer less clearly, or `null`. */
  const formProblem = (c: SavedConnection) => {
    if (!server) return null;
    if (!validPort(c.port)) return t("The port must be a number from 1 to 65535.");
    if (!hostOptional(c.driver) && !c.host.trim()) return t("The host is missing.");
    const missing = sshMissing(c.network_profile ? null : c.ssh);
    if (missing === "host") return t("The SSH host is missing.");
    if (missing === "user") return t("The SSH user is missing.");
    return null;
  };

  /** The password to test an "ask every time" connection with, when none is typed in the
   *  form: asked as opening the connection does; `null` when the question is cancelled. */
  async function testPassword(c: SavedConnection, typed: string): Promise<string | null> {
    if (typed !== "" || !asksPassword(c)) return typed;
    return new Promise((resolve) =>
      workbench().openDialog((close) => (
        <PasswordPrompt
          connection={c}
          error={null}
          onDone={(entered) => {
            close();
            resolve(entered);
          }}
        />
      )),
    );
  }

  /** `parsed`, read from the URL `text`, into the form's settings; `null`, with the error
   *  under the field, when the URL is for another database. */
  function takeUrl(text: string, parsed: ParsedConnection) {
    if (parsed.driver !== conn.driver) {
      setUrlError(otherDriverError(parsed.driver));
      return null;
    }
    const next = applyUrlEdit(conn, password, text, parsed);
    setUrlError(null);
    setConn(next.conn);
    setPassword(next.password);
    setNotes(parsed.notes);
    return next;
  }

  /** The URL being typed read into the form, as Test and Save need it even when its answer
   *  has not come yet (a paste then Enter); `null`, with the error under the field, when it
   *  does not read: nothing goes ahead with the settings from before it. */
  async function settleUrl(): Promise<{ c: SavedConnection; typed: string } | null> {
    const text = urlDraft;
    if (!urlMode || text === null || !text.trim()) return { c: conn, typed: password };
    try {
      const next = takeUrl(text, await api.parseConnectionUrl(text, conn.driver));
      if (!next) return null;
      setUrlDraft(next.draft);
      return { c: next.conn, typed: next.password };
    } catch (err) {
      setUrlError(toCommandError(err).message);
      return null;
    }
  }

  /** The form as Test and Save send it: the URL read, the login out of the host field. */
  async function formToSend(): Promise<{ c: SavedConnection; typed: string } | null> {
    const url = await settleUrl();
    return url && takeLogin(url.c, url.typed);
  }

  async function test() {
    const form = await formToSend();
    if (!form) return;
    const { c, typed } = form;
    const problem = formProblem(c);
    if (problem) return setResult({ ok: false, message: problem });
    const password = await testPassword(c, typed);
    if (password === null) return;
    const run = ++testRun.current;
    setBusy(true);
    setTesting(true);
    setResult(null);
    try {
      const server = await withDriver(() => api.testConnection(ownNetwork(c), passwordInput(password), sshSecrets()));
      if (run === testRun.current) setResult({ ok: true, server });
    } catch (e) {
      if (run === testRun.current) setResult({ ok: false, message: toCommandError(e).message });
    } finally {
      if (run === testRun.current) {
        setBusy(false);
        setTesting(false);
      }
    }
  }

  /** There is no backend cancel for a test: stop waiting, the late answer is dropped. */
  function cancelTest() {
    testRun.current++;
    setBusy(false);
    setTesting(false);
    setResult(null);
  }

  async function save(e: React.FormEvent) {
    e.preventDefault();
    const form = await formToSend();
    if (!form) return;
    const { c, typed } = form;
    const problem = formProblem(c);
    if (problem) return setResult({ ok: false, message: problem });
    setBusy(true);
    try {
      // Saved as agents get it: a production connection keeps no read-write access for later.
      // The form opens again as it was left, by host or by URL.
      const saved = withEffectiveAgentAccess(ownNetwork({ ...c, connect_by: urlMode ? "url" : "host" }));
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
    urlReading.current = !!text.trim();
    if (!text.trim()) {
      setUrlError(null);
      return;
    }
    const answered = () => {
      if (edit !== urlEdits.current) return false;
      urlReading.current = false;
      return true;
    };
    api.parseConnectionUrl(text, conn.driver).then(
      (parsed) => {
        if (!answered()) return;
        const next = takeUrl(text, parsed);
        if (!next) return;
        // A field already left shows the URL as the settings print it.
        setUrlDraft(document.activeElement === urlInput.current ? next.draft : null);
        setResult(null);
      },
      (err) => answered() && setUrlError(toCommandError(err).message),
    );
  }

  const otherDriverError = (driver: DriverKind) => t("This URL is for {driver}: go back to choose it.", { driver: driverName(driver) });

  function chooseConnectBy(mode: ConnectBy) {
    setConnectBy(mode);
    setUrlDraft(null);
    setUrlError(null);
  }

  /** The Host/URL radio group's keys: an arrow checks and focuses the other option. */
  function connectByKey(e: React.KeyboardEvent<HTMLDivElement>) {
    const keys: Record<string, ConnectBy> = { ArrowRight: "url", ArrowDown: "url", End: "url", ArrowLeft: "host", ArrowUp: "host", Home: "host" };
    let mode: ConnectBy | undefined = keys[e.key];
    if (!mode) return;
    e.preventDefault();
    // Two options: an arrow past the last one wraps to the first.
    if (e.key.startsWith("Arrow") && mode === connectBy) mode = connectBy === "host" ? "url" : "host";
    chooseConnectBy(mode);
    e.currentTarget.querySelector<HTMLButtonElement>(`[data-field=connect_by_${mode}]`)?.focus();
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

  // A dot after the name says the page holds settings (Network, Init): the same meaning on each.
  const tab = (id: Page, label: string, hasSettings = false) => (
    <button
      type="button"
      role="tab"
      title={hasSettings ? `${label} · ${t("Has settings")}` : label}
      aria-selected={page === id}
      onClick={() => setPage(id)}
    >
      {label}
      {hasSettings && (
        <span className="tab-dot" role="img" aria-label={t("Has settings")}>
          ●
        </span>
      )}
    </button>
  );
  const pageTitle = { general: "General", safety: "Appearance and safety", network: "Network", init: "Init" }[page];
  const networkSet = networkHasSettings(conn);
  const initSet = initHasSettings(conn);

  if (picking) {
    return (
      <DriverPicker
        onPick={(driver, preset, label) => {
          changeDriver(driver);
          setConn((c) => ({ ...c, ...preset, options: { ...c.options, ...preset.options } }));
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
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            else if (enterSaves(e)) {
              // Handled here, so the browser's own implicit submission does not save twice.
              e.preventDefault();
              e.currentTarget.requestSubmit();
            }
          }}
          // A required field left empty on another tab: show that tab.
          onInvalidCapture={(e) => {
            const tab = (e.target as HTMLElement).closest<HTMLElement>("[data-page]")?.dataset.page;
            if (tab === "general" || tab === "safety" || tab === "network" || tab === "init") setPage(tab);
          }}
        >
          <div className="dialog-header">
            <DriverIcon driver={conn.driver} label={tile} />
            <h2>{t(existing ? "Edit connection" : "New connection")}</h2>
            {!existing && (
              <div className="steps" aria-hidden="true">
                <span className="done">
                  <b>1</b> {t("Database")}
                </span>
                <i />
                <span className="on">
                  <b>2</b> {t("Connection")}
                </span>
              </div>
            )}
          </div>
          <div className="dialog-split">
          <div className="dialog-tabs" role="tablist" aria-orientation="vertical">
            {tab("general", t("General"))}
            {tab("safety", t("Appearance and safety"))}
            {server && !adbcProfile && tab("network", t("Network"), networkSet)}
            {tab("init", t("Init"), initSet)}
          </div>

          <div className="dialog-content">
          <h3 className="dialog-page-title">{t(pageTitle)}</h3>
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
                    <option value="redis">Redis</option>
                    <option value="elasticsearch">Elasticsearch / OpenSearch</option>
                    <option value="cassandra">Cassandra / ScyllaDB</option>
                    <option value="snowflake">Snowflake</option>
                    <option value="bigquery">BigQuery</option>
                    <option value="databricks">Databricks</option>
                    <option value="trino">Trino</option>
                    <option value="adbc">{t("Other ADBC driver")}</option>
                    <option value="oracle">Oracle</option>
                    <option value="db2">IBM Db2</option>
                    <option value="odbc">{t("Other ODBC driver")}</option>
                    <option value="plugin">{t("Plugin driver")}</option>
                  </select>
                </label>
              )}
            </div>

            {!server ? (
              <section className="form-section">
                <h3>{t("Connection")}</h3>
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
              </section>
            ) : adbcProfile ? (
              <section className="form-section">
                <h3>{t("Connection")}</h3>
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
              </section>
            ) : (
              <>
                <section className="form-section">
                <h3>{t("Connection")}</h3>
                {conn.driver === "plugin" && (
                  <PluginDriverField
                    value={conn.options?.plugin ?? ""}
                    onChange={(plugin) => setConn((c) => ({ ...c, options: { ...c.options, plugin } }))}
                  />
                )}
                {server && urlModeDriver(conn.driver) && (
                  <div className="connect-by" role="radiogroup" aria-label={t("Connect by")} onKeyDown={connectByKey}>
                    <span>{t("Connect by")}</span>
                    <div className="connect-by-options">
                      {(["host", "url"] as const).map((mode) => (
                        <button
                          type="button"
                          role="radio"
                          key={mode}
                          aria-checked={connectBy === mode}
                          tabIndex={connectBy === mode ? 0 : -1}
                          data-field={`connect_by_${mode}`}
                          onClick={() => chooseConnectBy(mode)}
                        >
                          {t(mode === "host" ? "Host" : "URL")}
                        </button>
                      ))}
                    </div>
                  </div>
                )}

                {urlMode && (
                  <div className="field">
                    <span id="conn-url-label">{t("URL")}</span>
                    <input
                      ref={urlInput}
                      aria-labelledby="conn-url-label"
                      value={urlDraft ?? printedUrl}
                      onChange={(e) => editUrl(e.target.value)}
                      onBlur={() => {
                        // A URL that does not read, or not yet, stays as typed with its error.
                        if (urlError === null && !urlReading.current) setUrlDraft(null);
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
                          conn.driver === "mysql" ||
                            conn.driver === "sqlserver" ||
                            conn.driver === "clickhouse" ||
                            conn.driver === "mongo" ||
                            conn.driver === "redis"
                            ? "Database (optional)"
                            : conn.driver === "cassandra"
                              ? "Keyspace (optional)"
                              : conn.driver === "elasticsearch"
                              ? "Index pattern (optional)"
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
                              : conn.driver === "redis"
                                ? "0"
                                : conn.driver === "elasticsearch"
                                  ? "logs-*"
                                  : undefined
                        }
                        title={
                          conn.driver === "mongo"
                            ? t("The default database, then connection string options after ?; srv=true for mongodb+srv. Users authenticate against admin unless authSource says otherwise.")
                            : conn.driver === "oracle"
                              ? t("The service name; with an empty host, a net service name (tnsnames.ora) or a whole connect descriptor.")
                              : conn.driver === "redis"
                                ? t("The database number, 0 when empty. TLS needs an SSL mode from require up, as rediss:// URLs do.")
                                : conn.driver === "elasticsearch"
                                  ? t("The indices the navigator lists, as a pattern (logs-*, or several separated by commas); all of them when empty. TLS needs an SSL mode from require up, as elasticsearchs:// URLs do.")
                                  : conn.driver === "cassandra"
                                    ? t("The keyspace to start in; the navigator lists all of them. The host may list several host:port pairs separated by commas. TLS needs an SSL mode from require up, as cassandras:// URLs do.")
                                    : undefined
                        }
                      />
                    </label>
                  )}
                  {conn.driver === "postgres" && (
                    <label className="field">
                      <span>{t("Connection pooler")}</span>
                      <select
                        value={conn.options?.pooler ?? ""}
                        onChange={(e) => setOption("pooler", e.target.value)}
                        data-field="pooler"
                        title={t("In transaction mode each transaction may get another server session: statements run unnamed, as PgBouncer, Supavisor and Neon's pooler need.")}
                      >
                        <option value="">{t("None or session mode")}</option>
                        <option value="transaction">{t("Transaction mode (PgBouncer, Supavisor, Neon)")}</option>
                      </select>
                    </label>
                  )}
                </div>
                )}
                {conn.driver === "postgres" && poolerHint && (
                  <div className="notice small pooler-hint" role="status">
                    <span>
                      {t("This is {service}'s pooler in transaction mode.", { service: poolerHint.service })}
                    </span>
                    <button type="button" onClick={() => setOption("pooler", "transaction")}>
                      {t("Use transaction mode")}
                    </button>
                  </div>
                )}
                {VENDOR_DRIVERS.includes(conn.driver) ? <VendorDriverState driver={conn.driver} /> : null}
                </section>

                <section className="form-section">
                <h3>{t("Authentication")}</h3>
                {/* In URL mode too: the URL field prints the user typed here, and a URL fills it. */}
                {(!libsql || conn.driver === "oracle") && (
                <div className="field-grid">
                  {!libsql && (
                    <label className="field">
                      <span>{t("User")}</span>
                      <input
                        value={conn.user}
                        onChange={(e) => set("user", e.target.value)}
                        title={
                          conn.driver === "elasticsearch"
                            ? t("Leave empty to send the password as an API key.")
                            : undefined
                        }
                      />
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
                <div className="field-grid">
                  <label className="field">
                    <span>{t("{secret} from", { secret })}</span>
                    <select
                      value={source}
                      onChange={(e) => {
                        const chosen = e.target.value as PasswordSource;
                        setHandWritten(chosen === "command");
                        setConn((c) => withPasswordSource(c, chosen));
                      }}
                      data-field="password_source"
                    >
                      <option value="keychain">{t("Saved in the keychain")}</option>
                      <option value="ask">{t("Ask every time")}</option>
                      {conn.driver === "postgres" && <option value="pgpass">{t(".pgpass file")}</option>}
                      <option value="command">{t("Shell command")}</option>
                      {MANAGERS.map((m) => (
                        <option key={m.id} value={m.id}>
                          {m.name}
                        </option>
                      ))}
                      {signInsFor(conn.driver, signIn?.id).map((s) => (
                        <option key={s.id} value={s.id}>
                          {s.id === "windows" ? t("Windows sign-in (SSPI, Kerberos)") : t("{service} token", { service: s.name })}
                        </option>
                      ))}
                    </select>
                  </label>
                  {signIn ? (
                    signIn.id === "aws_iam" && (
                      <>
                        <OptionField conn={conn} setOption={setOption} name="aws_region" label={t("AWS region")} placeholder={t("The profile's")} />
                        <OptionField conn={conn} setOption={setOption} name="aws_profile" label={t("AWS profile")} placeholder="default" />
                      </>
                    )
                  ) : manager ? (
                    <label className="field">
                      <span>{t("Secret in {manager}", { manager: manager.name })}</span>
                      <input
                        required
                        value={secretReference(conn)}
                        onChange={(e) => set("password_command", manager.command(e.target.value))}
                        placeholder={manager.example}
                        title={t("Quotes, $, ` and % are left out: the command runs in the system's shell.")}
                        data-field="secret_reference"
                      />
                    </label>
                  ) : source === "command" ? (
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
                {signIn && (
                  <p className="muted small sign-in-note" data-field="sign_in_note">
                    {signIn.command
                      ? tNodes("Runs {command} at each connection: its CLI must be installed and signed in. Tokens last minutes and are never saved.", {
                          command: <code>{signIn.command(conn)}</code>,
                        })
                      : WINDOWS
                        ? t("Signs in as the Windows user running PlatypusDB, through SSPI (Kerberos or NTLM).")
                        : t("Only PlatypusDB on Windows can sign in this way.")}
                    {conn.driver === "mysql" && signIn.command && ` ${t("The token goes in clear text: use TLS.")}`}
                  </p>
                )}
                {manager && conn.password_command && (
                  <p className="muted small manager-command">
                    {tNodes("Runs {command}: its CLI must be installed and signed in.", {
                      command: <code>{conn.password_command}</code>,
                    })}
                  </p>
                )}
                </section>
              </>
            )}


          </div>

          <div data-page="safety" hidden={page !== "safety"} className="dialog-page">
            <section className="form-section">
            <h3>{t("Appearance")}</h3>
            <div className="row">
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
            <label className="check">
              <input
                type="checkbox"
                data-field="text_json"
                checked={conn.text_json !== "plain"}
                onChange={(e) => set("text_json", e.target.checked ? undefined : "plain")}
              />
              {t("Format JSON found in text columns in the value panel")}
            </label>
            </section>

            <section className="form-section">
            <h3>{t("Safety")}</h3>
            <label className="check">
              <input type="checkbox" checked={conn.read_only} onChange={(e) => set("read_only", e.target.checked)} />
              {t("Read-only (warn before modifying data)")}
            </label>

            <label className="check">
              <input
                type="checkbox"
                data-field="no_export"
                checked={!!conn.restrictions?.no_export}
                onChange={(e) => set("restrictions", { ...conn.restrictions, no_export: e.target.checked })}
              />
              {t("No exports: its data goes to no file, other database or agent")}
            </label>

            <label className="check">
              <input
                type="checkbox"
                data-field="no_data_editing"
                checked={!!conn.restrictions?.no_data_editing}
                onChange={(e) => set("restrictions", { ...conn.restrictions, no_data_editing: e.target.checked })}
              />
              {t("No data editing: rows change only by SQL, not in the grid, imports or generated data")}
            </label>

            </section>

            <section className="form-section">
            <h3>{t("Agents")}</h3>
            <AgentAccessField conn={conn} onChange={(access) => set("agent_access", access)} />
            </section>

            {projects.length > 0 && (
              <section className="form-section">
                <h3>{t("Projects")}</h3>
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
                        {on && <Icon name="commit" size={12} />}
                      </button>
                    );
                  })}
                </div>
              </section>
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
              <PortForwardFields conn={conn} tunnel={!!(profile ? profile.ssh : conn.ssh)} setOption={setOption} />
              <label className="field">
                <span>{t("Proxy")}</span>
                <input
                  value={conn.options?.proxy ?? ""}
                  onChange={(e) => setOption("proxy", e.target.value)}
                  placeholder="socks5://proxy.example.com:1080"
                  spellCheck={false}
                  data-field="proxy"
                />
              </label>
              <p className="muted small">
                {tNodes("SOCKS5 or HTTP, before the SSH server when there is one: {socks} or {http}. A password can come from {env}.", {
                  socks: <code>socks5://user:password@host:1080</code>,
                  http: <code>http://host:3128</code>,
                  env: <code>{"${env:PROXY_PASSWORD}"}</code>,
                })}
              </p>
              <>
                <label className="field">
                  <span>{t("Command before connecting")}</span>
                  <input
                    value={conn.options?.before_connect ?? ""}
                    onChange={(e) => setOption("before_connect", e.target.value)}
                    placeholder={WINDOWS ? 'rasdial "Office VPN"' : "nmcli connection up office-vpn"}
                    spellCheck={false}
                    data-field="before_connect"
                  />
                </label>
                <label className="field">
                  <span>{t("Command after disconnecting")}</span>
                  <input
                    value={conn.options?.after_disconnect ?? ""}
                    onChange={(e) => setOption("after_disconnect", e.target.value)}
                    placeholder={WINDOWS ? 'rasdial "Office VPN" /disconnect' : "nmcli connection down office-vpn"}
                    spellCheck={false}
                    data-field="after_disconnect"
                  />
                </label>
              </>
              <p className="muted small">
                {t("Run by the system's shell, e.g. to bring a VPN up. The first runs before the first session that needs it, which waits for its end, a minute at most, and fails with it; the second runs when the last such session closes.")}
              </p>
            </div>
          )}

          <div data-page="init" hidden={page !== "init"} className="dialog-page">
            <p className="muted small">{t("Runs after every connection, and again when a lost connection is reopened.")}</p>
            <div className="field">
              <span>{t("Variables file (.env)")}</span>
              <div className="row tight">
                <input
                  className="grow"
                  value={conn.options?.env_file ?? ""}
                  onChange={(e) => setOption("env_file", e.target.value)}
                  placeholder={t("None: the environment's variables only")}
                  data-field="env_file"
                />
                <button
                  type="button"
                  onClick={async () => {
                    const path = await open({ multiple: false, directory: false });
                    if (typeof path === "string") setOption("env_file", path);
                  }}
                >
                  {t("Browse…")}
                </button>
              </div>
              <small className="muted">
                {tNodes("Any field, the password too, can read {example}: from the environment, then from this file.", {
                  example: <code key="example">{"${env:NAME}"}</code>,
                })}
              </small>
            </div>
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
          </div>
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

          <div className="actions">
            <button type="button" onClick={test} disabled={busy} aria-busy={testing || undefined} data-action="test-connection">
              {testing ? <Busy label={t("Testing…")} /> : t("Test connection")}
            </button>
            {testing && (
              <button type="button" onClick={cancelTest} data-action="cancel-test">
                {t("Cancel test")}
              </button>
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
            <span className="spacer" />
            {!existing && (
              <button type="button" onClick={() => back()} disabled={busy} title={t("Choose another database; this form is cleared")}>
                {t("Back")}
              </button>
            )}
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
