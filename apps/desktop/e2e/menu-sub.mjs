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

/** Opens the submenu of the open context menu whose label starts with `name` and waits until it
 *  is rendered; `false` if the menu has no such submenu. */
export async function openSubmenu(page, name) {
  const clicked = await page.eval(
    `(() => { const b = ${buttons}.find(b => b.hasAttribute('aria-haspopup') && b.textContent.startsWith(${JSON.stringify(name)})); b?.click(); return !!b; })()`,
  );
  if (clicked) await page.waitFor(`document.querySelector('.context-menu.submenu') !== null`, 5_000);
  return clicked;
}

/** The labels of the submenu `name` of the open context menu ([] if there is none). */
export async function submenuItems(page, name) {
  if (!(await openSubmenu(page, name))) return [];
  return page.eval(`[...document.querySelectorAll('.context-menu.submenu button span:first-child')].map(s => s.textContent)`);
}
