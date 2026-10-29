import { workbench } from "../workbench";
import type { SavedConnection } from "../api";

/** `heading` and `warnings` as SQL comments, the way every generated script starts. */
export const commentHeader = (heading: string, warnings: string[]): string =>
  `-- ${heading}\n${warnings.map((w) => `-- ${w}\n`).join("")}`;

/** The connections a schema or table of `connection` can be compared with (the same kind of
 *  database) and the one to start from: the first other one, or `connection` itself. */
export function compareTargets(connection: SavedConnection): { targets: SavedConnection[]; initial: SavedConnection } {
  const targets = workbench()
    .connections()
    .filter((c) => c.driver === connection.driver);
  return { targets, initial: targets.find((c) => c.id !== connection.id) ?? connection };
}
