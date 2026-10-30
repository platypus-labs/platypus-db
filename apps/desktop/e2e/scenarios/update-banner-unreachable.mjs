// An endpoint nobody listens on (offline, or a firewall) shows no banner and leaves the app usable.
import { noBannerScenario, offering } from "../update-helpers.mjs";

const scenario = noBannerScenario(offering("99.0.0", ""), { listening: false });
export const { setup, env } = scenario;
export default scenario.default;
