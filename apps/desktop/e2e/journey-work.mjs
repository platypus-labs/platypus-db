// What the analysis, development and agents journeys (s-jrn-*) share on top of journey.mjs:
// one `ui(page, mode)` whose actions click in the English and Italian runs and go through the
// keyboard in the "keys" run (Tab to the element, Enter or Space, arrows in the navigator and in
// menus, typing in fields), asserting at each stop that the focus is visible.
import assert from "node:assert/strict";
import { CTRL } from "./cdp.mjs";
import { focusVisible, keys, press, screenIsItalian, texts } from "./journey.mjs";
import { italian, pageTexts, status, visibleTab } from "./qa-jrn.mjs";
import { row } from "./qa-xfer.mjs";

export { status, visibleTab };

// The field of "SQL on the result" is one line: Tab leaves it, and Escape would close the SQL view.
const inEditor = `!!document.activeElement?.closest?.('.cm-content') && !document.activeElement.closest('.result-sql')`;

export function ui(page, mode) {
  const kb = mode === "keys";
  const t = texts(mode);
  const self = {
    kb,
    t,
    /** The focus rings seen so far; the keys run asserts it is not empty at the end. */
    stops: 0,

    /** One Tab press that leaves an editor first (Escape, which Tab alone would not). */
    async tabOnce() {
      await page.eval(`void (window.__focusBefore = document.activeElement)`);
      for (let attempt = 0; attempt < 3; attempt++) {
        if (await page.eval(inEditor)) {
          // The first Escape may be taken by the completion; the second lets Tab leave.
          await keys.escape(page);
          await keys.escape(page);
        }
        await keys.tab(page);
        try {
          await page.waitFor(`document.activeElement !== window.__focusBefore`, 1_500);
          return;
        } catch {
          // Tab indented the editor or did nothing: leave it with Escape and try again.
        }
      }
      throw new Error("Tab did not move the focus");
    },

    /** Presses Tab until the focus is on the element `expr` evaluates to; the ring must show. */
    async tabTo(expr, max = 300, { ring = true } = {}) {
      await page.waitFor(`(${expr}) != null`, 20_000);
      for (let i = 0; i < max; i++) {
        if (await page.eval(`document.activeElement === (${expr})`)) {
          if (ring) // The SQL editors draw no ring: their caret is the focus.
          assert.ok(await page.eval(`${focusVisible} || document.activeElement.closest('.cm-content') !== null`), `no focus ring on ${expr}`);
          self.stops++;
          return;
        }
        await self.tabOnce();
      }
      throw new Error(`Tab never reached ${expr}`);
    },

    /** Presses the button `expr`: a click, or Tab and Enter. */
    async tap(expr, options) {
      await page.waitFor(`(${expr}) != null && !(${expr}).disabled`, 30_000);
      if (!kb) return page.eval(`(${expr}).click()`);
      await self.tabTo(expr, undefined, options);
      await keys.enter(page);
    },

    /** Shows the tab `expr` of a tab list: a click, or the arrows from the selected tab (the
     *  tabs of a list share one Tab stop) and Enter. */
    async selectTab(expr) {
      await page.waitFor(`(${expr}) != null`, 20_000);
      if (!kb) return page.eval(`(${expr}).click()`);
      // Tabs that are each a Tab stop are pressed like buttons.
      if (await page.eval(`(${expr}).closest('[role=tablist]') === null || (${expr}).tabIndex >= 0`)) return self.tap(expr);
      const selected = `(${expr}).closest('[role=tablist]').querySelector('[role=tab][aria-selected=true]')`;
      await self.tabTo(selected);
      const vertical = await page.eval(`(${expr}).closest('[role=tablist]').getAttribute('aria-orientation') === 'vertical'`);
      for (let i = 0; i < 40 && !(await page.eval(`document.activeElement === (${expr})`)); i++) {
        await (vertical ? keys.down(page) : keys.right(page));
        await page.sleep(40);
      }
      assert.ok(await page.eval(`document.activeElement === (${expr})`), `the arrows never reached ${expr}`);
      assert.ok(await page.eval(focusVisible), `no focus ring on ${expr}`);
      if (await page.eval(`(${expr}).getAttribute('aria-selected') !== 'true'`)) await keys.enter(page);
      await page.waitFor(`(${expr}).getAttribute('aria-selected') === 'true'`, 5_000);
    },

    /** Ticks or clears the checkbox or radio `expr` (Space on the keyboard). */
    async check(expr, on = true) {
      await page.waitFor(`(${expr}) != null`, 15_000);
      if (await page.eval(`(${expr}).checked === ${on}`)) return;
      if (!kb) await page.eval(`(${expr}).click()`);
      else {
        await self.tabTo(expr);
        await press(page, " ", "Space", 32);
      }
      await page.waitFor(`(${expr}).checked === ${on}`, 5_000);
    },

    /** Gives the focus to the field `expr` and replaces its text with `text`. */
    async type(expr, text) {
      await page.waitFor(`(${expr}) != null`, 20_000);
      if (kb) await self.tabTo(expr);
      else await page.eval(`(${expr}).focus()`);
      await page.key("a", "KeyA", 65, CTRL);
      if (text === "") await page.key("Backspace", "Backspace", 8);
      else await page.type(text);
    },

    /** Chooses the option with value `value` in the select `expr` (arrows on the keyboard). */
    async pick(expr, value) {
      await page.waitFor(`(${expr}) != null && [...(${expr}).options].some(o => o.value === ${JSON.stringify(value)})`, 20_000);
      if (await page.eval(`(${expr}).value === ${JSON.stringify(value)}`)) return;
      if (!kb) {
        await page.eval(`(() => {
          const el = ${expr};
          Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, ${JSON.stringify(value)});
          el.dispatchEvent(new Event('change', { bubbles: true }));
        })()`);
      } else {
        await self.tabTo(expr);
        const at = await page.eval(`[...(${expr}).options].findIndex(o => o.value === (${expr}).value)`);
        const to = await page.eval(`[...(${expr}).options].findIndex(o => o.value === ${JSON.stringify(value)})`);
        for (let i = 0; i < Math.abs(to - at); i++) {
          await (to > at ? keys.down(page) : keys.up(page));
          await page.sleep(40);
        }
      }
      await page.waitFor(`(${expr}).value === ${JSON.stringify(value)}`, 5_000);
    },

    /** A press of a shortcut (CTRL by default). */
    shortcut: (key, code, keyCode, modifiers = CTRL) => page.key(key, code, keyCode, modifiers),

    /** Ctrl+K, the action's name, Enter. */
    async palette(title) {
      await page.key("k", "KeyK", 75, CTRL);
      await page.waitFor(`document.activeElement?.closest('.palette') != null && document.activeElement.tagName === 'INPUT'`, 5_000);
      await page.type(title);
      await page.waitFor(`document.querySelector('.palette [role=option][aria-selected=true] .palette-label')?.textContent === ${JSON.stringify(title)}`, 5_000);
      await keys.enter(page);
    },

    /** Moves the focus in the navigator to the row `labels.at(-1)` below the chain `labels`,
     *  opening the rows of the chain. The mouse runs open them with their arrow. */
    async treeTo(labels) {
      for (let i = 0; i < labels.length; i++) {
        const target = row(labels[i], labels.slice(0, i));
        await page.waitFor(`${target} !== undefined`, 30_000);
        if (!kb) {
          if (i < labels.length - 1 && (await page.eval(`${target}.getAttribute('aria-expanded') === 'false'`))) await page.eval(`${target}.querySelector('.twisty').click()`);
          continue;
        }
        if (!(await page.eval(`document.activeElement?.classList.contains('tree-row')`))) await self.tabTo(`document.querySelector('.tree-row[tabindex="0"]') ?? document.querySelector('.tree-row')`);
        for (let n = 0; n < 400; n++) {
          const delta = await page.eval(`(() => {
            const rows = [...document.querySelectorAll('.tree-row')];
            return rows.indexOf(${target}) - rows.indexOf(document.activeElement);
          })()`);
          if (delta === 0) break;
          await (delta > 0 ? keys.down(page) : keys.up(page));
          await page.waitFor(`document.activeElement.classList.contains('tree-row')`, 5_000);
        }
        assert.ok(await page.eval(`document.activeElement === ${target}`), `the focus did not reach ${labels[i]}`);
        assert.ok(await page.eval(focusVisible), `no focus ring on the navigator row ${labels[i]}`);
        self.stops++;
        if (i < labels.length - 1 && (await page.eval(`${target}.getAttribute('aria-expanded') === 'false'`))) {
          await keys.right(page);
          await page.waitFor(`${target}.getAttribute('aria-expanded') === 'true'`, 20_000);
        }
      }
    },

    /** The menu of the navigator row `labels.at(-1)`: the item path `items` (submenus with
     *  Right). The mouse runs right-click, the keyboard Shift+F10. */
    async rowMenu(labels, items) {
      await self.treeTo(labels);
      const target = row(labels.at(-1), labels.slice(0, -1));
      if (kb) await press(page, "F10", "F10", 121, 8);
      else {
        await page.eval(`${target}.querySelector('.tree-label').id = 'jrn-row'`);
        await page.eval(`document.getElementById('jrn-row').scrollIntoView({ block: 'center' })`);
        const r = await page.rect("#jrn-row");
        await page.eval(`document.getElementById('jrn-row')?.removeAttribute('id')`);
        await page.click(r.x + 10, r.y + r.h / 2, { button: "right" });
      }
      await self.chooseMenu(items);
    },

    /** Chooses the item path `items` in the context menu that is open (submenus with Right). */
    async chooseMenu(items) {
      await page.waitFor(`document.querySelector('.context-menu') !== null`, 10_000);
      if (kb) await page.waitFor(`document.activeElement?.closest('.context-menu') != null`, 5_000);
      for (let i = 0; i < items.length; i++) {
        const want = JSON.stringify(items[i]);
        const found = `[...document.querySelectorAll('.context-menu [role^=menuitem]')].find(b => b.textContent.trim().startsWith(${want}))`;
        await page.waitFor(`${found} !== undefined`, 10_000);
        if (!kb) {
          await page.eval(`${found}.scrollIntoView({ block: 'nearest' }); ${found}.click()`);
          continue;
        }
        for (let n = 0; n < 60 && !(await page.eval(`document.activeElement === ${found}`)); n++) {
          await keys.down(page);
          await page.sleep(30);
        }
        assert.ok(await page.eval(`document.activeElement === ${found}`), `the menu focus did not reach ${items[i]}: ${await page.eval(`document.activeElement.tagName + '.' + document.activeElement.className + ' | ' + document.activeElement.textContent.slice(0, 40) + ' | menu: ' + [...document.querySelectorAll('.context-menu [role^=menuitem]')].map(b => b.textContent.trim().slice(0, 25)).join(' / ')`)}`);
        await (i < items.length - 1 ? keys.right(page) : keys.enter(page));
        if (i < items.length - 1) await page.waitFor(`document.querySelector('.context-menu.submenu, .context-menu .submenu') !== null`, 5_000);
      }
    },

    /** Opens the editor (or data) of the row `labels.at(-1)`: a double click, or Enter. */
    async openRow(labels) {
      await self.treeTo(labels);
      const target = row(labels.at(-1), labels.slice(0, -1));
      if (kb) return keys.enter(page);
      await page.eval(`${target}.id = 'jrn-open'`);
      await page.clickSel("#jrn-open", { clickCount: 2 });
      await page.eval(`document.getElementById('jrn-open')?.removeAttribute('id')`);
    },

    /** Types `sql` as the visible editor's whole text and runs it (the statement at the cursor
     *  with Ctrl+Enter, or the script with Alt+X); resolves once the run is over. */
    async runSql(sql, { script = false, timeout = 60_000 } = {}) {
      const content = `${visibleTab}.querySelector('.cm-content')`;
      await self.type(content, sql);
      const tail = JSON.stringify(sql.replace(/\s+/g, " ").slice(-15));
      await page.waitFor(`${content}.textContent.replace(/\\s+/g, ' ').includes(${tail})`);
      // A completion list left open would take the keys.
      await keys.escape(page);
      await page.eval(`(() => { const s = ${visibleTab}.querySelector('.statusbar span'); if (s) s.textContent = ''; })()`);
      if (script) await page.key("x", "KeyX", 88, 1);
      else await page.key("Enter", "Enter", 13, CTRL);
      const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === ${JSON.stringify(t("Cancel"))})`;
      await page.waitFor(`${cancel}?.disabled === true && ${status} !== '' && !${status}.includes(${JSON.stringify(t("Executing…"))})`, timeout);
      return page.eval(status);
    },

    /** Closes the dialog `modal` (an expression): Escape on the keyboard, else its button. */
    async dismiss(modal, buttonText) {
      if (kb) await keys.escape(page);
      else await page.eval(`[...${modal}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(buttonText)}).click()`);
      await page.waitFor(`${modal} === null`, 10_000);
    },

    /** Shows the tab whose title starts with `prefix`: a click, or Ctrl+Tab through the tabs. */
    async activateTab(prefix) {
      const active = `document.querySelector('.tab.active')?.textContent.startsWith(${JSON.stringify(prefix)})`;
      if (await page.eval(active)) return;
      if (!kb) {
        await page.eval(`[...document.querySelectorAll('.tab')].find(t => t.textContent.startsWith(${JSON.stringify(prefix)})).click()`);
      } else {
        for (let i = 0; i < 20 && !(await page.eval(active)); i++) {
          await press(page, "Tab", "Tab", 9, CTRL);
          await page.sleep(150);
        }
      }
      await page.waitFor(active, 5_000);
    },

    /** On an Italian run, the screen has no English left (`where` names it for the failure). */
    italian: async (where, allow = []) => {
      if (mode !== "it") return;
      // journey.mjs's check, except that a long detail line that ends in an ellipsis (the saved
      // result's date and rows in the sidebar) is allowed to be cut: its title holds the rest.
      const { texts: shown, cut } = await page.eval(pageTexts);
      const dict = italian();
      const english = shown.map((s) => s.replace(/^@[a-z-]+: /, "")).filter((s) => dict[s] !== undefined && dict[s] !== s && !allow.includes(s));
      assert.deepEqual(english, [], `${where}: English left on an Italian screen`);
      assert.deepEqual(cut.filter((c) => !c.includes("span.tree-detail")), [], `${where}: texts cut`);
    },
    screenIsItalian,
  };
  return self;
}
