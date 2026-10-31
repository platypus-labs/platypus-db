// child_process with `windowsHide: true` by default. Started from a detached shell (no console),
// every console child would otherwise open its own Windows Terminal tab. Not for the app itself:
// hiding a GUI window is not wanted (those launches use node:child_process directly).
import * as cp from "node:child_process";

/** Adds `windowsHide: true` to the options, wherever they sit in the arguments. */
function hidden(args) {
  const last = args.at(-1);
  if (last && typeof last === "object" && !Array.isArray(last)) {
    return [...args.slice(0, -1), { windowsHide: true, ...last }];
  }
  return [...args, { windowsHide: true }];
}

export const spawn = (...args) => cp.spawn(...hidden(args));
export const spawnSync = (...args) => cp.spawnSync(...hidden(args));
export const execFileSync = (...args) => cp.execFileSync(...hidden(args));
