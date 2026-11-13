// The MCP bundle's manifest and the registry's server.json (scripts/mcpb.mjs).
import assert from "node:assert/strict";
import { test } from "node:test";
import { bundleName, manifest, SERVER_NAME, serverJson } from "./mcpb.mjs";

test("the manifest starts the bundled Windows executable as an MCP server", () => {
  const m = manifest("1.2.3");
  assert.equal(m.version, "1.2.3");
  assert.equal(m.server.type, "binary");
  assert.equal(m.server.entry_point, "server/platypus-db.exe");
  assert.deepEqual(m.server.mcp_config, { command: "${__dirname}/server/platypus-db.exe", args: ["--mcp"] });
  assert.deepEqual(m.compatibility.platforms, ["win32"]);
});

test("server.json lists the bundle of the release, whose URL names mcp as the registry asks", () => {
  const url = `https://github.com/platypus-labs/platypus-db/releases/download/v1.2.3/${bundleName("1.2.3")}`;
  const s = serverJson("1.2.3", url, "ab".repeat(32));
  assert.equal(s.name, SERVER_NAME);
  assert.equal(s.version, "1.2.3");
  assert.ok(s.description.length <= 100, "the registry caps the description at 100 characters");
  assert.equal(s.packages.length, 1);
  assert.equal(s.packages[0].registryType, "mcpb");
  assert.match(s.packages[0].identifier, /mcp/);
  assert.match(s.packages[0].identifier, /^https:\/\/github\.com\/.*\/releases\/download\//);
  assert.equal(s.packages[0].fileSha256.length, 64);
});
