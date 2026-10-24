/** A copy of `set` with `item` taken out if it was in, put in otherwise: for checkbox and
 *  chip pickers kept in React state. */
export function toggled<T>(set: ReadonlySet<T>, item: T): Set<T> {
  const next = new Set(set);
  if (!next.delete(item)) next.add(item);
  return next;
}
