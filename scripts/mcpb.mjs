// Builds the MCP bundle (.mcpb) of a release and the server.json that lists it in the official
// MCP Registry (registry.modelcontextprotocol.io). The bundle holds the Windows executable,
// started with --mcp: Claude Desktop installs it with one click. macOS is left out until the app
// is signed (Gatekeeper refuses an unsigned executable unpacked from a download), Linux because
// the executable needs the system's WebKitGTK.
//
//   node scripts/mcpb.mjs X.Y.Z <platypus-db.exe> [out folder]
//
// The executable comes from the release's MSI (msiexec /a on Windows, msiextract elsewhere). The
// bundle is `platypus-db-X.Y.Z-windows-x64.mcpb`: its URL must contain "mcp" for the registry.
// Packing uses the official CLI (npx @anthropic-ai/mcpb). See "MCP Registry" in
// docs/dev/RELEASING.md.
import { execFileSync, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "https://github.com/platypus-labs/platypus-db";
export const SERVER_NAME = "io.github.platypus-labs/platypus-db";
const DESCRIPTION = "Your PlatypusDB database connections for AI agents, with the access you give each one.";

/** The bundle's manifest.json for `version`. */
export function manifest(version) {
  return {
    manifest_version: "0.3",
    name: "platypus-db",
    display_name: "PlatypusDB",
    version,
    description: DESCRIPTION,
    long_description:
      "The MCP server of PlatypusDB, an open source desktop SQL client. It reads the connections saved in the " +
      "PlatypusDB app (PostgreSQL, MySQL, SQL Server, Oracle, SQLite, DuckDB, Snowflake and more) with the access " +
      "set for each one: none, read only (the default) or read and write. Passwords never reach the agent, and " +
      "every change an agent wants to run waits for approval in the app. Install the app and save your " +
      "connections there first.",
    author: { name: "PlatypusDB maintainers", url: "https://github.com/platypus-labs" },
    repository: { type: "git", url: REPO },
    homepage: "https://platypus-labs.github.io/platypus-db/",
    documentation: `${REPO}/blob/main/docs/user/AGENTS.md`,
    support: `${REPO}/issues`,
    icon: "icon.png",
    server: {
      type: "binary",
      entry_point: "server/platypus-db.exe",
      mcp_config: { command: "${__dirname}/server/platypus-db.exe", args: ["--mcp"] },
    },
    keywords: ["sql", "database", "postgresql", "mysql", "sqlite", "sql-server", "duckdb", "snowflake"],
    license: "AGPL-3.0-or-later",
    compatibility: { platforms: ["win32"] },
  };
}

/** The registry's server.json for the bundle published at `url` with `sha256`. */
export function serverJson(version, url, sha256) {
  return {
    $schema: "https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json",
    name: SERVER_NAME,
    title: "PlatypusDB",
    description: DESCRIPTION,
    repository: { url: REPO, source: "github" },
    websiteUrl: "https://platypus-labs.github.io/platypus-db/",
    version,
    packages: [{ registryType: "mcpb", identifier: url, fileSha256: sha256, transport: { type: "stdio" } }],
  };
}

export const bundleName = (version) => `platypus-db-${version}-windows-x64.mcpb`;

function main() {
  const [version, exe, outArg] = process.argv.slice(2);
  if (!version || !exe) {
    console.error("usage: node scripts/mcpb.mjs X.Y.Z <platypus-db.exe> [out folder]");
    process.exit(1);
  }
  const root = dirname(dirname(fileURLToPath(import.meta.url)));
  const out = resolve(outArg ?? join(root, "target", "mcpb", version));
  const stage = join(out, "stage");
  rmSync(stage, { recursive: true, force: true });
  mkdirSync(join(stage, "server"), { recursive: true });
  copyFileSync(exe, join(stage, "server", "platypus-db.exe"));
  copyFileSync(join(root, "docs", "brand", "avatar.png"), join(stage, "icon.png"));
  writeFileSync(join(stage, "manifest.json"), `${JSON.stringify(manifest(version), null, 2)}\n`);

  const bundle = join(out, bundleName(version));
  // npx is a .cmd file on Windows, which only a shell starts.
  if (process.platform === "win32") execSync(`npx -y @anthropic-ai/mcpb pack "${stage}" "${bundle}"`, { stdio: "inherit" });
  else execFileSync("npx", ["-y", "@anthropic-ai/mcpb", "pack", stage, bundle], { stdio: "inherit" });
  const sha256 = createHash("sha256").update(readFileSync(bundle)).digest("hex");
  const url = `${REPO}/releases/download/v${version}/${bundleName(version)}`;
  writeFileSync(join(out, "server.json"), `${JSON.stringify(serverJson(version, url, sha256), null, 2)}\n`);
  console.log(`Wrote ${bundle}\nsha256 ${sha256}\nWrote ${join(out, "server.json")}`);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
