/**
 * The connection fields of the databases reached through an ADBC driver: what the dialog
 * asks, kept in the connection's own fields or in its `options`
 * (`crates/platypus-driver-adbc/src/profile.rs` reads them).
 */
import type { DriverKind, SavedConnection, SslMode } from "../api";

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
      {
        key: "ssl_mode",
        label: "TLS",
        // `profile.rs` maps these to the driver's `SSL` options.
        choices: [
          { value: "prefer", label: "Automatic — HTTPS on port 443, HTTP otherwise" },
          { value: "disable", label: "Off — HTTP" },
          { value: "verify-full", label: "On — HTTPS, verify certificate" },
          { value: "require", label: "On — HTTPS, unverified" },
        ],
      },
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
