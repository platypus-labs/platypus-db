/**
 * The JDBC URL of a connection that goes through a JDBC driver the app downloads: made from the
 * driver's template (`jdbc:h2:tcp://{host}:{port}/{database}`) and the connection's host, port
 * and database, until the user writes the URL by hand.
 */

export interface UrlParts {
  template: string;
  host: string;
  port: number | null;
  database: string;
}

/** The URL of `parts`, or an empty one while there is no host to point it at. */
export function fillUrl({ template, host, port, database }: UrlParts): string {
  const name = host.trim();
  if (!name) return "";
  return (
    template
      .replace(":{port}", port === null ? "" : `:${port}`)
      .replace("{host}", name.includes(":") && !name.startsWith("[") ? `[${name}]` : name)
      .replace("{database}", database.trim())
      // A part left empty goes with its key: `;schema=` and `?databaseName=` mean nothing.
      .replace(/[?&;][\w.]+=(?=$|[&;])/g, "")
  );
}

/**
 * The URL to keep when the fields change from `before` to `after`: the URL the fields make,
 * unless `current` is none of the ones `before` made: a URL written by hand stays as it is.
 */
export function refillUrl(before: UrlParts, after: UrlParts, current: string): string {
  const typed = current.trim();
  return typed === "" || typed === fillUrl(before) ? fillUrl(after) : current;
}

/** A new JDBC connection starts on H2, whose port and URL are then filled in. */
export const JDBC_START: { port: number; options: Record<string, string> } = { port: 9092, options: { "jdbc.package": "h2" } };

/** Whether the options send a connection through a JDBC driver. */
export function jdbcOn(options: Record<string, string> | undefined): boolean {
  return ["jdbc.driver", "jdbc.jars", "jdbc.package"].some((k) => !!options?.[k]?.trim());
}
