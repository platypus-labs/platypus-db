// Helpers of the explain scenarios that read plans (group E): the explain tab shown, the
// paste dialog, the views and the comparison.
import assert from "node:assert/strict";
import { CTRL } from "./cdp.mjs";

export const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;
export const operations = `[...${explainTab}.querySelectorAll('.explain-op')].map(e => e.textContent)`;

/** A plan of PostgreSQL as psql prints it: a full scan of orders with its filter. */
export const PG_BEFORE = `                                                          QUERY PLAN
-------------------------------------------------------------------------------------------------------------------------------
 HashAggregate  (cost=494.68..494.73 rows=5 width=11) (actual time=16.745..16.749 rows=5 loops=1)
   Group Key: c.country
   ->  Hash Join  (cost=36.50..469.68 rows=5000 width=3) (actual time=1.006..15.522 rows=5000 loops=1)
         Hash Cond: (o.customer_id = c.id)
         ->  Seq Scan on orders o  (cost=0.00..420.00 rows=5000 width=4) (actual time=0.020..11.330 rows=5000 loops=1)
               Filter: (status = 'paid'::text)
               Rows Removed by Filter: 15000
         ->  Hash  (cost=24.00..24.00 rows=1000 width=11) (actual time=0.950..0.951 rows=1000 loops=1)
               Buckets: 1024  Batches: 1  Memory Usage: 52kB
               ->  Seq Scan on customers c  (cost=0.00..24.00 rows=1000 width=11) (actual time=0.010..3.950 rows=1000 loops=1)
 Planning Time: 0.390 ms
 Execution Time: 16.840 ms
(12 rows)`;

/** The same statement once orders has an index on status. */
export const PG_AFTER = ` HashAggregate  (cost=366.22..366.27 rows=5 width=11) (actual time=2.431..2.435 rows=5 loops=1)
   Group Key: c.country
   ->  Hash Join  (cost=92.54..341.22 rows=5000 width=3) (actual time=0.374..1.801 rows=5000 loops=1)
         Hash Cond: (o.customer_id = c.id)
         ->  Bitmap Heap Scan on orders o  (cost=59.04..294.54 rows=5000 width=4) (actual time=0.170..0.800 rows=5000 loops=1)
               Recheck Cond: (status = 'paid'::text)
               ->  Bitmap Index Scan on orders_status  (cost=0.00..57.79 rows=5000 width=0) (actual time=0.150..0.150 rows=5000 loops=1)
                     Index Cond: (status = 'paid'::text)
         ->  Hash  (cost=24.00..24.00 rows=1000 width=11) (actual time=0.200..0.200 rows=1000 loops=1)
               ->  Seq Scan on customers c  (cost=0.00..24.00 rows=1000 width=11) (actual time=0.010..0.110 rows=1000 loops=1)
 Planning Time: 0.482 ms
 Execution Time: 2.485 ms`;

/** Sets a form field's value as typing would, so React sees it. */
export async function setField(page, selector, value) {
  await page.eval(`(() => {
    const el = document.querySelector(${JSON.stringify(selector)});
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  })()`);
}

/** Runs the palette's command `title`. */
export async function palette(page, title) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type(title);
  await page.waitFor(`document.querySelector('.palette li.selected')?.textContent.includes(${JSON.stringify(title)})`);
  await page.key("Enter", "Enter", 13);
}

/** Pastes `text` in the open plan dialog, of `source` (`""` recognizes it), and opens it. */
export async function pastePlan(page, text, source = "") {
  await page.waitFor(`document.querySelector('.plan-dialog textarea') !== null`);
  await setField(page, ".plan-dialog textarea", text);
  await setField(page, ".plan-dialog select", source);
  await page.waitFor(`!document.querySelector('.plan-dialog button[type=submit]').disabled`);
  await page.eval(`document.querySelector('.plan-dialog button[type=submit]').click()`);
}

/** Switches the explain tab to the view `label`. */
export async function showView(page, label) {
  await page.eval(`[...${explainTab}.querySelectorAll('.object-sections button')].find(b => b.textContent === ${JSON.stringify(label)}).click()`);
}

/** Answers the Compare… question with `choice`. */
export async function compareWith(page, choice) {
  await page.eval(`[...${explainTab}.querySelectorAll('button')].find(b => b.textContent === 'Compare…').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  const choices = await page.eval(`[...document.querySelectorAll('.modal.confirm button')].map(b => b.textContent)`);
  assert.ok(choices.includes(choice), `${choice} in ${choices}`);
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === ${JSON.stringify(choice)}).click()`);
}
