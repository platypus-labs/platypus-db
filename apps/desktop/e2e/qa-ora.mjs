// What the Oracle QA scenarios (qa-ora-*) share: a saved connection to the Oracle container of
// docker/compose.yml (profile `oracle`, password from a command so there is no prompt), the
// Instant Client copied into the scenario's profile so no download runs, and SQL run beside
// the app with SQL*Plus inside the container (to seed and check objects outside the app).
//
// Instant Client comes from PLATYPUS_E2E_ORACLE_DRIVERS (a folder holding
// `drivers/vendor/oracle/<version>`), else from target/conformance-drivers, which
// `PLATYPUS_TEST_ORACLE_CLIENT=download cargo test -p platypus-conformance` fills.
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export * from "./qa-grid.mjs";

const here = dirname(fileURLToPath(import.meta.url));
export const PORT = 51521;
export const CONTAINER = "platypus-db-oracle-1";
export const CONN = "QA Oracle";

/** The folder holding `drivers/vendor/oracle`, or null. */
export function clientRoot() {
  const candidates = [
    process.env.PLATYPUS_E2E_ORACLE_DRIVERS,
    resolve(here, "../../../target/conformance-drivers"),
  ].filter(Boolean);
  return candidates.find((c) => existsSync(join(c, "drivers", "vendor", "oracle"))) ?? null;
}

/** A `skip()` for scenarios that need Oracle and, unless `download`, an Instant Client. */
export function needsOracle({ download = false } = {}) {
  return async () => {
    const up = await new Promise((done) => {
      const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
      const end = (ok) => {
        socket.destroy();
        done(ok);
      };
      socket.on("connect", () => end(true));
      socket.on("error", () => end(false));
      socket.on("timeout", () => end(false));
    });
    if (!up) return `no Oracle on port ${PORT} (docker compose -f docker/compose.yml --profile oracle up -d)`;
    if (!download && !clientRoot()) return "no Instant Client (set PLATYPUS_E2E_ORACLE_DRIVERS)";
    return null;
  };
}

/** Copies Instant Client into the profile `dir`. */
export function installClient(dir) {
  cpSync(join(clientRoot(), "drivers", "vendor", "oracle"), join(dir, "drivers", "vendor", "oracle"), { recursive: true });
}

/** A `[[connections]]` table for the Oracle container; `fields` overrides or adds fields. */
export function oracleToml(id, name = CONN, fields = {}) {
  const all = {
    driver: "oracle",
    host: "127.0.0.1",
    port: PORT,
    database: "FREEPDB1",
    user: "platypus",
    ssl_mode: "disable",
    save_password: false,
    password_command: "echo platypus",
    ...fields,
  };
  const body = Object.entries(all)
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `${k} = ${typeof v === "string" ? JSON.stringify(v) : v}`)
    .join("\n");
  return `[[connections]]\nid = "${id}"\nname = ${JSON.stringify(name)}\n${body}\n`;
}

/** Seeds the profile: Instant Client and the connections (`extra`: more TOML tables). */
export function oracleSetup(dir, { client = true, extra = "", fields = {} } = {}) {
  if (client) installClient(dir);
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 6\n\n${oracleToml("00000000-0000-4000-8000-0000000a0001", CONN, fields)}\n${extra}`,
  );
}

/** Runs `sql` (a SQL*Plus script) as `platypus` in the container; returns its output.
 *  Stops at the first error and throws it. */
export function ora(sql, { user = "platypus/platypus@FREEPDB1", check = true } = {}) {
  const script = `set define off\nset pagesize 0 feedback off heading off linesize 32767 trimout on trimspool on long 100000 serveroutput on\nwhenever sqlerror exit failure\n${sql}\nexit\n`;
  const r = spawnSync("docker", ["exec", "-i", CONTAINER, "sqlplus", "-s", "-l", user], { input: script, encoding: "utf8" });
  const out = (r.stdout ?? "").replace(/\r/g, "").trim();
  if (check && r.status !== 0) throw new Error(`oracle: ${out || r.stderr}`);
  return out;
}

/** Drops the objects named `names` (`TABLE x`, `VIEW y`…), ignoring the ones missing. */
export function dropAll(...objects) {
  const body = objects
    .map((o) => `BEGIN EXECUTE IMMEDIATE 'DROP ${o.replaceAll("'", "''")}'; EXCEPTION WHEN OTHERS THEN NULL; END;\n/`)
    .join("\n");
  ora(body, { check: false });
}

/** Connects the Oracle connection and waits for the schema `child` (default PLATYPUS). */
export async function connectOracle(page, child = "PLATYPUS", name = CONN) {
  const { treeRow } = await import("./qa-grid.mjs");
  await page.waitFor(`${treeRow(name)} !== undefined`);
  if (await page.eval(`${treeRow(name)}.getAttribute("aria-expanded") !== "true"`)) {
    await page.eval(`${treeRow(name)}.querySelector(".twisty").click()`);
  }
  await page.waitFor(`${treeRow(child)} !== undefined`, 60_000);
}

/** Connects and opens a SQL editor on the Oracle connection. */
export async function oracleEditor(page, name = CONN) {
  const { treeRow, visibleTab } = await import("./qa-grid.mjs");
  await connectOracle(page, "PLATYPUS", name);
  await page.eval(`document.querySelectorAll('#qa-conn').forEach(e => e.removeAttribute('id')); ${treeRow(name)}.id = 'qa-conn'`);
  await page.clickSel("#qa-conn", { clickCount: 2 });
  await page.waitFor(`${visibleTab}?.querySelector('.cm-content') != null`, 30_000);
  // Copies are read from window.__copied (see qa-grid.mjs `copied`).
  await page.eval(`navigator.clipboard.writeText = async (text) => { window.__copied = text; }`);
}

/** Runs `sql` (`script`: the whole editor, Alt+X) and returns the status and the error
 *  shown, if any. */
export async function runOra(page, sql, script = false) {
  const { run } = await import("./qa-grid.mjs");
  const status = await run(page, sql, script);
  return { status, error: await page.eval(resultError) };
}

/** The text of the shown result error, or null. */
export const resultError = `([...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')?.querySelector('.result-error')?.textContent ?? null)`;
