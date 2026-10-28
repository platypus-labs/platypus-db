/** The pooler mode of a PostgreSQL connection (`options.pooler`), and the one its host
 *  suggests: Supabase's pooler is in transaction mode on 6543 and in session mode on 5432,
 *  Neon's (`-pooler.` in the host) in transaction mode. */
export type PoolerMode = "session" | "transaction";

export interface PoolerSuggestion {
  mode: PoolerMode;
  service: string;
}

export function suggestedPooler(host: string, port: number | null): PoolerSuggestion | null {
  const h = host.trim().toLowerCase();
  if (h.endsWith(".pooler.supabase.com")) return { mode: port === 6543 ? "transaction" : "session", service: "Supabase" };
  if (/-pooler\.[^.]/.test(h) && h.includes("neon.tech")) return { mode: "transaction", service: "Neon" };
  return null;
}
