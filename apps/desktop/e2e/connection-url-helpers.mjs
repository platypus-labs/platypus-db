// What the third QA round's scenarios of the form's "Connect by: Host | URL" choice share
// (connection-url-*): switching mode, entering a URL as typing would, reading the form back, the
// saved connections file and the secrets left in the profile.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { button, dialog, fill, formField, newConnection, urlField } from "./conn-helpers.mjs";

export const url = `document.querySelector(${JSON.stringify(urlField)})`;
export const byUrl = `document.querySelector('.connection-dialog [data-field="connect_by_url"]')`;
export const byHost = `document.querySelector('.connection-dialog [data-field="connect_by_host"]')`;
/** The URL field's error under it, or "". */
export const urlError = `(document.querySelector('#conn-url-error')?.textContent ?? '')`;
/** The "Not used here: …" notes of the form, or "". */
export const notes = `(document.querySelector('.connection-dialog .url-notes')?.textContent ?? '')`;
/** The password (or auth token) field of the form. */
export const secretField = `document.querySelector('.connection-dialog [data-page=general] input[type=password]')`;

/** A key as the keyboard sends it: Enter also types its character, so it submits forms. */
export async function press(page, key, code, keyCode, modifiers = 0) {
  const text = key === "Enter" && !modifiers ? "\r" : undefined;
  await page.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", key, code, windowsVirtualKeyCode: keyCode, modifiers, text, unmodifiedText: text });
  await page.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: keyCode, modifiers });
  await page.sleep(120);
}
export const tab = (page) => press(page, "Tab", "Tab", 9);

/** A new connection's form for the picker's tile `label`, in URL mode. */
export async function newUrlForm(page, label) {
  await newConnection(page, label);
  await toUrl(page);
}

export async function toUrl(page) {
  await page.waitFor(`${byUrl} !== null`);
  await page.eval(`${byUrl}.click()`);
  await page.waitFor(`${url} !== null`);
  // The printed URL arrives from the backend.
  await page.sleep(250);
}

export async function toHost(page) {
  await page.eval(`${byHost}.click()`);
  await page.waitFor(`${url} === null && ${formField("Host")} !== undefined`);
}

/** Types `text` in the URL field as one edit (a paste) and waits for the backend's answer:
 *  the error, or the settings read (the form re-renders; a short wait covers both). */
export async function enterUrl(page, text) {
  await page.eval(fill(url, text));
  await page.sleep(500);
}

/** The value of each field of the General page, by its label. */
export const generalFields = (page) =>
  page.eval(
    `Object.fromEntries([...document.querySelectorAll('.connection-dialog [data-page=general] label.field')].map(l => [l.querySelector('span')?.textContent, l.querySelector('input, select, textarea')?.value]))`,
  );

/** Clicks Cancel and waits for the form to close. */
export async function cancel(page) {
  await page.eval(`${button(dialog, "Cancel")}.click()`);
  await page.waitFor(`${dialog} === null && document.querySelector('.modal.driver-picker') === null`);
}

/** The saved connections file of the profile. */
export const connectionsToml = (dir) => readFileSync(join(dir, "connections.toml"), "utf8");

/** The `[[connections]]` block of connections.toml named `name`. */
export function savedBlock(dir, name) {
  const blocks = connectionsToml(dir).split("[[connections]]");
  return blocks.find((b) => b.includes(`name = "${name}"`)) ?? "";
}

/** The profile's files (not WebView2's cache nor the bundled drivers) that hold `secret`. */
export function filesHolding(dir, secret) {
  const found = [];
  const walk = (d) => {
    for (const entry of readdirSync(d)) {
      if (entry === "webview" || entry === "drivers") continue;
      const path = join(d, entry);
      if (statSync(path).isDirectory()) walk(path);
      else if (statSync(path).size < 5_000_000 && readFileSync(path).includes(Buffer.from(secret))) found.push(path);
    }
  };
  walk(dir);
  return found;
}

/** Leaves the URL field (it shows the URL the settings print again) and returns it. */
export async function leaveUrl(page) {
  await page.eval(`${url}.focus()`);
  await page.eval(`${url}.blur()`);
  await page.sleep(250);
  return page.eval(`${url}.value`);
}
