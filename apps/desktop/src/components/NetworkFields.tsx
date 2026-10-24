import { open } from "@tauri-apps/plugin-dialog";
import type { DriverKind, PasswordInput, SavedSsh, SshSecrets, SslMode } from "../api";
import { SshFields, newSsh } from "./SshFields";

/** The SSH tunnel and SSL settings of a connection or of a network profile. */
export interface NetworkSettings {
  ssh?: SavedSsh | null;
  ssl_mode: SslMode;
  ssl_root_cert: string | null;
  ssl_client_cert?: string | null;
  ssl_client_key?: string | null;
}

export const SSL_MODES: { value: SslMode; label: string }[] = [
  { value: "disable", label: "disable — plaintext" },
  { value: "prefer", label: "prefer — TLS if available, unverified" },
  { value: "require", label: "require — TLS, unverified" },
  { value: "verify-ca", label: "verify-ca — TLS, verify certificate" },
  { value: "verify-full", label: "verify-full — TLS, verify certificate and host" },
];

/** An SSH server and its jump hosts. */
function sshChain(ssh: SavedSsh | null | undefined): SavedSsh[] {
  const out: SavedSsh[] = [];
  for (let s = ssh; s; s = s.jump) out.push(s);
  return out;
}

/** The `secret_id`s of `ssh` and its jump hosts. */
export const sshSecretIds = (ssh: SavedSsh | null | undefined) => new Set(sshChain(ssh).map((s) => s.secret_id));

/** The SSH secrets to send for `ssh`: typed ones, else keep what is saved (servers never
 *  saved have nothing to keep). */
export function sshSecretsOf(ssh: SavedSsh | null | undefined, typed: Record<string, string>, saved: Set<string>): SshSecrets {
  return Object.fromEntries(
    sshChain(ssh).map((s): [string, PasswordInput] => {
      const value = typed[s.secret_id] ?? "";
      if (value !== "") return [s.secret_id, { action: "set", value }];
      return [s.secret_id, saved.has(s.secret_id) && s.auth !== "agent" ? { action: "keep" } : { action: "clear" }];
    }),
  );
}

interface Props<T extends NetworkSettings> {
  value: T;
  onChange: (next: T) => void;
  /** For the CA hint: MySQL falls back to built-in roots, PostgreSQL to the system store. */
  driver: DriverKind | null;
  /** SSH passwords and passphrases typed, by `secret_id`. */
  typed: Record<string, string>;
  onTyped: (secretId: string, value: string) => void;
  /** Servers with a saved secret, for the placeholders. */
  saved: Set<string>;
}

async function browse(title: string): Promise<string | null> {
  const path = await open({ multiple: false, title });
  return typeof path === "string" ? path : null;
}

/** SSH tunnel with an optional jump host, then the SSL mode and certificates. */
export function NetworkFields<T extends NetworkSettings>({ value, onChange, driver, typed, onTyped, saved }: Props<T>) {
  const set = <K extends keyof NetworkSettings>(key: K, v: NetworkSettings[K]) => onChange({ ...value, [key]: v });
  const ssh = value.ssh ?? null;
  const verifies = value.ssl_mode === "verify-ca" || value.ssl_mode === "verify-full";

  const pathField = (key: "ssl_root_cert" | "ssl_client_cert" | "ssl_client_key", label: string, placeholder: string) => (
    <label className="field">
      <span>{label}</span>
      <div className="row tight">
        <input
          value={value[key] ?? ""}
          onChange={(e) => set(key, e.target.value || null)}
          placeholder={placeholder}
          data-field={key}
        />
        <button
          type="button"
          onClick={() => void browse(label).then((path) => path && set(key, path))}
        >
          Browse…
        </button>
      </div>
    </label>
  );

  return (
    <>
      <label className="check">
        <input type="checkbox" checked={ssh !== null} onChange={(e) => set("ssh", e.target.checked ? newSsh() : null)} />
        Connect through an SSH tunnel
      </label>
      {ssh && (
        <fieldset className="ssh-section">
          <SshFields
            ssh={ssh}
            onChange={(next) => set("ssh", next)}
            secret={typed[ssh.secret_id] ?? ""}
            onSecret={(v) => onTyped(ssh.secret_id, v)}
            saved={saved.has(ssh.secret_id)}
          />
          <label className="check">
            <input
              type="checkbox"
              checked={ssh.jump !== null}
              onChange={(e) => set("ssh", { ...ssh, jump: e.target.checked ? newSsh() : null })}
            />
            Through a jump host first
          </label>
          {ssh.jump && (
            <fieldset className="ssh-section">
              <legend className="muted small">Jump host</legend>
              <SshFields
                ssh={ssh.jump}
                onChange={(jump) => set("ssh", { ...ssh, jump })}
                secret={typed[ssh.jump.secret_id] ?? ""}
                onSecret={(v) => onTyped(ssh.jump!.secret_id, v)}
                saved={saved.has(ssh.jump.secret_id)}
              />
            </fieldset>
          )}
        </fieldset>
      )}

      <label className="field">
        <span>SSL mode</span>
        <select value={value.ssl_mode} onChange={(e) => set("ssl_mode", e.target.value as SslMode)}>
          {SSL_MODES.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
      </label>
      {verifies &&
        pathField(
          "ssl_root_cert",
          `CA certificate (PEM) — empty uses the ${driver === "mysql" ? "built-in Mozilla roots" : "system trust store"}`,
          "C:\\certs\\root.crt",
        )}
      {value.ssl_mode !== "disable" && (
        <>
          {pathField(
            "ssl_client_cert",
            "Client certificate (PEM) — for servers that authenticate clients by certificate",
            "C:\\certs\\client.crt",
          )}
          {pathField("ssl_client_key", "Client key (PEM)", "C:\\certs\\client.key")}
        </>
      )}
    </>
  );
}
