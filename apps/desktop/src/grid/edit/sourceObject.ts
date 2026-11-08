/** The object of the navigator a query in the editor reads, for the data editor. */
import type { ObjectMeta } from "../../api";

/**
 * The object named `name` among `objects`: exactly, else ignoring case where the database folds
 * names. Not where names are case-sensitive (MongoDB): `Orders` is another collection than
 * `orders`, and the edits of one would be saved to the other. Its kind comes with it, so that a
 * view stays read-only.
 */
export function sourceObject(objects: ObjectMeta[], name: string, caseSensitive: boolean): ObjectMeta | undefined {
  const exact = objects.find((o) => o.name === name);
  if (exact || caseSensitive) return exact;
  const folded = name.toLowerCase();
  return objects.find((o) => o.name.toLowerCase() === folded);
}
