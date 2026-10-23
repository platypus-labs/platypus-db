// Runs the end-to-end scenarios against the release build of the desktop app. A scenario
// exports its steps as `default (page, ctx)`, and optionally `setup(ctx)` to seed the
// profile before launch and `skip()` returning a reason not to run.
//
//   npm run e2e                 every scenario in e2e/scenarios
//   npm run e2e -- grid         scenarios whose file name contains "grid"
//
// Build first with `npm run tauri build -- --no-bundle`. Each scenario gets a fresh
// profile (PLATYPUS_DATA_DIR and its own WebView2 folder), so the user's connections,
// history and settings are never touched. Windows only for now: the page is driven
// through the WebView2 DevTools protocol.

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { connect } from "./cdp.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../target/release/platypus-db.exe");
const port = Number(process.env.PLATYPUS_E2E_PORT ?? 9333);
const filter = process.argv[2] ?? "";

if (process.platform !== "win32") {
  console.log("e2e: skipped, only Windows (WebView2) is supported for now");
  process.exit(0);
}

const scenarios = readdirSync(join(here, "scenarios"))
  .filter((f) => f.endsWith(".mjs") && f.includes(filter))
  .sort();

let failed = 0;
let passed = 0;
for (const file of scenarios) {
  const scenario = await import(pathToFileURL(join(here, "scenarios", file)).href);
  const skipped = await scenario.skip?.();
  if (skipped) {
    console.log(`skip ${file}: ${skipped}`);
    continue;
  }
  const dir = mkdtempSync(join(tmpdir(), "platypus-e2e-"));
  const ctx = { dir, shots: process.env.PLATYPUS_E2E_SHOTS ?? null };
  await scenario.setup?.(ctx);
  const app = spawn(exe, [], {
    env: {
      ...process.env,
      PLATYPUS_DATA_DIR: dir,
      WEBVIEW2_USER_DATA_FOLDER: join(dir, "webview"),
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const output = [];
  app.stdout.on("data", (d) => output.push(d));
  app.stderr.on("data", (d) => output.push(d));
  let exit = null;
  app.on("exit", (code, signal) => (exit = { code, signal }));
  const started = performance.now();
  let page;
  try {
    try {
      page = await connect(port, 60_000);
    } catch (e) {
      // Say why the page never appeared: the app may have died or WebView2 never started.
      console.log(diagnose(exit, Buffer.concat(output).toString()));
      throw e;
    }
    await scenario.default(page, ctx);
    passed++;
    console.log(`ok   ${file} (${Math.round(performance.now() - started)} ms)`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${file}\n     ${e instanceof Error ? (e.stack ?? e.message) : e}`);
    if (page && ctx.shots) await page.screenshot(join(ctx.shots, `${file}.failure.png`)).catch(() => {});
  } finally {
    page?.close();
    // The WebView2 processes are children of the app: end the whole tree.
    spawnSync("taskkill", ["/pid", String(app.pid), "/t", "/f"], { stdio: "ignore" });
    await new Promise((r) => setTimeout(r, 300));
    rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}

/** Why the app has no DevTools page: its state, output and the WebView2 runtime. */
function diagnose(exit, output) {
  const run = (cmd, args) => spawnSync(cmd, args, { encoding: "utf8" }).stdout?.trim() ?? "";
  const webviews = run("tasklist", ["/fi", "imagename eq msedgewebview2.exe", "/fo", "csv", "/nh"])
    .split("\n")
    .filter((l) => l.includes("msedgewebview2")).length;
  const runtime =
    run("reg", [
      "query",
      "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
      "/v",
      "pv",
    ]).match(/pv\s+REG_SZ\s+(\S+)/)?.[1] ?? "not found";
  return [
    `     app: ${exit ? `exited (code ${exit.code}, signal ${exit.signal})` : "still running"}`,
    `     msedgewebview2 processes: ${webviews}, WebView2 runtime: ${runtime}`,
    `     app output: ${output.trim() || "(none)"}`,
  ].join("\n");
}

console.log(`${passed} passed, ${failed} failed, ${scenarios.length - passed - failed} skipped`);
process.exit(failed ? 1 : 0);
