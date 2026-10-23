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
