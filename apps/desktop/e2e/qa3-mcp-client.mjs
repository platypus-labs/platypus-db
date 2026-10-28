// A raw MCP client for the qa3-mcp scenarios: starts the MCP server the Agents page
// registers (platypus-mcp next to the app) on a profile folder and keeps every line it
// writes, so scenarios can check the protocol itself: malformed input, notifications,
// concurrent requests, stdout cleanliness, the exit code when stdin closes.
import { spawn } from "node:child_process";
import { mcpExe, noMcp } from "./mcp.mjs";

export { mcpExe, noMcp };

/** Starts the server on the profile `dir` (not initialized). */
export function startRaw(dir, { exe = mcpExe, args = [], env = {} } = {}) {
  const child = spawn(exe, args, {
    env: { ...process.env, ...env, PLATYPUS_DATA_DIR: dir },
    stdio: ["pipe", "pipe", "pipe"],
  });
  const lines = [];
  const notJson = [];
  const stderr = [];
  const listeners = new Set();
  let buffer = "";
  let exit = null;
  const exited = new Promise((resolve) => {
    child.on("exit", (code, signal) => {
      exit = { code, signal };
      resolve(exit);
    });
  });
  child.on("error", (e) => stderr.push(`spawn error: ${e.message}`));
  child.stderr.on("data", (d) => stderr.push(d.toString()));
  child.stdout.on("data", (d) => {
    buffer += d.toString();
    let i;
    while ((i = buffer.indexOf("\n")) >= 0) {
      const line = buffer.slice(0, i).replace(/\r$/, "");
      buffer = buffer.slice(i + 1);
      if (!line.trim()) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        notJson.push(line);
        continue;
      }
      lines.push(msg);
      for (const l of listeners) l(msg);
    }
  });
  child.stdin.on("error", () => {});

  let next = 1;
  const writeLine = (text) => child.stdin.write(`${text}\n`);
  const send = (msg) => writeLine(JSON.stringify(msg));

  /** The next message matching `test`, already received or to come. */
  const waitFor = (test, timeoutMs = 60_000, what = "a message") =>
    new Promise((resolve, reject) => {
      const found = lines.find(test);
      if (found) return resolve(found);
      const timer = setTimeout(() => {
        listeners.delete(listener);
        reject(new Error(`no ${what} within ${timeoutMs} ms; stderr: ${stderr.join("").slice(-2000)}`));
      }, timeoutMs);
      const listener = (msg) => {
        if (!test(msg)) return;
        clearTimeout(timer);
        listeners.delete(listener);
        resolve(msg);
      };
      listeners.add(listener);
    });

  const client = {
    child,
    lines,
    notJson,
    stderr,
    get exit() {
      return exit;
    },
    exited,
    writeLine,
    send,
    waitFor,
    /** Sends a request; returns its id. */
    post(method, params, id = next++) {
      send(params === undefined ? { jsonrpc: "2.0", id, method } : { jsonrpc: "2.0", id, method, params });
      return id;
    },
    /** The response to the request `id`. */
    response(id, timeoutMs = 60_000) {
      return waitFor((m) => m.id === id && ("result" in m || "error" in m), timeoutMs, `answer to id ${JSON.stringify(id)}`);
    },
    async request(method, params, timeoutMs = 60_000) {
      return client.response(client.post(method, params), timeoutMs);
    },
    async initialize(name = "qa3-client") {
      const init = await client.request("initialize", {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name, version: "1.0" },
      });
      send({ jsonrpc: "2.0", method: "notifications/initialized" });
      return init;
    },
    /** Calls a tool: `{ text, error, protocol, raw }`. */
    async call(name, args = {}, timeoutMs) {
      const params = args === undefined ? { name } : { name, arguments: args };
      const r = await client.request("tools/call", params, timeoutMs);
      if (r.error) return { text: JSON.stringify(r.error), error: true, protocol: true, raw: r };
      return {
        text: r.result?.content?.map((c) => c.text ?? "").join("\n") ?? "",
        error: r.result?.isError === true,
        protocol: false,
        raw: r,
      };
    },
    /** Closes stdin and waits for the process to end (killed after `timeoutMs`). */
    async closeStdin(timeoutMs = 10_000) {
      child.stdin.end();
      const timer = new Promise((r) => setTimeout(() => r(null), timeoutMs));
      const result = await Promise.race([exited, timer]);
      if (result === null) child.kill();
      return result;
    },
    kill() {
      if (exit === null) child.kill();
    },
  };
  return client;
}

/** Starts and initializes the server. */
export async function startClient(dir, name = "qa3-client", options = {}) {
  const client = startRaw(dir, options);
  client.init = await client.initialize(name);
  return client;
}

/** A `[[connections]]` table for a SQLite file. */
export const sqliteToml = (id, name, file, extra = "") => `
[[connections]]
id = "${id}"
name = "${name}"
driver = "sqlite"
database = '${file}'
save_password = false
${extra}
`;
