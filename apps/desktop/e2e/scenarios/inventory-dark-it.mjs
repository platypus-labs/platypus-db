// The visual inventory in the dark theme, interface in Italian. Runs with PLATYPUS_INVENTORY set; see docs/INVENTORY.md.
import { runInventory, seed } from "../inventory.mjs";

export { skip } from "../inventory.mjs";

export const setup = ({ dir }) => seed(dir, { theme: "platypus_dark", lang: "it" });

export default (page, ctx) => runInventory(page, ctx, "dark-it");
