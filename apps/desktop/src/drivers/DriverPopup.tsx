import { createElement, useEffect, useState } from "react";
import { api, type DriverKind, type DriverStatus, DOWNLOADED_DRIVERS, VENDOR_DRIVERS, toCommandError } from "../api";
import { driverToDownload } from "./missingDriver";
import { DRIVER_NAMES } from "./names";
import { workbench } from "../workbench";
import { t } from "../i18n";
import { VendorPopup } from "./VendorDrivers";

interface Props {
  driver: DriverKind;
  /** Asked before any connection exists (the picker), so the button does not say "connect". */
  beforeConnecting?: boolean;
  onDone: (installed: boolean) => void;
}

const megabytes = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`;

/** Asks to download the driver a database needs, then downloads it with its progress. */
function DriverPopup({ driver, beforeConnecting, onDone }: Props) {
  const [status, setStatus] = useState<DriverStatus | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const name = DRIVER_NAMES[driver];

  useEffect(() => {
    api.driverStatus(driver).then(setStatus, (e: unknown) => setError(toCommandError(e).message));
  }, [driver]);

  async function download() {
    setError(null);
    setProgress({ done: 0, total: null });
    try {
      await api.installDriver(driver, (e) => setProgress({ done: e.done, total: e.total }));
      onDone(true);
    } catch (e) {
      const err = toCommandError(e);
      setProgress(null);
      if (err.kind !== "cancelled") setError(err.message);
    }
  }

  const pkg = status?.package;
  /** Closes without a download, unless one is running. */
  const dismiss = () => !progress && onDone(false);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && dismiss()}>
      <div className="modal driver-popup" onKeyDown={(e) => e.key === "Escape" && dismiss()}>
        {progress ? (
          <>
            <h2>{t("Downloading the {name} driver…", { name })}</h2>
            <progress max={progress.total ?? undefined} value={progress.total ? progress.done : undefined} />
            <p className="muted small">
              {progress.total ? t("{done} of {total}", { done: megabytes(progress.done), total: megabytes(progress.total) }) : megabytes(progress.done)}
            </p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" onClick={() => void api.cancelDriverInstall(driver)}>
                {t("Cancel")}
              </button>
            </div>
          </>
        ) : (
          <>
            <h2>{t("{name} needs a driver", { name })}</h2>
            <p className="muted small">{t("An ADBC driver, downloaded once for all {name} connections.", { name })}</p>
            {pkg && (
              <div className="driver-package">
                <strong>{pkg.title}</strong> · {pkg.version}
                <div className="muted small">
                  {t("License: {license} · from the dbc driver registry (Columnar)", { license: pkg.license })}
                  {pkg.sha256 ? ` · ${t("checked with SHA-256")}` : ""}
                </div>
              </div>
            )}
            {!status && !error && <p className="muted small">{t("Looking up the driver…")}</p>}
            {status?.error && <div className="notice error small">{status.error}</div>}
            {error && <div className="notice error small">{error}</div>}
            <div className="actions">
              <span className="muted small">{t("You can also do it later")}</span>
              <span className="spacer" />
              <button type="button" onClick={() => onDone(false)}>
                {t("Cancel")}
              </button>
              <button type="button" className="primary" disabled={!pkg} onClick={() => void download()}>
                {t(error ? "Try again" : beforeConnecting ? "Download" : "Download and connect")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** The popups open, by driver: sessions opened at once share one. */
const asking = new Map<DriverKind, Promise<boolean>>();

/** Shows the popup for `driver` (a vendor's client for Oracle and Db2); resolves `true` once
 *  its driver is installed. `beforeConnecting`: no connection is being made yet. */
export function askDriver(driver: DriverKind, options: { beforeConnecting?: boolean } = {}): Promise<boolean> {
  const open = asking.get(driver);
  if (open) return open;
  const answer = new Promise<boolean>((resolve) =>
    workbench().openDialog((close) =>
      createElement(VENDOR_DRIVERS.includes(driver) ? VendorPopup : DriverPopup, {
        driver,
        beforeConnecting: options.beforeConnecting,
        onDone: (installed: boolean) => {
          close();
          resolve(installed);
        },
      }),
    ),
  ).finally(() => asking.delete(driver));
  asking.set(driver, answer);
  return answer;
}

/** A database was picked for a new connection: offers its driver at once when it needs one
 *  that is not installed. The form stays usable either way; the answer changes nothing. */
export async function offerDriver(driver: DriverKind): Promise<void> {
  const missing = await driverToDownload(driver, {
    downloaded: DOWNLOADED_DRIVERS,
    vendor: VENDOR_DRIVERS,
    listDrivers: api.listDrivers,
    vendorStatus: api.vendorDriverStatus,
  });
  if (missing) await askDriver(driver, { beforeConnecting: true });
}

/** Runs `run`; when it fails for want of a driver, offers to download it and runs it again. */
export async function withDriver<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run();
  } catch (e) {
    const err = toCommandError(e);
    if (err.kind !== "driver_missing" || !err.code || !(await askDriver(err.code as DriverKind))) throw e;
    return run();
  }
}
