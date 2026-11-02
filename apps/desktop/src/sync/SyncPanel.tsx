import { useEffect, useState } from "react";
import { api, type SyncConflict, type SyncStatus } from "../api";
import { plural, t } from "../i18n";
import { Busy } from "../ui/State";
import { pickFolder } from "../projects/shared";
import { onSyncRan, runSync, scheduleSync, syncErrorText, syncSummary } from "./background";

/** What a conflict says, in one line. */
function conflictText(c: SyncConflict): string {
  if (c.kind === "first_sync") {
    if (c.copy) {
      return t("{name} ({what}) differed from {machine}'s at the first sync: the synced version was kept, this computer's is kept as {copy}.", {
        name: c.name,
        what: c.what,
        machine: c.machine,
        copy: c.copy,
      });
    }
    return c.replaced
      ? t("{name} ({what}) differed from {machine}'s at the first sync: the synced version was kept; this computer had {value}.", {
          name: c.name,
          what: c.what,
          machine: c.machine,
          value: c.replaced,
        })
      : t("{name} ({what}) differed from {machine}'s at the first sync: the synced version was kept.", { name: c.name, what: c.what, machine: c.machine });
  }
  if (c.kind === "deleted_and_edited") {
    return t("{name} ({what}) was deleted on {machine} and changed here: the change was kept.", { name: c.name, what: c.what, machine: c.machine });
  }
  return c.copy
    ? t("{name} ({what}) was changed on {machine} and here: the other version is kept as {copy}.", { name: c.name, what: c.what, machine: c.machine, copy: c.copy })
    : t("{name} ({what}) was changed on {machine} and here: the later change was kept.", { name: c.name, what: c.what, machine: c.machine });
}

/** Settings, Sync: the folder, the passphrase, when it runs, and the conflicts found. The
 *  passphrase is not the master password and is never written to the folder: forgetting it
 *  loses the synced copy (the data on each computer stays). */
export function SyncPanel() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [folder, setFolder] = useState("");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [minutes, setMinutes] = useState(15);
  const [onStart, setOnStart] = useState(true);
  const [busy, setBusy] = useState(false);
  // Which button's action is under way, for its spinner; the last message stays until the new one.
  const [doing, setDoing] = useState<"save" | "sync" | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const show = (s: SyncStatus, keepTyped = false) => {
    setStatus(s);
    setFolder((typed) => (keepTyped && typed) || (s.folder ?? ""));
    setMinutes(s.interval_minutes);
    setOnStart(s.on_start);
  };

  useEffect(() => {
    // What was typed before the answer came stays.
    api.syncStatus().then((s) => show(s, true), (e) => setMessage({ ok: false, text: syncErrorText(e) }));
    // A run in the background changes the time and the conflicts.
    return onSyncRan(() => void api.syncStatus().then(setStatus, () => {}));
  }, []);

  async function browse() {
    const picked = await pickFolder(t("Choose the sync folder"), folder || undefined);
    if (picked) setFolder(picked);
  }

  const needsPassphrase = !status?.has_passphrase || passphrase !== "";
  const mismatch = needsPassphrase && passphrase !== confirm;

  async function save(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setDoing("save");
    try {
      show(await api.syncConfigure(folder, passphrase === "" ? null : passphrase, minutes, onStart));
      setPassphrase("");
      setConfirm("");
      void scheduleSync();
      setMessage({ ok: true, text: t("Sync is set up. Use Sync now to start.") });
    } catch (err) {
      setMessage({ ok: false, text: syncErrorText(err) });
    } finally {
      setBusy(false);
      setDoing(null);
    }
  }

  async function syncNow() {
    setBusy(true);
    setDoing("sync");
    try {
      const report = await runSync();
      setMessage({ ok: true, text: syncSummary(report) ?? t("Everything is in sync.") });
      setStatus(await api.syncStatus());
    } catch (err) {
      setMessage({ ok: false, text: syncErrorText(err) });
    } finally {
      setBusy(false);
      setDoing(null);
    }
  }

  async function turnOff() {
    setBusy(true);
    try {
      show(await api.syncDisconnect());
      void scheduleSync();
      setMessage({ ok: true, text: t("Sync is off. The file in the folder stays.") });
    } catch (err) {
      setMessage({ ok: false, text: syncErrorText(err) });
    } finally {
      setBusy(false);
      setDoing(null);
    }
  }

  async function clearConflicts() {
    try {
      setStatus(await api.syncClearConflicts());
    } catch (err) {
      setMessage({ ok: false, text: syncErrorText(err) });
    }
  }

  const on = status?.folder != null && status.has_passphrase;

  return (
    <form className="sync-panel" data-sync={on ? "on" : "off"} onSubmit={save}>
      <p className="muted small">
        {t("Keeps your settings, connections (without passwords), projects and scripts the same on your computers, through a folder you choose: a cloud drive, a network share or a repository. Everything in the file is encrypted with the sync passphrase.")}
      </p>
      <label className="field">
        <span>{t("Sync folder")}</span>
        <div className="row tight">
          <input data-field="sync-folder" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder={t("A folder on a cloud drive or network share")} />
          <button type="button" data-action="sync-browse" onClick={() => void browse()}>
            {t("Browse…")}
          </button>
        </div>
      </label>
      <label className="field">
        <span>{status?.has_passphrase ? t("New sync passphrase (empty: keep the saved one)") : t("Sync passphrase")}</span>
        <input data-field="sync-passphrase" type="password" autoComplete="off" value={passphrase} onChange={(e) => setPassphrase(e.target.value)} />
      </label>
      {needsPassphrase && (
        <label className="field">
          <span>{t("Repeat the sync passphrase")}</span>
          <input data-field="sync-confirm" type="password" autoComplete="off" value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        </label>
      )}
      <p className="muted small">
        {t("This is not the master password. Keep it safe: it is not stored in the folder, and without it the synced copy cannot be read. On another computer, choose the same folder and enter the same passphrase.")}
      </p>
      <label className="setting">
        <span>
          <span className="setting-title">{t("Sync every (minutes)")}</span>
        </span>
        <input data-field="sync-interval" type="number" min={1} max={1440} value={minutes} onChange={(e) => setMinutes(Math.max(1, Number(e.target.value) || 1))} />
      </label>
      <label className="setting">
        <span>
          <span className="setting-title">{t("Sync when the app starts")}</span>
        </span>
        <input data-field="sync-on-start" type="checkbox" checked={onStart} onChange={(e) => setOnStart(e.target.checked)} />
      </label>
      <div className="muted small" data-field="sync-last">
        {status?.last_sync ? t("Last sync: {time}", { time: new Date(status.last_sync).toLocaleString() }) : t("Not synced yet.")}
      </div>
      {message && (
        <div className={`notice ${message.ok ? "ok" : "error"} small`} data-field="sync-message">
          {message.text}
        </div>
      )}
      <div className="actions">
        <button type="submit" className="primary" data-action="sync-save" aria-busy={doing === "save" || undefined} disabled={busy || folder.trim() === "" || (needsPassphrase && (passphrase === "" || mismatch))}>
          {doing === "save" ? <Busy label={t("Saving…")} /> : on ? t("Save") : t("Turn on sync")}
        </button>
        <button type="button" data-action="sync-now" aria-busy={doing === "sync" || undefined} disabled={busy || !on} onClick={() => void syncNow()}>
          {doing === "sync" ? <Busy label={t("Syncing…")} /> : t("Sync now")}
        </button>
        <button type="button" data-action="sync-off" disabled={busy || !on} onClick={() => void turnOff()}>
          {t("Turn off")}
        </button>
      </div>
      {mismatch && passphrase !== "" && confirm !== "" && <div className="muted small">{t("The passphrases do not match.")}</div>}
      {status && status.conflicts.length > 0 && (
        <section data-field="sync-conflicts">
          <h4>{plural(status.conflicts.length, "{n} change on both computers", "{n} changes on both computers")}</h4>
          <ul>
            {status.conflicts.map((c, i) => (
              <li key={`${c.at}-${i}`} data-conflict={c.name}>
                {conflictText(c)}
              </li>
            ))}
          </ul>
          <button type="button" data-action="sync-clear" onClick={() => void clearConflicts()}>
            {t("Clear the list")}
          </button>
        </section>
      )}
    </form>
  );
}
