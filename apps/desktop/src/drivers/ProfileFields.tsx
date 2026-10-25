import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  api,
  DOWNLOADED_DRIVERS,
  type DriverStatus,
  type OdbcSystem,
  type SavedConnection,
  toCommandError,
} from "../api";
import { askDriver } from "./DriverPopup";
import { type Profile, type ProfileField, fieldValue, withField } from "./profiles";

interface Props {
  conn: SavedConnection;
  profile: Profile;
  onChange: (conn: SavedConnection) => void;
  /** The password field, when the driver takes one. */
  password: React.ReactNode;
}

/** The fields of a database reached through an ADBC or ODBC driver, and its driver's state. */
export function ProfileFields({ conn, profile, onChange, password }: Props) {
  const [odbc, setOdbc] = useState<OdbcSystem | null>(null);
  const suggests = profile.fields.some((f) => f.suggest);
  useEffect(() => {
    if (suggests) api.odbcSystem().then(setOdbc, () => setOdbc(null));
  }, [suggests]);

  async function browse(key: ProfileField["key"]) {
    const path = await open({ multiple: false });
    if (typeof path === "string") onChange(withField(conn, key, path));
  }

  const suggestions = {
    "odbc-drivers": odbc?.drivers.map((d) => d.name) ?? [],
    "odbc-dsns": odbc?.data_sources.map((d) => d.name) ?? [],
  };

  return (
    <>
      <div className="profile-fields">
        {profile.fields.map((f) => (
          <label key={f.key} className="field" data-field={f.key}>
            <span>{f.label}</span>
            <div className="row tight">
              {f.choices ? (
                <select
                  value={fieldValue(conn, f.key)}
                  onChange={(e) => onChange(withField(conn, f.key, e.target.value))}
                >
                  {f.choices.map((c) => (
                    <option key={c.value} value={c.value}>
                      {c.label}
                    </option>
                  ))}
                </select>
              ) : (
                <>
                  <input
                    required={f.required}
                    type={f.key === "port" ? "number" : "text"}
                    value={fieldValue(conn, f.key)}
                    placeholder={f.placeholder}
                    list={f.suggest}
                    onChange={(e) => onChange(withField(conn, f.key, e.target.value))}
                  />
                  {f.suggest && (
                    <datalist id={f.suggest}>
                      {suggestions[f.suggest].map((c) => (
                        <option key={c} value={c} />
                      ))}
                    </datalist>
                  )}
                </>
              )}
              {f.file && (
                <button type="button" onClick={() => void browse(f.key)}>
                  Browse…
                </button>
              )}
            </div>
          </label>
        ))}
      </div>
      {password}
      {odbc && !odbc.available && (
        <div className="notice small">ODBC drivers are reached on Windows only for now.</div>
      )}
      {DOWNLOADED_DRIVERS.includes(conn.driver) && <DriverState conn={conn} />}
    </>
  );
}

/** "Driver: installed v1.2 · Remove", or "not installed · Download". */
function DriverState({ conn }: { conn: SavedConnection }) {
  const [status, setStatus] = useState<DriverStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => api.driverStatus(conn.driver).then(setStatus, (e: unknown) => setError(toCommandError(e).message));
  const ask = () => void askDriver(conn.driver).then(load);

  useEffect(() => {
    setStatus(null);
    void load();
  }, [conn.driver]);

  const installed = status?.installed;
  const newer = installed && status?.package && status.package.version !== installed.version;
  return (
    <div className="driver-state small" data-driver-state={installed ? "installed" : "missing"}>
      <span className={`dot ${installed ? "ok" : ""}`} />
      {!status && !error && <span className="muted">Driver: checking…</span>}
      {error && <span className="muted">Driver: {error}</span>}
      {status && installed && (
        <span>
          Driver: {installed.title} {installed.version} installed
          {newer && (
            <>
              {" · "}
              <button type="button" className="link" onClick={ask}>
                Update to {status.package!.version}
              </button>
            </>
          )}
          {" · "}
          <button
            type="button"
            className="link"
            onClick={() => void api.removeDriver(installed.name).then(load)}
          >
            Remove
          </button>
        </span>
      )}
      {status && !installed && (
        <span>
          Driver: not installed ·{" "}
          <button type="button" className="link" onClick={ask}>
            Download
          </button>
        </span>
      )}
    </div>
  );
}
