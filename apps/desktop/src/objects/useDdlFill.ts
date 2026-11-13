import { useEffect, useRef } from "react";
import { api, toCommandError, type ObjectTarget } from "../api";
import { blockSize, ddlText } from "./ddlBlocks";

/** How far the DDL of a "Generate DDL" tab has been read. */
interface DdlProgress {
  done: number;
  total: number;
}

/**
 * Reads the DDL of the objects still `pending` (after `done` were read) one block a render on
 * `session`: `add` gets the block's text, then `next` what is left (`null` when nothing is,
 * the read failed or `stop` was called) and how many were read. The tab keeps both, so a tab
 * drawn again goes on from where it was. A block asked for before Stop is still added.
 */
export function useDdlFill(
  pending: ObjectTarget[] | null | undefined,
  done: number,
  session: () => Promise<number>,
  add: (text: string) => void,
  next: (rest: ObjectTarget[] | null, done: number, stopped: boolean) => void,
): { progress: DdlProgress | null; stop: () => void } {
  const stopped = useRef(false);
  const calls = useRef({ session, add, next });
  calls.current = { session, add, next };

  useEffect(() => {
    if (!pending?.length) return;
    if (stopped.current) {
      calls.current.next(null, done, true);
      return;
    }
    // A run left behind by a tab drawn again, or closed, ends without a word.
    let gone = false;
    const block = pending.slice(0, blockSize(done));
    void (async () => {
      try {
        const results = await api.objectsDdl(await calls.current.session(), block);
        if (gone) return;
        calls.current.add(ddlText(block, results));
        const rest = pending.slice(block.length);
        calls.current.next(rest.length && !stopped.current ? rest : null, done + block.length, stopped.current && rest.length > 0);
      } catch (e) {
        if (gone) return;
        calls.current.add(`-- ${toCommandError(e).message.replace(/\s+/g, " ")}`);
        calls.current.next(null, done, false);
      }
    })();
    return () => {
      gone = true;
    };
  }, [pending]);

  return {
    progress: pending?.length ? { done, total: done + pending.length } : null,
    stop: () => (stopped.current = true),
  };
}
