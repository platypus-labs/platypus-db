import { useEffect, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { type Update, check } from "@tauri-apps/plugin-updater";
import { t } from "../i18n";
import { workbench } from "../workbench";

const CHECK_NOW = "platypus:check-for-updates";

/** Looks for a newer release now (Help › Check for updates): the banner offers it, or a note
 *  says there is none. */
export function checkForUpdates() {
  window.dispatchEvent(new Event(CHECK_NOW));
}

/** Looks for a newer release once at startup and offers to install it; a portable copy, or
 *  one a package manager installed, only says so, as the installer would not update it. */
export function UpdateBanner({ portable, packageManager }: { portable: boolean; packageManager: string | null }) {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  useEffect(() => {
    if (import.meta.env.DEV) return;
    // Offline, or no release published yet: nothing to offer.
    check().then(setUpdate, (e: unknown) => console.info("update check:", e));
  }, []);

  useEffect(() => {
    const now = () =>
      check().then(
        (found) => (found ? setUpdate(found) : workbench().notify(t("Already up to date"))),
        (e: unknown) => workbench().notify(t("Update failed: {message}", { message: String(e) })),
      );
    window.addEventListener(CHECK_NOW, now);
    return () => window.removeEventListener(CHECK_NOW, now);
  }, []);

  if (!update) return null;

  async function install(u: Update) {
    let total = 0;
    let done = 0;
    try {
      await u.downloadAndInstall((event) => {
        if (event.event === "Started") total = event.data.contentLength ?? 0;
        if (event.event === "Progress") {
          done += event.data.chunkLength;
          setProgress(total ? t("Downloading… {n}%", { n: Math.round((done / total) * 100) }) : t("Downloading…"));
        }
        if (event.event === "Finished") setProgress(t("Installing…"));
      });
      await relaunch();
    } catch (e) {
      setProgress(t("Update failed: {message}", { message: String(e) }));
    }
  }

  return (
    <div className="update-banner">
      <span>
        {t("PlatypusDB {version} is available", { version: update.version })}
        {update.body ? ` — ${update.body.split("\n")[0]}` : ""}.
      </span>
      <span className="spacer" />
      {portable && !progress && <span className="muted">{t("Replace this portable copy's executable with the new one.")}</span>}
      {packageManager && <span className="muted">{t("Update it with {manager} once the new version reaches it.", { manager: packageManager })}</span>}
      {progress ? (
        <span className="muted">{progress}</span>
      ) : (
        <>
          <button type="button" onClick={() => setUpdate(null)}>
            {t("Later")}
          </button>
          {!portable && !packageManager && (
            <button type="button" className="primary" onClick={() => void install(update)}>
              {t("Install and restart")}
            </button>
          )}
        </>
      )}
    </div>
  );
}
