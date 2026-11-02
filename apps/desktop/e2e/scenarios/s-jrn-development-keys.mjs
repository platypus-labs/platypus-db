// Journey "development" (keys run): see journeys/development.mjs.
import * as journey from "../journeys/development.mjs";

const MODE = "keys";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
