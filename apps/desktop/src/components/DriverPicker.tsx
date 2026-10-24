import { siDuckdb, siMariadb, siMysql, siPostgresql, siSqlite } from "simple-icons";
import type { DriverKind } from "../api";

/** A generic database cylinder, for SQL Server: simple-icons has no Microsoft logos. */
const DATABASE_PATH =
  "M12 2C7.03 2 3 3.57 3 5.5v13C3 20.43 7.03 22 12 22s9-1.57 9-3.5v-13C21 3.57 16.97 2 12 2zm0 2c4.42 0 " +
  "7 1.27 7 1.5S16.42 7 12 7 5 5.73 5 5.5 7.58 4 12 4zM5 8.1C6.6 8.68 9.1 9 12 9s5.4-.32 7-.9v3.4c0 .23-2.58 " +
  "1.5-7 1.5s-7-1.27-7-1.5V8.1zm0 6c1.6.58 4.1.9 7 .9s5.4-.32 7-.9v4.4c0 .23-2.58 1.5-7 1.5s-7-1.27-7-1.5v-4.4z";

interface Choice {
  driver: DriverKind;
  label: string;
  path: string;
  /** Badge background and logo color. */
  bg: string;
  fg: string;
}

/** The tiles of the picker; MariaDB speaks the MySQL protocol. */
const CHOICES: Choice[] = [
  { driver: "postgres", label: "PostgreSQL", path: siPostgresql.path, bg: `#${siPostgresql.hex}`, fg: "#fff" },
  { driver: "mysql", label: "MySQL", path: siMysql.path, bg: `#${siMysql.hex}`, fg: "#fff" },
  { driver: "mysql", label: "MariaDB", path: siMariadb.path, bg: `#${siMariadb.hex}`, fg: "#fff" },
  { driver: "sqlserver", label: "SQL Server", path: DATABASE_PATH, bg: "#cc2927", fg: "#fff" },
  { driver: "sqlite", label: "SQLite", path: siSqlite.path, bg: `#${siSqlite.hex}`, fg: "#fff" },
  // DuckDB's yellow needs a dark badge.
  { driver: "duckdb", label: "DuckDB", path: siDuckdb.path, bg: "#1b1b1b", fg: `#${siDuckdb.hex}` },
];

function DriverLogo({ choice }: { choice: Choice }) {
  return (
    <span className="driver-logo" style={{ background: choice.bg }}>
      <svg viewBox="0 0 24 24" aria-hidden="true" fill={choice.fg}>
        <path d={choice.path} />
      </svg>
    </span>
  );
}

/** First step of a new connection: the database, as a grid of logos. */
export function DriverPicker({ onPick, onClose }: { onPick: (driver: DriverKind) => void; onClose: () => void }) {
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal driver-picker" role="dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>New connection</h2>
        <p className="muted">Choose the database.</p>
        <div className="driver-grid">
          {CHOICES.map((c, i) => (
            <button key={c.label} type="button" className="driver-tile" autoFocus={i === 0} onClick={() => onPick(c.driver)}>
              <DriverLogo choice={c} />
              <span>{c.label}</span>
            </button>
          ))}
        </div>
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
