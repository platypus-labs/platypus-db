// Builds platypus-mcp, the MCP server for agents, and puts it where Tauri takes its
// external binaries (`src-tauri/binaries/platypus-mcp-<target triple>`), so the bundle
// ships it next to the app. `tauri build --config src-tauri/tauri.mcp.conf.json` runs this
// before building the app; Tauri gives the target in TAURI_ENV_TARGET_TRIPLE.
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, "../../..");

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: ["ignore", "pipe", "inherit"], encoding: "utf8" });
  if (result.status !== 0) {
    console.error(`stage-mcp: ${command} ${args.join(" ")} failed`);
    process.exit(result.status ?? 1);
  }
  return result.stdout;
}

const host = /^host: (.+)$/m.exec(run("rustc", ["-vV"]))?.[1]?.trim();
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
// Built for another target only when Tauri builds for one: the host's build is shared
// with the rest of the workspace.
const cross = triple !== host;
run("cargo", ["build", "--release", "-p", "platypus-mcp", ...(cross ? ["--target", triple] : [])]);

const target = process.env.CARGO_TARGET_DIR ? resolve(process.env.CARGO_TARGET_DIR) : join(root, "target");
const exe = triple.includes("windows") ? ".exe" : "";
const built = join(target, ...(cross ? [triple] : []), "release", `platypus-mcp${exe}`);
const staged = join(here, "..", "src-tauri", "binaries", `platypus-mcp-${triple}${exe}`);
mkdirSync(dirname(staged), { recursive: true });
copyFileSync(built, staged);
console.log(`stage-mcp: ${staged}`);
