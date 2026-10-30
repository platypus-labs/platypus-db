// A fake driver registry for the driver-version scenarios: a local HTTP server that serves an
// `index.yaml` with several versions of the Trino driver and their `.tar.gz` packages, as
// `dbc`'s registry does, and helpers that put an installed driver and its connections in a
// profile. The libraries are not real: opening a connection with one fails, which is what the
// compatibility check reports.
import { createHash } from "node:crypto";
import { mkdirSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

/** The registry's name for this system, as the app computes it (`platypus_drivers::platform`). */
export const PLATFORM = process.arch === "arm64" ? "windows_arm64" : "windows_amd64";

const LIBRARY = "libadbc_driver_trino.dll";

/** One file of a tar archive: a 512-byte header, then the data padded to 512 bytes. */
function tarEntry(name, data) {
  const header = Buffer.alloc(512);
  header.write(name, 0, "utf8");
  header.write("0000644\0", 100);
  header.write("0000000\0", 108);
  header.write("0000000\0", 116);
  header.write(`${data.length.toString(8).padStart(11, "0")}\0`, 124);
  header.write("00000000000\0", 136);
  header.write("        ", 148);
  header.write("0", 156);
  header.write("ustar\0", 257);
  header.write("00", 263);
  const sum = header.reduce((a, b) => a + b, 0);
  header.write(`${sum.toString(8).padStart(6, "0")}\0 `, 148);
  const padding = Buffer.alloc((512 - (data.length % 512)) % 512);
  return Buffer.concat([header, data, padding]);
}

/** A `.tar.gz` like the registry's: `MANIFEST` naming the library, and the library, whose
 *  bytes say which version it is. */
export function driverPackage(version) {
  const manifest = `name = "Trino"\nlicense = "Apache-2.0"\nversion = "${version}"\n\n[Files]\ndriver = "${LIBRARY}"\n`;
  const files = [
    tarEntry("MANIFEST", Buffer.from(manifest)),
    tarEntry(LIBRARY, Buffer.from(`not really a library: ${version}`)),
    Buffer.alloc(1024),
  ];
  return gzipSync(Buffer.concat(files));
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");

/**
 * Serves the registry on a free local port. `versions` are the registry's versions;
 * `corrupted` names those whose index hash does not match their package. Returns the URL for
 * `PLATYPUS_DRIVER_REGISTRY`. The server does not keep the runner alive.
 */
export async function fakeRegistry({ versions, corrupted = [] }) {
  const packages = new Map(versions.map((v) => [v, driverPackage(v)]));
  const entries = versions.map((v) => {
    const hash = corrupted.includes(v) ? sha256(Buffer.from("something else")) : sha256(packages.get(v));
    return `  - packages:\n    - platform: ${PLATFORM}\n      url: trino/${v}/pkg.tar.gz\n      hash: sha256:${hash}\n    version: ${v}\n`;
  });
  const index = `drivers:\n- name: Trino\n  license: Apache-2.0\n  path: trino\n  pkginfo:\n${entries.join("")}`;
  const server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://localhost").pathname;
    const version = /^\/trino\/([^/]+)\/pkg\.tar\.gz$/.exec(path)?.[1];
    const body = path === "/index.yaml" ? Buffer.from(index) : version ? packages.get(version) : undefined;
    response.writeHead(body ? 200 : 404, { "Content-Length": body?.length ?? 0 });
    response.end(body);
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  server.unref();
  return `http://127.0.0.1:${server.address().port}`;
}

/** Puts `version` of the Trino driver in the profile as an install leaves it, `active` or not. */
export function seedDriver(dir, version, active = true) {
  const root = join(dir, "drivers", "adbc", "trino");
  const folder = join(root, version);
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, LIBRARY), `not really a library: ${version}`);
  writeFileSync(
    join(folder, "driver.toml"),
    `name = "trino"\ntitle = "Trino"\nversion = "${version}"\nlicense = "Apache-2.0"\nlibrary = "${LIBRARY}"\n`,
  );
  if (active) writeFileSync(join(root, "ACTIVE"), version);
}

/** The ids of the connections `trinoConnections` writes. */
export const SAVED = "Trino saved password";
export const ASKS = "Trino asks for its password";

/** Two Trino connections: one that keeps its password (the check connects to it) and one that
 *  asks each time (the check leaves it alone). */
export function trinoConnections(dir) {
  const connection = (id, name, save) => `[[connections]]
id = "00000000-0000-4000-8000-0000000000${id}"
name = "${name}"
driver = "trino"
host = "127.0.0.1"
port = 9
database = "memory"
user = "platypus"
ssl_mode = "disable"
save_password = ${save}

[connections.options]
schema = "default"
`;
  writeFileSync(join(dir, "connections.toml"), `version = 6\n\n${connection("a1", SAVED, true)}\n${connection("a2", ASKS, false)}`);
}

/** Selectors shared by the scenarios. */
export const row = `document.querySelector('.modal.settings [data-driver="trino"]')`;
export const versionOf = `${row}?.querySelector('[data-field="version"]')?.textContent`;
export const badge = (version) => `${row}?.querySelector('.update-badge[data-update=${JSON.stringify(version)}]')`;
export const compat = `document.querySelector('.driver-compat')`;
export const button = (scope, text) => `[...${scope}.querySelectorAll('button')].find(b => b.textContent.trim() === ${JSON.stringify(text)})`;
/** A row of "Versions…". */
export const versionRow = (v) => `${row}?.querySelector('.driver-version[data-version=${JSON.stringify(v)}]')`;
