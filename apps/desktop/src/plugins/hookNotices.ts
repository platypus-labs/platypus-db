// Tells the user of a plugin's hook that failed: the action it was for went ahead, and the backend
// sends this once per plugin. A chunk of its own, loaded only once a plugin has hooks.
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import { workbench } from "../workbench";

listen<{ plugin: string; reason: string }>("plugin-hook-failed", ({ payload }) =>
  workbench().notify(t("The hook of the plugin “{name}” failed and the action went ahead: {reason}", { name: payload.plugin, reason: payload.reason })),
).catch((e: unknown) => console.warn("plugins", e));
