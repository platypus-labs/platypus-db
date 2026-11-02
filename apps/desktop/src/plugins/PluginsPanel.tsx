import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type InspectedPlugin, type InstalledPlugin, type PluginDescription, type PluginPermissions, toCommandError } from "../api";
import { msg, t } from "../i18n";
import { ask } from "../ui/ask";
import { answerFor } from "../testAnswers";
import { reloadPlugins, usePlugins } from "./store";
import { pickFolder } from "../projects/shared";
import { EmptyState } from "../ui/State";

const SCOPES: Record<string, string> = {
  editor: msg("SQL editor"),
  grid: msg("Result grid"),
  "navigator.object": msg("Navigator"),
};

/** What a plugin may do, one line each; a plugin without permissions says so. */
function PermissionList({ permissions }: { permissions: PluginPermissions }) {
  const lines: string[] = [];
  if (permissions.http.length > 0) lines.push(t("Send HTTP requests to: {hosts}", { hosts: permissions.http.join(", ") }));
  if (permissions.clipboard) lines.push(t("Put text on the clipboard"));
  if (lines.length === 0) return <p className="muted small">{t("No permissions: it only works on what the app gives it.")}</p>;
  return (
    <ul className="plugin-permissions small" data-field="permissions">
      {lines.map((line) => (
        <li key={line}>{line}</li>
      ))}
    </ul>
  );
}

/** The actions and the driver a plugin provides. */
function Provides({ description }: { description: PluginDescription }) {
  return (
    <ul className="plugin-provides small" data-field="provides">
      {description.actions.map((a) => (
        <li key={a.id} data-action-id={a.id}>
          {t("Action “{title}” in {place}", { title: a.title, place: t(SCOPES[a.scope] ?? a.scope) })}
        </li>
      ))}
      {description.driver && <li>{t("Driver “{name}”", { name: description.driver.name })}</li>}
    </ul>
  );
}

/** Settings › Plugins: the installed WebAssembly plugins, turned on or off and removed, and
 *  the install of a package after reviewing what it asks for. */
export function PluginsPanel() {
  const plugins = usePlugins();
  const [error, setError] = useState<string | null>(null);
  const [review, setReview] = useState<{ path: string; plugin: InspectedPlugin } | null>(null);
  const [busy, setBusy] = useState(false);

  async function attempt(work: () => Promise<void>) {
    setError(null);
    setBusy(true);
    try {
      await work();
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setBusy(false);
    }
  }

  async function pick(zip: boolean) {
    const picked = await (zip
      ? answerFor("file", () =>
          open({ multiple: false, title: t("Choose a plugin package"), filters: [{ name: t("Plugin package"), extensions: ["zip"] }] }),
        )
      : pickFolder(t("Choose a plugin folder")));
    if (typeof picked !== "string") return;
    await attempt(async () => setReview({ path: picked, plugin: await api.pluginInspect(picked) }));
  }

  const install = (path: string, hash: string) =>
    attempt(async () => {
      await api.pluginInstall(path, hash);
      setReview(null);
      await reloadPlugins();
    });

  const toggle = (plugin: InstalledPlugin) =>
    attempt(async () => {
      await api.pluginSetEnabled(plugin.manifest.id, !plugin.enabled);
      await reloadPlugins();
    });

  async function remove(plugin: InstalledPlugin) {
    if (!(await ask(t("Remove the plugin {name}? Its connections stop working.", { name: plugin.manifest.name }), { confirm: t("Remove"), danger: true }))) {
      return;
    }
    await attempt(async () => {
      await api.pluginRemove(plugin.manifest.id);
      await reloadPlugins();
    });
  }

  const installButtons = (
    <>
      <button type="button" data-action="install-folder" disabled={busy} onClick={() => void pick(false)}>
        {t("Install from a folder…")}
      </button>
      <button type="button" data-action="install-zip" disabled={busy} onClick={() => void pick(true)}>
        {t("Install from a .zip…")}
      </button>
    </>
  );
  return (
    <div className="plugins-panel">
      <p className="muted small">
        {t("Plugins add actions to the editor, the grid and the navigator, and drivers for other databases. They run in a sandbox: no files, no programs, and the network only to the hosts they list.")}
      </p>
      {error && (
        <div className="notice error small" data-field="plugin-error">
          {error}
        </div>
      )}
      {(plugins.length > 0 || review) && <div className="driver-actions">{installButtons}</div>}
      {review && (
        <div className="driver-block plugin-review" data-field="plugin-review" data-plugin={review.plugin.manifest.id}>
          <strong>
            {review.plugin.manifest.name} · {review.plugin.manifest.version}
          </strong>
          {review.plugin.manifest.author && <div className="muted small">{review.plugin.manifest.author}</div>}
          {review.plugin.manifest.description && <p className="small">{review.plugin.manifest.description}</p>}
          {review.plugin.replaces && (
            <p className="small" data-field="replaces">
              {t("Replaces the installed version {version}.", { version: review.plugin.replaces })}
            </p>
          )}
          <Provides description={review.plugin.description} />
          <h4>{t("It asks to")}</h4>
          <PermissionList permissions={review.plugin.manifest.permissions} />
          <div className="driver-actions">
            <button type="button" className="primary" data-action="confirm-install" disabled={busy} onClick={() => void install(review.path, review.plugin.hash)}>
              {t("Install and allow")}
            </button>
            <button type="button" data-action="cancel-install" disabled={busy} onClick={() => setReview(null)}>
              {t("Cancel")}
            </button>
          </div>
        </div>
      )}
      {plugins.length === 0 && !review && (
        <EmptyState icon="extension" title={t("No plugin installed.")} actions={installButtons} />
      )}
      {plugins.map((p) => (
        <div key={p.manifest.id} className="driver-block" data-plugin={p.manifest.id} data-enabled={p.enabled}>
          <div className="driver-row-inner">
            <div>
              <strong>{p.manifest.name}</strong> · <span data-field="version">{p.manifest.version}</span>
              {p.manifest.author && <div className="muted small">{p.manifest.author}</div>}
            </div>
            <span className="spacer" />
            <label className="check">
              <input type="checkbox" data-action="toggle" checked={p.enabled} disabled={busy} onChange={() => void toggle(p)} />
              {t("Turned on")}
            </label>
            <button type="button" data-action="remove" disabled={busy} onClick={() => void remove(p)}>
              {t("Remove")}
            </button>
          </div>
          {p.manifest.description && <p className="muted small">{p.manifest.description}</p>}
          <Provides description={p.description} />
          <PermissionList permissions={p.approved} />
        </div>
      ))}
    </div>
  );
}
