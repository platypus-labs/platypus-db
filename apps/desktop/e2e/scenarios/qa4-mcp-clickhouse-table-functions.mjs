// QA4 MCP / ClickHouse read-only: table functions that reach the network get past the check.
// Steps: a read-only agent connection to ClickHouse (docker/compose.yml); a local HTTP listener the
//   container can reach (host.docker.internal); run_query `SELECT * FROM <fn>('http://host.docker.internal:<port>/<bucket>/', 'k', 's')`
//   for url (control), deltaLake, iceberg, hudi.
// Expected: refused as `url`, `s3`, `hdfs`... are ("acts outside the transaction or on the server's files"),
//   no request reaches the listener.
// Actual: `url` is refused, but deltaLake, iceberg and hudi (also deltaLakeCluster, icebergS3, icebergCluster,
//   hudiCluster, azureBlobStorageCluster, fileCluster, ytsaurus, paimon...) run: the ClickHouse server
//   makes GET/HEAD requests to the given host (SSRF from the server's network, e.g. cloud metadata) and
//   fileCluster reads server files. The ClickHouse driver sets no `readonly` setting, so the list is the only guard.
// Code: crates/platypus-mcp/src/access.rs CLICKHOUSE_OUTSIDE (the `calls` list lacks these functions).
import assert from "node:assert/strict";
import { writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { join } from "node:path";
import { needs } from "../qa-grid.mjs";
import { noMcp, startClient } from "../qa3-mcp-client.mjs";

export const skip = async () => noMcp() ?? (await needs("clickhouse")());

export function setup({ dir }) {
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 7\n[[connections]]\nid = "00000000-0000-4000-8000-000000000451"\nname = "QA4 ch"\ndriver = "clickhouse"\nhost = "localhost"\nport = 58123\ndatabase = "platypus"\nuser = "platypus"\nssl_mode = "disable"\nsave_password = false\npassword_command = "echo platypus"\n`,
  );
}

export default async function (_page, { dir }) {
  const hits = [];
  const server = createServer((q, r) => {
    hits.push(`${q.method} ${q.url}`);
    r.statusCode = 404;
    r.end("no");
  }).listen(0, "0.0.0.0");
  await new Promise((r) => server.on("listening", r));
  const port = server.address().port;
  const mcp = await startClient(dir, "qa4-ch");
  const problems = [];
  try {
    for (const [fn, bucket] of [["url", "c"], ["deltaLake", "dddd"], ["iceberg", "iiii"], ["hudi", "hhhh"]]) {
      const arg = fn === "url" ? `'http://host.docker.internal:${port}/${bucket}','CSV'` : `'http://host.docker.internal:${port}/${bucket}/','k','s'`;
      const before = hits.length;
      const r = await mcp.call("run_query", { connection: "QA4 ch", sql: `SELECT * FROM ${fn}(${arg})` });
      const reached = hits.slice(before);
      const refused = r.error && /read-only for agents/.test(r.text);
      if (!refused || reached.length) problems.push(`${fn}: ${refused ? "refused" : "not refused"}, ${reached.length} request(s) reached the listener (${r.text.slice(0, 80)})`);
    }
  } finally {
    mcp.kill();
    server.close();
  }
  assert.deepEqual(problems, []);
}
