// Complete user journey, import-dbeaver, mode "it": see journeys/import-dbeaver.mjs.
import * as journey from "../journeys/import-dbeaver.mjs";

const MODE = "it";
export const ready = `document.querySelector('.sidebar-header') !== null`;
export const setup = (ctx) => journey.setup(ctx, MODE);
export default (page, ctx) => journey.run(page, ctx, MODE);
