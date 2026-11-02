// Upgrade from release v0.3.0 to the build under test. See upgrade.mjs.
import { upgradeScenario } from "../upgrade.mjs";

const scenario = upgradeScenario("v0.3.0", { vault: false });
export const skip = scenario.skip;
export const setup = scenario.setup;
export default scenario.run;
