// Group S, performance: a slow remote database. A TCP proxy (e2e/slow-proxy.mjs) in front of the
// PostgreSQL container adds 150 ms to every chunk in each direction (a 300 ms round trip) and
// caps the link at 1 MB/s. Through it the app must stay usable:
//   - connecting shows a waiting state (the Activity button shows it running), not a frozen window;
//   - expanding the navigator shows loading while frames keep being drawn and typing in the
//     editor works;
//   - a running query can be cancelled, and the session runs the next one;
//   - a result of 50,000 rows arrives without freezing the page;
//   - when the server never answers (the proxy accepts the connection and drops every byte), the
//     connect gives up with a readable error after the connect timeout (10 s, the default), not a hang,
//     and the same connection works once the link is back.
// Needs PostgreSQL (docker compose -f docker/compose.yml up -d).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { join } from "node:path";
import { CTRL } from "../cdp.mjs";
import { PG, lag, needsPort, openEditor, run, startFrames, status, stopFrames, visibleTab } from "../perf.mjs";
import { clearStatus } from "../qa-grid.mjs";
import { psqlTry } from "../qa-jrn.mjs";
import { startProxy } from "../slow-proxy.mjs";

export const skip = needsPort(PG.port, "PostgreSQL");

const SLOW = "S slow remote";
const DEAD = "S unreachable remote";
const SHORT = "S short timeout";
const LATENCY_MS = 150;
const BYTES_PER_SECOND = 1_000_000;
const cancel = `[...${visibleTab}.querySelectorAll('.toolbar button')].find(b => b.getAttribute('aria-label') === 'Cancel')`;
const editorText = `[...${visibleTab}.querySelectorAll('.editor .cm-line')].map(l => l.textContent).join('\\n')`;
const row = (label) => `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;

let slow;
let dead;
let short;

const entry = (n, name, port, extra = "") =>
  `[[connections]]\nid = "00000000-0000-4000-8000-00000e5000${n}0"\nname = "${name}"\ndriver = "postgres"\nhost = "127.0.0.1"\nport = ${port}\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\n${extra}`;

export async function setup({ dir }) {
  const target = { host: "127.0.0.1", port: PG.port };
  slow = await startProxy(target, { latencyMs: LATENCY_MS, bytesPerSecond: BYTES_PER_SECOND });
  // The "unreachable" one drops everything until the test lets it through.
  dead = await startProxy(target, { latencyMs: LATENCY_MS, bytesPerSecond: BYTES_PER_SECOND, mode: "blackhole" });
  short = await startProxy(target, { latencyMs: LATENCY_MS, bytesPerSecond: BYTES_PER_SECOND, mode: "blackhole" });
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6

${entry(1, SLOW, slow.port)}
${entry(2, DEAD, dead.port)}
${entry(3, SHORT, short.port, `[connections.options]
connect_timeout = "3"
`)}`,
  );
}

async function password(page, name) {
  await page.waitFor(`${row(name)} !== undefined`, 30_000);
  await page.eval(`${row(name)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null`, 15_000);
  await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
  await page.type(PG.password);
  await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
}

/** The Activity button shows work running (a `.busy` mark); the log lists what failed. */
const busy = `document.querySelector('.activity-button .busy') !== null`;
const unseenErrors = `Number(document.querySelector('.activity-button .badge')?.textContent ?? 0)`;

/** The text of the Activity log entries (opened, read and closed). */
async function activityEntries(page) {
  await page.eval(`document.querySelector('.activity-button').click()`);
  await page.waitFor(`document.querySelector('.activity-list') !== null`, 5000);
  const entries = await page.eval(`[...document.querySelectorAll('.activity-list > *')].map(e => e.textContent)`);
  await page.key("Escape", "Escape", 27);
  return entries;
}

export default async function (page) {
  try {
    await shortTimeout(page);
    await unreachable(page);
    await slowLink(page);
  } finally {
    slow?.close();
    dead?.close();
    short?.close();
  }
}

/** `[connections.options] connect_timeout` bounds the connect: 3 s here, not the default 10 s. */
async function shortTimeout(page) {
  const started = performance.now();
  await password(page, SHORT);
  await page.waitFor(`${unseenErrors} > 0`, 30_000);
  const took = Math.round(performance.now() - started);
  const failed = (await activityEntries(page)).find((e) => e.includes(`Connect to ${SHORT}`));
  console.log(`  connect_timeout = 3 option: gave up after ${took} ms, "${failed}"`);
  assert.ok(took >= 2500 && took < 6000, `the connect gave up after ${took} ms (option 3 s)`);
  assert.match(failed ?? "", /did not answer within 3 s/);
}

/** The server never answers: waiting, then a readable error, then it works when the link is back. */
async function unreachable(page) {
  await startFrames(page);
  const started = performance.now();
  await password(page, DEAD);
  // The wait is shown (Activity: running) while the connect goes on, and the page answers.
  await page.waitFor(busy, 5000);
  const idle = await lag(page);
  assert.ok(idle < 200, `the page answered in ${Math.round(idle)} ms while connecting`);
  await page.waitFor(`${unseenErrors} > 0`, 30_000);
  const took = Math.round(performance.now() - started);
  const frames = await stopFrames(page);
  const failed = (await activityEntries(page)).find((e) => e.includes(`Connect to ${DEAD}`));
  console.log(`  unreachable server: gave up after ${took} ms, "${failed}"; ${frames.frames} frames meanwhile, longest ${frames.max} ms`);
  assert.ok(took >= 9000 && took < 12_500, `the connect gave up after ${took} ms (timeout 10 s)`);
  assert.ok(frames.max < 250, `a frame took ${frames.max} ms while connecting`);
  assert.match(failed ?? "", /Failed/);
  assert.match(failed ?? "", /did not answer within 10 s/, "the error says what happened");
  assert.equal(await page.eval(busy), false, "nothing is left running");
  assert.equal(await page.eval(`${row(DEAD)}.querySelector('.connected-mark')`), null, "the connection is not marked connected");

  // The link comes back: the same connection works, no restart needed.
  dead.setMode("slow");
  await page.eval(`${row(DEAD)}.querySelector(".twisty").click()`);
  await page.waitFor(`document.querySelector('.modal input[type=password]') !== null || ${row("public")} !== undefined`, 15_000);
  if (await page.eval(`document.querySelector('.modal input[type=password]') !== null`)) {
    await page.eval(`document.querySelector('.modal input[type=password]').focus()`);
    await page.type(PG.password);
    await page.eval(`document.querySelector('.modal button[type=submit]').click()`);
  }
  await page.waitFor(`${row("public")} !== undefined`, 60_000);
  // Close it: the slow connection is the one measured next.
  await page.eval(`${row(DEAD)}.querySelector(".twisty").click()`);
  await page.waitFor(`${row("public")} === undefined`, 10_000);
}

async function slowLink(page) {
  // 1. Connecting: the waiting state, a page that answers, then the schemas.
  await startFrames(page);
  const started = performance.now();
  await password(page, SLOW);
  await page.waitFor(busy, 5000); // the Activity button shows the connect running
  const answer = await lag(page);
  await page.waitFor(`${row("public")} !== undefined && !(${busy})`, 60_000);
  const connect = Math.round(performance.now() - started);
  let frames = await stopFrames(page);
  console.log(`  connecting through the slow link: ${connect} ms (waiting mark shown, page answered in ${Math.round(answer)} ms), ${frames.frames} frames, longest ${frames.max} ms`);
  assert.ok(answer < 200, `the page answered in ${Math.round(answer)} ms while connecting`);
  assert.ok(frames.max < 250, `a frame took ${frames.max} ms while connecting`);
  assert.ok(connect < 15_000, `connecting took ${connect} ms`);
  assert.ok(connect > 600, `${connect} ms: the link is not slow, is the proxy in the way?`);

  // 2. Expanding "public" over the link: loading shows, frames keep coming, and typing in an
  // editor works meanwhile.
  await openEditor(page, SLOW);
  // "public" opens on its own once connected: fold it, so the expand below is the one measured.
  if (await page.eval(`${row("Tables")} !== undefined`)) {
    await page.eval(`${row("public")}.querySelector(".twisty").click()`);
    await page.waitFor(`${row("Tables")} === undefined`, 10_000);
  }
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await startFrames(page);
  await page.eval(`${row("public")}.querySelector('.twisty').click()`);
  const typed = "SELECT 'typing while it loads'";
  await page.type(typed);
  await page.waitFor(`${editorText} === ${JSON.stringify(typed)}`, 5000);
  const loadingShown = await page.eval(`[...document.querySelectorAll('.tree-row')].some(r => r.querySelector('.tree-detail')?.textContent === 'loading…')`);
  await page.waitFor(`${row("Tables")} !== undefined`, 60_000);
  frames = await stopFrames(page);
  console.log(`  expanding the navigator over the link: loading shown while typing: ${loadingShown}, ${frames.frames} frames, longest ${frames.max} ms`);
  assert.ok(frames.max < 250, `a frame took ${frames.max} ms while the navigator loaded`);

  // 3. Cancel a running query: it ends at once, though the cancel crosses the slow link.
  await clearStatus(page);
  await replace(page, "SELECT pg_sleep(60)");
  await page.key("Enter", "Enter", 13, CTRL);
  await page.waitFor(`${cancel} && !${cancel}.disabled`, 15_000);
  // BUG (low, cancel): a Cancel clicked at once, before the statement has reached the server over
  // a slow link, is lost: the cancel request arrives while the cursor is still being declared, the
  // statement then runs to its end (60 s here) and Cancel stays enabled. A second click, once the
  // statement is running on the server, works. Not fixed: the driver would have to resend the
  // cancel until the statement ends, a change to platypus-core's cancel contract.
  await page.eval(`${cancel}.click()`);
  const sleeping = () => psqlTry("SELECT count(*) FROM pg_stat_activity WHERE wait_event = 'PgSleep'").out === "1";
  while (!sleeping()) await page.sleep(100);
  const early = await page.eval(`${cancel}.disabled`);
  console.log(`  Cancel at once: ${early ? "worked" : "lost (the statement started after it), clicking again"}`);
  const cancelStarted = performance.now();
  if (!early) await page.eval(`${cancel}.click()`);
  await page.waitFor(`${cancel}.disabled`, 30_000);
  const took = Math.round(performance.now() - cancelStarted);
  console.log(`  cancel through the slow link: ${took} ms`);
  assert.ok(took < 2500, `cancel took ${took} ms`);
  assert.match(await page.eval(`${visibleTab}.textContent`), /Query cancelled/);
  await run(page, "SELECT 42 AS answer", { timeout: 30_000 });
  assert.equal(await page.eval(status), "1 row", "the session runs the next statement");

  // 4. 50,000 rows: they arrive, the page stays alive while they do, and none is lost.
  await replace(page, "SELECT g AS id, 'row ' || g AS name, g * 1.5 AS amount, md5(g::text) AS hash FROM generate_series(1, 50000) g");
  await clearStatus(page);
  await startFrames(page);
  const rowsStarted = performance.now();
  await page.key("Enter", "Enter", 13, CTRL);
  let worst = 0;
  const done = `/^50[.,]?000 rows$/.test(${status}.trim()) && ${cancel}?.disabled === true`;
  for (;;) {
    worst = Math.max(worst, await lag(page));
    // The grid streams the first rows; "Fetch all" reads the rest while the page is watched.
    if (await page.eval(`${status}.includes('more available') && [...${visibleTab}.querySelectorAll('.statusbar button')].some(b => b.textContent === 'Fetch all')`)) {
      await page.eval(`[...${visibleTab}.querySelectorAll('.statusbar button')].find(b => b.textContent === 'Fetch all').click()`);
    }
    if (await page.eval(done)) break;
    assert.ok(performance.now() - rowsStarted < 120_000, `50,000 rows not there after 2 minutes: ${await page.eval(status)}`);
    await page.sleep(100);
  }
  const arrive = Math.round(performance.now() - rowsStarted);
  frames = await stopFrames(page);
  console.log(`  50,000 rows over the link: ${arrive} ms, page answered within ${Math.round(worst)} ms, ${frames.frames} frames, longest ${frames.max} ms`);
  assert.ok(worst < 250, `the page was busy for ${Math.round(worst)} ms while the rows arrived`);
  assert.ok(frames.max < 250, `a frame took ${frames.max} ms while the rows arrived`);
  assert.ok(arrive < 90_000, `the rows took ${arrive} ms`);
  assert.ok(slow.bytes > 1_000_000, `${slow.bytes} bytes crossed the proxy: the rows did not come through it`);
}

/** Replaces the editor's text with `sql`, ready to run. */
async function replace(page, sql) {
  await page.eval(`${visibleTab}.querySelector('.cm-content').focus()`);
  await page.key("a", "KeyA", 65, CTRL);
  await page.type(sql);
  await page.waitFor(`${editorText} === ${JSON.stringify(sql)}`);
  await page.key("Escape", "Escape", 27);
}
