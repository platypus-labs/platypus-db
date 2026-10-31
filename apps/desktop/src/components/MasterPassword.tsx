import { useEffect, useState } from "react";
import { api, toCommandError, type VaultStatus } from "../api";
import { t } from "../i18n";

/** Asks for the master password; `onDone(false)` when the user goes on without it. */
export function UnlockDialog({ onDone }: { onDone: (unlocked: boolean) => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function unlock(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api.unlockVault(password);
      onDone(true);
    } catch (err) {
      setError(toCommandError(err).message);
      setBusy(false);
    }
  }

  return (
    <div className="modal-backdrop">
      <form className="modal small unlock-vault" onSubmit={unlock} onKeyDown={(e) => e.key === "Escape" && onDone(false)}>
        <h2>{t("Master password")}</h2>
        <p className="muted small">{t("The saved passwords are encrypted. Enter the master password to use them.")}</p>
        <label className="field">
          <span>{t("Master password")}</span>
          <input autoFocus type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
        </label>
        {error && <div className="notice error small">{error}</div>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={() => onDone(false)} disabled={busy}>
            {t("Later")}
          </button>
          <button type="submit" className="primary" disabled={busy || password === ""}>
            {t("Unlock")}
          </button>
        </div>
      </form>
    </div>
  );
}

type Mode = "enable" | "change" | "disable" | null;

/** The shortest new master password the vault takes (`MIN_PASSWORD_LEN` in platypus-store). */
const MIN_PASSWORD_LENGTH = 8;

/** The master password in the Settings: turn it on or off, change it, lock now. */
export function MasterPasswordPanel() {
  const [status, setStatus] = useState<VaultStatus | null>(null);
  const [mode, setMode] = useState<Mode>(null);
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    api.vaultStatus().then(setStatus, (e) => setMessage({ ok: false, text: toCommandError(e).message }));
  }, []);

  const start = (m: Mode) => {
    setMode(m);
    setCurrent("");
    setNext("");
    setConfirm("");
    setMessage(null);
  };

  async function run(action: () => Promise<VaultStatus>, done: string) {
    setBusy(true);
    setMessage(null);
    try {
      setStatus(await action());
      setMode(null);
      setMessage({ ok: true, text: done });
    } catch (e) {
      setMessage({ ok: false, text: toCommandError(e).message });
    } finally {
      setBusy(false);
    }
  }

  const needsNew = mode === "enable" || mode === "change";
  const mismatch = needsNew && confirm !== "" && confirm !== next;
  // As the vault counts: characters, not UTF-16 units.
  const tooShort = needsNew && next !== "" && [...next].length < MIN_PASSWORD_LENGTH;
  const ready =
    (mode === "disable" && current !== "") ||
    (mode === "enable" && next !== "" && !tooShort && next === confirm) ||
    (mode === "change" && current !== "" && next !== "" && !tooShort && next === confirm);

  function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!ready) return;
    if (mode === "enable") void run(() => api.enableVault(next), t("Saved passwords moved into the encrypted file."));
    if (mode === "change") void run(() => api.changeVaultPassword(current, next), t("Master password changed."));
    if (mode === "disable") void run(() => api.disableVault(current), t("Saved passwords moved back to the system keychain."));
  }

  if (!status) return null;
  return (
    <div className="master-password">
      <p className="muted small">
        {t(
          status.enabled
            ? status.unlocked
              ? "Saved passwords are in an encrypted file, unlocked for this session."
              : "Saved passwords are in an encrypted file, locked."
            : "Saved passwords are in the system keychain. A master password moves them into a file encrypted with it (Argon2id and AES-256-GCM).",
        )}
      </p>
      {mode === null && (
        <div className="row tight">
          {!status.enabled && (
            <button type="button" onClick={() => start("enable")}>
              {t("Set a master password…")}
            </button>
          )}
          {status.enabled && (
            <>
              <button type="button" onClick={() => start("change")}>
                {t("Change…")}
              </button>
              <button type="button" onClick={() => start("disable")}>
                {t("Turn off…")}
              </button>
              <button type="button" disabled={!status.unlocked} onClick={() => void run(() => api.lockVault(), t("Locked."))}>
                {t("Lock now")}
              </button>
            </>
          )}
        </div>
      )}
      {mode !== null && (
        <form className="master-password-form" onSubmit={submit}>
          {mode !== "enable" && (
            <label className="field">
              <span>{t("Current master password")}</span>
              <input autoFocus type="password" value={current} onChange={(e) => setCurrent(e.target.value)} />
            </label>
          )}
          {needsNew && (
            <>
              <label className="field">
                <span>{t("New master password")}</span>
                <input
                  autoFocus={mode === "enable"}
                  type="password"
                  value={next}
                  onChange={(e) => setNext(e.target.value)}
                />
              </label>
              <label className="field">
                <span>{t("Repeat it")}</span>
                <input type="password" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
              </label>
              {tooShort && <div className="error-text small">{t("The master password must be at least 8 characters long.")}</div>}
              {mismatch && <div className="error-text small">{t("The two passwords differ.")}</div>}
              {mode === "enable" && (
                <div className="muted small">{t("It cannot be recovered: without it the saved passwords are lost.")}</div>
              )}
            </>
          )}
          <div className="row tight">
            <button type="submit" className="primary" disabled={busy || !ready}>
              {t(mode === "enable" ? "Turn on" : mode === "change" ? "Change" : "Turn off")}
            </button>
            <button type="button" onClick={() => start(null)} disabled={busy}>
              {t("Cancel")}
            </button>
          </div>
        </form>
      )}
      {message && <div className={`notice small ${message.ok ? "ok" : "error"}`}>{message.text}</div>}
    </div>
  );
}
