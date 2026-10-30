// The update banner: a newer release announced by `latest.json` shows its version and the
// first line of its notes, with Later and Install and restart; Later dismisses it. The
// endpoint is a local server (PLATYPUS_UPDATE_URL); nothing is downloaded or installed.
import assert from "node:assert/strict";
import { banner, bannerButton, offering, serve, stop, updateEnv } from "../update-helpers.mjs";
import { seedSqlite } from "../conn-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, offering("99.1.0", "Faster grids and a new dark theme.\nSecond line, not in the banner."));
};
export const env = updateEnv;

export default async function (page, ctx) {
  try {
    await page.waitFor(`${banner} !== null`, 30_000);
    const text = await page.eval(`${banner}.textContent`);
    assert.match(text, /PlatypusDB 99\.1\.0 is available — Faster grids and a new dark theme\./);
    assert.doesNotMatch(text, /Second line/);
    // A regular install: it offers the installer, and says nothing about portable copies
    // or package managers.
    assert.doesNotMatch(text, /portable|Update it with/);
    assert.equal(await page.eval(`${bannerButton("Later")} !== undefined`), true);
    assert.equal(await page.eval(`${bannerButton("Install and restart")} !== undefined`), true);
    assert.equal(ctx.update.hits.length, 1, "one check at startup");

    await page.eval(`${bannerButton("Later")}.click()`);
    await page.waitFor(`${banner} === null`);
    // Dismissed for this run: nothing brings it back, and no second check was made.
    await new Promise((r) => setTimeout(r, 1_500));
    assert.equal(await page.eval(`${banner} === null`), true);
    assert.equal(ctx.update.hits.length, 1);
  } finally {
    await stop(ctx);
  }
}
