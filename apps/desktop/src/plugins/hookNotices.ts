// Tells the user of a plugin's hook that failed: the action it was for went ahead, and the backend
// sends this once per plugin; with no plugin named, the list of plugins cannot be read and no hook
// ran. A chunk of its own, loaded only once a plugin has hooks or that list failed.
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import { workbench } from "../workbench";

listen<{ plugin: string; reason: string }>("plugin-hook-failed", ({ payload }) =>
  workbench().notify(
    payload.plugin
      ? t("The hook of the plugin “{name}” failed and the action went ahead: {reason}", { name: payload.plugin, reason: payload.reason })
      : t("The list of plugins cannot be read, so no hook ran and the action went ahead: {reason}", { reason: payload.reason }),
  ),
).catch((e: unknown) => console.warn("plugins", e));
