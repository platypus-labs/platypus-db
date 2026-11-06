// The plugin catalog (Settings > Plugin catalog) against an index on a local server. With no address
// and at start nothing is contacted, and the app reads the index only when the page opens or the
// user asks. An address that is not HTTPS, a hash that differs, a package larger than the limit, a
// zip whose paths climb out, an entry that is not its package: each is refused with its reason, and
// nothing installs. A package with no valid signature asks in a separate question; a signed one goes
// straight to the same review as a manual install. An update lists what it asks anew, a step back
// asks first, and removing leaves no files.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { CTRL } from "../cdp.mjs";
import { setValue } from "../admin-users.mjs";
import { minisignKey, sha256, storedZip } from "../plugin-packages.mjs";

const EXAMPLES = resolve(dirname(fileURLToPath(import.meta.url)), "../../../../examples/plugins");
const panel = `document.querySelector('.catalog-panel')`;
const entry = (id) => `${panel}?.querySelector('[data-catalog-entry="${id}"]')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const unsigned = `${panel}?.querySelector('[data-field="unsigned-confirm"]')`;
const errorNotice = `(${panel}?.querySelector('[data-field="catalog-error"]')?.textContent ?? '')`;

const themeManifest = (id, version, extra = "") => `id = "${id}"
name = "${id}"
version = "${version}"
api = 2
kinds = ["theme"]
${extra}
[theme]
name = "${id}"
dark = { bg = "#102030" }
`;

const trusted = minisignKey();
const stranger = minisignKey();
let port = 0;
/** The paths the app asked of the server, in order. */
const asked = [];
const files = new Map();
let server;

const zipOf = (id, version) => storedZip({ "plugin.toml": themeManifest(id, version) });

function entryFor(id, version, bytes, overrides = {}) {
  const name = `${id}-${version}.zip`;
  files.set(`/${name}`, bytes);
  return {
    id,
    name: `Plugin ${id}`,
    version,
    description: `The ${id} plugin.`,
    url: name,
    sha256: sha256(bytes),
    signature: trusted.sign(bytes),
    ...overrides,
  };
}

function publish(plugins) {
  files.set("/index.json", JSON.stringify({ format: 1, plugins }));
}

export function setup({ dir }) {
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
  return new Promise((done) => {
    server = createServer((request, response) => {
      asked.push(request.url);
      const body = files.get(request.url);
      if (body === undefined) {
        response.writeHead(404);
        response.end();
        return;
      }
      response.writeHead(200);
      response.end(body);
    });
    server.listen(0, "127.0.0.1", () => {
      port = server.address().port;
      done();
    });
  });
}

const indexUrl = () => `http://127.0.0.1:${port}/index.json`;

async function openCatalog(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugin catalog");
  await page.waitFor(`${panel} !== null`, 15_000);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

async function saveUrl(page, url) {
  await setValue(page, `.catalog-panel [data-field="catalog-url"] input`, url);
  await page.eval(`${panel}.querySelector('[data-action="save-url"]').click()`);
}

/** Waits for something outside the page. */
async function until(done) {
  const deadline = Date.now() + 10_000;
  while (!done()) {
    assert.ok(Date.now() < deadline, String(done));
    await new Promise((r) => setTimeout(r, 100));
  }
}

const refreshButton = `${panel}.querySelector('[data-action="refresh"]')`;

/** Asks to read the index again, once any read before is over. */
async function refresh(page) {
  await page.waitFor(`!${refreshButton}.disabled`, 15_000);
  await page.eval(`${refreshButton}.click()`);
}

async function clickInstall(page, id) {
  await page.waitFor(`!${entry(id)}?.querySelector('[data-action="install-entry"]').disabled`, 15_000);
  await page.eval(`${entry(id)}.querySelector('[data-action="install-entry"]').click()`);
}

/** Asks to install `id` and waits for a refusal with `why`. Nothing is left behind. */
async function refusedWith(page, dir, id, why) {
  await clickInstall(page, id);
  await page.waitFor(`(${errorNotice}).includes(${JSON.stringify(why)})`, 20_000);
  assert.equal(await page.eval(`${review} == null`), true, id);
  assert.equal(existsSync(join(dir, `plugins/${id}`)), false, id);
  const downloads = join(dir, "plugins/.downloads");
  assert.deepEqual(existsSync(downloads) ? readdirSync(downloads) : [], [], id);
}

async function confirmReview(page, id) {
  await page.waitFor(`${review}?.dataset.plugin === ${JSON.stringify(id)}`, 20_000);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${review} == null`, 15_000);
}

async function nothingUntilAsked(page) {
  // The app started, and has no address.
  assert.equal(asked.length, 0);
  await openCatalog(page);
  await page.waitFor(`${panel}.textContent.includes('No index set')`);
  assert.equal(await page.eval(`${panel}.querySelector('[data-field="catalog-url"] input').value`), "");
  assert.equal(asked.length, 0, "an empty address contacts nothing");
}

async function refusesPlainHttp(page) {
  await saveUrl(page, "http://plugins.example.com/index.json");
  await page.waitFor(`(${errorNotice}).includes('not HTTPS')`);
  await saveUrl(page, "file:///c:/index.json");
  await page.waitFor(`(${errorNotice}).includes('not HTTPS')`);
  await page.waitFor(`${panel}.textContent.includes('No index set')`);
  assert.equal(asked.length, 0);
}

async function readsWhenAsked(page, dir) {
  publish([]);
  await saveUrl(page, indexUrl());
  await page.waitFor(`${panel}.querySelector('[data-field="catalog-empty"]') !== null`, 15_000);
  assert.deepEqual(asked, ["/index.json"]);
  assert.match(readFileSync(join(dir, "settings.toml"), "utf8"), /catalog_url/);

  // Read again when the user asks.
  await refresh(page);
  await until(() => asked.length === 2);

  // An index that is not one says so, and an entry it cannot use is listed apart.
  files.set("/index.json", "{not json");
  await refresh(page);
  await page.waitFor(`(${errorNotice}).includes('not a plugin index')`, 15_000);
  publish([entryFor("good", "1.0.0", zipOf("good", "1.0.0")), { id: "broken", name: "x" }]);
  await refresh(page);
  await page.waitFor(`${entry("good")} != null`, 15_000);
  assert.equal(await page.eval(errorNotice), "", "the error goes with a good read");
  assert.match(await page.eval(`${panel}.querySelector('[data-field="catalog-problems"]').textContent`), /broken/);
  assert.match(await page.eval(`${entry("good")}.textContent`), /The good plugin/);
}

async function trustsAKey(page) {
  // What is not a key is refused.
  await setValue(page, `.catalog-panel [data-field="key-text"]`, "not a key");
  await page.waitFor(`!${panel}.querySelector('[data-action="trust"]').disabled`);
  await page.eval(`${panel}.querySelector('[data-action="trust"]').click()`);
  await page.waitFor(`(${errorNotice}).includes('not a minisign public key')`);
  await setValue(page, `.catalog-panel [data-field="key-name"]`, "Jane");
  await setValue(page, `.catalog-panel [data-field="key-text"]`, trusted.publicKey);
  await page.eval(`${panel}.querySelector('[data-action="trust"]').click()`);
  await page.waitFor(`${panel}.querySelector('[data-key-name="Jane"]') !== null`);
  assert.equal(await page.eval(`${panel}.querySelector('[data-field="key-text"]').value`), "");
}

async function refusesWhatFailsItsChecks(page, dir) {
  const climbing = storedZip({ "plugin.toml": themeManifest("climbs", "1.0.0"), "../../evil.txt": "x" });
  const big = Buffer.alloc((64 << 20) + 1);
  publish([
    entryFor("hash", "1.0.0", zipOf("hash", "1.0.0"), { sha256: "00".repeat(32) }),
    entryFor("climbs", "1.0.0", climbing),
    entryFor("big", "1.0.0", big),
    entryFor("named", "1.0.0", zipOf("another", "1.0.0")),
    entryFor("versioned", "1.0.0", zipOf("versioned", "1.0.1")),
    entryFor("bad-color", "1.0.0", storedZip({ "plugin.toml": themeManifest("bad-color", "1.0.0").replace("#102030", "url(x)") })),
  ]);
  await refresh(page);
  await page.waitFor(`${entry("hash")} != null`, 15_000);
  await refusedWith(page, dir, "hash", "SHA-256");
  await refusedWith(page, dir, "climbs", "climbs out");
  await refusedWith(page, dir, "big", "larger than");
  await refusedWith(page, dir, "named", '"another"');
  await refusedWith(page, dir, "versioned", "1.0.1");
  await refusedWith(page, dir, "bad-color", "not a colour");
  assert.equal(existsSync(join(dir, "evil.txt")), false);
}

async function unsignedAsksApart(page, dir) {
  const bytes = zipOf("unsigned", "1.0.0");
  publish([
    entryFor("unsigned", "1.0.0", bytes, { signature: undefined }),
    entryFor("strange", "1.0.0", zipOf("strange", "1.0.0"), { signature: stranger.sign(zipOf("strange", "1.0.0")) }),
    entryFor("altered", "1.0.0", zipOf("altered", "1.0.0"), { signature: trusted.sign(Buffer.from("another package")) }),
  ]);
  await refresh(page);
  await page.waitFor(`${entry("unsigned")} != null`, 15_000);

  for (const [id, why] of [
    ["strange", "key you trust"],
    ["altered", "altered"],
    ["unsigned", "no signature"],
  ]) {
    await clickInstall(page, id);
    await page.waitFor(`${unsigned}?.dataset.plugin === ${JSON.stringify(id)}`, 15_000);
    const text = await page.eval(`${unsigned}.textContent`);
    assert.ok(text.includes(why), `${id}: ${text}`);
    assert.match(text, /no valid signature/);
    // The review has not opened, and nothing is installed until this is answered.
    assert.equal(await page.eval(`${review} == null`), true);
    assert.equal(existsSync(join(dir, `plugins/${id}`)), false);
  }
  // Cancel: nothing.
  await page.eval(`${unsigned}.querySelector('[data-action="cancel-unsigned"]').click()`);
  await page.waitFor(`${unsigned} == null`);
  assert.equal(existsSync(join(dir, "plugins/unsigned")), false);

  // The separate answer leads to the review, which still says it is not signed.
  await clickInstall(page, "unsigned");
  await page.waitFor(`${unsigned} != null`, 15_000);
  await page.eval(`${unsigned}.querySelector('[data-action="install-unsigned"]').click()`);
  await page.waitFor(`${review}?.dataset.plugin === 'unsigned'`, 15_000);
  const shown = `${review}.querySelector('[data-field="signature"]')`;
  assert.equal(await page.eval(`${shown}.dataset.state`), "unsigned");
  // Cancelling the review leaves no download.
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} == null`);
  await until(() => !existsSync(join(dir, "plugins/.downloads")));
  assert.equal(existsSync(join(dir, "plugins/unsigned")), false);
}

async function installsSignedAfterItsReview(page, dir) {
  publish([entryFor("sea", "1.0.0", zipOf("sea", "1.0.0"))]);
  await refresh(page);
  await page.waitFor(`${entry("sea")}?.dataset.status === 'new'`, 15_000);
  await clickInstall(page, "sea");
  await page.waitFor(`${review}?.dataset.plugin === 'sea'`, 15_000);
  const text = await page.eval(`${review}.textContent`);
  assert.match(text, /Signed by a key you trust: Jane/);
  assert.match(text, /Theme “sea”/);
  assert.equal(existsSync(join(dir, "plugins/sea")), false, "nothing without the click");
  await confirmReview(page, "sea");
  await page.waitFor(`${entry("sea")}?.dataset.status === 'current'`, 15_000);
  assert.ok(existsSync(join(dir, "plugins/sea/1.0.0/plugin.toml")));
  assert.equal(existsSync(join(dir, "plugins/.downloads")), false);
  // It is an installed plugin as any other.
  await page.settingsTab("Plugins");
  await page.waitFor(`document.querySelector('.plugins-panel .driver-block[data-plugin="sea"]') !== null`, 15_000);
  await page.settingsTab("Plugin catalog");
  await page.waitFor(`${panel} !== null`, 15_000);
}

/** The example of actions as `version`, with `permissions` in place of its own, zipped. */
function actionsZip(version, permissions) {
  const manifest = readFileSync(join(EXAMPLES, "sql-tools/plugin.toml"), "utf8")
    .replace('version = "1.0.0"', `version = "${version}"`)
    .replace("clipboard = true", permissions);
  return storedZip({ "plugin.toml": manifest, "plugin.wasm": readFileSync(join(EXAMPLES, "sql-tools/plugin.wasm")) });
}

async function anUpdateAskingForMoreAsksAgain(page) {
  publish([entryFor("sql-tools", "1.0.0", actionsZip("1.0.0", "clipboard = true"))]);
  await refresh(page);
  await page.waitFor(`${entry("sql-tools")}?.dataset.status === 'new'`, 15_000);
  await clickInstall(page, "sql-tools");
  await page.waitFor(`${review}?.dataset.plugin === 'sql-tools'`, 15_000);
  // A first install lists what it asks in plain words, as a manual one.
  assert.match(await page.eval(`${review}.textContent`), /Put text on the clipboard/);
  await confirmReview(page, "sql-tools");
  await page.waitFor(`${entry("sql-tools")}?.dataset.status === 'current'`, 15_000);

  publish([entryFor("sql-tools", "1.1.0", actionsZip("1.1.0", 'clipboard = true\nhttp = ["api.example.com"]'))]);
  await refresh(page);
  await page.waitFor(`${entry("sql-tools")}?.dataset.status === 'update'`, 15_000);
  assert.match(await page.eval(`${entry("sql-tools")}.textContent`), /Installed: 1\.0\.0/);
  await clickInstall(page, "sql-tools");
  await page.waitFor(`${review}?.dataset.plugin === 'sql-tools'`, 15_000);
  assert.match(await page.eval(`${review}.querySelector('[data-field="replaces"]').textContent`), /1\.0\.0/);
  const added = await page.eval(`${review}.querySelector('[data-field="added-permissions"]').textContent`);
  assert.match(added, /api\.example\.com/);
  assert.doesNotMatch(added, /clipboard/, "only what is new");
  // Until the answer, the installed version stays as it was.
  await page.eval(`${review}.querySelector('[data-action="cancel-install"]').click()`);
  await page.waitFor(`${review} == null`);
  await page.waitFor(`${entry("sql-tools")}?.dataset.status === 'update'`);
  await clickInstall(page, "sql-tools");
  await confirmReview(page, "sql-tools");
  await page.waitFor(`${entry("sql-tools")}?.dataset.status === 'current'`, 15_000);

  // Then the update of a theme, for what comes next.
  publish([entryFor("sea", "1.1.0", zipOf("sea", "1.1.0"))]);
  await refresh(page);
  await page.waitFor(`${entry("sea")}?.dataset.status === 'update'`, 15_000);
  await clickInstall(page, "sea");
  await confirmReview(page, "sea");
  await page.waitFor(`${entry("sea")}?.dataset.status === 'current'`, 15_000);
}

async function aStepBackAsksFirst(page, dir) {
  publish([entryFor("sea", "1.0.0", zipOf("sea", "1.0.0"))]);
  await refresh(page);
  await page.waitFor(`${entry("sea")}?.dataset.status === 'older'`, 15_000);
  await clickInstall(page, "sea");
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  assert.match(await page.eval(`document.querySelector('.modal.confirm').textContent`), /over the newer 1\.1\.0/);
  // Cancel: it stays.
  await page.eval(`[...document.querySelectorAll('.modal.confirm button')].find(b => b.textContent === 'Cancel').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.ok(existsSync(join(dir, "plugins/sea/1.1.0")));
  await clickInstall(page, "sea");
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await confirmReview(page, "sea");
  await page.waitFor(`${entry("sea")}?.dataset.status === 'current'`, 15_000);
  assert.ok(existsSync(join(dir, "plugins/sea/1.0.0")));
  assert.equal(existsSync(join(dir, "plugins/sea/1.1.0")), false);
}

async function removingLeavesNoFiles(page, dir) {
  await page.settingsTab("Plugins");
  const row = `document.querySelector('.plugins-panel .driver-block[data-plugin="sea"]')`;
  await page.waitFor(`${row} !== null`, 15_000);
  await page.eval(`${row}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger').click()`);
  await page.waitFor(`${row} === null`);
  assert.equal(existsSync(join(dir, "plugins/sea")), false);
  await page.settingsTab("Plugin catalog");
  await page.waitFor(`${entry("sea")}?.dataset.status === 'new'`, 15_000);
}

async function aKeyRemovedNoLongerCounts(page) {
  await page.eval(`${panel}.querySelector('[data-key-name="Jane"] [data-action="untrust"]').click()`);
  await page.waitFor(`${panel}.querySelector('[data-key-name="Jane"]') === null`);
  await clickInstall(page, "sea");
  await page.waitFor(`${unsigned}?.dataset.plugin === 'sea'`, 15_000);
  assert.match(await page.eval(`${unsigned}.textContent`), /trust no key/);
  await page.eval(`${unsigned}.querySelector('[data-action="cancel-unsigned"]').click()`);
}

/** Opening another page and coming back reads the index again; nothing else does. */
async function noPolling(page) {
  await closeSettings(page);
  const before = asked.length;
  await new Promise((done) => setTimeout(done, 1500));
  assert.equal(asked.length, before, "no background polling");
  await openCatalog(page);
  await page.waitFor(`${entry("sea")} != null`, 15_000);
  assert.equal(asked.length, before + 1);
  await closeSettings(page);
}

/** After a reload with an address set, the app contacts nothing until the page opens. */
async function nothingAtStart(page) {
  const before = asked.length;
  await page.eval(`location.reload()`);
  await page.waitFor(`document.querySelector('.shell') !== null`, 30_000);
  await new Promise((done) => setTimeout(done, 1500));
  assert.equal(asked.length, before, "the app starts without contacting the index");
}

export default async function (page, { dir }) {
  try {
    await nothingUntilAsked(page);
    await refusesPlainHttp(page);
    await readsWhenAsked(page, dir);
    await trustsAKey(page);
    await refusesWhatFailsItsChecks(page, dir);
    await unsignedAsksApart(page, dir);
    await installsSignedAfterItsReview(page, dir);
    await anUpdateAskingForMoreAsksAgain(page);
    await aStepBackAsksFirst(page, dir);
    await removingLeavesNoFiles(page, dir);
    await aKeyRemovedNoLongerCounts(page);
    await noPolling(page);
    await nothingAtStart(page);
  } finally {
    server?.close();
  }
}
