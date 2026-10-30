// An app a package manager installed (the marker `share/platypusdb/package-manager` beside
// `bin/`) only announces the new version: it says how to update, offers Later, and no Install
// button. The marker is written next to the executable under test (target/share/...) and
// removed at the end.
import assert from "node:assert/strict";
import { existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { seedSqlite } from "../conn-helpers.mjs";
import { banner, bannerButton, offering, serve, stop, updateEnv } from "../update-helpers.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../../target/release/platypus-db.exe");
const share = join(dirname(dirname(exe)), "share", "platypusdb");
const marker = join(share, "package-manager");
const sharedBefore = existsSync(share);

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  mkdirSync(share, { recursive: true });
  writeFileSync(marker, "Flatpak\n");
  await serve(ctx, offering("99.2.0", "Announced only."));
};
export const env = updateEnv;

export default async function (page, ctx) {
  try {
    await page.waitFor(`${banner} !== null`, 30_000);
    const text = await page.eval(`${banner}.textContent`);
    assert.match(text, /PlatypusDB 99\.2\.0 is available — Announced only\./);
    assert.match(text, /Update it with Flatpak once the new version reaches it\./);
    assert.equal(await page.eval(`${bannerButton("Install and restart")} === undefined`), true);
    await page.eval(`${bannerButton("Later")}.click()`);
    await page.waitFor(`${banner} === null`);
  } finally {
    rmSync(sharedBefore ? marker : share, { recursive: true, force: true });
    await stop(ctx);
  }
}
