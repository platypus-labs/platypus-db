import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  toCommandError,
  type ConnectionType,
  type DriverKind,
  type PasswordInput,
  type Project,
  type SavedConnection,
  type SavedSsh,
  type ServerInfo,
  type SshSecrets,
  type SslMode,
} from "../api";
import { CONNECTION_TYPES } from "../connections";
import { toggled } from "../sets";
import { basename } from "../workspace";
import { SshFields, newSsh } from "./SshFields";

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

/** An SSH server and its jump hosts. */
function chain(ssh: SavedSsh | null | undefined): SavedSsh[] {
  const out: SavedSsh[] = [];
  for (let s = ssh; s; s = s.jump) out.push(s);
  return out;
}

export function ConnectionDialog({ initial, existing, projects, memberOf, onSaved, onClose }: Props) {
  const [conn, setConn] = useState(initial);
  const [inProjects, setInProjects] = useState(() => new Set(memberOf));
  const [password, setPassword] = useState("");
  /** SSH passwords and passphrases typed, by `secret_id`. */
  const [sshTyped, setSshTyped] = useState<Record<string, string>>({});
  const [page, setPage] = useState<"general" | "ssh">("general");
  const savedSsh = new Set(existing ? chain(initial.ssh).map((s) => s.secret_id) : []);
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

  // Empty fields keep what is saved; servers never saved have nothing to keep.
  const sshSecrets = (): SshSecrets =>
    Object.fromEntries(
      chain(conn.ssh).map((s): [string, PasswordInput] => {
        const typed = sshTyped[s.secret_id] ?? "";
        if (typed !== "") return [s.secret_id, { action: "set", value: typed }];
        return [s.secret_id, savedSsh.has(s.secret_id) && s.auth !== "agent" ? { action: "keep" } : { action: "clear" }];
      }),
    );

  const ssh = conn.ssh ?? null;
  const setSsh = (next: SavedSsh | null) => set("ssh", next);

  async function test() {
    setBusy(true);
    setResult(null);
    try {
      setResult({ ok: true, server: await api.testConnection(conn, passwordInput(), sshSecrets()) });
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
      onSaved(await api.saveConnection(conn, passwordInput(), sshSecrets(), [...inProjects]));
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
      <form
        className="modal"
        onSubmit={save}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
        // A required field left empty on the other tab: show that tab.
        onInvalidCapture={(e) => {
          const tab = (e.target as HTMLElement).closest<HTMLElement>("[data-page]")?.dataset.page;
          if (tab === "general" || tab === "ssh") setPage(tab);
        }}
      >
        <h2>{existing ? "Edit connection" : "New connection"}</h2>
        {conn.driver !== "sqlite" && (
          <div className="dialog-tabs" role="tablist">
            <button type="button" role="tab" aria-selected={page === "general"} onClick={() => setPage("general")}>
              General
            </button>
            <button type="button" role="tab" aria-selected={page === "ssh"} onClick={() => setPage("ssh")}>
              SSH{ssh ? " ●" : ""}
            </button>
          </div>
        )}
        <div data-page="general" hidden={page !== "general" && conn.driver !== "sqlite"} className="dialog-page">

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

        {conn.driver !== "sqlite" && (
          <div data-page="ssh" hidden={page !== "ssh"} className="dialog-page">
        <label className="check">
          <input type="checkbox" checked={ssh !== null} onChange={(e) => setSsh(e.target.checked ? newSsh() : null)} />
          Connect through an SSH tunnel
        </label>
        {ssh && (
          <fieldset className="ssh-section">
            <SshFields
              ssh={ssh}
              onChange={(next) => setSsh(next)}
              secret={sshTyped[ssh.secret_id] ?? ""}
              onSecret={(v) => setSshTyped((t) => ({ ...t, [ssh.secret_id]: v }))}
              saved={savedSsh.has(ssh.secret_id)}
            />
            <label className="check">
              <input
                type="checkbox"
                checked={ssh.jump !== null}
                onChange={(e) => setSsh({ ...ssh, jump: e.target.checked ? newSsh() : null })}
              />
              Through a jump host first
            </label>
            {ssh.jump && (
              <fieldset className="ssh-section">
                <legend className="muted small">Jump host</legend>
                <SshFields
                  ssh={ssh.jump}
                  onChange={(jump) => setSsh({ ...ssh, jump })}
                  secret={sshTyped[ssh.jump.secret_id] ?? ""}
                  onSecret={(v) => setSshTyped((t) => ({ ...t, [ssh.jump!.secret_id]: v }))}
                  saved={savedSsh.has(ssh.jump.secret_id)}
                />
              </fieldset>
            )}
          </fieldset>
        )}
          </div>
        )}

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
