import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import type { SavedSsh, SshAuthKind } from "../api";
import { t } from "../i18n";

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
  // The port as typed: it may be empty while editing, and 22 is used then.
  const [portText, setPortText] = useState(String(ssh.port));
  const shownPort = (Number(portText) || 22) === ssh.port ? portText : String(ssh.port);

  async function browseKey() {
    const path = await open({ multiple: false, title: "Private key" });
    if (typeof path === "string") set("key_path", path);
  }

  return (
    <>
      <div className="row">
        <label className="field grow">
          <span>{t("SSH host")}</span>
          <input required value={ssh.host} onChange={(e) => set("host", e.target.value)} placeholder="bastion.example.com" />
        </label>
        <label className="field port">
          <span>{t("Port")}</span>
          <input
            type="number"
            min={1}
            max={65535}
            value={shownPort}
            placeholder="22"
            onChange={(e) => {
              setPortText(e.target.value);
              set("port", Number(e.target.value) || 22);
            }}
          />
        </label>
      </div>
      <div className="row">
        <label className="field grow">
          <span>{t("SSH user")}</span>
          <input required value={ssh.user} onChange={(e) => set("user", e.target.value)} />
        </label>
        <label className="field grow">
          <span>{t("Login")}</span>
          <select value={ssh.auth} onChange={(e) => set("auth", e.target.value as SshAuthKind)}>
            <option value="password">{t("Password")}</option>
            <option value="key">{t("Private key")}</option>
            <option value="agent">{t("SSH agent")}</option>
          </select>
        </label>
      </div>
      {ssh.auth === "key" && (
        <label className="field">
          <span>{t("Private key file")}</span>
          <div className="row tight">
            <input
              required
              value={ssh.key_path ?? ""}
              onChange={(e) => set("key_path", e.target.value || null)}
              placeholder="~/.ssh/id_ed25519"
            />
            <button type="button" onClick={() => void browseKey()}>
              {t("Browse…")}
            </button>
          </div>
        </label>
      )}
      {ssh.auth !== "agent" && (
        <label className="field">
          <span>{t(ssh.auth === "key" ? "Key passphrase (if any) — saved in the system keychain" : "SSH password — saved in the system keychain")}</span>
          <input
            type="password"
            autoComplete="off"
            value={secret}
            placeholder={saved ? t("Empty keeps the saved one") : ""}
            onChange={(e) => onSecret(e.target.value)}
          />
        </label>
      )}
    </>
  );
}
