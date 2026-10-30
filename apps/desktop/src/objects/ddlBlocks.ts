/** How "Generate DDL" reads many objects: in blocks, a call each, so the tab fills as they
 *  come and the read can stop between two blocks. */
import type { ObjectDdl, ObjectTarget } from "../api";

/** The first block is small, so the tab shows some DDL at once; the next ones save calls. */
export const FIRST_BLOCK = 5;
export const BLOCK = 25;

/** How many objects the next block reads, after `done` were read. */
export const blockSize = (done: number) => (done === 0 ? FIRST_BLOCK : BLOCK);

/** The text of a block: each object's DDL, or a comment saying why it has none. */
export function ddlText(objects: ObjectTarget[], results: ObjectDdl[]): string {
  return objects
    .map((o, i) => {
      const r = results[i];
      if (r && "ddl" in r) return r.ddl;
      const why = (r?.error ?? "no answer").replace(/\s+/g, " ");
      return `-- ${o.schema}.${o.name}: ${why}`;
    })
    .join("\n\n");
}
