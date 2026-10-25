import {
  siCockroachlabs,
  siDuckdb,
  siGooglecloud,
  siMariadb,
  siMysql,
  siNeon,
  siPostgresql,
  siSqlite,
  siSupabase,
  siTimescale,
  siTurso,
} from "simple-icons";
import type { DriverKind, SavedConnection } from "../api";

/** What a profile fills in beyond the driver's defaults. */
export type Preset = Partial<Pick<SavedConnection, "port" | "user" | "database" | "ssl_mode">>;

/** A generic database cylinder, for SQL Server and YugabyteDB, which simple-icons lacks. */
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
  preset?: Preset;
}

const DARK = "#1b1b1b";

/** The tiles of the picker; MariaDB speaks the MySQL protocol. */
const CHOICES: Choice[] = [
  { driver: "postgres", label: "PostgreSQL", path: siPostgresql.path, bg: `#${siPostgresql.hex}`, fg: "#fff" },
  { driver: "mysql", label: "MySQL", path: siMysql.path, bg: `#${siMysql.hex}`, fg: "#fff" },
  { driver: "mysql", label: "MariaDB", path: siMariadb.path, bg: `#${siMariadb.hex}`, fg: "#fff" },
  { driver: "sqlserver", label: "SQL Server", path: DATABASE_PATH, bg: "#cc2927", fg: "#fff" },
  { driver: "sqlite", label: "SQLite", path: siSqlite.path, bg: `#${siSqlite.hex}`, fg: "#fff" },
  // DuckDB's yellow needs a dark badge.
  { driver: "duckdb", label: "DuckDB", path: siDuckdb.path, bg: DARK, fg: `#${siDuckdb.hex}` },
  // Turso's cloud takes verified TLS; a local sqld usually runs without it.
  {
    driver: "libsql",
    label: "Turso / libSQL",
    path: siTurso.path,
    bg: DARK,
    fg: `#${siTurso.hex}`,
    preset: { ssl_mode: "verify-full" },
  },
];

/** Servers and services that speak the PostgreSQL protocol: the PostgreSQL driver, preset. */
const PG_WIRE: Choice[] = [
  {
    driver: "postgres",
    label: "CockroachDB",
    path: siCockroachlabs.path,
    bg: `#${siCockroachlabs.hex}`,
    fg: "#fff",
    preset: { port: 26257 },
  },
  {
    driver: "postgres",
    label: "YugabyteDB",
    path: DATABASE_PATH,
    bg: "#ff5f3b",
    fg: "#fff",
    preset: { port: 5433, user: "yugabyte", database: "yugabyte" },
  },
  { driver: "postgres", label: "TimescaleDB", path: siTimescale.path, bg: DARK, fg: `#${siTimescale.hex}` },
  // The hosted services take TLS only.
  {
    driver: "postgres",
    label: "Supabase",
    path: siSupabase.path,
    bg: DARK,
    fg: `#${siSupabase.hex}`,
    preset: { user: "postgres", database: "postgres", ssl_mode: "require" },
  },
  { driver: "postgres", label: "Neon", path: siNeon.path, bg: DARK, fg: `#${siNeon.hex}`, preset: { ssl_mode: "require" } },
  {
    driver: "postgres",
    label: "AlloyDB",
    path: siGooglecloud.path,
    bg: `#${siGooglecloud.hex}`,
    fg: "#fff",
    preset: { ssl_mode: "require" },
  },
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

function Tiles({ choices, onPick, focus }: { choices: Choice[]; onPick: Props["onPick"]; focus?: boolean }) {
  return (
    <div className="driver-grid">
      {choices.map((c, i) => (
        <button
          key={c.label}
          type="button"
          className="driver-tile"
          autoFocus={focus && i === 0}
          onClick={() => onPick(c.driver, c.preset ?? {})}
        >
          <DriverLogo choice={c} />
          <span>{c.label}</span>
        </button>
      ))}
    </div>
  );
}

interface Props {
  onPick: (driver: DriverKind, preset: Preset) => void;
  onClose: () => void;
}

/** First step of a new connection: the database, as a grid of logos. */
export function DriverPicker({ onPick, onClose }: Props) {
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal driver-picker" role="dialog" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>New connection</h2>
        <p className="muted">Choose the database.</p>
        <Tiles choices={CHOICES} onPick={onPick} focus />
        <h3 className="driver-section">PostgreSQL-compatible</h3>
        <Tiles choices={PG_WIRE} onPick={onPick} />
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
