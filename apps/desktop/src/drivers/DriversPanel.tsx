import { useEffect, useState } from "react";
import { api, type InstalledDriver, toCommandError } from "../api";

/** The drivers downloaded for the databases reached through ADBC, with removal. */
export function DriversPanel() {
  const [drivers, setDrivers] = useState<InstalledDriver[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.listDrivers().then(setDrivers, (e: unknown) => setError(toCommandError(e).message));
  }, []);

  async function remove(name: string) {
    try {
      setDrivers(await api.removeDriver(name));
    } catch (e) {
      setError(toCommandError(e).message);
    }
  }

  return (
    <div className="drivers-panel">
      <p className="muted small">
        Snowflake, BigQuery, Databricks and Trino connect through ADBC drivers, downloaded the first time a
        connection needs one.
      </p>
      {error && <div className="notice error small">{error}</div>}
      {drivers?.length === 0 && <p className="muted small">No driver downloaded yet.</p>}
      {drivers?.map((d) => (
        <div key={d.name} className="driver-row" data-driver={d.name}>
          <div>
            <strong>{d.title}</strong> · {d.version}
            <div className="muted small" title={d.library}>
              {d.license}
            </div>
          </div>
          <span className="spacer" />
          <button type="button" onClick={() => void remove(d.name)}>
            Remove
          </button>
        </div>
      ))}
    </div>
  );
}
