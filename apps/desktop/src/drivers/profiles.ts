/**
 * The connection fields of the databases reached through an ADBC driver or the generic ODBC
 * driver: what the dialog asks, kept in the connection's own fields or in its `options`
 * (`crates/platypus-driver-adbc/src/profile.rs` and `crates/platypus-driver-odbc/src/profile.rs`
 * read them).
 */
import type { DriverKind, SavedConnection } from "../api";

type Key = "host" | "port" | "database" | "user" | `options.${string}`;

export interface ProfileField {
  key: Key;
  label: string;
  placeholder?: string;
  required?: boolean;
  /** Picked with the file dialog. */
  file?: boolean;
  /** Suggested from the system's ODBC drivers or data sources. */
  choices?: "odbc-drivers" | "odbc-dsns";
}

export interface Profile {
  fields: ProfileField[];
  /** The password field's name; `null` when the driver signs in otherwise. */
  secret: string | null;
}

export const PROFILES: Partial<Record<DriverKind, Profile>> = {
  snowflake: {
    fields: [
      { key: "host", label: "Account", placeholder: "xy12345.eu-west-1", required: true },
      { key: "user", label: "User" },
      { key: "options.warehouse", label: "Warehouse", placeholder: "COMPUTE_WH" },
      { key: "options.role", label: "Role" },
      { key: "database", label: "Database" },
      { key: "options.schema", label: "Schema" },
    ],
    secret: "Password",
  },
  bigquery: {
    fields: [
      { key: "options.project", label: "Project", placeholder: "my-project-123", required: true },
      { key: "options.dataset", label: "Dataset" },
      {
        key: "options.credentials_file",
        label: "Service account key (JSON; empty: Google's default credentials)",
        file: true,
      },
    ],
    secret: null,
  },
  databricks: {
    fields: [
      { key: "host", label: "Server hostname", placeholder: "dbc-1234.cloud.databricks.com", required: true },
      { key: "options.http_path", label: "HTTP path", placeholder: "/sql/1.0/warehouses/…", required: true },
      { key: "database", label: "Catalog", placeholder: "main" },
      { key: "options.schema", label: "Schema", placeholder: "default" },
    ],
    secret: "Access token",
  },
  trino: {
    fields: [
      { key: "host", label: "Host", required: true },
      { key: "port", label: "Port" },
      { key: "user", label: "User", required: true },
      { key: "database", label: "Catalog", placeholder: "hive" },
      { key: "options.schema", label: "Schema" },
    ],
    secret: "Password (optional)",
  },
  adbc: {
    fields: [
      { key: "options.driver_file", label: "Driver library", required: true, file: true },
      { key: "options.uri", label: "URI", placeholder: "grpc+tls://host:port" },
      { key: "user", label: "User" },
    ],
    secret: "Password",
  },
  odbc: {
    fields: [
      { key: "options.dsn", label: "Data source (DSN)", choices: "odbc-dsns" },
      { key: "options.driver", label: "or driver", choices: "odbc-drivers" },
      {
        key: "options.connection_string",
        label: "Connection string (with the driver: the rest of it)",
        placeholder: "Server=host;Database=sales",
      },
      { key: "user", label: "User" },
    ],
    secret: "Password (optional)",
  },
};

export function fieldValue(conn: SavedConnection, key: Key): string {
  if (key.startsWith("options.")) return conn.options?.[key.slice("options.".length)] ?? "";
  if (key === "port") return conn.port === null ? "" : String(conn.port);
  return conn[key as "host" | "database" | "user"];
}

export function withField(conn: SavedConnection, key: Key, value: string): SavedConnection {
  if (key.startsWith("options.")) {
    const options = { ...conn.options };
    const name = key.slice("options.".length);
    if (value) options[name] = value;
    else delete options[name];
    return { ...conn, options };
  }
  if (key === "port") return { ...conn, port: value ? Number(value) : null };
  return { ...conn, [key]: value };
}
