import { useEffect, useRef, useState } from "react";
import { api, toCommandError, type ObjectTarget } from "../api";
import { ddlBlocks, ddlText } from "./ddlBlocks";

/** How far the DDL of a "Generate DDL" tab has been read. */
export interface DdlProgress {
  done: number;
  total: number;
}

/**
 * Reads the DDL of `objects` a block at a time on `session` and gives each block's text to
 * `add` as it comes; `end` runs once all are read, the read failed or `stop` was called (also
 * when the tab closes). Nothing is read without objects.
 */
export function useDdlFill(
  objects: ObjectTarget[] | null | undefined,
  session: () => Promise<number>,
  add: (text: string) => void,
  end: (stopped: DdlProgress | null) => void,
): { progress: DdlProgress | null; stop: () => void } {
  const [progress, setProgress] = useState<DdlProgress | null>(null);
  const stopRun = useRef(() => {});
  const calls = useRef({ session, add, end });
  calls.current = { session, add, end };

  useEffect(() => {
    if (!objects?.length) return;
    // Each run has its own flags: a run left behind by a remount or a closed tab stops
    // without a word (`gone`), one stopped by the user says how far it went.
    let stopped = false;
    let gone = false;
    stopRun.current = () => (stopped = true);
    let done = 0;
    const total = objects.length;
    setProgress({ done, total });
    void (async () => {
      try {
        const id = await calls.current.session();
        for (const [from, to] of ddlBlocks(total)) {
          if (stopped) break;
          const block = objects.slice(from, to);
          const results = await api.objectsDdl(id, block);
          if (stopped) break;
          calls.current.add(ddlText(block, results));
          done = to;
          setProgress({ done, total });
        }
        if (gone) return;
        setProgress(null);
        calls.current.end(done < total ? { done, total } : null);
      } catch (e) {
        if (gone) return;
        calls.current.add(`-- ${toCommandError(e).message.replace(/\s+/g, " ")}`);
        setProgress(null);
        calls.current.end(null);
      }
    })();
    return () => {
      stopped = true;
      gone = true;
    };
  }, [objects]);

  return { progress, stop: () => stopRun.current() };
}
