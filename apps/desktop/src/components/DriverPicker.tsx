import {
  siApachearrow,
  siClickhouse,
  siCockroachlabs,
  siDatabricks,
  siDuckdb,
  siGooglebigquery,
  siGooglecloud,
  siMariadb,
  siMongodb,
  siMysql,
  siNeon,
  siPostgresql,
  siSnowflake,
  siSqlite,
  siSupabase,
  siTimescale,
  siTrino,
  siTurso,
} from "simple-icons";
import { useState } from "react";
import type { DriverKind, SavedConnection } from "../api";
import { Icon } from "../ui/Icon";
import { t, msg } from "../i18n";

/** What a profile fills in beyond the driver's defaults. */
export type Preset = Partial<Pick<SavedConnection, "port" | "user" | "database" | "ssl_mode">>;

/** A generic database cylinder, for SQL Server, YugabyteDB, Oracle, Db2 and ODBC, which
 *  simple-icons lacks. */
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
  // ClickHouse's yellow needs a dark badge too.
  { driver: "clickhouse", label: "ClickHouse", path: siClickhouse.path, bg: DARK, fg: `#${siClickhouse.hex}` },
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
  { driver: "mongo", label: "MongoDB", path: siMongodb.path, bg: DARK, fg: `#${siMongodb.hex}` },
  // Through ADBC drivers, downloaded when a connection first needs one.
  { driver: "snowflake", label: "Snowflake", path: siSnowflake.path, bg: `#${siSnowflake.hex}`, fg: "#fff" },
  { driver: "bigquery", label: "BigQuery", path: siGooglebigquery.path, bg: `#${siGooglebigquery.hex}`, fg: "#fff" },
  { driver: "databricks", label: "Databricks", path: siDatabricks.path, bg: `#${siDatabricks.hex}`, fg: "#fff" },
  { driver: "trino", label: "Trino", path: siTrino.path, bg: `#${siTrino.hex}`, fg: "#fff" },
  { driver: "adbc", label: msg("Other ADBC driver"), path: siApachearrow.path, bg: DARK, fg: "#fff" },
  // Oracle through Instant Client, Db2 and the rest through ODBC: installed when needed.
  { driver: "oracle", label: "Oracle", path: DATABASE_PATH, bg: "#c74634", fg: "#fff" },
  { driver: "db2", label: "IBM Db2", path: DATABASE_PATH, bg: "#0f62fe", fg: "#fff" },
  { driver: "odbc", label: msg("Other ODBC driver"), path: DATABASE_PATH, bg: DARK, fg: "#fff" },
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

function Tiles({ choices, onPick }: { choices: Choice[]; onPick: (choice: Choice) => void }) {
  return (
    <div className="driver-grid">
      {choices.map((c) => (
        <button key={c.label} type="button" className="driver-tile" onClick={() => onPick(c)}>
          <DriverLogo choice={c} />
          <span>{t(c.label)}</span>
        </button>
      ))}
    </div>
  );
}

interface Props {
  onPick: (driver: DriverKind, preset: Preset) => void;
  onClose: () => void;
}

/** Lower case without accents, to compare what the user types with the labels. */
const fold = (text: string) => text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

/** The choices whose (translated) name, or driver, contains `needle`, folded. */
function matching(choices: Choice[], needle: string): Choice[] {
  return choices.filter((c) => fold(t(c.label)).includes(needle) || c.driver.includes(needle));
}

/** First step of a new connection: the database, as a grid of logos, with a search. */
export function DriverPicker({ onPick, onClose }: Props) {
  const [filter, setFilter] = useState("");
  const needle = fold(filter.trim());
  const main = matching(CHOICES, needle);
  const pgWire = matching(PG_WIRE, needle);
  const first = main[0] ?? pgWire[0];
  const pick = (c: Choice) => onPick(c.driver, c.preset ?? {});
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal driver-picker" onKeyDown={(e) => e.key === "Escape" && onClose()}>
        <h2>{t("New connection")}</h2>
        <p className="muted small">{t("Choose the database.")}</p>
        <div className="modal-search">
          <Icon name="search" className="modal-search-icon" />
          <input
            autoFocus
            placeholder={t("Search databases…")}
            aria-label={t("Search databases")}
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            onKeyDown={(e) => {
              // Enter takes the first match (PostgreSQL with no search), as the first tile
              // did when it had the focus; Escape clears the search first, then closes.
              if (e.key === "Enter" && first) pick(first);
              else if (e.key === "Escape" && filter) {
                e.stopPropagation();
                setFilter("");
              }
            }}
          />
        </div>
        {main.length > 0 && <Tiles choices={main} onPick={pick} />}
        {pgWire.length > 0 && (
          <>
            <h3 className="driver-section">{t("PostgreSQL-compatible")}</h3>
            <Tiles choices={pgWire} onPick={pick} />
          </>
        )}
        {!first && <p className="muted driver-none">{t("No database matches.")}</p>}
        <div className="actions">
          <span className="spacer" />
          <button type="button" onClick={onClose}>
            {t("Cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
