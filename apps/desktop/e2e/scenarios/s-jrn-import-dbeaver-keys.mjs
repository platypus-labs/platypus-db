// Complete user journey, import-dbeaver, mode "keys": see journeys/import-dbeaver.mjs.
import * as journey from "../journeys/import-dbeaver.mjs";

const MODE = "keys";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
