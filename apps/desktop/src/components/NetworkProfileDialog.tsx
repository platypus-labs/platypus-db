import { useState } from "react";
import { api, type DriverKind, type NetworkProfile, toCommandError } from "../api";
import { NetworkFields, sshSecretIds, sshSecretsOf } from "./NetworkFields";
import { ask } from "../ui/ask";
import { t } from "../i18n";

interface Props {
  initial: NetworkProfile;
  /** True when editing a profile that is saved already. */
  existing: boolean;
  /** The driver of the connection the dialog was opened from, for the hints. */
  driver: DriverKind;
  onSaved: (all: NetworkProfile[], saved: NetworkProfile | null) => void;
  onClose: () => void;
}

export function newNetworkProfile(): NetworkProfile {
  return {
    id: crypto.randomUUID(),
    name: "",
    ssl_mode: "prefer",
    ssl_root_cert: null,
    ssl_client_cert: null,
    ssl_client_key: null,
    ssh: null,
  };
}

/** Edits a network profile: an SSH tunnel and SSL settings shared by connections. */
export function NetworkProfileDialog({ initial, existing, driver, onSaved, onClose }: Props) {
  const [profile, setProfile] = useState(initial);
  const [typed, setTyped] = useState<Record<string, string>>({});
  const [saved] = useState(() => sshSecretIds(existing ? initial.ssh : null));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function run(action: () => Promise<void>) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (e) {
      setError(toCommandError(e).message);
      setBusy(false);
    }
  }

  const save = (e: React.FormEvent) => {
    e.preventDefault();
    void run(async () => onSaved(await api.saveNetworkProfile(profile, sshSecretsOf(profile.ssh, typed, saved)), profile));
  };
  const remove = async () => {
    if (!(await ask(t('Delete the network profile "{name}"?', { name: profile.name }), { confirm: t("Delete"), danger: true }))) return;
    void run(async () => onSaved(await api.deleteNetworkProfile(profile.id), null));
  };

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <form
        className="modal network-profile"
        onSubmit={save}
        onKeyDown={(e) => {
          if (e.key !== "Escape") return;
          // Only this dialog closes, not the connection dialog under it.
          e.stopPropagation();
          onClose();
        }}
      >
        <h2>{t(existing ? "Edit network profile" : "New network profile")}</h2>
        <p className="muted small">{t("The connections that use it share its SSH tunnel and SSL settings.")}</p>
        <label className="field">
          <span>{t("Name")}</span>
          <input
            autoFocus
            required
            value={profile.name}
            onChange={(e) => setProfile({ ...profile, name: e.target.value })}
            placeholder={t("Office VPN bastion")}
          />
        </label>
        <NetworkFields
          value={profile}
          onChange={setProfile}
          driver={driver}
          typed={typed}
          onTyped={(id, v) => setTyped((t) => ({ ...t, [id]: v }))}
          saved={saved}
        />
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          {existing && (
            <button type="button" onClick={() => void remove()} disabled={busy}>
              {t("Delete")}
            </button>
          )}
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
  );
}
