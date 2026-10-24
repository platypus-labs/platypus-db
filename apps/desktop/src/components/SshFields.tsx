import { open } from "@tauri-apps/plugin-dialog";
import type { SavedSsh, SshAuthKind } from "../api";

/** A new SSH server with its own keychain entry. */
export function newSsh(): SavedSsh {
  return { host: "", port: 22, user: "", auth: "password", key_path: null, secret_id: crypto.randomUUID(), jump: null };
}

interface Props {
  ssh: SavedSsh;
  onChange: (ssh: SavedSsh) => void;
  /** The typed password or passphrase; empty keeps the saved one. */
  secret: string;
  onSecret: (value: string) => void;
  /** Whether a secret is saved already, for the placeholder. */
  saved: boolean;
}

/** Host, user and login of one SSH server. */
export function SshFields({ ssh, onChange, secret, onSecret, saved }: Props) {
  const set = <K extends keyof SavedSsh>(key: K, value: SavedSsh[K]) => onChange({ ...ssh, [key]: value });

  async function browseKey() {
    const path = await open({ multiple: false, title: "Private key" });
    if (typeof path === "string") set("key_path", path);
  }

  return (
    <>
      <div className="row">
        <label className="field grow">
          <span>SSH host</span>
          <input required value={ssh.host} onChange={(e) => set("host", e.target.value)} placeholder="bastion.example.com" />
        </label>
        <label className="field port">
          <span>Port</span>
          <input
            type="number"
            min={1}
            max={65535}
            value={ssh.port}
            onChange={(e) => set("port", Number(e.target.value) || 22)}
          />
        </label>
      </div>
      <div className="row">
        <label className="field grow">
          <span>SSH user</span>
          <input required value={ssh.user} onChange={(e) => set("user", e.target.value)} />
        </label>
        <label className="field grow">
          <span>Login</span>
          <select value={ssh.auth} onChange={(e) => set("auth", e.target.value as SshAuthKind)}>
            <option value="password">Password</option>
            <option value="key">Private key</option>
            <option value="agent">SSH agent</option>
          </select>
        </label>
      </div>
      {ssh.auth === "key" && (
        <label className="field">
          <span>Private key file</span>
          <div className="row tight">
            <input
              required
              value={ssh.key_path ?? ""}
              onChange={(e) => set("key_path", e.target.value || null)}
              placeholder="~/.ssh/id_ed25519"
            />
            <button type="button" onClick={() => void browseKey()}>
              Browse…
            </button>
          </div>
        </label>
      )}
      {ssh.auth !== "agent" && (
        <label className="field">
          <span>{ssh.auth === "key" ? "Key passphrase (if any)" : "SSH password"} — saved in the system keychain</span>
          <input
            type="password"
            autoComplete="off"
            value={secret}
            placeholder={saved ? "Empty keeps the saved one" : ""}
            onChange={(e) => onSecret(e.target.value)}
          />
        </label>
      )}
    </>
  );
}
