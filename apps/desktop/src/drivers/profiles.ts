/**
 * The connection fields of the databases reached through an ADBC driver or the generic ODBC
 * driver: what the dialog asks, kept in the connection's own fields or in its `options`
 * (`crates/platypus-driver-adbc/src/profile.rs` and `crates/platypus-driver-odbc/src/profile.rs`
 * read them).
 */
import type { DriverKind, SavedConnection, SslMode } from "../api";
import { msg } from "../i18n";

const OPTION = "options.";

type Key = "host" | "port" | "database" | "user" | "ssl_mode" | `${typeof OPTION}${string}`;

export interface ProfileField {
  key: Key;
  label: string;
  placeholder?: string;
  required?: boolean;
  /** Picked with the file dialog. */
  file?: boolean;
  /** Picked from a list instead of typed. */
  choices?: { value: string; label: string }[];
  /** Typed, with suggestions from the system's ODBC drivers or data sources. */
  suggest?: "odbc-drivers" | "odbc-dsns";
}

export interface Profile {
  fields: ProfileField[];
  /** The password field's name; `null` when the driver signs in otherwise. */
  secret: string | null;
}

export const PROFILES: Partial<Record<DriverKind, Profile>> = {
  snowflake: {
    fields: [
      { key: "host", label: msg("Account"), placeholder: "xy12345.eu-west-1", required: true },
      { key: "user", label: msg("User") },
      { key: "options.warehouse", label: msg("Warehouse"), placeholder: "COMPUTE_WH" },
      { key: "options.role", label: msg("Role") },
      { key: "database", label: msg("Database") },
      { key: "options.schema", label: msg("Schema") },
    ],
    secret: msg("Password"),
  },
  bigquery: {
    fields: [
      { key: "options.project", label: msg("Project"), placeholder: "my-project-123", required: true },
      { key: "options.dataset", label: msg("Dataset") },
      {
        key: "options.credentials_file",
        label: msg("Service account key (JSON; empty: Google's default credentials)"),
        file: true,
      },
    ],
    secret: null,
  },
  databricks: {
    fields: [
      { key: "host", label: msg("Server hostname"), placeholder: "dbc-1234.cloud.databricks.com", required: true },
      { key: "options.http_path", label: msg("HTTP path"), placeholder: "/sql/1.0/warehouses/…", required: true },
      { key: "database", label: msg("Catalog"), placeholder: "main" },
      { key: "options.schema", label: msg("Schema"), placeholder: "default" },
    ],
    secret: msg("Access token"),
  },
  trino: {
    fields: [
      { key: "host", label: msg("Host"), required: true },
      { key: "port", label: msg("Port") },
      { key: "user", label: msg("User"), required: true },
      { key: "database", label: msg("Catalog"), placeholder: "hive" },
      { key: "options.schema", label: msg("Schema") },
      {
        key: "ssl_mode",
        label: msg("TLS"),
        // `profile.rs` maps these to the driver's `SSL` options.
        choices: [
          { value: "prefer", label: msg("Automatic — HTTPS on port 443, HTTP otherwise") },
          { value: "disable", label: msg("Off — HTTP") },
          { value: "verify-full", label: msg("On — HTTPS, verify certificate") },
          { value: "require", label: msg("On — HTTPS, unverified") },
        ],
      },
    ],
    secret: msg("Password (optional)"),
  },
  adbc: {
    fields: [
      { key: "options.driver_file", label: msg("Driver library"), required: true, file: true },
      { key: "options.uri", label: msg("URI"), placeholder: "grpc+tls://host:port" },
      { key: "user", label: msg("User") },
    ],
    secret: msg("Password"),
  },
  odbc: {
    fields: [
      { key: "options.dsn", label: msg("Data source (DSN)"), suggest: "odbc-dsns" },
      { key: "options.driver", label: msg("or driver"), suggest: "odbc-drivers" },
      {
        key: "options.connection_string",
        label: msg("Connection string (with the driver: the rest of it)"),
        placeholder: "Server=host;Database=sales",
      },
      { key: "user", label: msg("User") },
    ],
    secret: msg("Password (optional)"),
  },
};

export function fieldValue(conn: SavedConnection, key: Key): string {
  if (key.startsWith(OPTION)) return conn.options?.[key.slice(OPTION.length)] ?? "";
  if (key === "port") return conn.port === null ? "" : String(conn.port);
  if (key === "ssl_mode") return conn.ssl_mode;
  return conn[key as "host" | "database" | "user"];
}

export function withField(conn: SavedConnection, key: Key, value: string): SavedConnection {
  if (key.startsWith(OPTION)) {
    const options = { ...conn.options };
    const name = key.slice(OPTION.length);
    if (value) options[name] = value;
    else delete options[name];
    return { ...conn, options };
  }
  if (key === "port") return { ...conn, port: value ? Number(value) : null };
  if (key === "ssl_mode") return { ...conn, ssl_mode: value as SslMode };
  return { ...conn, [key]: value };
}
