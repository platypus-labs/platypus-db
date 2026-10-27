// JRN-10 (medium). The command palette (Ctrl+K, "Search actions, connections, tables and
// recent queries…") has no action to add a connection, import connections or open a data
// file: typing "new connection" says "Nothing matches.", "import" finds nothing. These are
// the first things a new user does, and the welcome page shows them as the main actions;
// with the keyboard the only way is a dozen Tab presses. DBeaver has them in its menus and
// its Ctrl+3 quick access.
// Steps: first start (no connections); Ctrl+K; type "new connection" (then "import").
// Expected: "New connection…" (and "Import connections…", "Open a data file…") listed and
// run with Enter. Actual: nothing.
// Suspected: apps/desktop/src/App.tsx registers the global actions (~1200-1280): none for
// these, which exist only as buttons (navigator header, welcome page).
import assert from "node:assert/strict";
import { CTRL } from "../cdp.mjs";

export const ready = `document.querySelector('.sidebar-header') !== null`;
const items = `[...document.querySelectorAll('.palette [role=option] .palette-label')].map(l => l.textContent)`;

async function search(page, text) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type(text);
  await page.waitFor(`document.querySelector('.palette input').value === ${JSON.stringify(text)}`);
  await page.sleep(300);
  const found = await page.eval(items);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);
  return found;
}

export default async function (page) {
  assert.ok((await search(page, "new connection")).some((l) => /^New connection/.test(l)), "no New connection in the palette");
  assert.ok((await search(page, "import")).some((l) => /^Import connections/.test(l)), "no Import connections in the palette");
  assert.ok((await search(page, "data file")).some((l) => /data file/i.test(l)), "no Open a data file in the palette");
}
