// An endpoint answering 500 shows no banner and leaves the app usable.
import { noBannerScenario } from "../update-helpers.mjs";

const scenario = noBannerScenario((_req, res) => {
  res.writeHead(500);
  res.end("boom");
});
export const { setup, env } = scenario;
export default scenario.default;
