// The update banner: a newer release announced by `latest.json` shows its version and the
// first line of its notes, with Later and Install and restart; Later dismisses it. The
// endpoint is a local server (PLATYPUS_UPDATE_URL); nothing is downloaded or installed.
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { banner, bannerButton, expectBanner, offering, serve, stop, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, offering("99.1.0", "Faster grids and a new dark theme.\nSecond line, not in the banner."));
};
export const env = updateEnv;

export default async function (page, ctx) {
  try {
    // A regular install: it offers the installer, and says nothing about portable copies
    // or package managers.
    await expectBanner(page, {
      text: [/PlatypusDB 99\.1\.0 is available — Faster grids and a new dark theme\./],
      absent: [/Second line/, /portable|Update it with/],
      install: true,
    });
    assert.ok(ctx.update.hits.length >= 1, "the app checked at startup");
    const checks = ctx.update.hits.length;

    await page.eval(`${bannerButton("Later")}.click()`);
    await page.waitFor(`${banner} === null`);
    // Dismissed for this run: nothing brings it back, and no second check was made.
    await new Promise((r) => setTimeout(r, 1_500));
    assert.equal(await page.eval(`${banner} === null`), true);
    assert.equal(ctx.update.hits.length, checks);
  } finally {
    await stop(ctx);
  }
}
