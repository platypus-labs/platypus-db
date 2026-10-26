import { useEffect, useState } from "react";
import { api, type DriverKind, type OdbcSystem, type VendorStatus, toCommandError, VENDOR_DRIVERS } from "../api";
import { askDriver } from "./DriverPopup";
import { DRIVER_NAMES } from "./names";
import { t, msg, tNodes } from "../i18n";

const megabytes = (bytes: number) => `${(bytes / 1048576).toFixed(1)} MB`;

/** A link shown as text to copy: the app opens no browser. */
function Address({ url }: { url: string }) {
  return (
    <span className="address" title={url}>
      {url.replace(/^https:\/\//, "")}
    </span>
  );
}

/** What the vendor's client is for, in the popup. */
const PURPOSE: Partial<Record<DriverKind, string>> = {
  oracle: msg("Oracle's client library, downloaded once for all Oracle connections."),
  db2: msg("IBM's ODBC driver, downloaded once and registered with the Windows ODBC driver manager."),
  duckdb: msg("DuckDB's library, downloaded once for DuckDB databases and for reading CSV, Parquet, JSON and Excel files."),
};

/** The popup's question where "{name} needs {client}" would repeat the name. */
const QUESTION: Partial<Record<DriverKind, string>> = {
  duckdb: msg("Download DuckDB?"),
};

/** Where the app installs nothing: what to do instead. */
const ELSEWHERE: Partial<Record<DriverKind, string>> = {
  oracle: msg(
    "PlatypusDB downloads Oracle Instant Client on Windows and Linux. Here, install Instant Client from Oracle and add its folder to the library path, then try again.",
  ),
  db2: msg("IBM Db2 is reached through ODBC, on Windows only for now."),
  duckdb: msg("PlatypusDB downloads DuckDB on Windows, macOS and Linux, on x64 and ARM processors; not on this system."),
};

interface Props {
  driver: DriverKind;
  onDone: (installed: boolean) => void;
}

/** Asks to install the vendor's client a database needs, then installs it: the download with
 *  its progress, then, for an ODBC driver, its registration, which Windows asks
 *  administrator rights for. */
export function VendorPopup({ driver, onDone }: Props) {
  const [status, setStatus] = useState<VendorStatus | null>(null);
  const [phase, setPhase] = useState<"ask" | "download" | "register">("ask");
  const [progress, setProgress] = useState<{ done: number; total: number | null }>({ done: 0, total: null });
  const [error, setError] = useState<string | null>(null);
  const name = DRIVER_NAMES[driver];

  useEffect(() => {
    api.vendorDriverStatus(driver).then(setStatus, (e: unknown) => setError(toCommandError(e).message));
  }, [driver]);

  async function install() {
    setError(null);
    setPhase("download");
    try {
      await api.installVendorDriver(driver, (e) => {
        if (e.kind === "progress") setProgress({ done: e.done, total: e.total });
        else setPhase("register");
      });
      onDone(true);
    } catch (e) {
      const err = toCommandError(e);
      setPhase("ask");
      if (err.kind !== "cancelled") setError(err.message);
    }
  }

  const pkg = status?.package;
  const busy = phase !== "ask";
  const close = () => !busy && onDone(false);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div
        className="modal driver-popup"
        data-vendor={driver}
        onKeyDown={(e) => e.key === "Escape" && close()}
      >
        {phase === "download" && (
          <>
            <h2>{pkg?.title ? t("Downloading {name}…", { name: pkg.title }) : t("Downloading the {name} client…", { name })}</h2>
            <progress max={progress.total ?? undefined} value={progress.total ? progress.done : undefined} />
            <p className="muted small">
              {progress.total ? t("{done} of {total}", { done: megabytes(progress.done), total: megabytes(progress.total) }) : megabytes(progress.done)}
            </p>
            <div className="actions">
              <span className="spacer" />
              <button type="button" onClick={() => void api.cancelVendorInstall(driver)}>
                {t("Cancel")}
              </button>
            </div>
          </>
        )}
        {phase === "register" && (
          <>
            <h2>{t("Registering the driver…")}</h2>
            <p className="small">
              {t("Confirm the Windows prompt (User Account Control) to register {name} with the ODBC driver manager.", { name: pkg?.title ?? "" })}
            </p>
          </>
        )}
        {phase === "ask" && (
          <>
            <h2>
              {QUESTION[driver]
                ? t(QUESTION[driver])
                : pkg?.title
                  ? t("{name} needs {client}", { name, client: pkg.title })
                  : t("{name} needs a client library", { name })}
            </h2>
            <p className="muted small">{PURPOSE[driver] && t(PURPOSE[driver])}</p>
            {pkg && (
              <div className="driver-package">
                <strong>{pkg.title}</strong> · {pkg.version} ·{" "}
                {megabytes(pkg.archives.reduce((sum, a) => sum + a.size, 0))}
                <div className="muted small">
                  {tNodes("License: {license} ({address})", { license: pkg.license, address: <Address url={pkg.license_url} /> })}
                </div>
                <div className="muted small">
                  {tNodes("From the vendor ({address}) · checked with SHA-256", { address: <Address url={pkg.vendor_url} /> })}
                </div>
              </div>
            )}
            {pkg?.register && (
              <div className="notice small" data-notice="uac">
                {t("Windows will ask for administrator permission (User Account Control) to register the driver with the system's ODBC driver manager.")}
              </div>
            )}
            {status && !pkg && <div className="notice small">{ELSEWHERE[driver] && t(ELSEWHERE[driver])}</div>}
            {!status && !error && <p className="muted small">{t("Looking up the client…")}</p>}
            {error && <div className="notice error small">{error}</div>}
            <div className="actions">
              <span className="muted small">{t("Later: Settings › Drivers")}</span>
              <span className="spacer" />
              <button type="button" onClick={() => onDone(false)}>
                {t("Cancel")}
              </button>
              <button type="button" className="primary" disabled={!pkg} onClick={() => void install()}>
                {t(error ? "Try again" : pkg?.register ? "Download and install" : "Download and connect")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** "Client: Oracle Instant Client 23.26.4 installed · Remove", or "not installed · Install". */
export function VendorDriverState({ driver }: { driver: DriverKind }) {
  const [status, setStatus] = useState<VendorStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    setError(null);
    return api.vendorDriverStatus(driver).then(setStatus, (e: unknown) => setError(toCommandError(e).message));
  };

  useEffect(() => {
    setStatus(null);
    void load();
  }, [driver]);

  async function remove() {
    try {
      setStatus(await api.removeVendorDriver(driver));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  const installed = status?.installed;
  return (
    <div className="driver-state small" data-driver-state={status?.ready ? "installed" : "missing"}>
      <span className={`dot ${status?.ready ? "ok" : ""}`} />
      {!status && !error && <span className="muted">{t("Client: checking…")}</span>}
      {error && <span className="muted">{t("Client: {message}", { message: error })}</span>}
      {status && (
        <span>
          {installed
            ? t("Client: {name} installed", { name: `${installed.title} ${installed.version}` })
            : status.system_driver
              ? t("Client: {name} (installed in the system)", { name: status.system_driver })
              : t("Client: not installed")}
          {status.package && !installed && (
            <>
              {" · "}
              <button type="button" className="link" onClick={() => void askDriver(driver).then(load)}>
                {t("Install")}
              </button>
            </>
          )}
          {installed && (
            <>
              {" · "}
              <button type="button" className="link" onClick={() => void remove()}>
                {t("Remove")}
              </button>
            </>
          )}
        </span>
      )}
    </div>
  );
}

/** Settings › Drivers: the vendors' clients, and the ODBC drivers and data sources of the
 *  system. */
export function VendorDriversSection() {
  const [odbc, setOdbc] = useState<OdbcSystem | null>(null);
  useEffect(() => {
    api.odbcSystem().then(setOdbc, () => setOdbc(null));
  }, []);
  return (
    <div className="vendor-drivers">
      <p className="muted small">
        {t("Oracle connects through Oracle Instant Client, IBM Db2 through IBM's ODBC driver, and DuckDB databases and data files through DuckDB's library: each installed from its vendor the first time it is needed.")}
      </p>
      {VENDOR_DRIVERS.map((d) => (
        <div key={d} className="driver-row" data-vendor-driver={d}>
          <strong>{DRIVER_NAMES[d]}</strong>
          <span className="spacer" />
          <VendorDriverState driver={d} />
        </div>
      ))}
      {odbc?.available && (
        <details className="odbc-system">
          <summary className="small">
            {t("ODBC drivers of the system ({drivers}) and data sources ({sources})", { drivers: odbc.drivers.length, sources: odbc.data_sources.length })}
          </summary>
          <ul className="small">
            {odbc.drivers.map((d) => (
              <li key={d.name} title={d.path ?? undefined}>
                {d.name}
              </li>
            ))}
          </ul>
          <ul className="small">
            {odbc.data_sources.map((d) => (
              <li key={d.name}>
                {d.name} <span className="muted">· {d.driver}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
