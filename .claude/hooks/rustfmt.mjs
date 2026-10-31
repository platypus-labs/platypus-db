// PostToolUse hook: formats the Rust file Claude just edited, so `cargo fmt --check` stays green.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// the hook shell does not always have ~/.cargo/bin on PATH
const local = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "bin", "rustfmt");
const rustfmt = existsSync(`${local}.exe`) || existsSync(local) ? local : "rustfmt";

let input = "";
for await (const chunk of process.stdin) input += chunk;
const file = JSON.parse(input).tool_input?.file_path ?? "";
if (file.endsWith(".rs")) {
  // rustfmt finds rustfmt.toml from the file's directory upwards
  const result = spawnSync(rustfmt, ["--edition", "2024", file], { encoding: "utf8" });
  // a syntax error mid-edit is not worth blocking on: report it, never fail the edit
  if (result.status !== 0) console.error(`rustfmt: ${result.stderr || result.error?.message}`);
}
