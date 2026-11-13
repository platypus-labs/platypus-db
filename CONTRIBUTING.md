# Contributing to PlatypusDB

Thanks for helping. This page covers how to build and test PlatypusDB, how work is split and
merged, and the style the code follows. By taking part you agree to the
[Code of Conduct](CODE_OF_CONDUCT.md).

- [Building](#building)
- [Finding something to work on](#finding-something-to-work-on)
- [While you work](#while-you-work)
- [Verification](#verification)
- [Finishing a change](#finishing-a-change)
- [License of contributions](#license-of-contributions)

## Building

You need Rust stable (≥ 1.99) with `rustfmt` and `clippy`, Node.js ≥ 24 (see `.nvmrc`), the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system, and Docker
for the dev databases. [docs/dev/DEVELOPMENT.md](docs/dev/DEVELOPMENT.md) has the project layout, the
commands to run the app and the CLI, the sample database and the dev databases.

```bash
cd apps/desktop
npm install
npm run tauri dev
```

How features plug into the app (actions, tab kinds, settings, driver capabilities) is explained
in [docs/dev/EXTENDING.md](docs/dev/EXTENDING.md).

## Finding something to work on

Planned work is in [docs/dev/ROADMAP.md](docs/dev/ROADMAP.md) (in Italian), split into **groups** that
own separate areas of the code, so that several people can work in parallel without conflicts.

**Outside contributors**: open an issue first for anything larger than a small fix, then send
a pull request from a fork.

**Team members** take a group with a lock:

1. Update: `git fetch origin`, `git pull`; on a group branch also `git merge origin/main`.
2. In the "Lock dei gruppi" table of the roadmap, pick a free group whose dependencies are
   ready, following the recommended waves. A group marked `✅ fatto (…)` is open for the
   priorities left; one marked `✅ completato` is closed. A lock expires when its branch has no
   commits for more than 3 days (`git log -1 --format=%cd origin/<branch>`).
3. On an up-to-date `main`, change only your group's row to `🔒 <name> · <branch> · <YYYY-MM-DD>`,
   commit it alone as `chore(roadmap): lock gruppo <X>` and push it before writing any code. If
   the push is rejected, `git pull --rebase` and check the row again: if someone else took the
   group meanwhile, pick another one. Never force-push `main`.
4. Branch from `main` as `group-<letter>-<topic>` (for example `group-d-editor`) and push it at
   least once a day, or the lock expires. If your group depends on a group not merged yet, branch
   from that group's branch and merge after it.

## While you work

- **Stay in your group's area.** A change to a shared file (see "Mappa dei conflitti" in the
  roadmap) goes in a small, separate commit that extends an API, without feature logic.
- **Database features go through the `Dialect` or the driver capabilities**, never through
  checks on the driver name scattered in the UI.
- **Before adding a type, a `Connection` method or a module to `platypus-core` or
  `platypus-sql`**, check that it does not already exist on `main` or on another group's branch
  (`git grep <name> origin/<branch>`).
- **New logic comes with a new test.** Anything that touches a driver also goes into the
  conformance suite (`crates/platypus-conformance`).
- **Leave the code better than you found it**: remove duplication, simplify, rename what is
  unclear and pick the efficient solution (no superfluous allocations, copies, queries or
  renders). Refactors that keep the behavior are welcome inside a feature branch.
- **Commits** are in English, in [Conventional Commits](https://www.conventionalcommits.org)
  style: `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`. Stage only your own files
  (`git add <file>`, not `git add -A`).
- **clippy runs in `pedantic` mode with `-D warnings`.** The lints that fire most:
  `too_many_lines` (split functions over 100 lines), `struct_excessive_bools` (more than 3
  `bool` fields: derive one or use an enum), `format_push_string` (use `write!`),
  `missing_panics_doc` (no `expect`/`unwrap` in public functions).

### End-to-end tests

The scenarios in `apps/desktop/e2e/scenarios/` drive the compiled app (Windows, WebView2). They
run much slower on CI runners than locally, so:

- Never wait for a condition that is already true before the action: a status such as "3 rows"
  may be the old one. Clear `.statusbar span` first, or wait for a signal that changes (the
  Cancel button disabled at the end of a run).
- Result cards appear when a script starts, not when it ends; while it runs the editor ignores
  Ctrl+Enter.
- After `page.type(...)` in the editor, wait for `.cm-line` to hold the text before running it.
- CodeMirror ignores keys for 75 ms after the completion list opens.

## Verification

Everything must be green before a change is called done:

```bash
cargo fmt --all --check
npm --prefix apps/desktop run build    # before clippy: the desktop crate needs the built frontend
cargo clippy --workspace --all-targets
cargo test --workspace --exclude platypus-desktop
```

Then the release build and the end-to-end tests (they drive the release executable, not the dev
server, so rebuild first):

```bash
npm --prefix apps/desktop run tauri build -- --no-bundle
npm --prefix apps/desktop run e2e
```

And the conformance suite against the dev databases:

```bash
docker compose -f docker/compose.yml up -d --wait
cargo test -p platypus-conformance -- --nocapture
```

The conformance suite skips the databases it cannot reach and prints `skipping ...`: check that
it tried them all.

GitHub Actions do not run automatically during development: `ci.yml` and `release.yml` start
only by hand, when the maintainers decide to. Local verification is the check, so run all the
steps above before merging, even when a change does not touch the UI or the drivers.

## Finishing a change

1. **Refactor** the whole diff (`git diff origin/main...HEAD`) for duplication, naming and
   superfluous work, without changing behavior, in a separate `refactor:` commit.
2. **Verify** as above.
3. **Review** the branch and fix what the review finds.
4. **Document**: the group's "Stato" line in the roadmap, [docs/dev/EXTENDING.md](docs/dev/EXTENDING.md)
   for a new extension point, [docs/user/FEATURES.md](docs/user/FEATURES.md) for user-facing features (and the README in English and
   Italian when the overview changes), and the
   [changelog](CHANGELOG.md).
5. **Merge** into `main` (team members, no pull request):

   ```bash
   git fetch origin
   git merge origin/main          # on the group branch; resolve conflicts here, then verify again
   git checkout main && git pull
   git merge --no-ff <branch>
   git push origin main
   ```

   Merge only with the verification green, run again after merging `origin/main`. If the push is
   rejected because `main` moved, start again from `git fetch`. Never force-push.
6. **Release the lock** with a `chore(roadmap): rilascio gruppo <X>` commit: `✅ completato` only
   when all of the group's P1, P2 and P3 items are done (the group's "Stato" line becomes
   `**Stato: completato**` too), `✅ fatto (<priorities done>)` when something is left, `libero`
   when no priority level was finished. If you abandon the work, release the lock at once.

## License of contributions

PlatypusDB is licensed under the [GNU AGPL v3.0 or later](LICENSE). By submitting a
contribution you agree that it is licensed under the same terms.

The name and the logo are not: PlatypusDB™ and the PlatypusDB logo are trademarks of the
PlatypusDB maintainers, and the logo and icon files listed in [NOTICE](NOTICE) stay reserved.
Contributing does not grant any right to use the marks; what anyone may do with them is in
[TRADEMARKS.md](TRADEMARKS.md).
