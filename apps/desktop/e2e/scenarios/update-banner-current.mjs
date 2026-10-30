// The update banner stays away when `latest.json` offers the version already installed.
import { seedSqlite } from "../conn-helpers.mjs";
import { APP_VERSION, expectNoBanner, offering, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, offering(APP_VERSION, "Nothing new."));
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx);
}
