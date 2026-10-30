// A release with no build for this platform (valid manifest, other platforms only) shows no banner.
import { json, noBannerScenario } from "../update-helpers.mjs";

const scenario = noBannerScenario(json({ version: "99.0.0", notes: "x", platforms: { "linux-x86_64": { signature: "s", url: "http://127.0.0.1:9/x" } } }));
export const { setup, env } = scenario;
export default scenario.default;
