// The visual inventory in the light theme, interface in English. Runs with PLATYPUS_INVENTORY set; see docs/INVENTORY.md.
import { runInventory, seed } from "../inventory.mjs";

export { skip } from "../inventory.mjs";

export const setup = ({ dir }) => seed(dir, { theme: "platypus_light", lang: "en" });

export default (page, ctx) => runInventory(page, ctx, "light-en");
