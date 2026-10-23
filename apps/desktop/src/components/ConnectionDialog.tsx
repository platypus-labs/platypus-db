import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  toCommandError,
  type DriverKind,
  type PasswordInput,
  type SavedConnection,
  type ServerInfo,
  type SslMode,
} from "../api";
import { basename } from "../workspace";

interface Props {
  initial: SavedConnection;
  /** True when editing a connection that already exists in the store. */
  existing: boolean;
  onSaved: (all: SavedConnection[]) => void;
  onClose: () => void;
}

const SSL_MODES: { value: SslMode; label: string }[] = [
  { value: "disable", label: "disable — plaintext" },
  { value: "prefer", label: "prefer — TLS if available, unverified" },
  { value: "require", label: "require — TLS, unverified" },
  { value: "verify-ca", label: "verify-ca — TLS, verify certificate" },
  { value: "verify-full", label: "verify-full — TLS, verify certificate and host" },
];

const COLORS = ["", "#2f9e44", "#1c7ed6", "#f08c00", "#d6336c", "#c92a2a"];

const DEFAULT_PORTS: Record<DriverKind, number | null> = { postgres: 5432, mysql: 3306, sqlite: null };

const SCHEMES: Record<string, DriverKind> = {
  "postgres:": "postgres",
  "postgresql:": "postgres",
  "mysql:": "mysql",
  "mariadb:": "mysql",
};

/** Turns a `postgres://` or `mysql://` URL into form fields. */
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

export function ConnectionDialog({ initial, existing, onSaved, onClose }: Props) {
  const [conn, setConn] = useState(initial);
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<
    { ok: true; server: ServerInfo } | { ok: false; message: string } | null
  >(null);

  const set = <K extends keyof SavedConnection>(key: K, value: SavedConnection[K]) =>
    setConn((c) => ({ ...c, [key]: value }));

  // An empty password field keeps the stored one when editing.
  const passwordInput = (): PasswordInput =>
    password !== ""
      ? { action: "set", value: password }
      : existing
        ? { action: "keep" }
        : { action: "clear" };

  async function test() {
    setBusy(true);
    setResult(null);
    try {
      setResult({ ok: true, server: await api.testConnection(conn, passwordInput()) });
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
      onSaved(await api.saveConnection(conn, passwordInput()));
    } catch (err) {
      setResult({ ok: false, message: toCommandError(err).message });
      setBusy(false);
    }
  }

  function changeDriver(driver: DriverKind) {
    setConn((c) => ({
      ...c,
      driver,
      // Keep a custom port, but follow the default one when switching drivers.
      port: c.port === DEFAULT_PORTS[c.driver] ? DEFAULT_PORTS[driver] : c.port,
      database: c.driver === "sqlite" || driver === "sqlite" ? "" : c.database,
    }));
  }

  async function browseFile() {
    const path = await open({
      multiple: false,
      filters: [
        { name: "SQLite database", extensions: ["db", "sqlite", "sqlite3", "db3"] },
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

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form className="modal" onSubmit={save} onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{existing ? "Edit connection" : "New connection"}</h2>

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

        {conn.driver === "sqlite" ? (
          <label className="field">
            <span>Database file</span>
            <div className="row tight">
              <input

                required
                value={conn.database}
                onChange={(e) => set("database", e.target.value)}
                placeholder="C:\data\app.db"
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
              <span>Database{conn.driver === "mysql" ? " (optional)" : ""}</span>
              <input value={conn.database} onChange={(e) => set("database", e.target.value)} />
            </label>

            <div className="row">
              <label className="field grow">
                <span>User</span>
                <input value={conn.user} onChange={(e) => set("user", e.target.value)} />
              </label>
              <label className="field grow">
                <span>Password</span>
                <input
                  type="password"
                  value={password}
                  placeholder={existing && conn.save_password ? "•••••• (saved)" : ""}
                  onChange={(e) => setPassword(e.target.value)}
                  autoComplete="off"
                />
              </label>
            </div>

            <label className="check">
              <input
                type="checkbox"
                checked={conn.save_password}
                onChange={(e) => set("save_password", e.target.checked)}
              />
              Save password in the system keychain
            </label>

            <div className="row">
              <label className="field grow">
                <span>SSL mode</span>
                <select value={conn.ssl_mode} onChange={(e) => set("ssl_mode", e.target.value as SslMode)}>
                  {SSL_MODES.map((m) => (
                    <option key={m.value} value={m.value}>
                      {m.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
            {(conn.ssl_mode === "verify-ca" || conn.ssl_mode === "verify-full") && (
              <label className="field">
                <span>
                  CA certificate (PEM) — empty uses the{" "}
                  {conn.driver === "mysql" ? "built-in Mozilla roots" : "system trust store"}
                </span>
                <input
                  value={conn.ssl_root_cert ?? ""}
                  onChange={(e) => set("ssl_root_cert", e.target.value || null)}
                  placeholder="C:\certs\root.crt"
                />
              </label>
            )}
          </>
        )}

        <label className="check">
          <input
            type="checkbox"
            checked={conn.read_only}
            onChange={(e) => set("read_only", e.target.checked)}
          />
          Read-only (warn before modifying data)
        </label>

        {result && (
          <div className={result.ok ? "notice ok" : "notice error"}>
            {result.ok
              ? `Connected: ${result.server.product} ${result.server.version}${
                  conn.driver === "sqlite" ? "" : result.server.encrypted ? " · TLS" : " · not encrypted"
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
  );
}
