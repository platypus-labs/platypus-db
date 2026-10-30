// node --test "scripts/*.test.mjs"
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { assets, fill, generate, values } from "./packaging.mjs";

const templates = join(dirname(dirname(fileURLToPath(import.meta.url))), "packaging");

test("fill replaces placeholders or lists the missing ones", () => {
  assert.deepEqual(fill("v@VERSION@ on @DATE@", { VERSION: "1.2.3", DATE: "2026-11-01" }), {
    text: "v1.2.3 on 2026-11-01",
  });
  assert.deepEqual(fill("@A@ @B@ @A@", { B: "x" }), { missing: ["A"] });
  assert.deepEqual(fill("me@example.com", {}), { text: "me@example.com" });
});

test("values hashes the installers that are there", () => {
  const dir = mkdtempSync(join(tmpdir(), "packaging-"));
  writeFileSync(join(dir, assets("1.2.3").WINDOWS_NSIS), "abc");
  const found = values("1.2.3", "2026-11-01", dir);
  assert.equal(found.SHA256_WINDOWS_NSIS, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  assert.equal(
    found.URL_WINDOWS_NSIS,
    "https://github.com/PaoloCisly/platypus-db/releases/download/v1.2.3/PlatypusDB_1.2.3_x64-setup.exe",
  );
  assert.equal(found.SHA256_LINUX_DEB, undefined);
});

test("generate fills every manifest when every installer is there, and skips the others", () => {
  const dir = mkdtempSync(join(tmpdir(), "packaging-"));
  for (const file of Object.values(assets("1.2.3"))) writeFileSync(join(dir, file), file);
  const all = values("1.2.3", "2026-11-01", dir);
  const out = mkdtempSync(join(tmpdir(), "packaging-out-"));
  const done = generate(templates, out, all);
  assert.deepEqual(done.map((d) => d.manager).sort(), ["aur", "flatpak", "homebrew", "winget"]);
  assert.ok(done.every((d) => !d.missing), JSON.stringify(done));
  const cask = readFileSync(join(out, "homebrew", "platypusdb.rb"), "utf8");
  assert.match(cask, /version "1\.2\.3"/);
  assert.ok(cask.includes(all.SHA256_MACOS_ARM) && cask.includes(all.SHA256_MACOS_INTEL));
  assert.ok(readFileSync(join(out, "aur", ".SRCINFO"), "utf8").includes(all.SHA256_LINUX_DEB));
  assert.ok(!/@[A-Z0-9_]+@/.test(readFileSync(join(out, "flatpak", "dev.platypusdb.desktop.yml"), "utf8")));

  const windowsOnly = values("1.2.3", "2026-11-01", mkdtempSync(join(tmpdir(), "packaging-")));
  const skipped = generate(templates, mkdtempSync(join(tmpdir(), "packaging-out-")), windowsOnly);
  assert.ok(skipped.every((d) => d.missing), "no installers: every manifest is skipped");
});
