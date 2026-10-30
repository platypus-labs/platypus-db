// A `latest.json` that is not JSON, or that the server fails to deliver, shows no banner and
// does not hurt the app: the check's error is only logged.
import { seedSqlite } from "../conn-helpers.mjs";
import { expectNoBanner, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, (_req, res) => {
    res.writeHead(200, { "content-type": "application/json" });
    res.end("{ this is not json");
  });
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx);
}
