// Upgrade from release v0.2.1 to the build under test, with the master password set in v0.2.1. See upgrade.mjs.
import { upgradeScenario } from "../upgrade.mjs";

const scenario = upgradeScenario("v0.2.1", { vault: true });
export const skip = scenario.skip;
export const setup = scenario.setup;
export default scenario.run;
