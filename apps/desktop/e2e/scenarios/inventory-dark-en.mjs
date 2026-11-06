// The visual inventory in the dark theme, interface in English. Runs with PLATYPUS_INVENTORY set; see docs/dev/INVENTORY.md.
import { runInventory, seed } from "../inventory.mjs";

export { skip } from "../inventory.mjs";

export const setup = ({ dir }) => seed(dir, { theme: "platypus_dark", lang: "en" });

export default (page, ctx) => runInventory(page, ctx, "dark-en");
