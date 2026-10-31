// Where the scenarios that run a database's own command-line tools (psql, pg_dump, mysql,
// mysqldump) find them: on the PATH, else in the portable server folders of the dev machine
// (D:\Dev\Tools\dbs) or in the folders named by PLATYPUS_E2E_PGBIN / PLATYPUS_E2E_MYSQLBIN.
import { spawnSync } from "./proc.mjs";
import { existsSync } from "node:fs";
import { delimiter, join } from "node:path";

const pathKey = () => Object.keys(process.env).find((k) => k.toLowerCase() === "path") ?? "PATH";

/** Whether `tool` runs from the PATH of this process. */
function onPath(tool) {
  return spawnSync(tool, ["--version"], { stdio: "ignore" }).status === 0;
}

/** The folder of the client tools `tools` (`psql`, `pg_dump`…): `""` when the PATH has them,
 *  a folder to put on the PATH, or `null` when there are none. */
function folderOf(tools, candidates) {
  if (tools.every(onPath)) return "";
  return candidates.filter(Boolean).find((dir) => tools.every((t) => existsSync(join(dir, `${t}.exe`)))) ?? null;
}

export const pgFolder = () =>
  folderOf(["psql", "pg_dump"], [process.env.PLATYPUS_E2E_PGBIN, "D:/Dev/Tools/dbs/pgsql/bin"]);
export const mysqlFolder = () =>
  folderOf(["mysql", "mysqldump"], [process.env.PLATYPUS_E2E_MYSQLBIN, "D:/Dev/Tools/dbs/mysql-8.4.7-winx64/bin"]);

/** The environment entry that puts `folders` (those not null or empty) in front of the PATH. */
export function withPath(...folders) {
  const key = pathKey();
  return { [key]: [...folders.filter(Boolean), process.env[key] ?? ""].join(delimiter) };
}

/** Whether `tool` runs from the PATH of this process. */
export const hasOnPath = onPath;
