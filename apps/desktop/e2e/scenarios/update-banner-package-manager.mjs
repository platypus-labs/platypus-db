// An app a package manager installed (the marker `share/platypusdb/package-manager` beside
// `bin/`) only announces the new version: it says how to update, offers Later, and no Install
// button. The marker is written next to the executable under test (target/share/...) and
// removed at the end.
import { existsSync, mkdirSync, readdirSync, rmSync, rmdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { seedSqlite } from "../conn-helpers.mjs";
import { banner, bannerButton, expectBanner, offering, serve, stop, updateEnv } from "../update-helpers.mjs";

const shareOf = (ctx) => join(dirname(dirname(ctx.exe)), "share", "platypusdb");
const markerOf = (ctx) => join(shareOf(ctx), "package-manager");

/** Removes the marker, then its folder if it is empty. */
function cleanUp(ctx) {
  rmSync(markerOf(ctx), { force: true });
  const share = shareOf(ctx);
  if (existsSync(share) && readdirSync(share).length === 0) rmdirSync(share);
}

export const setup = async (ctx) => {
  try {
    seedSqlite(ctx.dir);
    mkdirSync(shareOf(ctx), { recursive: true });
    writeFileSync(markerOf(ctx), "Flatpak\n");
    await serve(ctx, offering("99.2.0", "Announced only."));
  } catch (e) {
    cleanUp(ctx);
    throw e;
  }
};
export const env = updateEnv;

export default async function (page, ctx) {
  try {
    await expectBanner(page, {
      text: [/PlatypusDB 99\.2\.0 is available — Announced only\./, /Update it with Flatpak once the new version reaches it\./],
      install: false,
    });
    await page.eval(`${bannerButton("Later")}.click()`);
    await page.waitFor(`${banner} === null`);
  } finally {
    cleanUp(ctx);
    await stop(ctx);
  }
}
