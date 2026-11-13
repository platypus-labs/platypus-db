// QA5-CONN-7 (low). In the connection form, Enter in a text field (Name, Host, User...) does
// nothing: the form is not a <form>, so a keyboard-only user has to Tab to the Save button.
// Every other dialog of the app (rename, name, filters, password prompt) submits on Enter.
// Steps: New connection, SQLite; Name "kbd", Database file; focus Name; press Enter.
// Expected: the connection is saved and the dialog closes. Actual: nothing happens.
// Code: apps/desktop/src/components/ConnectionDialog.tsx (the fields are not inside a form with
//   onSubmit; Save is a plain button).
import assert from "node:assert/strict";
import { dialog, fillForm, formField, newConnection, seedSqlite } from "../conn-helpers.mjs";
import { treeLabels } from "../navigator-helpers.mjs";

export function setup({ dir }) {
  seedSqlite(dir);
}

export default async function (page, ctx) {
  await newConnection(page, "SQLite");
  await fillForm(page, { Name: "kbd", "Database file": `${ctx.dir}/kbd.db` });
  await page.eval(`${formField("Name")}.focus()`);
  await page.key("Enter", "Enter", 13, 0);
  await page.sleep(1200);
  assert.equal(await page.eval(`${dialog} === null`), true, "Enter in the Name field did not save the connection");
  assert.ok((await page.eval(treeLabels)).includes("kbd"));
}
