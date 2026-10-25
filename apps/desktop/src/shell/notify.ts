import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { defineSetting, getSetting } from "../settings";

export const notifySetting = defineSetting<number>({
  key: "editor.notify_after_seconds",
  section: "SQL editor",
  label: "Notify when a run ends after (seconds)",
  description: "A system notification when the window is in the background; 0 turns it off.",
  default: 10,
  control: { type: "number", min: 0, step: 5 },
});

/** Tells the system a run that began at `startedAt` ended, when it took longer than the
 *  setting and the window is not in front. */
export async function notifyRunEnded(startedAt: number, connection: string, failed: boolean) {
  const after = getSetting(notifySetting);
  const seconds = Math.round((Date.now() - startedAt) / 1000);
  if (after <= 0 || seconds < after || document.hasFocus()) return;
  try {
    if (!(await isPermissionGranted()) && (await requestPermission()) !== "granted") return;
    sendNotification({
      title: failed ? `Query failed on ${connection}` : `Query finished on ${connection}`,
      body: `After ${seconds} s`,
    });
  } catch (e) {
    console.warn("notification", e);
  }
}
