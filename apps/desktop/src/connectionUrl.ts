/** Connections pasted as a URL or a connection string, and copied back as a URL. The parsing
 *  is the backend's (`parse_connection_url`); this decides when to ask it and applies its
 *  answer to the form. */
import type { DriverKind, ParsedConnection, SavedConnection } from "./api";

/** `scheme://…`, `jdbc:…`, `key=value;key=value` (ADO.NET, ODBC) or `key=value key=value`
 *  (libpq): worth reading as a connection rather than pasting as text. A single `a=b;` is
 *  text: it may be a name. */
export function looksLikeConnection(text: string): boolean {
  const s = text.trim();
  if (!s || s.includes("\n")) return false;
  return (
    /^[a-z][a-z0-9+.-]*:\/\//i.test(s) ||
    /^jdbc:/i.test(s) ||
    /^[^=;]+=[^;]*;\s*[^=;\s][^=;]*=/.test(s) ||
    /^[a-z_]+\s*=\s*(?:'(?:[^'\\]|\\.)*'|\S+)\s+[a-z_]+\s*=/.test(s)
  );
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

/** A login typed in the host field, `user:password@host:port`, split into its fields;
 *  `null` when the host holds none (or is a MongoDB seed list, or a URL, which the paste
 *  handler reads). The password may hold `@` and `:`: the host is after the last `@`. */
export function splitLogin(text: string): { user: string; password: string | null; host: string; port: number | null } | null {
  const s = text.trim();
  const at = s.lastIndexOf("@");
  if (at <= 0 || s.includes("://")) return null;
  const login = s.slice(0, at);
  const address = s.slice(at + 1);
  if (!address || /[\s/,@]/.test(address)) return null;
  const colon = login.indexOf(":");
  const user = colon < 0 ? login : login.slice(0, colon);
  const password = colon < 0 ? null : login.slice(colon + 1);
  const withPort = /^(\[[^\]]+\]|[^:[\]]+):(\d{1,5})$/.exec(address);
  if (withPort) {
    const port = Number(withPort[2]);
    if (port < 1 || port > 65535) return null;
    return { user, password, host: withPort[1]!.replace(/^\[|\]$/g, ""), port };
  }
  return { user, password, host: address.replace(/^\[|\]$/g, ""), port: null };
}

/** Whether a port typed in the form can be one; none is fine (the driver's default). */
export const validPort = (port: number | null) => port === null || (Number.isInteger(port) && port >= 1 && port <= 65535);

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

/** How the form's address is entered: the host, port and database fields, or one URL. */
export type ConnectBy = "host" | "url";

/** The drivers whose form can be edited as a URL: a URL form with a server behind it (the
 *  file-based ones keep their file field). */
export const urlModeDriver = (driver: DriverKind) => hasUrl(driver) && driver !== "sqlite" && driver !== "duckdb";

/** The URL typed in the form read as its settings: the connection with the URL's host,
 *  port, database, user and options, but the name as it was (a half-typed URL must not
 *  name the connection). A password in the URL goes to the password field (`password`,
 *  the typed one when the URL has none) and `draft` is then dropped: the field shows the
 *  URL printed from the settings, which has none. */
export function applyUrlEdit(
  conn: SavedConnection,
  password: string,
  typed: string,
  parsed: ParsedConnection,
): { conn: SavedConnection; password: string; draft: string | null } {
  const next = { ...withParsed(conn, parsed), name: conn.name, driver: conn.driver };
  const hasPassword = parsed.password != null;
  return { conn: next, password: hasPassword ? parsed.password! : password, draft: hasPassword ? null : typed };
}
