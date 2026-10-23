import { useEffect, useState } from "react";
import { relaunch } from "@tauri-apps/plugin-process";
import { type Update, check } from "@tauri-apps/plugin-updater";

/** Looks for a newer release once at startup and offers to install it. */
export function UpdateBanner() {
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState<string | null>(null);

  useEffect(() => {
    if (import.meta.env.DEV) return;
    // Offline, or no release published yet: nothing to offer.
    check().then(setUpdate, (e: unknown) => console.info("update check:", e));
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
          setProgress(total ? `Downloading… ${Math.round((done / total) * 100)}%` : "Downloading…");
        }
        if (event.event === "Finished") setProgress("Installing…");
      });
      await relaunch();
    } catch (e) {
      setProgress(`Update failed: ${String(e)}`);
    }
  }

  return (
    <div className="update-banner">
      <span>
        PlatypusDB {update.version} is available{update.body ? ` — ${update.body.split("\n")[0]}` : ""}.
      </span>
      <span className="spacer" />
      {progress ? (
        <span className="muted">{progress}</span>
      ) : (
        <>
          <button className="primary" onClick={() => void install(update)}>
            Install and restart
          </button>
          <button onClick={() => setUpdate(null)}>Later</button>
        </>
      )}
    </div>
  );
}
