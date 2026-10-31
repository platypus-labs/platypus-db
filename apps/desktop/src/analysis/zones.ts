/** The time zones the results loaded into DuckDB name, as the note on them says it: the
 *  first, which SQL shows all their instants in, and the others; `null` when they name one
 *  or none, and there is nothing to say. */
export function mixedZones(zones: readonly string[]): { zone: string; others: string } | null {
  const [zone, ...others] = zones;
  if (zone === undefined || others.length === 0) return null;
  return { zone, others: others.join(", ") };
}
