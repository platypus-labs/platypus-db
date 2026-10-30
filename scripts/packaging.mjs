// Fills the package manager manifests in `packaging/` (winget, Homebrew cask, AUR, Flatpak)
// for a release: version, date, download URLs and the SHA-256 of the installers, read from the
// folder where the release's files were downloaded. A manifest whose installers are not there
// is skipped (v0.2.1 has only the Windows ones). Submitting them is in docs/RELEASING.md.
//
//   node scripts/packaging.mjs 0.3.0 ~/Downloads/v0.3.0
//   node scripts/packaging.mjs 0.3.0 ~/Downloads/v0.3.0 --date 2026-11-20 --out packaging-out
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const releases = "https://github.com/PaoloCisly/platypus-db/releases/download";

/** The installers the manifests point at, by placeholder name, as Tauri names them. */
export function assets(version) {
  return {
    WINDOWS_NSIS: `PlatypusDB_${version}_x64-setup.exe`,
    MACOS_ARM: `PlatypusDB_${version}_aarch64.dmg`,
    MACOS_INTEL: `PlatypusDB_${version}_x64.dmg`,
    LINUX_DEB: `PlatypusDB_${version}_amd64.deb`,
  };
}

/** The `@NAME@` placeholders of `values`, for a release whose files are in `dir`: the
 *  `SHA256_` and `URL_` of the installers found there, the others always. */
export function values(version, date, dir) {
  const found = { VERSION: version, DATE: date };
  for (const [name, file] of Object.entries(assets(version))) {
    const path = join(dir, file);
    if (!existsSync(path)) continue;
    found[`SHA256_${name}`] = createHash("sha256").update(readFileSync(path)).digest("hex");
    found[`URL_${name}`] = `${releases}/v${version}/${file}`;
  }
  return found;
}

/** `template` with its placeholders replaced, or the names `values` lacks. */
export function fill(template, values) {
  const missing = new Set();
  const text = template.replace(/@([A-Z0-9_]+)@/g, (_, name) => {
    if (name in values) return values[name];
    missing.add(name);
    return "";
  });
  return missing.size ? { missing: [...missing] } : { text };
}

/** Every file under `dir`, hidden ones too (`.SRCINFO`), relative to it. */
function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path).map((f) => join(name, f)) : [name];
  });
}

/** Writes the filled manifests of each `packaging/` folder to `out`; returns what it did. */
export function generate(templates, out, values) {
  return readdirSync(templates).map((manager) => {
    const filled = files(join(templates, manager)).map((file) => ({
      file,
      ...fill(readFileSync(join(templates, manager, file), "utf8"), values),
    }));
    const missing = [...new Set(filled.flatMap((f) => f.missing ?? []))];
    if (missing.length) return { manager, missing };
    for (const { file, text } of filled) {
      mkdirSync(dirname(join(out, manager, file)), { recursive: true });
      writeFileSync(join(out, manager, file), text);
    }
    return { manager };
  });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name) => {
    const at = args.indexOf(name);
    return at < 0 ? undefined : args.splice(at, 2)[1];
  };
  const date = option("--date") ?? new Date().toISOString().slice(0, 10);
  const [version, dir] = args;
  const out = option("--out") ?? join(root, "target", "packaging", version ?? "");
  if (!version || !dir || !/^\d+\.\d+\.\d+/.test(version) || !/^\d{4}-\d{2}-\d{2}$/.test(date)) {
    console.error("usage: node scripts/packaging.mjs <version> <release files folder> [--date YYYY-MM-DD] [--out folder]");
    process.exit(2);
  }
  for (const { manager, missing } of generate(join(root, "packaging"), out, values(version, date, dir))) {
    console.log(missing ? `${manager}: skipped, missing ${missing.join(", ")}` : `${manager}: ${relative(process.cwd(), join(out, manager))}`);
  }
}
