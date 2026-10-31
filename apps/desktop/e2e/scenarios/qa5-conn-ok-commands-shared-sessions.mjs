// Regression guard: the commands around a connection run once for all its sessions. The
// command before runs at the first session (the navigator's), not again for query tabs opened
// later; closing one of two tabs, or both, does not run the command after (the navigator's
// session is still open); Disconnect runs it once. Needs PostgreSQL from docker/compose.yml.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { DBS, connectTo, menu, needs, saveConnections } from "../qa-nav-helpers.mjs";

export const skip = needs("postgres");

export function setup({ dir }) {
  const log = join(dir, "log.txt");
  saveConnections(
    dir,
    DBS.postgres.connection("QA PG", "00000000-0000-4000-8000-0000000a5901", `
[connections.options]
before_connect = 'echo b>> "${log}"'
after_disconnect = 'echo a>> "${log}"'
`),
  );
}

const lines = (dir) => (existsSync(join(dir, "log.txt")) ? readFileSync(join(dir, "log.txt"), "utf8").split(/\r?\n/).filter(Boolean).join(",") : "");

export default async function (page, { dir }) {
  await connectTo(page, "QA PG", "platypus");
  await page.sleep(2500);
  assert.equal(lines(dir), "b");
  for (let i = 0; i < 2; i++) {
    await menu(page, "QA PG", "New SQL script");
    await page.waitFor(`document.querySelectorAll('.query-tab').length === ${i + 1}`, 15_000);
    await page.sleep(800);
  }
  assert.equal(lines(dir), "b", "no second command before for the tabs' sessions");
  for (let i = 0; i < 2; i++) {
    await page.key("w", "KeyW", 87, 2);
    await page.waitFor(`document.querySelectorAll('.query-tab').length === ${1 - i}`, 10_000);
    await page.sleep(1200);
  }
  assert.equal(lines(dir), "b", "the navigator's session still holds the connection");
  await menu(page, "QA PG", "Disconnect");
  await page.sleep(2500);
  assert.equal(lines(dir), "b,a");
}
