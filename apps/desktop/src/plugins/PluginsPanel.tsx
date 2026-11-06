import { type ReactNode, Suspense, lazy, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type InspectedPlugin, type InstalledPlugin, type PluginDescription, type PluginHookEvent, type PluginManifest, type PluginPermissions, toCommandError } from "../api";
import { msg, t } from "../i18n";
import { ask, choose } from "../ui/ask";
import { answerFor } from "../testAnswers";
import { pluginListProblem, reloadPlugins, usePlugins } from "./store";
import { pickFolder } from "../projects/shared";
import { EmptyState } from "../ui/State";
import { PALETTE_KEYS, customPreset } from "../theme";
import { pluginThemeOf } from "../themes/color";
import { lowContrast } from "../themes/contrast";
import { keepableSettings } from "./catalogEntry";

const SCOPES: Record<string, string> = {
  editor: msg("SQL editor"),
  grid: msg("Result grid"),
  "navigator.object": msg("Navigator"),
};

const HOOK_EVENTS: Record<PluginHookEvent, string> = {
  before_query: msg("Runs before a statement, and may stop it"),
  after_query: msg("Runs after a statement"),
  on_connect: msg("Runs when a connection opens"),
  before_save: msg("Runs before edited rows are saved, and may stop it"),
};

/** The settings page, loaded when a plugin's is opened. */
const PluginSettings = lazy(() => import("./PluginSettings"));

/** What a plugin may do, one line each in plain words; a plugin without permissions says so. */
function permissionLines(permissions: PluginPermissions): string[] {
  const lines: string[] = [];
  if (permissions.http.length > 0) lines.push(t("Send HTTP requests to: {hosts}", { hosts: permissions.http.join(", ") }));
  if (permissions.clipboard) lines.push(t("Put text on the clipboard"));
  if (permissions.hooks) lines.push(t("Know when statements run and connections open, see statements with sensitive values masked, and stop a statement or a save"));
  if (permissions.tabs) lines.push(t("Show its own pages in tabs and side panels"));
  if (permissions.grid) lines.push(t("Read the cells of results, with sensitive columns masked, and add columns, cell views and cell commands to the result grid"));
  return lines;
}

function PermissionList({ permissions, field = "permissions" }: { permissions: PluginPermissions; field?: string }) {
  const lines = permissionLines(permissions);
  if (lines.length === 0) return <p className="muted small">{t("No permissions: it only works on what the app gives it.")}</p>;
  return (
    <ul className="plugin-permissions small" data-field={field}>
      {lines.map((line) => (
        <li key={line}>{line}</li>
      ))}
    </ul>
  );
}

/** What a plugin provides, from its module's description and its manifest. */
function Provides({ description, manifest }: { description: PluginDescription; manifest: PluginManifest }) {
  const settings = manifest.settings ?? [];
  const secrets = settings.filter((f) => f.type === "secret");
  const grid = manifest.grid ? [...(manifest.grid.columns ?? []), ...(manifest.grid.renderers ?? []), ...(manifest.grid.actions ?? [])] : [];
  return (
    <ul className="plugin-provides small" data-field="provides">
      {description.actions.map((a) => (
        <li key={a.id} data-action-id={a.id}>
          {t("Action “{title}” in {place}", { title: a.title, place: t(SCOPES[a.scope] ?? a.scope) })}
        </li>
      ))}
      {description.driver && <li>{t("Driver “{name}”", { name: description.driver.name })}</li>}
      {manifest.theme && <li>{t("Theme “{name}”", { name: manifest.theme.name })}</li>}
      {manifest.hooks?.map((event) => (
        <li key={event} data-hook={event}>
          {t(HOOK_EVENTS[event])}
        </li>
      ))}
      {manifest.tabs?.map((tab) => (
        <li key={tab.kind}>{t("Tab “{title}”", { title: tab.title })}</li>
      ))}
      {manifest.panels?.map((panel) => (
        <li key={panel.id}>{t("Side panel “{title}”", { title: panel.title })}</li>
      ))}
      {grid.map((item) => (
        <li key={item.id}>{t("“{title}” in the result grid", { title: item.title })}</li>
      ))}
      {settings.length > 0 && <li>{t("Its own settings: {fields}", { fields: settings.map((f) => f.label).join(", ") })}</li>}
      {secrets.length > 0 && <li>{t("Kept in the system's keychain: {fields}", { fields: secrets.map((f) => f.label).join(", ") })}</li>}
    </ul>
  );
}

/** A warning when a theme's text would be hard to read; the theme still installs. */
function ThemeContrast({ theme }: { theme: NonNullable<PluginManifest["theme"]> }) {
  const shown = pluginThemeOf(theme, PALETTE_KEYS);
  if (!shown) return null;
  const variants = [...(shown.light ? [false] : []), ...(shown.dark ? [true] : [])];
  const pairs = variants.flatMap((dark) =>
    lowContrast(customPreset(shown, dark).colors).map((p) => `${dark ? t("dark") : t("light")}: ${p.fg} / ${p.bg} (${p.ratio}:1)`),
  );
  if (pairs.length === 0) return null;
  return (
    <p className="notice warn small" data-field="theme-contrast">
      {t("Some text of this theme may be hard to read: {pairs}", { pairs: pairs.join(", ") })}
    </p>
  );
}

/** The installed version has another signer: nothing says this is the same publisher. */
function SignerChangeNotice({ change }: { change: NonNullable<InspectedPlugin["signer_change"]> }) {
  const signed = (key: string | null) => (key === null ? t("not signed") : t("signed by {key}", { key }));
  return (
    <p className="notice warn small" data-field="signer-change">
      {t("This may be another publisher: the installed version is {from}, and this package is {to}. Installing clears the plugin's secrets and what its pages keep.", { from: signed(change.from), to: signed(change.to) })}
    </p>
  );
}

/** What a package provides and asks for, and the answer to it: the review of every install, from a
 *  file or from the catalog. `children` goes above what it asks. */
export function PluginReview(props: { plugin: InspectedPlugin; busy: boolean; onInstall: () => void; onCancel: () => void; children?: ReactNode }) {
  const { plugin, busy, onInstall, onCancel, children } = props;
  return (
    <div className="driver-block plugin-review" data-field="plugin-review" data-plugin={plugin.manifest.id}>
      <strong>
        {plugin.manifest.name} · {plugin.manifest.version}
      </strong>
      {plugin.manifest.author && <div className="muted small">{plugin.manifest.author}</div>}
      {plugin.manifest.description && <p className="small">{plugin.manifest.description}</p>}
      {plugin.replaces && (
        <p className="small" data-field="replaces">
          {t("Replaces the installed version {version}.", { version: plugin.replaces })}
        </p>
      )}
      {children}
      <Provides description={plugin.description} manifest={plugin.manifest} />
      {plugin.manifest.theme && <ThemeContrast theme={plugin.manifest.theme} />}
      {plugin.signer_change && <SignerChangeNotice change={plugin.signer_change} />}
      <h4>{t("It asks to")}</h4>
      <PermissionList permissions={plugin.manifest.permissions} />
      {plugin.replaces && permissionLines(plugin.added).length > 0 && (
        <>
          <h4>{t("New since the installed version")}</h4>
          <PermissionList permissions={plugin.added} field="added-permissions" />
        </>
      )}
      <div className="driver-actions">
        <button type="button" className="primary" data-action="confirm-install" disabled={busy} onClick={onInstall}>
          {t("Install and allow")}
        </button>
        <button type="button" data-action="cancel-install" disabled={busy} onClick={onCancel}>
          {t("Cancel")}
        </button>
      </div>
    </div>
  );
}

/** Settings › Plugins: the installed WebAssembly plugins, turned on or off and removed, and
 *  the install of a package after reviewing what it asks for. */
export function PluginsPanel() {
  const plugins = usePlugins();
  const listProblem = pluginListProblem();
  const [error, setError] = useState<string | null>(null);
  const [review, setReview] = useState<{ path: string; plugin: InspectedPlugin } | null>(null);
  const [busy, setBusy] = useState(false);
  // The plugin whose settings page is open.
  const [settingsOf, setSettingsOf] = useState<string | null>(null);

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
    const question = t("Remove the plugin {name}? Its connections stop working.", { name: plugin.manifest.name });
    // Only what can stay is asked: a secret always leaves the keychain.
    const keep = keepableSettings(plugin.manifest.settings)
      ? await choose<boolean>(question, [
          { label: t("Remove and keep its settings"), value: true },
          { label: t("Remove everything"), value: false, danger: true },
        ])
      : (await ask(question, { confirm: t("Remove"), danger: true }))
        ? false
        : null;
    if (keep === null) return;
    // Its page goes with it: installed again, it starts closed.
    setSettingsOf((open) => (open === plugin.manifest.id ? null : open));
    await attempt(async () => {
      try {
        await api.pluginRemove(plugin.manifest.id, keep);
      } finally {
        // Removed in part it is off the list, and the error says what is left.
        await reloadPlugins();
      }
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
      {listProblem && (
        <div className="notice error small" data-field="plugin-list-error">
          {t("The list of plugins cannot be read, so no plugin runs and no hook guards a statement: {reason}", { reason: listProblem })}
        </div>
      )}
      {(plugins.length > 0 || review) && <div className="driver-actions">{installButtons}</div>}
      {review && <PluginReview plugin={review.plugin} busy={busy} onInstall={() => void install(review.path, review.plugin.hash)} onCancel={() => setReview(null)} />}
      {plugins.length === 0 && !review && (
        <EmptyState icon="extension" title={t("No plugin installed.")} actions={installButtons} />
      )}
      {plugins.map((p) => (
        <div key={p.manifest.id} className="driver-block" data-plugin={p.manifest.id} data-enabled={p.enabled} data-broken={p.broken || undefined}>
          <div className="driver-row-inner">
            <div>
              <strong>{p.manifest.name}</strong> · <span data-field="version">{p.manifest.version}</span>
              {p.manifest.author && <div className="muted small">{p.manifest.author}</div>}
            </div>
            <span className="spacer" />
            {(p.manifest.settings?.length ?? 0) > 0 && (
              <button
                type="button"
                data-action="settings"
                aria-expanded={settingsOf === p.manifest.id}
                onClick={() => setSettingsOf(settingsOf === p.manifest.id ? null : p.manifest.id)}
              >
                {t("Settings")}
              </button>
            )}
            {!p.broken && (
              <label className="check">
                <input type="checkbox" data-action="toggle" checked={p.enabled} disabled={busy} onChange={() => void toggle(p)} />
                {t("Turned on")}
              </label>
            )}
            <button type="button" data-action="remove" disabled={busy} onClick={() => void remove(p)}>
              {t("Remove")}
            </button>
          </div>
          {p.broken && (
            <p className="notice warn small" data-field="plugin-broken">
              {t("Its files are missing or damaged, and it does not run. Remove it, and install it again if you still want it.")}
            </p>
          )}
          {p.manifest.description && <p className="muted small">{p.manifest.description}</p>}
          <Provides description={p.description} manifest={p.manifest} />
          {p.manifest.theme && <ThemeContrast theme={p.manifest.theme} />}
          <PermissionList permissions={p.approved} />
          {settingsOf === p.manifest.id && (
            <Suspense fallback={null}>
              <PluginSettings plugin={p} />
            </Suspense>
          )}
        </div>
      ))}
    </div>
  );
}
