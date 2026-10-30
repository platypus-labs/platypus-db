// What the Redis scenarios (scenarios/redis-*.mjs) share: the Redis of docker/compose.yml
// (password `platypus`, port 46379), a small RESP client to seed and check keys behind the
// app's back, and the saved connections.
import { writeFileSync } from "node:fs";
import { connect as tcp } from "node:net";
import { join } from "node:path";

export const PORT = 46379;
const PASSWORD = "platypus";

/** A reason to skip when Redis is not running. */
export function skip() {
  return new Promise((resolve) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT, timeout: 1000 });
    const done = (reason) => {
      socket.destroy();
      resolve(reason);
    };
    socket.on("connect", () => done(null));
    socket.on("error", () => done(`no Redis on port ${PORT} (docker compose -f docker/compose.yml up -d redis)`));
    socket.on("timeout", () => done(`no Redis on port ${PORT}`));
  });
}

/** One RESP reply parsed from `buf` at `at`: `[value, next]`, or `null` when incomplete. */
function parse(buf, at) {
  const end = buf.indexOf("\r\n", at);
  if (end < 0) return null;
  const type = String.fromCharCode(buf[at]);
  const line = buf.toString("latin1", at + 1, end);
  const next = end + 2;
  switch (type) {
    case "+":
      return [line, next];
    case "-":
      return [new Error(line), next];
    case ":":
      return [Number(line), next];
    case "$": {
      const n = Number(line);
      if (n < 0) return [null, next];
      if (buf.length < next + n + 2) return null;
      return [buf.subarray(next, next + n), next + n + 2];
    }
    case "*": {
      const n = Number(line);
      if (n < 0) return [null, next];
      const items = [];
      let pos = next;
      for (let i = 0; i < n; i++) {
        const item = parse(buf, pos);
        if (!item) return null;
        items.push(item[0]);
        pos = item[1];
      }
      return [items, pos];
    }
    default:
      throw new Error(`unexpected RESP type ${type}`);
  }
}

/** Runs `commands` (arrays of strings or Buffers) on database `db`; the replies, bulk strings as
 *  Buffers. */
export function redis(db, commands) {
  const all = [["AUTH", PASSWORD], ["SELECT", String(db)], ...commands];
  const encode = (args) =>
    Buffer.concat([
      Buffer.from(`*${args.length}\r\n`),
      ...args.flatMap((a) => {
        const b = Buffer.isBuffer(a) ? a : Buffer.from(String(a));
        return [Buffer.from(`$${b.length}\r\n`), b, Buffer.from("\r\n")];
      }),
    ]);
  return new Promise((resolve, reject) => {
    const socket = tcp({ host: "127.0.0.1", port: PORT });
    let buf = Buffer.alloc(0);
    const replies = [];
    socket.on("connect", () => socket.write(Buffer.concat(all.map(encode))));
    socket.on("data", (chunk) => {
      buf = Buffer.concat([buf, chunk]);
      let pos = 0;
      for (;;) {
        const r = parse(buf, pos);
        if (!r) break;
        replies.push(r[0]);
        pos = r[1];
      }
      buf = buf.subarray(pos);
      if (replies.length === all.length) {
        socket.end();
        resolve(replies.slice(2));
      }
    });
    socket.on("error", reject);
  });
}

/** A Buffer reply as text. */
export const text = (b) => (Buffer.isBuffer(b) ? b.toString("utf8") : b);

/** A saved Redis connection to database `db` (password by command: no prompt). */
export function connection(n, name, db, extra = "") {
  return `[[connections]]
id = "00000000-0000-4000-8000-00000000c${String(n).padStart(3, "0")}"
name = "${name}"
driver = "redis"
host = "127.0.0.1"
port = ${PORT}
database = "${db}"
user = ""
ssl_mode = "disable"
save_password = false
password_command = "echo ${PASSWORD}"
${extra}`;
}

export function saveConnections(dir, ...entries) {
  writeFileSync(join(dir, "connections.toml"), `version = 7\n\n${entries.join("\n")}`);
}
