// A proxy in front of the database (Connection › Network › Proxy): the test goes through a
// SOCKS5 proxy, which is asked for the database's address and says why it cannot reach it;
// an HTTP proxy that wants a login says so; a URL that is not a proxy's is refused before
// anything runs. Saved with the connection, it marks the Network tab. The proxies are fakes
// started by the scenario, in front of a closed port.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { button, dialog, editConnection, fill, saveForm, testConnection } from "../conn-helpers.mjs";

const NAME = "E2E proxy";
const proxy = `document.querySelector('.connection-dialog [data-field=proxy]')`;
const networkTab = `[...document.querySelectorAll('.dialog-tabs button')].find(b => b.textContent.startsWith('Network'))`;

export function setup({ dir }) {
  const toml = `version = 7

[[connections]]
id = "00000000-0000-4000-8000-0000000f0401"
name = "${NAME}"
driver = "postgres"
host = "db.internal"
port = 5432
database = "x"
user = "x"
ssl_mode = "disable"
`;
  writeFileSync(join(dir, "connections.toml"), toml);
}

/** Starts a server on a free port that answers each connection with `answer`. */
function listen(answer) {
  return new Promise((resolve) => {
    const server = createServer(answer);
    server.listen(0, "127.0.0.1", () => resolve(server));
  });
}

/** A SOCKS5 proxy that records the address it is asked for and answers "refused". */
function socks5(asked) {
  return listen((socket) => {
    let step = 0;
    socket.on("data", (data) => {
      if (step++ === 0) return socket.write(Buffer.from([5, 0]));
      // CONNECT with a name: 5 1 0 3 len name port.
      const name = data.subarray(5, 5 + data[4]).toString();
      asked.push(`${name}:${data.readUInt16BE(5 + data[4])}`);
      socket.end(Buffer.from([5, 5, 0, 1, 0, 0, 0, 0, 0, 0]));
    });
    socket.on("error", () => {});
  });
}

/** An HTTP proxy that wants a login. */
function httpNeedingLogin() {
  return listen((socket) => {
    socket.once("data", () => socket.end("HTTP/1.1 407 Proxy Authentication Required\r\n\r\n"));
    socket.on("error", () => {});
  });
}

export default async function (page, { dir }) {
  const asked = [];
  const socks = await socks5(asked);
  const http = await httpNeedingLogin();
  try {
    await editConnection(page, NAME);
    await page.eval(`${networkTab}.click()`);
    await page.waitFor(`${proxy} !== null`);

    // Not a proxy's URL: refused before anything is tried.
    await page.eval(fill(proxy, "proxy.lan:1080"));
    assert.match(await testConnection(page), /socks5:\/\/host:port or http:\/\/host:port/);

    // SOCKS5: asked for the database by its name, and says why it cannot reach it.
    await page.eval(fill(proxy, `socks5://127.0.0.1:${socks.address().port}`));
    const refused = await testConnection(page);
    assert.match(refused, /SOCKS5 proxy 127\.0\.0\.1:\d+: cannot reach db\.internal:5432: connection refused/, refused);
    assert.deepEqual(asked, ["db.internal:5432"]);

    // HTTP, without the login the proxy wants.
    await page.eval(fill(proxy, `http://127.0.0.1:${http.address().port}`));
    assert.match(await testConnection(page), /HTTP proxy .*asks for a user and password/);

    // An unreachable proxy names itself.
    await page.eval(fill(proxy, "socks5://127.0.0.1:1"));
    assert.match(await testConnection(page), /SOCKS5 proxy 127\.0\.0\.1:1/);

    // Saved with the connection: the Network tab is marked, the URL comes back.
    const url = `socks5://127.0.0.1:${socks.address().port}`;
    await page.eval(fill(proxy, url));
    await saveForm(page);
    assert.ok(readFileSync(join(dir, "connections.toml"), "utf8").includes(url), "saved");
    await editConnection(page, NAME);
    assert.ok((await page.eval(`${networkTab}.textContent`)).includes("●"), "the Network tab is marked");
    await page.eval(`${networkTab}.click()`);
    await page.waitFor(`${proxy}?.value === ${JSON.stringify(url)}`);
    await page.eval(`${button(dialog, "Cancel")}.click()`);
    await page.waitFor(`${dialog} === null`);
  } finally {
    socks.close();
    http.close();
  }
}
