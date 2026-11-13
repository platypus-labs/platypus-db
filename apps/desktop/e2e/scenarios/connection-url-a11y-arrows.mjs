// URL-10 (low). The "Connect by" choice says it is a radio group (role=radiogroup, two
// role=radio buttons) but does not behave as one: the arrow keys do nothing, and both
// options are Tab stops. The WAI-ARIA radio group pattern has one Tab stop (the checked
// option) and the arrows move the choice.
// Steps: New connection > PostgreSQL; Tab to "Host" of Connect by; press Right arrow.
// Expected: URL checked and focused; the unchecked option has tabindex -1.
// Actual: nothing changes; Tab goes from Host to URL.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:482-500 (buttons with
// role=radio, no onKeyDown nor roving tabindex).
import assert from "node:assert/strict";
import { newConnection, seedSqlite } from "../conn-helpers.mjs";
import { byHost, byUrl, cancel, press, url } from "../connection-url-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newConnection(page, "PostgreSQL");
  await page.eval(`${byHost}.focus()`);
  assert.equal(await page.eval(`${byHost}.getAttribute('aria-checked')`), "true");
  const hostTabStop = await page.eval(`${byUrl}.tabIndex`);
  await press(page, "ArrowRight", "ArrowRight", 39);
  await page.sleep(300);
  const checked = await page.eval(`${byUrl}.getAttribute('aria-checked')`);
  const focused = await page.eval(`document.activeElement === ${byUrl}`);
  const field = await page.eval(`${url} !== null`);
  await cancel(page);
  assert.equal(checked, "true", "Right arrow does not choose URL");
  assert.equal(focused, true);
  assert.equal(field, true);
  assert.equal(hostTabStop, -1, "the unchecked option is a Tab stop");
}
