// Journey "analysis" (en run): see journeys/analysis.mjs.
import * as journey from "../journeys/analysis.mjs";

const MODE = "en";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
