import { createElement, useEffect, useState } from "react";
import { api, type DriverKind, type DriverStatus, VENDOR_DRIVERS, toCommandError } from "../api";
import { DRIVER_NAMES } from "./names";
import { workbench } from "../workbench";
import { VendorPopup } from "./VendorDrivers";

interface Props {
  driver: DriverKind;
  onDone: (installed: boolean) => void;
}

const megabytes = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`;

/** Asks to download the driver a database needs, then downloads it with its progress. */
function DriverPopup({ driver, onDone }: Props) {
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
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && !progress && onDone(false)}>
      <div className="modal driver-popup" onKeyDown={(e) => e.key === "Escape" && !progress && onDone(false)}>
        {progress ? (
          <>
            <h2>Downloading the {name} driver…</h2>
            <progress max={progress.total ?? undefined} value={progress.total ? progress.done : undefined} />
            <p className="muted small">
              {megabytes(progress.done)}
              {progress.total ? ` of ${megabytes(progress.total)}` : ""}
            </p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" onClick={() => void api.cancelDriverInstall()}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            <h2>{name} needs a driver</h2>
            <p className="muted small">An ADBC driver, downloaded once for all {name} connections.</p>
            {pkg && (
              <div className="driver-package">
                <strong>{pkg.title}</strong> · {pkg.version}
                <div className="muted small">
                  License: {pkg.license} · from the dbc driver registry (Columnar)
                  {pkg.sha256 ? " · checked with SHA-256" : ""}
                </div>
              </div>
            )}
            {!status && !error && <p className="muted small">Looking up the driver…</p>}
            {status?.error && <div className="notice error small">{status.error}</div>}
            {error && <div className="notice error small">{error}</div>}
            <div className="actions">
              <span className="muted small">Later: Settings › Drivers</span>
              <span className="spacer" />
              <button type="button" onClick={() => onDone(false)}>
                Cancel
              </button>
              <button type="button" className="primary" disabled={!pkg} onClick={() => void download()}>
                {error ? "Try again" : "Download and connect"}
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
 *  its driver is installed. */
export function askDriver(driver: DriverKind): Promise<boolean> {
  const open = asking.get(driver);
  if (open) return open;
  const answer = new Promise<boolean>((resolve) =>
    workbench().openDialog((close) =>
      createElement(VENDOR_DRIVERS.includes(driver) ? VendorPopup : DriverPopup, {
        driver,
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
