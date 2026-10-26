// A minimal MCP client for scenarios: starts `platypus-mcp` (next to the app's executable)
// on a profile folder and calls its tools over JSON-RPC on stdio.
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.env.PLATYPUS_E2E_EXE ?? resolve(here, "../../../target/release/platypus-db.exe");
export const mcpExe = join(dirname(exe), "platypus-mcp.exe");

/** Why MCP scenarios cannot run, or null. */
export function noMcp() {
  return existsSync(mcpExe) ? null : `no platypus-mcp.exe next to ${exe}`;
}

/** Starts the server on the profile `dir`, initialized. */
export async function startMcp(dir, clientName = "qa-client") {
  const child = spawn(mcpExe, [], {
    env: { ...process.env, PLATYPUS_DATA_DIR: dir },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let buffer = "";
  const waiting = new Map();
  const stderr = [];
  child.stderr.on("data", (d) => stderr.push(d.toString()));
  child.stdout.on("data", (d) => {
    buffer += d.toString();
    let i;
    while ((i = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, i).trim();
      buffer = buffer.slice(i + 1);
      if (!line) continue;
      const msg = JSON.parse(line);
      if (msg.id != null && waiting.has(msg.id)) {
        waiting.get(msg.id)(msg);
        waiting.delete(msg.id);
      }
    }
  });
  let next = 1;
  const send = (msg) => child.stdin.write(`${JSON.stringify(msg)}\n`);
  const request = (method, params, timeoutMs = 60_000) =>
    new Promise((resolveMsg, reject) => {
      const id = next++;
      const timer = setTimeout(() => reject(new Error(`no answer to ${method}; stderr: ${stderr.join("")}`)), timeoutMs);
      waiting.set(id, (m) => {
        clearTimeout(timer);
        resolveMsg(m);
      });
      send({ jsonrpc: "2.0", id, method, params });
    });
  const init = await request("initialize", {
    protocolVersion: "2025-06-18",
    capabilities: {},
    clientInfo: { name: clientName, version: "1.0" },
  });
  send({ jsonrpc: "2.0", method: "notifications/initialized" });
  return {
    init,
    stderr,
    request,
    /** Calls a tool: `{ text, error }`. */
    async call(name, args = {}, timeoutMs) {
      const r = await request("tools/call", { name, arguments: args }, timeoutMs);
      if (r.error) return { text: JSON.stringify(r.error), error: true, protocol: true };
      return { text: r.result?.content?.[0]?.text ?? "", error: r.result?.isError === true };
    },
    close() {
      child.stdin.end();
      child.kill();
    },
  };
}
