// Group J against the dev MariaDB: as admin-sessions-mysql.mjs, with MariaDB's own lock
// tables (information_schema.INNODB_LOCK_WAITS and INNODB_LOCKS).
import { connections, needs } from "../qa-grid.mjs";
import { lockFlow } from "./admin-sessions-mysql.mjs";

const DB = "mariadb";
const NAME = "QA J MariaDB";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  await lockFlow(page, DB, NAME);
}
