// Points the site's download links at the files of a release, with their sizes, so a download
// button downloads the file instead of opening the releases page. The site loads nothing from
// other sites, so the links are written into the pages: run this after each release is published.
//
//   node scripts/site-release.mjs           # the latest release
//   node scripts/site-release.mjs v0.6.1    # a given tag
//
// It reads the release from the GitHub API and rewrites, in the pages of site/:
//   data-asset="<key>" href="..."     the file's address
//   data-size="<key>">...<            its size
//   data-release="version">...<       the version
//   data-windows="..."                the Windows installer, used by the "Download for Windows" button
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = "platypus-labs/platypus-db";
const site = join(dirname(dirname(fileURLToPath(import.meta.url))), "site");

// Each file the download page offers, found among the release's assets by the end of its name.
const ASSETS = {
  exe: /_x64-setup\.exe$/,
  msi: /_x64_en-US\.msi$/,
  "dmg-arm": /_aarch64\.dmg$/,
  "dmg-intel": /_x64\.dmg$/,
  deb: /_amd64\.deb$/,
  rpm: /\.x86_64\.rpm$/,
  appimage: /_amd64\.AppImage$/,
};

const tag = process.argv[2];
const url = `https://api.github.com/repos/${REPO}/releases/${tag ? `tags/${tag}` : "latest"}`;
const response = await fetch(url, { headers: { Accept: "application/vnd.github+json" } });
if (!response.ok) {
  console.error(`${url}: ${response.status} ${response.statusText}`);
  process.exit(1);
}
const release = await response.json();
const version = release.tag_name.replace(/^v/, "");

const files = {};
for (const [key, pattern] of Object.entries(ASSETS)) {
  const asset = release.assets.find((a) => pattern.test(a.name));
  if (!asset) {
    console.error(`${release.tag_name}: no asset matches ${pattern} (${key})`);
    process.exit(1);
  }
  files[key] = { href: asset.browser_download_url, size: `${Math.round(asset.size / 2 ** 20)} MiB` };
}

const pages = readdirSync(site).filter((name) => name.endsWith(".html"));
for (const name of pages) {
  const path = join(site, name);
  const before = readFileSync(path, "utf8");
  const after = before
    .replace(/data-asset="([\w-]+)" href="[^"]*"/g, (all, key) => (files[key] ? `data-asset="${key}" href="${files[key].href}"` : all))
    .replace(/data-size="([\w-]+)">[^<]*</g, (all, key) => (files[key] ? `data-size="${key}">${files[key].size}<` : all))
    .replace(/data-release="version">[^<]*</g, `data-release="version">${version}<`)
    .replace(/data-windows="[^"]*"/g, `data-windows="${files.exe.href}"`);
  if (after !== before) {
    writeFileSync(path, after);
    console.log(`site/${name}: ${release.tag_name}`);
  }
}
