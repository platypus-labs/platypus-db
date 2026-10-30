// The update banner stays away when `latest.json` offers the version already installed.
import { APP_VERSION, noBannerScenario, offering } from "../update-helpers.mjs";

const scenario = noBannerScenario(offering(APP_VERSION, "Nothing new."));
export const { setup, env } = scenario;
export default scenario.default;
