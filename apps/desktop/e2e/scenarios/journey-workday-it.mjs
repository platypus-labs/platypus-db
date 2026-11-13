// Complete user journey, workday, mode "it": see journeys/workday.mjs.
import * as journey from "../journeys/workday.mjs";

const MODE = "it";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
