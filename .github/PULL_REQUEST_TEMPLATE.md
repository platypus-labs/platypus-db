## What and why

<!-- What the change does for users, and the issue or roadmap item it closes. -->

## How it was tested

<!-- New tests, scenarios, the databases tried by hand. -->

## Checklist

- [ ] `cargo fmt --all --check`
- [ ] `npm --prefix apps/desktop run build`
- [ ] `cargo clippy --workspace --all-targets` (pedantic, no warnings)
- [ ] `cargo test --workspace --exclude platypus-desktop`
- [ ] UI changes: `npm --prefix apps/desktop run tauri build -- --no-bundle`, then `npm --prefix apps/desktop run e2e`
- [ ] Driver changes: `docker compose -f docker/compose.yml up -d --wait`, then `cargo test -p platypus-conformance` (no database skipped)
- [ ] New logic has tests; driver changes are in the conformance suite
- [ ] Docs updated: docs/FEATURES.md for user features (README and README.it.md for the overview), docs/EXTENDING.md for extension points, CHANGELOG.md
- [ ] Commits follow Conventional Commits, in English
