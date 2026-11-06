// What a plugin's frame may say to the app, read before anything acts on it; and the script the
// frame is given, on a fake window.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import vm from "node:vm";
import { changeFollower } from "../src/plugins/changeFollower.ts";
import { FRAME_REQUESTS, FrameGuard, MAX_IN_FLIGHT, MAX_PARAMS, hung, readFrameMessage, refusedBy } from "../src/plugins/frameProtocol.ts";

const here = dirname(fileURLToPath(import.meta.url));
const request = (over) => ({ platypus: 1, type: "request", id: 1, name: "notify", params: '{"text":"hi"}', ...over });

test("a request has known fields and one of the known names", () => {
  assert.deepEqual(readFrameMessage(request()), { kind: "request", id: 1, name: "notify", params: { text: "hi" } });
  assert.deepEqual(readFrameMessage({ platypus: 1, type: "pong", n: 4 }), { kind: "pong", n: 4 });
  const refused = readFrameMessage(request({ name: "run_sql" }));
  assert.equal(refused.kind, "refused");
  assert.match(refused.reason, /no such request: run_sql/);
  // A request of another version is no attack: it gets its answer and the page goes on.
  assert.equal(refused.strike, false);
  for (const secret of ["connections", "passwords", "query_results", "invoke", "__proto__", "constructor"]) {
    assert.equal(readFrameMessage(request({ name: secret })).kind, "refused", secret);
  }
  assert.ok(!FRAME_REQUESTS.some((name) => /sql|connection|password|invoke/.test(name) && name !== "open_query"));
});

test("anything else is not one of the app's messages", () => {
  for (const broken of [
    null,
    undefined,
    "text",
    42,
    [],
    {},
    { platypus: 2, type: "pong", n: 1 },
    { platypus: 1 },
    { platypus: 1, type: "unknown" },
    { platypus: 1, type: "pong", n: "1" },
    { platypus: 1, type: "pong", n: Number.NaN },
    { platypus: 1, type: "pong", n: Number.POSITIVE_INFINITY },
    { platypus: 1, type: "pong", n: 1.5 },
    { platypus: 1, type: "pong", n: 2 ** 53 },
    request({ id: Number.NaN }),
    { platypus: 1, type: "pong", n: 1, junk: "x".repeat(1000) },
    request({ id: "1" }),
    request({ id: 1.5 }),
    request({ name: 7 }),
    request({ params: { text: "an object is not bounded" } }),
    request({ junk: "x".repeat(1000) }),
  ]) {
    assert.equal(readFrameMessage(broken).kind, "broken", JSON.stringify(broken)?.slice(0, 60));
  }
});

test("parameters are bounded and JSON, and a bad one gets its answer", () => {
  const huge = readFrameMessage(request({ params: `"${"x".repeat(MAX_PARAMS)}"` }));
  assert.equal(huge.kind, "refused");
  assert.match(huge.reason, /larger than/);
  assert.equal(huge.strike, true);
  const garbled = readFrameMessage(request({ params: "{not json" }));
  assert.equal(garbled.kind, "refused");
  assert.equal(garbled.strike, true);
  assert.equal(readFrameMessage(request({ params: `"${"x".repeat(MAX_PARAMS - 2)}"` })).kind, "request");
});

test("a permission it was not given is refused before the backend, which asks again", () => {
  const none = { http: [], clipboard: false, hooks: false, tabs: true, grid: false };
  assert.match(refusedBy("clipboard", none), /not allowed/);
  assert.match(refusedBy("http", none), /not allowed/);
  for (const name of ["notify", "setting", "state_get", "state_set", "open_query", "run_action"]) {
    assert.equal(refusedBy(name, none), null, name);
  }
  const all = { ...none, http: ["a.example.com"], clipboard: true };
  assert.equal(refusedBy("clipboard", all), null);
  assert.equal(refusedBy("http", all), null);
});

test("a flood is dropped, then stops the frame; a steady page never notices", () => {
  const guard = new FrameGuard(0);
  const flood = Array.from({ length: 700 }, () => guard.admit(0));
  assert.equal(flood.filter((a) => a === "serve").length, 100);
  assert.ok(flood.includes("drop"));
  assert.equal(flood.at(-1), "stop");

  const steady = new FrameGuard(0);
  for (let i = 0; i < 5000; i += 1) assert.equal(steady.admit(i * 25), "serve");
});

test("a burst passes, and the rate comes back with time", () => {
  const guard = new FrameGuard(0);
  for (let i = 0; i < 100; i += 1) assert.equal(guard.admit(0), "serve");
  assert.equal(guard.admit(0), "drop");
  assert.equal(guard.admit(1000), "serve");
});

test("what the user sees comes at most once a second", () => {
  const guard = new FrameGuard(0);
  assert.equal(guard.noisy(0), true);
  assert.equal(guard.noisy(500), false);
  assert.equal(guard.noisy(1000), true);
});

test("three broken messages stop a frame", () => {
  const guard = new FrameGuard(0);
  assert.equal(guard.strike(), false);
  assert.equal(guard.strike(), false);
  assert.equal(guard.strike(), true);
});

test("a frame counts as hung once it leaves pings unanswered", () => {
  assert.equal(hung(3, 3), false);
  assert.equal(hung(3, 0), false);
  assert.equal(hung(4, 0), true);
  assert.ok(MAX_IN_FLIGHT > 0);
});

/** The script a frame is given, on a fake window whose parent is `app`. */
function bootstrap() {
  const source = readFileSync(join(here, "../../../crates/platypus-plugins/src/frame_bootstrap.js"), "utf8");
  const sent = [];
  const app = { postMessage: (message, target) => sent.push({ message, target }) };
  let onMessage;
  const window = { parent: app, addEventListener: (_, listener) => (onMessage = listener) };
  vm.runInNewContext(source, { window, Map, Set, Promise, JSON, String, Object, Error });
  return { window, app, sent, receive: (data, source = app) => onMessage({ data, source }) };
}

test("the page's script sends bounded text and takes answers from the app only", async () => {
  const { window, app, sent, receive } = bootstrap();
  const answer = window.platypus.request("setting", { key: "a" });
  const { message, target } = sent[0];
  assert.equal(target, "*");
  const data = JSON.parse(JSON.stringify(message));
  assert.deepEqual(data, { platypus: 1, type: "request", id: 1, name: "setting", params: '{"key":"a"}' });
  assert.equal(readFrameMessage(data).kind, "request");

  receive({ platypus: 1, type: "answer", id: 1, value: '"forged"' }, { not: "the app" });
  receive({ platypus: 1, type: "answer", id: 1, value: '"real"' }, app);
  assert.equal(await answer, "real");

  const failing = window.platypus.request("http", { url: "x" });
  receive({ platypus: 1, type: "answer", id: 2, error: "not allowed" });
  await assert.rejects(failing, /not allowed/);
});

test("the page's script answers pings and tells of events, late listeners too", () => {
  const { window, sent, receive } = bootstrap();
  receive({ platypus: 1, type: "ping", n: 9 });
  assert.deepEqual(JSON.parse(JSON.stringify(sent[0].message)), { platypus: 1, type: "pong", n: 9 });
  assert.equal(readFrameMessage(JSON.parse(JSON.stringify(sent[0].message))).kind, "pong");

  receive({ platypus: 1, type: "event", event: "init", data: '{"plugin":"p"}' });
  const heard = [];
  const stop = window.platypus.on((event, data) => heard.push([event, data.plugin ?? data.x]));
  receive({ platypus: 1, type: "event", event: "theme", data: '{"x":1}' });
  stop();
  receive({ platypus: 1, type: "event", event: "theme", data: '{"x":2}' });
  assert.deepEqual(heard, [
    ["init", "p"],
    ["theme", 1],
  ]);
});

test("the changes of plugins are followed from the first time it is asked for, and at no cost before", async () => {
  const heard = [];
  let reloads = 0;
  const follow = changeFollower(
    async (handler) => heard.push(handler),
    () => (reloads += 1),
  );
  assert.equal(heard.length, 0, "nothing listens until it is asked");
  follow();
  follow();
  assert.equal(heard.length, 1);
  heard[0]();
  assert.equal(reloads, 1);

  // A listener that could not be set up is tried again.
  let tries = 0;
  const warn = console.warn;
  console.warn = () => {};
  const flaky = changeFollower(async () => {
    tries += 1;
    throw new Error("no window");
  }, () => {});
  flaky();
  await new Promise((done) => setTimeout(done));
  flaky();
  await new Promise((done) => setTimeout(done));
  console.warn = warn;
  assert.equal(tries, 2);
});
