/** `rows` as a plugin may read them: the value of each column in `hidden` as `mask`, whether or
 *  not the screen masks it, and `null` left `null`. No app imports: the unit tests load it. */
export function maskRows(rows: unknown[][], hidden: boolean[], mask: string): unknown[][] {
  if (!hidden.some(Boolean)) return rows;
  return rows.map((row) => row.map((value, c) => (hidden[c] && value !== null ? mask : value)));
}
