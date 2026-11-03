// What the user-journey scenarios (qa2-jrn-*) share: a real restart of the app inside one
// scenario (the window closed as a user closes it, then the executable started again on the
// same profile), the PostgreSQL container check and a few page probes.
import { spawn as spawnApp } from "node:child_process";
import { spawnSync } from "./proc.mjs";
import { readFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { connect } from "./cdp.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../target/release/platypus-db.exe");
const port = Number(process.env.PLATYPUS_E2E_PORT ?? 9333);
const started = [];

export const visibleTab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
export const status = `(${visibleTab}?.querySelector('.statusbar span')?.textContent ?? '')`;
export const treeRow = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
export const button = (scope, text) =>
  `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)} || b.getAttribute('aria-label') === ${JSON.stringify(text)} || b.title === ${JSON.stringify(text)})`;

/** The app processes this run started: the harness's and the ones started by `restart`. */
function appPids() {
  const out = spawnSync(
    "powershell",
    [
      "-NoProfile",
      "-Command",
      `Get-CimInstance Win32_Process -Filter "Name='${exe.split(/[\\/]/).pop()}' AND ParentProcessId=${process.pid}" | ForEach-Object { $_.ProcessId }`,
    ],
    { encoding: "utf8" },
  ).stdout;
  return out
    .split(/\s+/)
    .filter(Boolean)
    .map(Number);
}

const portFree = () =>
  new Promise((done) => {
    const socket = tcp({ host: "127.0.0.1", port, timeout: 500 });
    socket.on("connect", () => {
      socket.destroy();
      done(false);
    });
    socket.on("error", () => done(true));
    socket.on("timeout", () => {
      socket.destroy();
      done(true);
    });
  });

/** Closes the window as a user would (its close button) and returns what stopped it, if
 *  anything: the text of a dialog that asked first, or null once the app has exited. With
 *  `answer` the dialog's button of that text is clicked. */
export async function closeApp(page, { answer = null, timeout = 20_000 } = {}) {
  // Not awaited: the window can go before the page answers, and the answer then never comes.
  page.eval(`(() => {
    const own = document.querySelector('.window-controls .close');
    if (own) own.click();
    else window.__TAURI_INTERNALS__.invoke('plugin:window|close', { label: 'main' });
  })()`).catch(() => {});
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (appPids().length === 0) break;
    const asked = await page
      .eval(`document.querySelector('.modal')?.textContent ?? null`)
      .catch(() => null);
    if (asked && answer === null) return asked;
    if (asked && answer !== null) {
      await page.eval(`[...document.querySelector('.modal').querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(answer)})?.click()`).catch(() => {});
      answer = null;
    }
    await new Promise((r) => setTimeout(r, 300));
  }
  if (appPids().length) throw new Error("the app did not exit after its window was closed");
  return null;
}

/** Starts the app again on the scenario's profile and returns its page once `ready` holds. */
export async function launch(ctx, ready = `document.querySelector('.sidebar-header') !== null`) {
  // The DevTools port is the old WebView2's until it has gone.
  const deadline = Date.now() + 20_000;
  while (!(await portFree()) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 200));
  const app = spawnApp(exe, [], {
    env: {
      ...process.env,
      PLATYPUS_DATA_DIR: ctx.dir,
      WEBVIEW2_USER_DATA_FOLDER: join(ctx.dir, "webview"),
      PLATYPUS_DEVTOOLS_PORT: String(port),
    },
    stdio: "ignore",
  });
  started.push(app.pid);
  const page = await connect(port, 60_000);
  await page.waitFor(ready, 60_000);
  return page;
}

/** Closes the window and starts the app again: the user's restart. */
export async function restart(page, ctx, options = {}) {
  const asked = await closeApp(page, options);
  if (asked !== null) throw new Error(`closing the window asked: ${asked}`);
  return reopen(page, ctx, options.ready);
}

/** Starts the app again after `closeApp` and points `page` at it: the harness keeps using the
 *  same object (its failure report reads the new window), and its closing the page at the end
 *  of the scenario ends the new app too. */
export async function reopen(page, ctx, ready) {
  const fresh = await launch(ctx, ready);
  page.close();
  page.send = fresh.send;
  page.close = () => {
    fresh.close();
    stopStarted();
  };
  return page;
}

/** Ends the apps `restart` started (the harness ends only its own). */
export function stopStarted() {
  for (const pid of started.splice(0)) spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], { stdio: "ignore" });
}
process.on("exit", stopStarted);

/** A `skip()` for the PostgreSQL container. */
export const needsPostgres = () =>
  new Promise((resolveSkip) => {
    const socket = tcp({ host: "127.0.0.1", port: 55432, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolveSkip(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done("no PostgreSQL on port 55432 (docker compose -f docker/compose.yml up -d)"));
    socket.on("timeout", () => done("no PostgreSQL on port 55432"));
  });

/** Runs `sql` with psql in the PostgreSQL container: `{ ok, out }` with the output or the error. */
export function psqlTry(sql) {
  const r = spawnSync("docker", ["exec", "platypus-db-postgres-1", "psql", "-U", "platypus", "-d", "platypus", "-At", "-v", "ON_ERROR_STOP=1", "-c", sql], {
    encoding: "utf8",
  });
  return { ok: r.status === 0, out: (r.status === 0 ? r.stdout : r.stderr).trim() };
}

/** Runs `sql` with psql in the PostgreSQL container; returns its output. */
export function psql(sql) {
  const r = psqlTry(sql);
  if (!r.ok) throw new Error(`psql failed: ${r.out}`);
  return r.out;
}

/** A PostgreSQL connection of docker/compose.yml for connections.toml; the password is asked. */
export const pgConnection = (id, name, extra = "") => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "postgres"
host = "localhost"
port = 55432
database = "platypus"
user = "platypus"
ssl_mode = "disable"
show_all_databases = true
save_password = false
${extra}
`;

/** Expands the connection `name`, answering its password prompt with `password`. */
export async function connectWith(page, name, password = "platypus") {
  await page.waitFor(`${treeRow(name)} !== undefined`);
  await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type(password);
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') === null`, 30_000);
}

/** Expands the tree rows `labels` in order (each one below the one before) and waits for `last`. */
export async function openPath(page, labels, last, timeout = 30_000) {
  const after = (label, chain) => `(() => {
    const rows = [...document.querySelectorAll('.tree-row')];
    const text = (r) => r.querySelector('.tree-label')?.textContent;
    let at = -1;
    for (const w of ${JSON.stringify(chain)}) { at = rows.findIndex((r, i) => i > at && text(r) === w); if (at < 0) return undefined; }
    return rows.find((r, i) => i > at && text(r) === ${JSON.stringify(label)});
  })()`;
  for (let i = 0; i < labels.length; i++) {
    const r = after(labels[i], labels.slice(0, i));
    await page.waitFor(`${r} !== undefined`, timeout);
    await page.eval(`${r}.getAttribute('aria-expanded') !== 'true' && ${r}.querySelector('.twisty').click()`);
  }
  await page.waitFor(`${after(last, labels)} !== undefined`, timeout);
}

/** The Italian dictionary (src/i18n/it.ts), English text to Italian. */
export function italian() {
  const text = readFileSync(resolve(here, "../src/i18n/it.ts"), "utf8");
  const dict = {};
  const str = String.raw`"(?:[^"\\]|\\.)*"`;
  for (const m of text.matchAll(new RegExp(`(${str})\\s*:\\s*(${str})`, "g"))) dict[JSON.parse(m[1])] = JSON.parse(m[2]);
  return dict;
}

/** Every text the page shows now, with the title, aria-label and placeholder attributes, and
 *  the elements whose text is cut (wider than their box). Code, data and names are left out. */
export const pageTexts = `(() => {
  const skip = '.cm-content, .cm-gutters, .tree-label, .sql-preview, pre, code, .grid-scroll, .chart-svg text, .tab-title, kbd';
  const texts = new Set();
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    const n = walker.currentNode;
    const t = n.textContent.replace(/\\s+/g, ' ').trim();
    if (!t || !/[A-Za-z]{2}/.test(t)) continue;
    const el = n.parentElement;
    if (!el || el.closest(skip) || el.closest('[hidden]') || el.getClientRects().length === 0) continue;
    texts.add(t);
  }
  for (const el of document.querySelectorAll('[title],[aria-label],[placeholder]')) {
    if (el.getClientRects().length === 0) continue;
    for (const a of ['title', 'aria-label', 'placeholder']) {
      const v = el.getAttribute(a);
      if (v && /[A-Za-z]{2}/.test(v)) texts.add('@' + a + ': ' + v);
    }
  }
  const cut = [];
  for (const el of document.querySelectorAll('button, label, span, h1, h2, h3, th, td, li, a, p, div.tab, option, legend, dt, dd, strong')) {
    if (el.children.length > 2 || el.closest(skip)) continue;
    const s = getComputedStyle(el);
    if (el.getClientRects().length === 0) continue;
    if (el.scrollWidth > el.clientWidth + 1 && (s.overflow.includes('hidden') || s.textOverflow === 'ellipsis' || s.overflowX === 'hidden') && el.textContent.trim()) {
      cut.push(el.textContent.replace(/\\s+/g, ' ').trim().slice(0, 120) + ' [' + el.tagName.toLowerCase() + '.' + [...el.classList].join('.') + ' ' + el.clientWidth + '<' + el.scrollWidth + ']');
    }
  }
  return { texts: [...texts], cut };
})()`;

export const shotter =(page, shots, prefix) => async (name, p = page) => {
  if (shots) await p.screenshot(join(shots, `${prefix}-${name}.png`));
};
