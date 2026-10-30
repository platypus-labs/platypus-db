import { useEffect, useState } from "react";
import { api, type DriverKind, type DriverVersions, toCommandError } from "../api";
import { t } from "../i18n";
import { mergeVersions } from "./compat";
import { checkCompatibility, installVersion } from "./updates";

interface Props {
  driver: DriverKind;
  /** The installed versions or the active one changed. */
  onChanged: () => void;
  onError: (message: string | null) => void;
}

/** "Versions…" of a downloaded driver: what the registry has for this platform, newest first,
 *  with the installed and the active ones marked; an earlier version can be installed or
 *  activated, and an installed one removed. */
export function DriverVersionsList({ driver, onChanged, onError }: Props) {
  const [versions, setVersions] = useState<DriverVersions | null>(null);
  /** The version being downloaded. */
  const [busy, setBusy] = useState<string | null>(null);

  const load = () => api.driverVersions(driver).then(setVersions, (e: unknown) => onError(toCommandError(e).message));
  useEffect(() => {
    void load();
  }, [driver]);

  /** Runs `change` (which installs or activates `version`), then checks the connections. */
  async function change(version: string, run: () => Promise<{ version: string; previous: string | null }>) {
    onError(null);
    setBusy(version);
    try {
      const done = await run();
      await load();
      onChanged();
      checkCompatibility(driver, done.version, done.previous, {
        ask: false,
        onChanged: () => {
          void load();
          onChanged();
        },
      });
    } catch (e) {
      const err = toCommandError(e);
      if (err.kind !== "cancelled") onError(err.message);
    } finally {
      setBusy(null);
    }
  }

  const install = (version: string) =>
    change(version, async () => {
      const { installed, previous } = await installVersion(driver, version);
      return { version: installed.version, previous };
    });
  const activate = (version: string) =>
    change(version, async () => {
      const previous = activeOf(versions);
      return { version: (await api.activateDriverVersion(driver, version)).version, previous };
    });

  async function remove(version: string) {
    onError(null);
    try {
      await api.removeDriverVersion(driver, version);
      await load();
      onChanged();
    } catch (e) {
      onError(toCommandError(e).message);
    }
  }

  if (!versions) return <p className="muted small">{t("Looking up the versions…")}</p>;
  const installed = new Map(versions.installed.map((v) => [v.version, v]));
  const all = mergeVersions(
    versions.available.map((p) => p.version),
    versions.installed.map((v) => v.version),
  );
  return (
    <div className="driver-versions" data-versions={driver}>
      {versions.error && <div className="notice small">{t("The registry cannot be reached: {message}", { message: versions.error })}</div>}
      {all.map((version) => {
        const have = installed.get(version);
        const available = versions.available.some((p) => p.version === version);
        return (
          <div key={version} className="driver-version" data-version={version} data-state={have ? (have.active ? "active" : "installed") : "available"}>
            <strong>{version}</strong>
            {have?.active && <span className="update-badge active">{t("In use")}</span>}
            {have && !have.active && <span className="muted small">{t("Installed")}</span>}
            <span className="spacer" />
            {busy === version && <progress aria-label={t("Downloading…")} />}
            {!have && available && (
              <button type="button" data-action="install" disabled={busy !== null} onClick={() => void install(version)}>
                {t("Install")}
              </button>
            )}
            {have && !have.active && (
              <button type="button" data-action="activate" disabled={busy !== null} onClick={() => void activate(version)}>
                {t("Use this version")}
              </button>
            )}
            {have && (
              <button type="button" data-action="remove-version" disabled={busy !== null} onClick={() => void remove(version)}>
                {t("Delete")}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}

/** The version in use among those listed. */
function activeOf(versions: DriverVersions | null): string | null {
  return versions?.installed.find((v) => v.active)?.version ?? null;
}
