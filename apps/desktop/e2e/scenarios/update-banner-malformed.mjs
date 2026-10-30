// A `latest.json` that is not JSON shows no banner and does not hurt the app: the check's
// error is only logged.
import { json, noBannerScenario } from "../update-helpers.mjs";

const scenario = noBannerScenario(json("{ this is not json"));
export const { setup, env } = scenario;
export default scenario.default;
