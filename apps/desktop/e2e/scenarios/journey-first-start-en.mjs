// Complete user journey, first start, mode "en": see journeys/first-start.mjs.
import { needsPostgres } from "../journey-helpers.mjs";
import * as journey from "../journeys/first-start.mjs";

const MODE = "en";
export const skip = needsPostgres;
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
