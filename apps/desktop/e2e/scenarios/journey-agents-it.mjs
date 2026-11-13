// Journey "agents" (it run): see journeys/agents.mjs.
import * as journey from "../journeys/agents.mjs";

const MODE = "it";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const skip = journey.skip;
export const env = journey.env;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
