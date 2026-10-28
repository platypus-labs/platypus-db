// URL-09 (low). The URL field of the URL mode has no accessible name: its "URL" caption is a
// <span> in a <div>, not a <label> (the Host, Port and other fields are labels), and the
// input has no aria-label / aria-labelledby. A screen reader announces "edit text" only, and
// a click on the caption does not focus the field.
// Steps: New connection > PostgreSQL > Connect by URL; inspect the URL field.
// Expected: the input is labelled "URL" (label element, aria-labelledby or aria-label).
// Actual: no label.
// Suspected: apps/desktop/src/components/ConnectionDialog.tsx:506-507 (`<div className=
// "field"><span>URL</span>` instead of `<label className="field">`).
import assert from "node:assert/strict";
import { seedSqlite } from "../conn-helpers.mjs";
import { cancel, newUrlForm, url } from "../qa3-url.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page) {
  await newUrlForm(page, "PostgreSQL");
  // The radio group is named.
  assert.equal(await page.eval(`document.querySelector('.connection-dialog [role=radiogroup]').getAttribute('aria-label')`), "Connect by");
  const name = await page.eval(`(() => {
    const el = ${url};
    if (el.labels && el.labels.length) return [...el.labels].map(l => l.textContent.trim()).join(' ');
    const by = el.getAttribute('aria-labelledby');
    if (by) return by.split(/\s+/).map(id => document.getElementById(id)?.textContent ?? '').join(' ').trim();
    return el.getAttribute('aria-label') ?? '';
  })()`);
  await cancel(page);
  assert.match(name, /URL/);
}
