// A release with no build for this platform (valid manifest, empty `platforms`) shows no banner.
import { seedSqlite } from "../conn-helpers.mjs";
import { expectNoBanner, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, (_req, res) => {
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ version: "99.0.0", notes: "x", platforms: { "linux-x86_64": { signature: "s", url: "http://127.0.0.1:9/x" } } }));
  });
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx);
}
