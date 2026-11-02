import { useEffect, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, toCommandError, type DiagnosticsInfo } from "../api";
import { t } from "../i18n";
import { copyText } from "../ui/copy";

/** Asks where to save the diagnostic package, then saves it; resolves with the path, or
 *  `null` when the dialog was closed. */
async function saveDiagnostics(): Promise<string | null> {
  const day = new Date().toISOString().slice(0, 10);
  const path = await save({ filters: [{ name: "Zip", extensions: ["zip"] }], defaultPath: `platypus-diagnostics-${day}.zip` });
  if (!path) return null;
  await api.saveDiagnostics(path);
  return path;
}

/** Errors the web view did not handle go to the app's log. */
export function startErrorLog() {
  const log = (message: string) => void api.logError(message).catch(() => {});
  window.addEventListener("error", (e) => log(`${e.message} (${e.filename}:${e.lineno}:${e.colno})${e.error instanceof Error && e.error.stack ? `\n${e.error.stack}` : ""}`));
  window.addEventListener("unhandledrejection", (e) => {
    const reason: unknown = e.reason;
    log(`unhandled rejection: ${reason instanceof Error ? (reason.stack ?? reason.message) : toCommandError(reason).message}`);
  });
}

/** The Diagnostics page of the Settings: where the logs are, and the package to attach to a
 *  bug report. */
export function DiagnosticsPanel() {
  const [info, setInfo] = useState<DiagnosticsInfo | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.diagnosticsInfo().then(setInfo, (e: unknown) => setError(toCommandError(e).message));
  }, []);

  async function saveIt() {
    setError(null);
    try {
      setSaved(await saveDiagnostics());
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="diagnostics-panel">
      <p className="muted small">
        {t("PlatypusDB keeps a log of its starts, errors and crashes on this computer; nothing is sent anywhere. To report a problem, save the diagnostic package and attach it to the report.")}
      </p>
      <p className="muted small">
        {t("The package holds the version, the system, the settings without passwords, how many connections of each database you have, and the logs. Error messages in the logs may name a server or a table: look at them before sharing the package.")}
      </p>
      {info?.log_dir && (
        <div className="row tight diagnostics-logs">
          <span className="small">
            {t("Logs")}: <code data-field="log_dir">{info.log_dir}</code>
          </span>
          <button type="button" className="small" onClick={() => void copyText(info.log_dir ?? "", t("Logs"))}>
            {t("Copy")}
          </button>
        </div>
      )}
      <div className="row tight">
        <button type="button" onClick={() => void saveIt()} data-action="save-diagnostics">
          {t("Save diagnostic package…")}
        </button>
      </div>
      {saved && <div className="notice small">{t("Saved to {path}", { path: saved })}</div>}
      {error && <div className="notice error small">{error}</div>}
    </div>
  );
}

/** After a crash, offers the diagnostic package once: saving it or dismissing the banner
 *  forgets the crash. */
export function CrashBanner() {
  const [crashed, setCrashed] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.diagnosticsInfo().then((i) => setCrashed(i.crash !== null), () => {});
  }, []);

  if (!crashed) return null;
  const dismiss = () => {
    setCrashed(false);
    void api.dismissCrash();
  };
  async function saveIt() {
    try {
      if (await saveDiagnostics()) dismiss();
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="update-banner crash-banner">
      <span>{error ?? t("PlatypusDB closed unexpectedly last time. A diagnostic package helps report the problem.")}</span>
      <span className="spacer" />
      <button type="button" onClick={dismiss}>
        {t("Dismiss")}
      </button>
      <button type="button" className="primary" onClick={() => void saveIt()}>
        {t("Save diagnostic package…")}
      </button>
    </div>
  );
}
