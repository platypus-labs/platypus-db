// QA4 administration, SQL Server: the Parameters view gives an option the unit its name ends
// with in parentheses, so "ft crawl bandwidth (max)" shows 100 as "100 max" and
// "ft crawl bandwidth (min)" shows 0 as "0 min".
// Steps: connect to SQL Server, open Server activity > Parameters, search "bandwidth".
// Expected: the values are plain numbers (100, 0, 100, 0): "(max)" and "(min)" mean maximum and
//   minimum of the bandwidth, they are no unit ("min" would even read as minutes).
// Actual: "100 max", "0 min", "100 max", "0 min" (default and value alike).
// Code: crates/platypus-driver-mssql/src/monitor.rs `unit_of` takes any text in the trailing
//   parentheses of the name as the unit; (max) and (min) of these four options are not units
//   (the others, (s), (min) of recovery interval, (KB), (MB), (B), (%), are).
import assert from "node:assert/strict";
import { connections, needs } from "../qa-grid.mjs";
import { openAdmin, parameterCell, parameterRow, setSearch, showSection } from "../admin.mjs";

const DB = "mssql";
const NAME = "QA4 admin SQL Server units";

export const skip = needs(DB);

export function setup({ dir }) {
  connections(dir, [[NAME, DB]]);
}

export default async function (page) {
  await openAdmin(page, NAME, DB);
  await showSection(page, "Parameters");
  await page.waitFor(`${parameterRow("ft crawl bandwidth (max)")} != null`, 30_000);
  // "bandwidth" keeps the four options in view: "ft crawl bandwidth" would hide the notify ones.
  await setSearch(page, "bandwidth");
  for (const name of ["ft crawl bandwidth (max)", "ft notify bandwidth (max)", "ft crawl bandwidth (min)", "ft notify bandwidth (min)"]) {
    await page.waitFor(`${parameterRow(name)} != null`);
    for (const col of [1, 2]) {
      const text = await parameterCell(page, name, col);
      assert.match(text, /^\d+$/, `${name} column ${col} is "${text}": (max) and (min) are no units`);
    }
  }
}
