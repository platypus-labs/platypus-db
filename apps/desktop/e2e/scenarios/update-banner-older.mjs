// The update banner stays away when `latest.json` offers an older version than the installed
// one (a rolled-back release): the app never downgrades itself.
import { noBannerScenario, offering } from "../update-helpers.mjs";

const scenario = noBannerScenario(offering("0.0.1", "Old news."));
export const { setup, env } = scenario;
export default scenario.default;
