// QA ui: Escape no longer closes the Settings after a click on an empty part of the dialog.
//
// Steps: open the Settings (Ctrl+,), click its title (or any text, or empty space in it),
// press Escape.
// Expected: the Settings close, as with the focus in a field.
// Actual: nothing happens: the click moves the focus to <body>, and Escape is only handled by
// the dialog's own onKeyDown, which then never sees it.
// Code: components/SettingsDialog.tsx (`onKeyDown={(e) => e.key === "Escape" && onClose()}` on
// the .modal div, which is not focusable).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";

export function setup({ dir }) {
  writeFileSync(join(dir, "connections.toml"), "version = 1\n");
}

export const ready = `document.querySelector('.shell') !== null`;

export default async function (page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings')?.contains(document.activeElement) === true`);
  const r = await page.rect(".modal.settings h2");
  await page.click(r.x + 4, r.y + r.h / 2);
  await page.waitFor(`!document.querySelector('.modal.settings').contains(document.activeElement)`);
  await page.key("Escape", "Escape", 27);
  await page.sleep(500);
  assert.equal(await page.eval(`document.querySelector('.modal.settings') === null`), true, "Escape did not close the Settings");
}
