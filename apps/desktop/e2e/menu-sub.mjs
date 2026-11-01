// Items of the navigator's object menu live in submenus (Generate SQL, Data, Maintenance):
// `revealItem` opens the one holding `item`, when the open context menu does not list it.

const buttons = `[...document.querySelectorAll('.context-menu button')]`;
const listed = (item) => `${buttons}.some(b => b.textContent.startsWith(${JSON.stringify(item)}))`;

/** Makes `item` (its text, or a prefix) a button of the open context menu; `false` if no
 *  submenu has it. The submenu buttons are the ones with `aria-haspopup`. */
export async function revealItem(page, item) {
  if (await page.eval(listed(item))) return true;
  const count = await page.eval(`${buttons}.filter(b => b.hasAttribute('aria-haspopup')).length`);
  for (let i = 0; i < count; i++) {
    await page.eval(`${buttons}.filter(b => b.hasAttribute('aria-haspopup'))[${i}].click()`);
    // The submenu renders on the next frame; give it a moment before reading it.
    for (let tries = 0; tries < 10; tries++) {
      if (await page.eval(listed(item))) return true;
      await page.sleep(50);
    }
  }
  return false;
}
