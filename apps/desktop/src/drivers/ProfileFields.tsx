import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, DOWNLOADED_DRIVERS, type DriverStatus, type SavedConnection, toCommandError } from "../api";
import { askDriver } from "./DriverPopup";
import { type Profile, type ProfileField, fieldValue, withField } from "./profiles";
import { t } from "../i18n";

interface Props {
  conn: SavedConnection;
  profile: Profile;
  onChange: (conn: SavedConnection) => void;
  /** The password field, when the driver takes one. */
  password: React.ReactNode;
}

/** The fields of a database reached through an ADBC driver, and its driver's state. */
export function ProfileFields({ conn, profile, onChange, password }: Props) {
  async function browse(key: ProfileField["key"]) {
    const path = await open({ multiple: false });
    if (typeof path === "string") onChange(withField(conn, key, path));
  }

  return (
    <>
      <div className="profile-fields">
        {profile.fields.map((f) => (
          <label key={f.key} className="field" data-field={f.key}>
            <span>{t(f.label)}</span>
            <div className="row tight">
              {f.choices ? (
                <select
                  value={fieldValue(conn, f.key)}
                  onChange={(e) => onChange(withField(conn, f.key, e.target.value))}
                >
                  {f.choices.map((c) => (
                    <option key={c.value} value={c.value}>
                      {t(c.label)}
                    </option>
                  ))}
                </select>
              ) : (
                <input
                  required={f.required}
                  type={f.key === "port" ? "number" : "text"}
                  value={fieldValue(conn, f.key)}
                  placeholder={f.placeholder && t(f.placeholder)}
                  onChange={(e) => onChange(withField(conn, f.key, e.target.value))}
                />
              )}
              {f.file && (
                <button type="button" onClick={() => void browse(f.key)}>
                  {t("Browse…")}
                </button>
              )}
            </div>
          </label>
        ))}
      </div>
      {password}
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
      {!status && !error && <span className="muted">{t("Driver: checking…")}</span>}
      {error && <span className="muted">{t("Driver: {message}", { message: error })}</span>}
      {status && installed && (
        <span>
          {t("Driver: {name} installed", { name: `${installed.title} ${installed.version}` })}
          {newer && (
            <>
              {" · "}
              <button type="button" className="link" onClick={ask}>
                {t("Update to {version}", { version: status.package!.version })}
              </button>
            </>
          )}
          {" · "}
          <button
            type="button"
            className="link"
            onClick={() => void api.removeDriver(installed.name).then(load)}
          >
            {t("Remove")}
          </button>
        </span>
      )}
      {status && !installed && (
        <span>
          {t("Driver: not installed")} ·{" "}
          <button type="button" className="link" onClick={ask}>
            {t("Download")}
          </button>
        </span>
      )}
    </div>
  );
}
