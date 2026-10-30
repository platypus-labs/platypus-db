/** How "Generate DDL" reads many objects: in blocks, a call each, so the tab fills as they
 *  come and the read can stop between two blocks. */
import type { ObjectDdl, ObjectTarget } from "../api";

/** The first block is small, so the tab shows some DDL at once; the next ones save calls. */
export const FIRST_BLOCK = 5;
export const BLOCK = 25;

/** The `[from, to)` ranges of the blocks of `total` objects. */
export function ddlBlocks(total: number): [number, number][] {
  const out: [number, number][] = [];
  for (let from = 0; from < total; ) {
    const to = Math.min(total, from + (from === 0 ? FIRST_BLOCK : BLOCK));
    out.push([from, to]);
    from = to;
  }
  return out;
}

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

/** `text` added after what the editor holds, a blank line between them. */
export const appended = (current: string, text: string) => (current.trim() === "" ? text : `\n\n${text}`);
