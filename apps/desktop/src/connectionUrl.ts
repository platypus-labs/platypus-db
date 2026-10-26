/** Connections pasted as a URL or a connection string, and copied back as a URL. The parsing
 *  is the backend's (`parse_connection_url`); this decides when to ask it and applies its
 *  answer to the form. */
import type { DriverKind, ParsedConnection, SavedConnection } from "./api";

/** `scheme://…`, `jdbc:…`, or `key=value;key=value`: worth reading as a connection rather
 *  than pasting as text. */
export function looksLikeConnection(text: string): boolean {
  const s = text.trim();
  if (!s || s.includes("\n")) return false;
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(s) || /^jdbc:/i.test(s) || /^[^=;]+=[^;]*;/.test(s);
}

/** The drivers whose connections have a URL (`connection_url` answers for them). */
const URL_DRIVERS: ReadonlySet<DriverKind> = new Set([
  "postgres",
  "mysql",
  "sqlserver",
  "clickhouse",
  "libsql",
  "mongo",
  "sqlite",
  "duckdb",
]);

export const hasUrl = (driver: DriverKind) => URL_DRIVERS.has(driver);

/** The form's connection with the fields of a parsed URL; the name, when empty, becomes the
 *  database's (without its options) or the host. */
export function withParsed(conn: SavedConnection, parsed: ParsedConnection): SavedConnection {
  const next: SavedConnection = {
    ...conn,
    driver: parsed.driver,
    host: parsed.host,
    port: parsed.port,
    database: parsed.database,
    user: parsed.user,
    ssl_mode: parsed.ssl_mode,
    ssl_root_cert: parsed.ssl_root_cert ?? conn.ssl_root_cert,
    ssl_client_cert: parsed.ssl_client_cert ?? conn.ssl_client_cert ?? null,
    ssl_client_key: parsed.ssl_client_key ?? conn.ssl_client_key ?? null,
    options: { ...conn.options, ...parsed.options },
    // `.pgpass` is PostgreSQL's: another driver keeps the password in the keychain.
    ...(parsed.driver !== "postgres" && conn.pgpass ? { pgpass: false, save_password: true } : {}),
  };
  const fileName = parsed.database.split(/[\\/]/).pop() ?? "";
  const named = parsed.driver === "sqlite" || parsed.driver === "duckdb" ? fileName : parsed.database.split("?")[0];
  return { ...next, name: conn.name || named || parsed.host };
}
