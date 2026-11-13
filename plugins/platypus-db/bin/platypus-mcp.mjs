#!/usr/bin/env node
// Starts the PlatypusDB app as the plugin's MCP server (`platypus-db --mcp`), passing stdin,
// stdout and stderr through. The app is looked for in PLATYPUS_DB (a path or a command), then
// where its installers put it on Windows and macOS, then on PATH (the Linux packages).
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** Where the installers put the app on this system, most likely first. */
function installed() {
  if (process.platform === "win32") {
    const local = process.env.LOCALAPPDATA ?? join(homedir(), "AppData", "Local");
    const programs = process.env.ProgramFiles ?? "C:\Program Files";
    return [join(local, "PlatypusDB", "platypus-db.exe"), join(programs, "PlatypusDB", "platypus-db.exe")];
  }
  if (process.platform === "darwin") {
    const app = join("PlatypusDB.app", "Contents", "MacOS", "platypus-db");
    return [join("/Applications", app), join(homedir(), "Applications", app)];
  }
  return [];
}

const app = process.env.PLATYPUS_DB || installed().find((path) => existsSync(path)) || "platypus-db";
const child = spawn(app, ["--mcp", ...process.argv.slice(2)], { stdio: "inherit" });

child.on("error", (error) => {
  process.stderr.write(
    `PlatypusDB was not found (${app}: ${error.message}). Install the app, or set PLATYPUS_DB to its executable.\n`,
  );
  process.exit(1);
});
child.on("exit", (code, signal) => process.exit(signal ? 1 : (code ?? 0)));
for (const signal of ["SIGINT", "SIGTERM"]) process.on(signal, () => child.kill(signal));
