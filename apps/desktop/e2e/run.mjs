// Runs the end-to-end scenarios against the release build of the desktop app. A scenario
// exports its steps as `default (page, ctx)`, and optionally `setup(ctx)` to seed the
// profile before launch, `env(ctx)` with environment variables for the app, `skip()`
// returning a reason not to run and `ready`, the condition to wait for before the steps
// (by default the navigator showing a connection).
//
//   npm run e2e                 every scenario in e2e/scenarios
//   npm run e2e -- grid         scenarios whose file name contains "grid"
//   npm run e2e -- --skip qa-perf,oracle
//                               every scenario but those whose file name contains one of
//                               these words (also PLATYPUS_E2E_SKIP=qa-perf,oracle)
//
// Build first with `npm run tauri build -- --no-bundle`. Each scenario gets a fresh
// profile (PLATYPUS_DATA_DIR and its own WebView2 folder), so the user's connections,
// history and settings are never touched. Windows only for now: the page is driven
// through the WebView2 DevTools protocol.

import { spawn as spawnApp } from "node:child_process";
import { spawnSync } from "./proc.mjs";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, linkSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createServer } from "node:net";
import { connect } from "./cdp.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../target/release/platypus-db.exe");
const port = Number(process.env.PLATYPUS_E2E_PORT ?? 9333);
const args = process.argv.slice(2);
const skipAt = args.indexOf("--skip");
/** Words that leave out the scenarios whose file name contains one of them. */
const skips = [process.env.PLATYPUS_E2E_SKIP ?? "", skipAt < 0 ? "" : (args[skipAt + 1] ?? "")]
  .flatMap((list) => list.split(","))
  .map((word) => word.trim())
  .filter((word) => word !== "");
const filter = args.find((a, i) => !a.startsWith("--") && (skipAt < 0 || i !== skipAt + 1)) ?? "";

if (process.platform !== "win32") {
  console.log("e2e: skipped, only Windows (WebView2) is supported for now");
  process.exit(0);
}

const scenarios = readdirSync(join(here, "scenarios"))
  .filter((f) => f.endsWith(".mjs") && f.includes(filter))
  .filter((f) => {
    const word = skips.find((w) => f.includes(w));
    if (word) console.log(`skip ${f}: --skip ${word}`);
    return !word;
  })
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
  const root = mkdtempSync(join(tmpdir(), "platypus-e2e-"));
  // A portable scenario runs a copy of the executable with a `data` folder next to it, which
  // holds the profile (no PLATYPUS_DATA_DIR): `dir` is that folder.
  const portable = scenario.portable === true;
  const dir = portable ? join(root, "app", "data") : root;
  const appExe = portable ? join(root, "app", "platypus-db.exe") : exe;
  if (portable) {
    mkdirSync(dir, { recursive: true });
    copyFileSync(exe, appExe);
  }
  // `port` lets a scenario reach the app's other windows.
  const ctx = { dir, root, port, exe: appExe, sourceExe: exe, shots: process.env.PLATYPUS_E2E_SHOTS ?? null };
  if (ctx.shots) mkdirSync(ctx.shots, { recursive: true });
  await scenario.setup?.(ctx);
  englishInterface(dir);
  quietAnimations(dir, file);
  await agentDefaults(dir);
  if (scenario.duckdb !== false) installDuckDb(dir);
  const env = {
    ...process.env,
    // Extra variables of the scenario, e.g. where other tools keep their files.
    ...(scenario.env?.(ctx) ?? {}),
    WEBVIEW2_USER_DATA_FOLDER: join(root, "webview"),
    PLATYPUS_DEVTOOLS_PORT: String(port),
  };
  if (portable) delete env.PLATYPUS_DATA_DIR;
  else env.PLATYPUS_DATA_DIR = dir;
  ctx.env = env;
  const app = spawnApp(appExe, [], { env, stdio: ["ignore", "pipe", "pipe"] });
  ctx.app = app;
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
    // A cold start (first launch of WebView2 on a CI runner) can take over 10 s to render
    // the saved connections; scenarios start once the navigator shows them.
    await page.waitFor(scenario.ready ?? `document.querySelector('.tree-row') !== null`, 60_000);
    await scenario.default(page, ctx);
    passed++;
    console.log(`ok   ${file} (${Math.round(performance.now() - started)} ms)`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${file}\n     ${e instanceof Error ? (e.stack ?? e.message) : e}`);
    // What the window showed, for failures that only happen on the CI runner.
    const state = await page
      ?.eval(`(() => {
        const tab = [...document.querySelectorAll('.query-tab, .object-tab')].find(t => t.style.display !== 'none');
        return JSON.stringify({
          tabs: [...document.querySelectorAll('.tab')].map(t => t.textContent),
          status: tab?.querySelector('.statusbar')?.textContent ?? null,
          editBar: tab?.querySelector('.edit-bar')?.textContent ?? null,
          notices: [...document.querySelectorAll('.notice, .error, .modal')].map(n => n.textContent.slice(0, 200)),
        });
      })()`)
      .catch((err) => `unavailable: ${err}`);
    if (state) console.log(`     page: ${state}`);
    if (page && ctx.shots) await page.screenshot(join(ctx.shots, `${file}.failure.png`)).catch(() => {});
    // The Activity log says which connection or task failed; it opens last, over the page.
    const activity = await page
      ?.eval(`(async () => {
        document.querySelector('.activity-button')?.click();
        await new Promise((r) => setTimeout(r, 300));
        return JSON.stringify([...document.querySelectorAll('.activity-list > *')].map((e) => e.textContent.slice(0, 200)));
      })()`)
      .catch((err) => `unavailable: ${err}`);
    if (activity) console.log(`     activity: ${activity}`);
  } finally {
    page?.close();
    // The WebView2 processes are children of the app: end the whole tree.
    spawnSync("taskkill", ["/pid", String(app.pid), "/t", "/f"], { stdio: "ignore" });
    // WebView2 processes the tree no longer holds would keep the DevTools port and the
    // profile: they are found by their profile folder.
    spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | ` +
          `Where-Object { $_.CommandLine -like '*${root.replaceAll("'", "''")}*' } | ` +
          `ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }`,
      ],
      { stdio: "ignore" },
    );
    await new Promise((r) => setTimeout(r, 300));
    forgetKeychainEntries(dir);
    // WebView2 can hold its profile files a few seconds after the tree is killed: a folder
    // left behind in the temp directory is no reason to stop the run.
    try {
      rmSync(root, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
    } catch (e) {
      console.log(`     (left ${root}: ${e instanceof Error ? e.message : e})`);
    }
  }
}

/** Removes from the Windows Credential Manager what the scenario's profile in `dir` left there:
 *  the passwords of its connections (and the other secrets under the ids its connections.toml
 *  names) and the seal of its agent policy (`platypus_mcp::policy::Seal::of`), which every
 *  profile has its own of. Hundreds of throwaway profiles fill the Credential Manager until
 *  nothing more can be saved there ("Windows error code 8"). */
function forgetKeychainEntries(dir) {
  const targets = [];
  const connections = join(dir, "connections.toml");
  if (existsSync(connections)) {
    const ids = readFileSync(connections, "utf8").match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) ?? [];
    for (const id of new Set(ids.map((i) => i.toLowerCase()))) targets.push(`${id}.platypusdb`);
  }
  // The seal's name: the SHA-256 of the profile folder as Rust's `canonicalize` gives it
  // (`\\?\C:\...`), lowercased, its first 16 bytes as a UUID.
  try {
    const path = realpathSync.native(dir);
    const verbatim = path.startsWith("\\\\") ? `\\\\?\\UNC\\${path.slice(2)}` : `\\\\?\\${path}`;
    const hex = createHash("sha256").update(verbatim.toLowerCase()).digest("hex");
    const id = [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20, 32)].join("-");
    targets.push(`${id}.platypusdb-agent-policy`);
  } catch {
    // No folder, no seal.
  }
  for (const target of targets) spawnSync("cmdkey", [`/delete:${target}`], { stdio: "ignore" });
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

/** DuckDB as the app would have downloaded it, from the copy the Rust tests download into
 *  target/test-drivers (or $CARGO_TARGET_DIR/test-drivers) (`cargo test -p platypus-driver-duckdb`);
 *  hard links, as each profile gets one. A scenario that sets `export const duckdb = false`
 *  starts without it. */
function installDuckDb(dir) {
  const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : resolve(here, "../../../target");
  const from = join(target, "test-drivers", "drivers", "vendor", "duckdb");
  if (!existsSync(from)) {
    console.log("e2e: no DuckDB in target/test-drivers; run `cargo test -p platypus-driver-duckdb` first");
    return;
  }
  const copy = (src, dst) => {
    mkdirSync(dst, { recursive: true });
    for (const entry of readdirSync(src, { withFileTypes: true })) {
      const [a, b] = [join(src, entry.name), join(dst, entry.name)];
      if (entry.isDirectory()) copy(a, b);
      else
        try {
          linkSync(a, b);
        } catch {
          copyFileSync(a, b);
        }
    }
  };
  copy(from, join(dir, "drivers", "vendor", "duckdb"));
}

/** A port free on this machine now. */
function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port: free } = server.address();
      server.close(() => resolvePort(free));
    });
  });
}

/** The agents' bridge on a port of its own, away from an app the user has open, and agents'
 *  changes run without asking, as before the bridge, unless the scenario sets the keys. */
async function agentDefaults(dir) {
  const file = join(dir, "settings.toml");
  let text = existsSync(file) ? readFileSync(file, "utf8") : "";
  const lines = [];
  if (!/^bridge_port\s*=/m.test(text)) lines.push(`bridge_port = ${await freePort()}`);
  if (!/^confirm_writes\s*=/m.test(text)) lines.push("confirm_writes = false");
  if (lines.length === 0) return;
  text = text.includes("[agents]") ? text.replace("[agents]", `[agents]\n${lines.join("\n")}`) : `${text}\n[agents]\n${lines.join("\n")}\n`;
  writeFileSync(file, text);
}

/** Adds `key = value` to the `[appearance]` table of the scenario's settings.toml, unless the
 *  scenario sets that key itself. */
function appearanceDefault(dir, key, value) {
  const file = join(dir, "settings.toml");
  const text = existsSync(file) ? readFileSync(file, "utf8") : "";
  if (new RegExp(`^${key}\\s*=`, "m").test(text)) return;
  const line = `${key} = ${value}`;
  writeFileSync(file, text.includes("[appearance]") ? text.replace("[appearance]", `[appearance]\n${line}`) : `[appearance]\n${line}\n\n${text}`);
}

/** Scenarios read the English interface whatever the language of the machine, unless one
 *  sets the language itself. */
function englishInterface(dir) {
  appearanceDefault(dir, "language", '"en"');
}

/** Animations are off, so a scenario never waits for a transition, except the one that tests
 *  motion (or any that sets the key itself). */
function quietAnimations(dir, file) {
  if (file !== "customize-motion-sqlite.mjs") appearanceDefault(dir, "animations", '"off"');
}
