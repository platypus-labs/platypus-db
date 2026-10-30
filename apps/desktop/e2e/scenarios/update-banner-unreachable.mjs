// An endpoint nobody listens on (offline, or a firewall) shows no banner and leaves the app usable.
import { seedSqlite } from "../conn-helpers.mjs";
import { expectNoBanner, offering, serve, updateEnv } from "../update-helpers.mjs";

export const setup = async (ctx) => {
  seedSqlite(ctx.dir);
  await serve(ctx, offering("99.0.0", ""), { listening: false });
};
export const env = updateEnv;

export default async function (page, ctx) {
  await expectNoBanner(page, ctx, { asked: false });
}
