// Frees the space the `target` folder gathers over time: the incremental caches, and the
// build outputs no build has read or written for `--days` days (default 7), such as the old
// versions of a dependency after `Cargo.lock` moved on. Where the system does not record
// reads (often on Windows), an output counts from when it was built: Cargo rebuilds what it
// still needs.
//
//   node scripts/clean-target.mjs             incremental caches, outputs older than 7 days
//   node scripts/clean-target.mjs --days 30
//   node scripts/clean-target.mjs --dry-run   lists the size it would free
import { existsSync, readdirSync, rmSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const daysAt = args.indexOf("--days");
const days = daysAt < 0 ? 7 : Number(args[daysAt + 1]);
if (!Number.isFinite(days) || days < 0) {
  console.error("--days takes a number of days");
  process.exit(2);
}
const dryRun = args.includes("--dry-run");
const target = join(dirname(dirname(fileURLToPath(import.meta.url))), "target");
const before = Date.now() - days * 86_400_000;

/** Bytes under `path`, files and folders. */
function size(path) {
  const stat = statSync(path, { throwIfNoEntry: false });
  if (!stat) return 0;
  if (!stat.isDirectory()) return stat.size;
  return readdirSync(path).reduce((sum, name) => sum + size(join(path, name)), 0);
}

/** When `path`, or the newest file under it, was last read or written. */
function lastUsed(path) {
  const stat = statSync(path);
  if (!stat.isDirectory()) return Math.max(stat.atimeMs, stat.mtimeMs);
  return readdirSync(path).reduce((latest, name) => Math.max(latest, lastUsed(join(path, name))), stat.mtimeMs);
}

let freed = 0;
function remove(path) {
  freed += size(path);
  if (!dryRun) rmSync(path, { recursive: true, force: true });
}

/** The profile folders: `debug`, `release`, and those under a target triple. */
function profiles(dir, depth = 0) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .flatMap((e) => {
      const path = join(dir, e.name);
      if (existsSync(join(path, ".fingerprint"))) return [path];
      return depth === 0 ? profiles(path, 1) : [];
    });
}

for (const profile of profiles(target)) {
  remove(join(profile, "incremental"));
  // A crate's outputs share its name and hash: each entry is one build of one crate.
  for (const sub of ["deps", "build", ".fingerprint"]) {
    const dir = join(profile, sub);
    if (!existsSync(dir)) continue;
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (lastUsed(path) < before) remove(path);
    }
  }
}

const mb = Math.round(freed / 1e6);
console.log(dryRun ? `Would free ${mb} MB in ${target}` : `Freed ${mb} MB in ${target}`);
