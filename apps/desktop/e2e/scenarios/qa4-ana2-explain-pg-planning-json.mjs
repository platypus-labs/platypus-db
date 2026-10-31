// qa4-ana2-7 (low): the properties panel of the root step of a PostgreSQL "Explain analyze"
// shows PostgreSQL's whole "Planning" object as one line of JSON, and "Triggers" with no value.
// Steps: PostgreSQL, SELECT * FROM demo.orders WHERE status = 'paid', Explain analyze; the root
//   step's property list (Tree view, right side).
// Expected: readable rows (Planning: Shared Hit Blocks 262, ... or the object left out), and no
//   property with an empty value.
// Actual: "Planning  {"Shared Hit Blocks":262,"Shared Read Blocks":0,...}" and "Triggers  " (empty).
// Code: crates/platypus-driver-pg/src/explain.rs (properties of a node: values that are objects
//   or empty arrays are written with serde_json's to_string).
import assert from "node:assert/strict";
import { editorMenu, openEditor, saveConnections, setEditor, skipUnless } from "../qa-sql.mjs";

export const skip = () => skipUnless("pg");
export const setup = ({ dir }) => saveConnections(dir, ["pg"]);
const explainTab = `[...document.querySelectorAll('.explain-tab')].find(t => t.style.display !== 'none')`;

export default async function (page) {
  await openEditor(page, "QA PG");
  await setEditor(page, "SELECT * FROM demo.orders WHERE status = 'paid'");
  await editorMenu(page, "Explain analyze");
  await page.waitFor(`${explainTab}?.querySelector('.explain-node') != null`, 20_000);
  await page.sleep(500);
  const text = await page.eval(`${explainTab}.innerText`);
  assert.doesNotMatch(text, /Planning\s+\{"/, "an object written as JSON text");
  assert.doesNotMatch(text, /Triggers\s*\n/, "a property with no value");
}
