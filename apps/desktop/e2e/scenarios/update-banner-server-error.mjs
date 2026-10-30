// An endpoint answering 500 (or 404, as before the first release is published) shows no
// banner and leaves the app usable.
import { seedSqlite } from "../conn-helpers.mjs";
import { expectNoBanner, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, (_req, res) => {
    res.writeHead(500);
    res.end("boom");
  });
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx);
}
