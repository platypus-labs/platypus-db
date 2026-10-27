// Regression guard: Copy as URL from the connection menu never carries a secret, and what it
// copies reads back (pasted in the picker) as the same connection: user names and databases
// with reserved characters are percent-encoded, an IPv6 host is bracketed, MongoDB options
// that hold a secret (`password=`, AWS_SESSION_TOKEN) are left out, libSQL's token is never
// in the URL. Oracle (no URL scheme) has no Copy as URL.
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, formField, menu, menuItem, openPicker, paste, picker, treeRow, urlField } from "../conn-helpers.mjs";

const CONNECTIONS = [
  ["01", "QA pg odd", `driver = "postgres"\nhost = "::1"\nport = 5433\ndatabase = "my db/ü"\nuser = "u@x:y/z"\nssl_mode = "require"`],
  ["02", "QA mongo", `driver = "mongo"\nhost = "h1:1,h2:2"\nport = 27017\ndatabase = "shop?replicaSet=rs0&authSource=admin&password=leak1&authMechanismProperties=AWS_SESSION_TOKEN:leak2"\nuser = "u"`],
  ["03", "QA libsql", `driver = "libsql"\nhost = "db-acme.turso.io"\nssl_mode = "verify-full"\nsave_password = true`],
  ["04", "QA oracle", `driver = "oracle"\nhost = "h"\nport = 1521\ndatabase = "FREEPDB1"\nuser = "u"`],
];

export function setup({ dir }) {
  let toml = "version = 7\n";
  for (const [n, name, body] of CONNECTIONS) {
    toml += `\n[[connections]]\nid = "00000000-0000-4000-8000-0000000000${n}"\nname = "${name}"\n${body.includes("save_password") ? "" : "save_password = false\n"}${body}\n`;
  }
  writeFileSync(join(dir, "connections.toml"), toml);
}

async function copyUrl(page, name) {
  await page.eval(`window.__copied = undefined; navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
  await page.eval(`document.getElementById('qa-copy')?.removeAttribute('id'); ${treeRow(name)}.id = 'qa-copy'`);
  const items = await menu(page, "#qa-copy");
  if (!items.includes("Copy as URL")) {
    await page.key("Escape", "Escape", 27);
    await page.waitFor(`document.querySelector('.context-menu') === null`);
    return null;
  }
  await menuItem(page, "Copy as URL");
  await page.waitFor(`window.__copied !== undefined`);
  return page.eval(`window.__copied`);
}

async function readBack(page, url) {
  await openPicker(page);
  await page.eval(paste(`document.querySelector(${JSON.stringify(urlField)})`, url));
  await page.waitFor(`${dialog} !== null`);
  const fields = await page.eval(`Object.fromEntries([...document.querySelectorAll('.connection-dialog [data-page=general] label.field')].map(l => [l.querySelector('span').textContent, l.querySelector('input, select')?.value]))`);
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && ${picker} === null`);
  return fields;
}

export default async function (page) {
  await page.waitFor(`${treeRow("QA pg odd")} !== undefined`);

  const pg = await copyUrl(page, "QA pg odd");
  assert.equal(pg, "postgres://u%40x%3Ay%2Fz@[::1]:5433/my%20db%2F%C3%BC?sslmode=require");
  const back = await readBack(page, pg);
  assert.equal(back.Host, "::1");
  assert.equal(back.Port, "5433");
  assert.equal(back.Database, "my db/ü");
  assert.equal(back.User, "u@x:y/z");

  const mongo = await copyUrl(page, "QA mongo");
  assert.doesNotMatch(mongo, /leak1|leak2|password=|AWS_SESSION_TOKEN/);
  assert.equal(mongo, "mongodb://u@h1:1,h2:2/shop?replicaSet=rs0&authSource=admin");

  const libsql = await copyUrl(page, "QA libsql");
  // `libsql://` means verified TLS (CONN-04): the default mode is not written out.
  assert.equal(libsql, "libsql://db-acme.turso.io");

  assert.equal(await copyUrl(page, "QA oracle"), null);
}
