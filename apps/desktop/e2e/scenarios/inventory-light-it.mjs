// The visual inventory in the light theme, interface in Italian. Runs with PLATYPUS_INVENTORY set; see docs/dev/INVENTORY.md.
import { runInventory, seed } from "../inventory.mjs";

export { skip } from "../inventory.mjs";

export const setup = ({ dir }) => seed(dir, { theme: "platypus_light", lang: "it" });

export default (page, ctx) => runInventory(page, ctx, "light-it");
