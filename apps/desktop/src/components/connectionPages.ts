// Which pages of the connection dialog hold non-default settings, for the dot after their name.
// Types only from the app: the unit tests load this module without it.
import type { SavedConnection } from "../api";

const filled = (v: string | null | undefined) => !!v?.trim();

/** The Network page: a profile, an SSH tunnel, TLS other than the default `prefer`, a
 *  certificate, a proxy, a cloud port forward or a command around the connection. */
export function networkHasSettings(c: SavedConnection): boolean {
  const o = c.options ?? {};
  return (
    !!c.network_profile ||
    !!c.ssh ||
    (c.ssl_mode !== undefined && c.ssl_mode !== "prefer") ||
    [c.ssl_root_cert, c.ssl_client_cert, c.ssl_client_key, o.proxy, o.forward, o.before_connect, o.after_disconnect].some(filled)
  );
}

/** The Init page: a variables file, a default schema or time zone, bootstrap statements. */
export function initHasSettings(c: SavedConnection): boolean {
  const o = c.options ?? {};
  return [c.init?.schema, c.init?.time_zone, c.init?.statements, o.env_file].some(filled);
}
