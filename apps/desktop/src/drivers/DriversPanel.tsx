import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api, type DriverKind, type DriverUpdate, type InstalledDriver, toCommandError } from "../api";
import { t } from "../i18n";
import { checkCompatibility, installVersion, kindOf, refreshUpdates, useDriverUpdates } from "./updates";
import { VendorDriversSection } from "./VendorDrivers";
import { DriverVersionsList } from "./DriverVersionsList";

/** The drivers downloaded for the databases reached through ADBC, with their updates, their
 *  versions and removal. */
export function DriversPanel() {
  const [drivers, setDrivers] = useState<InstalledDriver[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  /** The result of the last "Check for updates" click. */
  const [checked, setChecked] = useState<string | null>(null);
  const updates = useDriverUpdates();

  const reload = () => api.listDrivers().then(setDrivers, (e: unknown) => setError(toCommandError(e).message));
  useEffect(() => {
    void reload();
    getVersion().then(setAppVersion, () => setAppVersion(null));
  }, []);

  async function remove(name: string) {
    try {
      setDrivers(await api.removeDriver(name));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  async function checkNow() {
    setChecking(true);
    setError(null);
    setChecked(null);
    try {
      const result = await refreshUpdates(true);
      if (result.error) setError(result.error);
      else setChecked(result.updates.length === 0 ? t("All drivers are up to date.") : null);
    } catch (e) {
      setError(toCommandError(e).message);
    } finally {
      setChecking(false);
    }
  }

  return (
    <div className="drivers-panel">
      <p className="muted small">
        {t("Snowflake, BigQuery, Databricks and Trino connect through ADBC drivers, downloaded the first time a connection needs one.")}
      </p>
      <p className="muted small" data-field="native-drivers">
        {appVersion
          ? t("The drivers for PostgreSQL, MySQL, SQLite, SQL Server and the other databases are built into PlatypusDB {version} and update with the app.", { version: appVersion })
          : t("The drivers for PostgreSQL, MySQL, SQLite, SQL Server and the other databases are built into PlatypusDB and update with the app.")}
      </p>
      {error && <div className="notice error small">{error}</div>}
      <div className="driver-actions">
        <button type="button" data-action="check-updates" disabled={checking} onClick={() => void checkNow()}>
          {checking ? t("Checking…") : t("Check for updates")}
        </button>
        {checked && (
          <span className="muted small" data-field="checked">
            {checked}
          </span>
        )}
      </div>
      {drivers?.length === 0 && <p className="muted small">{t("No driver downloaded yet.")}</p>}
      {drivers?.map((d) => (
        <DriverRow
          key={d.name}
          driver={d}
          update={updates.find((u) => u.name === d.name)}
          onChanged={() => void reload()}
          onRemove={() => void remove(d.name)}
          onError={setError}
        />
      ))}
      <VendorDriversSection />
    </div>
  );
}

interface RowProps {
  driver: InstalledDriver;
  update: DriverUpdate | undefined;
  onChanged: () => void;
  onRemove: () => void;
  onError: (message: string | null) => void;
}

function DriverRow({ driver, update, onChanged, onRemove, onError }: RowProps) {
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [showVersions, setShowVersions] = useState(false);
  const kind: DriverKind | null = kindOf(driver.name);

  async function applyUpdate(u: DriverUpdate) {
    if (!kind) return;
    onError(null);
    setProgress({ done: 0, total: null });
    try {
      const { installed, previous } = await installVersion(kind, u.package.version, (done, total) => setProgress({ done, total }));
      onChanged();
      checkCompatibility(kind, installed.version, previous, { ask: false, onChanged });
    } catch (e) {
      const err = toCommandError(e);
      if (err.kind !== "cancelled") onError(err.message);
    } finally {
      setProgress(null);
    }
  }

  return (
    <div className="driver-block" data-driver={driver.name}>
      <div className="driver-row-inner">
        <div>
          <strong>{driver.title}</strong> · <span data-field="version">{driver.version}</span>
          {update && (
            <span className="update-badge" data-update={update.package.version} title={t("A newer version is available")}>
              {t("Update available: {version}", { version: update.package.version })}
            </span>
          )}
          <div className="muted small" title={driver.library}>
            {driver.license}
          </div>
        </div>
        <span className="spacer" />
        {progress && (
          <progress max={progress.total ?? undefined} value={progress.total ? progress.done : undefined} aria-label={t("Downloading…")} />
        )}
        {update && kind && (
          <button type="button" className="primary" data-action="update" disabled={!!progress} onClick={() => void applyUpdate(update)}>
            {t("Update")}
          </button>
        )}
        {kind && (
          <button type="button" data-action="versions" aria-expanded={showVersions} onClick={() => setShowVersions((s) => !s)}>
            {t("Versions…")}
          </button>
        )}
        <button type="button" data-action="remove" disabled={!!progress} onClick={onRemove}>
          {t("Remove")}
        </button>
      </div>
      {showVersions && kind && <DriverVersionsList driver={kind} onChanged={onChanged} onError={onError} />}
    </div>
  );
}
