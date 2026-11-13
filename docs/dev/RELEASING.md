# Releasing

Releases are built by `.github/workflows/release.yml` when a `v*` tag is pushed. It builds
installers for Windows (NSIS, MSI), macOS (Apple silicon and Intel) and Linux (AppImage,
deb, rpm), and attaches them to a **draft** GitHub release with `latest.json`, the file the
in-app updater reads.

## One-time setup

1. **Updater key.** Installed apps only accept updates signed with the key whose public part
   is in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private
   key is `~/.tauri/platypusdb.key` on the machine that generated it. Add it to the
   repository secrets:
   - `TAURI_SIGNING_PRIVATE_KEY`: the contents of `platypusdb.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its password (empty for the current key)

   Keep a backup of the private key outside this machine. If it is lost, installed copies
   can no longer be updated automatically; a new key means a new public key in the config
   and a manual reinstall for existing users.

2. **Code signing (optional, recommended).** Without it Windows SmartScreen and macOS
   Gatekeeper warn on first launch.
   - Windows: `WINDOWS_CERTIFICATE` (the `.pfx`, base64) and `WINDOWS_CERTIFICATE_PASSWORD`.
     See "Windows code signing plan" below for which certificate to get.
   - macOS: `APPLE_CERTIFICATE` (the `.p12`, base64), `APPLE_CERTIFICATE_PASSWORD`,
     `APPLE_SIGNING_IDENTITY`, and for notarization `APPLE_ID`, `APPLE_PASSWORD`
     (an app-specific password) and `APPLE_TEAM_ID`.

## Cutting a release

1. Set the version in `Cargo.toml` (`workspace.package.version`),
   `apps/desktop/package.json` and `apps/desktop/src-tauri/tauri.conf.json`.
2. Commit, then tag and push:

   ```bash
   git tag v0.1.0
   git push origin main v0.1.0
   ```

3. Wait for the workflow, check the draft release (installers and `latest.json` for every
   platform), write the release notes and publish it. The updater only sees published
   releases: `releases/latest/download/latest.json` resolves to the newest non-draft one.

## Publishing plan

Every version goes through the same steps, with or without GitHub Actions:

1. **Version and tag.** Bump the three version fields (see above), commit `chore: version
   X.Y.Z` on `main`, tag `vX.Y.Z` (annotated, `PlatypusDB X.Y.Z`) and push both. The tag push
   starts the Release workflow.
2. **Installers.**
   - Actions available: the workflow builds every platform into a draft release.
   - No Actions minutes (or the workflow fails): build Windows locally (see "Building
     installers locally") and create the draft by hand, see "Local Windows release" below.
     macOS and Linux are added later by running the workflow on the same tag.
3. **Check the draft.** Installers for each platform, a `.sig` next to each, `latest.json`
   listing every platform present. Install the new version over the previous one and check
   that connections, passwords, scripts and settings are kept.
4. **Publish** with release notes (what changed for the user, grouped by area). Publishing
   is what makes the updater see the version.
5. **Check the auto-update** from the previous published version: it must offer the new
   one, download it and restart into it.
6. **Package managers** (once the repository is public, their downloads must be reachable
   without a login): see "Package managers" below.
7. **Announce** (README badge, site, discussions) once the repository is public.

### Local Windows release

```bash
cd apps/desktop
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/platypusdb.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
npm run tauri build
```

The build writes `target/release/bundle/nsis/PlatypusDB_X.Y.Z_x64-setup.exe` and
`bundle/msi/PlatypusDB_X.Y.Z_x64_en-US.msi`, each with a `.sig`. Write `latest.json` with
the signatures (the contents of the `.sig` files) and the download URLs
`https://github.com/platypus-labs/platypus-db/releases/download/vX.Y.Z/<file>`, under the keys
`windows-x86_64` (NSIS), `windows-x86_64-nsis` and `windows-x86_64-msi`, plus `version`,
`notes` and `pub_date`. Then:

```bash
gh release create vX.Y.Z --draft --verify-tag --title "PlatypusDB vX.Y.Z" --notes "..." <the two installers, their .sig files and latest.json>
```

When the workflow later runs on the same tag, `tauri-action` finds the release by tag and
adds the macOS and Linux assets, merging them into `latest.json`; check that it did not
create a second draft.

## MCP Registry

The official MCP Registry (registry.modelcontextprotocol.io) lists PlatypusDB as
`io.github.platypus-labs/platypus-db`, with an MCP bundle (`.mcpb`) that Claude Desktop installs in
one click. After publishing a release, run the **MCP Registry** workflow:

```bash
gh workflow run mcp-registry.yml -f version=X.Y.Z
```

It unpacks the Windows executable from the release's MSI, builds
`platypus-db-X.Y.Z-windows-x64.mcpb` and `server.json` with `scripts/mcpb.mjs`, attaches the
bundle to the release, and publishes `server.json` signed in with the workflow's GitHub identity
(no token to keep). Locally, `node scripts/mcpb.mjs X.Y.Z <platypus-db.exe>` writes both to
`target/mcpb/X.Y.Z/`; `mcp-publisher validate target/mcpb/X.Y.Z/server.json` checks it. The bundle
is Windows only: macOS waits for a signed app (Gatekeeper refuses an unsigned executable unpacked
from a download), Linux needs the system's WebKitGTK.

## Package managers

`packaging/` holds the manifests for winget, Homebrew (cask), the AUR (`platypusdb-bin`, from
the `.deb`) and Flathub, with `@VERSION@`, `@DATE@` and `@SHA256_…@` placeholders. Download
the release's installers into a folder, then fill them:

```bash
node scripts/packaging.mjs X.Y.Z <folder with the installers>
```

The filled manifests land in `target/packaging/X.Y.Z/<manager>/`; a manager whose installers
are missing is skipped (a Windows-only release fills only winget). Then submit each one:

- **winget**: a pull request to `microsoft/winget-pkgs` with the three files under
  `manifests/p/PlatypusLabs/PlatypusDB/X.Y.Z/` (or `wingetcreate submit <folder>`). Check first
  with `winget validate <folder>` and `winget install --manifest <folder>`.
- **Homebrew**: the first version as a pull request adding `Casks/p/platypusdb.rb` to
  `Homebrew/homebrew-cask` (`brew audit --new --cask platypusdb` first); later versions are
  bumped by Homebrew's autobump, or with `brew bump-cask-pr platypusdb --version X.Y.Z`.
  Homebrew wants a notable, signed and notarized app: until macOS signing is done, a tap
  of our own (`platypus-labs/homebrew-tap`) is the alternative.
- **AUR**: push `PKGBUILD` and `.SRCINFO` to `ssh://aur@aur.archlinux.org/platypusdb-bin.git`
  (an AUR account with an SSH key); check with `makepkg -si` on Arch first.
- **Flathub**: the first time, a pull request to `flathub/flathub` (branch `new-pr`) with
  `io.github.platypus_labs.platypus-db.yml`; Flathub then creates the
  `flathub/io.github.platypus_labs.platypus-db` repository, where later versions go. The Flatpak
  ID is under `io.github.platypus_labs` because Flathub verifies the ID's owner, here through the
  GitHub organization (no `platypusdb.dev` domain is registered); the app's own identifier and
  its data folders stay `dev.platypusdb.desktop`. The metainfo file goes with it. Check with
  `flatpak-builder --user --install --force-clean build io.github.platypus_labs.platypus-db.yml` and
  `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.platypus_labs.platypus-db.yml`.

The AUR and Flatpak packages install `share/platypusdb/package-manager` next to the
executable: the app then announces a new version but leaves the update to the package
manager (the built-in updater would otherwise replace files the package manager owns). winget and
Homebrew install the normal installer, which updates itself.

## Repository showcase

Set once, when the repository becomes public (GitHub › the repository's About and Settings):

- **Description**: "Fast, lightweight SQL client for PostgreSQL, MySQL, SQL Server, SQLite,
  DuckDB and more, with an MCP server for AI agents. Rust + Tauri."
- **Website**: the site, once it is online. It is published by `.github/workflows/pages.yml`,
  started by hand; the steps and the open question of the domain are in `site/README.md`.
- **Topics**: `sql-client`, `database`, `database-client`, `database-gui`, `postgresql`,
  `mysql`, `sqlite`, `sql-server`, `duckdb`, `rust`, `tauri`, `mcp`, `mcp-server`,
  `desktop-app`, `dbeaver-alternative`.
- **Social preview**: `docs/brand/social-preview.png` (1280×640). Its source is
  `docs/brand/social-preview.html`; after changing it, or the screenshot it shows, run
  `node scripts/social-preview.mjs` (it needs Chrome, Edge or Chromium), then upload the PNG
  in Settings › General › Social preview.
- **Organization picture**: `docs/brand/avatar.png` (512×512, from `avatar.html`), in the
  organization's Settings › Profile.
- **Discussions**: on, with the categories Announcements (releases), Q&A, Ideas and Show
  and tell; then add a "Ask a question" link to Q&A in `.github/ISSUE_TEMPLATE/config.yml`.
- **`good first issue`**: label small, self-contained tasks with a pointer to the files to
  touch: a missing translation, a keyboard shortcut, a type the grid shows as text, a driver
  test for one more server version. Keep five or so open.

## Windows code signing plan

Unsigned installers make SmartScreen show "Windows protected your PC" and the publisher as
unknown. A self-signed certificate does not help. Options, in order of preference:

1. **Certum Open Source Code Signing** (about 30-60 EUR a year): issued to individual
   developers in the EU for open-source projects. The key lives on a smart card or in the
   SimplySign cloud, so there is no `.pfx`: sign with Tauri's `bundle.windows.signCommand`
   (a command that receives the file path) instead of the `WINDOWS_CERTIFICATE` secrets,
   locally and, with the cloud key, in the workflow.
2. **SignPath Foundation**: free signing for open-source projects, done in their CI
   pipeline after approval. Needs working GitHub Actions.
3. **Azure Trusted Signing** (about 10 USD a month): cheap and CI-friendly, but open to
   individuals only in some countries; check availability in Italy first.
4. A classic OV/EV certificate (Sectigo, DigiCert, 200-500 EUR a year): since 2023 the key
   must be on a hardware token.

Even signed, a new publisher builds SmartScreen reputation with downloads (EV certificates
no longer skip this since 2024), but the warning names the publisher and fades much sooner.
After getting the certificate: configure signing, rebuild, replace the installers and
their `.sig` files in the draft, and regenerate `latest.json` (the signatures change with
the file).

## Building installers locally

```bash
cd apps/desktop
# PowerShell: $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content ~/.tauri/platypusdb.key -Raw
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/platypusdb.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
npm run tauri build
```

Without the key variables the build fails at the updater signature step; use
`npm run tauri build -- --no-bundle` to build just the executable.
