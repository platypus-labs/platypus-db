// A fake update endpoint for the update banner scenarios: a local HTTP server standing in for
// GitHub's `latest.json`, reached through `PLATYPUS_UPDATE_URL` (the app accepts only a local
// address there). No scenario installs anything: the manifest's download is never fetched.
import assert from "node:assert/strict";
import { createServer } from "node:http";

/** The version of the app under test, from `crates/platypus-core`'s workspace version. */
export const APP_VERSION = "0.2.1";

export const banner = `document.querySelector('.update-banner')`;
export const bannerButton = (text) =>
  `[...${banner}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;

/** A `latest.json` body offering `version` for every Windows target. */
export function manifest(version, notes = "") {
  const platform = { signature: "e2e-unsigned", url: "http://127.0.0.1:9/never-downloaded.exe" };
  return JSON.stringify({
    version,
    notes,
    pub_date: "2026-01-01T00:00:00Z",
    platforms: { "windows-x86_64": platform, "windows-aarch64": platform },
  });
}

/** Starts the server for a scenario, answering with `respond(req, res)`; `ctx.update` keeps
 *  its URL and the paths asked so far. With `listening: false` the port is left closed. */
export async function serve(ctx, respond, { listening = true } = {}) {
  const hits = [];
  const server = createServer((req, res) => {
    hits.push(req.url);
    respond(req, res);
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const url = `http://127.0.0.1:${server.address().port}/latest.json`;
  if (!listening) await new Promise((done) => server.close(done));
  ctx.update = { server, url, hits };
}

/** A `respond` offering `version` with `notes`. */
export const offering = (version, notes) => (_req, res) => {
  res.writeHead(200, { "content-type": "application/json" });
  res.end(manifest(version, notes));
};

export const updateEnv = (ctx) => ({ PLATYPUS_UPDATE_URL: ctx.update.url });

export const stop = (ctx) => new Promise((done) => (ctx.update.server.listening ? ctx.update.server.close(done) : done()));

/** The banner is the page's only reaction to the check: wait until the app asked (when the
 *  endpoint is up), give the answer time to be handled, then expect no banner and a live page. */
export async function expectNoBanner(page, ctx, { asked = true } = {}) {
  try {
    if (asked) {
      const until = Date.now() + 30_000;
      while (ctx.update.hits.length === 0) {
        assert.ok(Date.now() < until, "the app never asked for latest.json");
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    await new Promise((r) => setTimeout(r, 2_500));
    assert.equal(await page.eval(`${banner} === null`), true, "no update banner expected");
    // The failed or empty check left the app usable.
    assert.equal(await page.eval(`document.querySelector('.tree-row') !== null`), true);
  } finally {
    await stop(ctx);
  }
}
