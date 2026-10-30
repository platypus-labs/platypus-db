// The update banner stays away when `latest.json` offers an older version than the installed
// one (a rolled-back release): the app never downgrades itself.
import { seedSqlite } from "../conn-helpers.mjs";
import { expectNoBanner, offering, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, offering("0.0.1", "Old news."));
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx);
}
