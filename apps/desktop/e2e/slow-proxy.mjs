// A TCP proxy that behaves like a database far away over a poor link: every chunk is delayed
// by `latencyMs` in each direction and the bytes flow at no more than `bytesPerSecond`.
// `mode`: "slow" (as said), "blackhole" (connections are accepted, nothing is forwarded in
// either direction: the server never answers) or "closed" (new connections are reset).
// Used by the slow-remote scenario; the order of the bytes is kept.
import { connect, createServer } from "node:net";

/** Sends the chunks written to it to `socket`, each after `latency()` ms and after the
 *  bandwidth has paid for the ones before it. */
class Lane {
  constructor(socket, proxy) {
    this.socket = socket;
    this.proxy = proxy;
    this.free = 0; // when the link is free again (ms, performance.now())
  }

  /** Ends the socket once what was pushed has been written. */
  finish() {
    const wait = Math.max(0, this.free + this.proxy.latencyMs - performance.now()) + 20;
    setTimeout(() => this.socket.end(), wait);
  }

  push(chunk) {
    const { latencyMs, bytesPerSecond } = this.proxy;
    const now = performance.now();
    const sent = Math.max(now, this.free) + (chunk.length / bytesPerSecond) * 1000;
    this.free = sent;
    const wait = sent + latencyMs - now;
    this.proxy.bytes += chunk.length;
    setTimeout(() => {
      if (this.proxy.mode === "slow" && !this.socket.destroyed) this.socket.write(chunk);
    }, wait);
  }
}

/** Starts a proxy in front of `target` ({ host, port }) on a free local port. */
export async function startProxy(target, { latencyMs = 150, bytesPerSecond = 2_000_000, mode = "slow" } = {}) {
  const sockets = new Set();
  const proxy = { latencyMs, bytesPerSecond, mode, bytes: 0, connections: 0, port: 0 };
  const server = createServer((client) => {
    proxy.connections++;
    sockets.add(client);
    client.on("close", () => sockets.delete(client));
    client.on("error", () => {});
    if (proxy.mode === "closed") return client.destroy();
    if (proxy.mode === "blackhole") return client.resume();
    const upstream = connect(target);
    sockets.add(upstream);
    const up = new Lane(upstream, proxy);
    const down = new Lane(client, proxy);
    // A side that closes still has its last chunks in flight (a PostgreSQL cancel request is one
    // packet and a close): the other side ends once they have been written, not before.
    upstream.on("close", () => {
      sockets.delete(upstream);
      down.finish();
    });
    upstream.on("error", () => client.destroy());
    client.on("close", () => up.finish());
    client.on("data", (chunk) => (proxy.mode === "slow" ? up.push(chunk) : undefined));
    upstream.on("data", (chunk) => (proxy.mode === "slow" ? down.push(chunk) : undefined));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  proxy.port = server.address().port;
  /** Stops forwarding at once ("blackhole") or lets traffic flow again ("slow"). */
  proxy.setMode = (m) => {
    proxy.mode = m;
  };
  proxy.close = () => {
    server.close();
    for (const s of sockets) s.destroy();
  };
  return proxy;
}
