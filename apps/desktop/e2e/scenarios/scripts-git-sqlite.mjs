// Group O, P3: Git for the saved scripts. The Scripts panel's Git bar turns the scripts folder
// into a repository, shows per-script badges, the diff of a changed script, commits chosen files
// (asking the author once), and pulls and pushes against a bare remote: fast-forward, divergence
// with "Pull with merge", a merge that stops on conflicts and is aborted, and a failing remote.
// The other side of the remote is a clone driven with the git command line.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";

const NAME = "E2E SQLite";
const row = (label) =>
  `[...document.querySelectorAll('.tree-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const scriptRow = (label) =>
  `[...document.querySelectorAll('.script-row')].find(r => r.querySelector('.tree-label')?.textContent === ${JSON.stringify(label)})`;
const badgeOf = (label) => `(${scriptRow(label)}?.querySelector('.git-badge')?.textContent ?? '')`;
const menuItems = `[...document.querySelectorAll('.context-menu button span:first-child')].map(s => s.textContent)`;
const barButton = (label) =>
  `[...document.querySelectorAll('.git-bar button')].find(b => b.textContent === ${JSON.stringify(label)} || b.getAttribute('aria-label') === ${JSON.stringify(label)})`;
const note = `(document.querySelector('.git-note')?.textContent ?? '')`;
const sync = `(document.querySelector('.git-sync')?.textContent ?? '')`;
const toast = `(document.querySelector('.toast')?.textContent ?? '')`;
const confirmButton = (label) =>
  `[...document.querySelectorAll('.modal.confirm .actions button')].find(b => b.textContent === ${JSON.stringify(label)})`;

const git = (cwd, ...args) => execFileSync("git", args, { cwd, encoding: "utf8" });

export function setup({ dir }) {
  const db = new DatabaseSync(join(dir, "e2e.db"));
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY);");
  db.close();
  writeFileSync(
    join(dir, "connections.toml"),
    `version = 1

[[connections]]
id = "00000000-0000-4000-8000-000000000001"
name = "${NAME}"
driver = "sqlite"
database = '${join(dir, "e2e.db")}'
save_password = false
`,
  );
}

async function rightClick(page, expression) {
  await page.eval(`${expression}.id = 'e2e-target'`);
  const r = await page.rect("#e2e-target");
  await page.click(r.x + 20, r.y + r.h / 2, { button: "right" });
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`document.getElementById('e2e-target').removeAttribute('id')`);
  return page.eval(menuItems);
}

async function closeMenu(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);
}

/** A new script named `name`, saved with `text`, through the panel. */
async function newScript(page, name) {
  await page.eval(`document.querySelector('.scripts-panel button[aria-label="New script"]').click()`);
  await page.waitFor(`document.querySelector('.script-rename') !== null`);
  await page.eval(`document.querySelector('.script-rename').select()`);
  await page.type(name);
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${scriptRow(name)} !== undefined`);
}

/** Refreshes the Git bar after files changed outside the app. */
async function refresh(page) {
  await page.eval(`${barButton("Refresh")}.click()`);
}

export default async function (page, { dir }) {
  try {
    execFileSync("git", ["--version"]);
  } catch {
    console.log("skipping scripts-git: git is not installed on this machine");
    return;
  }
  const scripts = join(dir, "scripts", "all");
  await page.waitFor(`${row(NAME)} !== undefined`);

  // Without a repository the bar offers to make one; cancelling changes nothing.
  await newScript(page, "alpha");
  writeFileSync(join(scripts, "alpha.sql"), "SELECT 1;\n");
  await page.waitFor(`${barButton("Turn this project's scripts into a Git repository")} !== undefined`);
  assert.equal(await page.eval(`document.querySelector('.git-branch') === null`), true);
  await page.eval(`${barButton("Turn this project's scripts into a Git repository")}.click()`);
  await page.waitFor(`${confirmButton("Turn into a repository")} !== undefined`);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.confirm') === null`);
  assert.equal(existsSync(join(scripts, ".git")), false);

  // Turning it into one shows the branch, and the new script is not tracked yet.
  await page.eval(`${barButton("Turn this project's scripts into a Git repository")}.click()`);
  await page.waitFor(`${confirmButton("Turn into a repository")} !== undefined`);
  await page.eval(`${confirmButton("Turn into a repository")}.click()`);
  await page.waitFor(`document.querySelector('.git-branch') !== null`, 15_000);
  assert.ok(existsSync(join(scripts, ".git")));
  await page.waitFor(`${badgeOf("alpha")} === '?'`);
  assert.equal(await page.eval(`${scriptRow("alpha")}.querySelector('.git-badge').title`), "Not tracked yet");
  // No upstream yet: nothing to pull, and no arrows.
  assert.equal(await page.eval(`${barButton("Pull")}.disabled`), true);
  assert.equal(await page.eval(`${barButton("Push")}.disabled`), true, "nothing to push before the first commit");
  assert.equal(await page.eval(`document.querySelector('.git-sync') === null`), true);

  // Make Git forget the author, so the first commit has to ask for one.
  git(scripts, "config", "user.name", "");
  git(scripts, "config", "user.email", "");

  // The Commit dialog lists the changed files; it needs a message and at least one file.
  await page.eval(`${barButton("Commit…")}.click()`);
  await page.waitFor(`document.querySelector('.git-commit') !== null`);
  const listed = await page.eval(`[...document.querySelectorAll('.git-commit-path')].map(e => e.textContent)`);
  assert.ok(listed.includes("alpha.sql"), `${listed}`);
  const submit = `document.querySelector('.git-commit button.primary')`;
  assert.equal(await page.eval(`${submit}.disabled`), true, "an empty message cannot be committed");
  await page.type("first script");
  await page.waitFor(`document.querySelector('.git-commit textarea').value === 'first script'`);
  assert.equal(await page.eval(`${submit}.disabled`), false);
  const tick = (path) =>
    `[...document.querySelectorAll('.git-commit-files li')].find(li => li.textContent.includes(${JSON.stringify(path)})).querySelector('input').click()`;
  // Unticking every file leaves nothing to commit; then only the links file is left out.
  for (const path of listed) await page.eval(tick(path));
  await page.waitFor(`${submit}.disabled === true`);
  for (const path of listed) await page.eval(tick(path));
  await page.waitFor(`${submit}.disabled === false`);
  if (listed.includes(".links.json")) await page.eval(tick(".links.json"));

  // Git has no author: it is asked once. An incomplete answer cannot be saved.
  await page.eval(`${submit}.click()`);
  await page.waitFor(`document.querySelector('.git-identity') !== null`);
  const save = `document.querySelector('.git-identity button.primary')`;
  assert.equal(await page.eval(`${save}.disabled`), true);
  await page.type("E2E Tester");
  await page.eval(`document.querySelector('.git-identity input[type=email]').focus()`);
  await page.type("not-an-email");
  await page.waitFor(`document.querySelector('.git-identity input[type=email]').value === 'not-an-email'`);
  assert.equal(await page.eval(`${save}.disabled`), true, "an email needs an @");
  await page.eval(`document.querySelector('.git-identity input[type=email]').select()`);
  await page.type("e2e@example.com");
  await page.waitFor(`${save}.disabled === false`);
  await page.eval(`${save}.click()`);
  await page.waitFor(`document.querySelector('.git-commit') === null`, 15_000);
  await page.waitFor(`${note}.startsWith('Committed ')`);
  assert.equal(git(scripts, "log", "--format=%s").trim(), "first script");
  assert.equal(git(scripts, "config", "user.name").trim(), "E2E Tester");
  const committed = git(scripts, "show", "--name-only", "--format=").split(/\r?\n/);
  assert.ok(committed.includes("alpha.sql"), `${committed}`);
  assert.ok(!committed.includes(".links.json"), "the unticked file stays out of the commit");
  await page.waitFor(`${badgeOf("alpha")} === ''`);

  // A changed script gets M, and "Show changes" shows its diff, read-only.
  writeFileSync(join(scripts, "alpha.sql"), "SELECT 2;\nSELECT 3;\n");
  await refresh(page);
  await page.waitFor(`${badgeOf("alpha")} === 'M'`);
  assert.equal(await page.eval(`${scriptRow("alpha")}.querySelector('.git-badge').title`), "Modified");
  let items = await rightClick(page, scriptRow("alpha"));
  assert.ok(items.includes("Show changes (Git diff)"), `${items}`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Show changes (Git diff)').click()`);
  await page.waitFor(`document.querySelector('.git-diff .git-diff-line') !== null`);
  const lines = (kind) => `[...document.querySelectorAll('.git-diff-line.${kind} .git-diff-text')].map(e => e.textContent)`;
  assert.deepEqual(await page.eval(lines("removed")), ["SELECT 1;"]);
  assert.deepEqual(await page.eval(lines("added")), ["SELECT 2;", "SELECT 3;"]);
  assert.equal(await page.eval(`document.querySelector('.git-diff textarea, .git-diff input') === null`), true, "the diff is read-only");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.git-diff') === null`);

  // An untracked script shows as all added; a script that did not change has no such entry.
  await newScript(page, "beta");
  writeFileSync(join(scripts, "beta.sql"), "SELECT 'b';\n");
  await refresh(page);
  await page.waitFor(`${badgeOf("beta")} === '?'`);
  items = await rightClick(page, scriptRow("beta"));
  assert.ok(items.includes("Show changes (Git diff)"), `${items}`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent === 'Show changes (Git diff)').click()`);
  await page.waitFor(`document.querySelector('.git-diff .git-diff-line.added') !== null`);
  assert.deepEqual(await page.eval(lines("added")), ["SELECT 'b';"]);
  assert.equal(await page.eval(`document.querySelectorAll('.git-diff-line.removed').length`), 0);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.git-diff') === null`);

  // Commit both: the author is known now, so no question; the badges and the menu entry go.
  await page.eval(`${barButton("Commit…")}.click()`);
  await page.waitFor(`document.querySelector('.git-commit') !== null`);
  await page.type("alpha and beta");
  await page.waitFor(`document.querySelector('.git-commit textarea').value === 'alpha and beta'`);
  await page.eval(`document.querySelector('.git-commit button.primary').click()`);
  await page.waitFor(`document.querySelector('.git-commit') === null`, 15_000);
  await page.waitFor(`${badgeOf("alpha")} === '' && ${badgeOf("beta")} === ''`);
  assert.equal(git(scripts, "log", "--format=%s").trim().split(/\r?\n/)[0], "alpha and beta");
  items = await rightClick(page, scriptRow("alpha"));
  assert.ok(!items.includes("Show changes (Git diff)"), `${items}`);
  await closeMenu(page);

  // A script in a folder: the folder is marked while something under it has changed.
  mkdirSync(join(scripts, "sub"));
  writeFileSync(join(scripts, "sub", "delta.sql"), "SELECT 'd';\n");
  await refresh(page);
  await page.waitFor(`${scriptRow("delta")} !== undefined && ${badgeOf("delta")} === '?'`);
  assert.equal(await page.eval(`${scriptRow("sub")}.querySelector('.git-badge.folder') !== null`), true);
  assert.equal(await page.eval(`${scriptRow("alpha")}.querySelector('.git-badge.folder') === null`), true);
  git(scripts, "add", "sub/delta.sql");
  git(scripts, "commit", "-q", "-m", "delta");
  await refresh(page);
  await page.waitFor(`${scriptRow("sub")}.querySelector('.git-badge.folder') === null`);

  // A bare remote: the branch has no upstream, so Push publishes it and starts tracking.
  const remote = join(dir, "remote.git");
  git(dir, "init", "--bare", "-q", "--initial-branch=main", remote);
  git(scripts, "branch", "-M", "main");
  git(scripts, "remote", "add", "origin", remote);
  await refresh(page);
  await page.waitFor(`${barButton("Push")}.disabled === false`);
  assert.equal(await page.eval(`${barButton("Pull")}.disabled`), true, "pull needs an upstream");
  await page.eval(`${barButton("Push")}.click()`);
  await page.waitFor(`${note} === 'Pushed'`, 30_000);
  await page.waitFor(`${sync} === '↑0 ↓0'`);
  assert.equal(await page.eval(`document.querySelector('.git-branch').textContent`), "main");
  assert.equal(await page.eval(`document.querySelector('.git-branch').title`), "Tracks origin/main");
  assert.equal(git(remote, "log", "--format=%s", "main").trim().split(/\r?\n/)[0], "delta");

  // Another clone changes a script and pushes; Refresh fetches and shows it behind.
  const other = join(dir, "other");
  git(dir, "clone", "-q", remote, other);
  git(other, "config", "user.name", "Other");
  git(other, "config", "user.email", "other@example.com");
  writeFileSync(join(other, "beta.sql"), "SELECT 'b from other';\n");
  git(other, "commit", "-q", "-am", "beta from other");
  git(other, "push", "-q");
  await refresh(page);
  await page.waitFor(`${sync} === '↑0 ↓1'`, 30_000);
  assert.equal(await page.eval(`${barButton("Push")}.disabled`), false);
  await page.eval(`${barButton("Pull")}.click()`);
  await page.waitFor(`${note} === 'Pulled the latest changes'`, 30_000);
  assert.equal(readFileSync(join(scripts, "beta.sql"), "utf8").trim(), "SELECT 'b from other';");
  await page.waitFor(`${sync} === '↑0 ↓0'`);

  // Pulling with nothing new says so.
  await page.eval(`${barButton("Pull")}.click()`);
  await page.waitFor(`${note} === 'Already up to date'`, 30_000);

  // Both sides have new commits: the pull is refused until the user agrees to a merge.
  writeFileSync(join(scripts, "alpha.sql"), "SELECT 'local';\n");
  git(scripts, "commit", "-q", "-am", "alpha local");
  writeFileSync(join(other, "gamma.sql"), "SELECT 'g';\n");
  git(other, "add", "gamma.sql");
  git(other, "commit", "-q", "-m", "gamma");
  git(other, "push", "-q");
  await refresh(page);
  await page.waitFor(`${sync} === '↑1 ↓1'`, 30_000);
  await page.eval(`${barButton("Pull")}.click()`);
  await page.waitFor(`${confirmButton("Pull with merge")} !== undefined`, 30_000);
  assert.equal(readFileSync(join(scripts, "alpha.sql"), "utf8").trim(), "SELECT 'local';");
  assert.equal(existsSync(join(scripts, "gamma.sql")), false, "nothing is merged before the user agrees");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${note} === 'Not pulled: the branches have diverged'`);
  assert.equal(existsSync(join(scripts, "gamma.sql")), false);
  await page.eval(`${barButton("Pull")}.click()`);
  await page.waitFor(`${confirmButton("Pull with merge")} !== undefined`, 30_000);
  await page.eval(`${confirmButton("Pull with merge")}.click()`);
  await page.waitFor(`${note} === 'Merged the remote changes'`, 30_000);
  assert.ok(existsSync(join(scripts, "gamma.sql")));
  await page.waitFor(`${scriptRow("gamma")} !== undefined`);
  await page.waitFor(`${sync} === '↑2 ↓0'`);
  await page.eval(`${barButton("Push")}.click()`);
  await page.waitFor(`${note} === 'Pushed'`, 30_000);
  await page.waitFor(`${sync} === '↑0 ↓0'`);

  // The same line changed on both sides: the merge stops, names the file, and can be aborted.
  git(other, "pull", "-q", "--ff-only");
  writeFileSync(join(other, "alpha.sql"), "SELECT 'theirs';\n");
  git(other, "commit", "-q", "-am", "alpha theirs");
  git(other, "push", "-q");
  writeFileSync(join(scripts, "alpha.sql"), "SELECT 'mine';\n");
  git(scripts, "commit", "-q", "-am", "alpha mine");
  await refresh(page);
  await page.waitFor(`${sync} === '↑1 ↓1'`, 30_000);
  await page.eval(`${barButton("Pull")}.click()`);
  await page.waitFor(`${confirmButton("Pull with merge")} !== undefined`, 30_000);
  await page.eval(`${confirmButton("Pull with merge")}.click()`);
  await page.waitFor(`${note}.startsWith('The merge stopped on conflicts in: alpha.sql')`, 30_000);
  await page.waitFor(`document.querySelector('.git-merging') !== null`);
  assert.ok((await page.eval(`document.querySelector('.git-merging').textContent`)).includes("alpha.sql"));
  await page.waitFor(`${badgeOf("alpha")} === 'U'`);
  assert.equal(await page.eval(`${barButton("Pull")}.disabled && ${barButton("Push")}.disabled`), true, "no pull or push in the middle of a merge");
  assert.ok(readFileSync(join(scripts, "alpha.sql"), "utf8").includes("<<<<<<<"));
  await page.eval(`${barButton("Abort merge")}.click()`);
  await page.waitFor(`${note} === 'Merge aborted'`, 15_000);
  await page.waitFor(`document.querySelector('.git-merging') === null`);
  await page.waitFor(`${badgeOf("alpha")} === ''`);
  assert.equal(readFileSync(join(scripts, "alpha.sql"), "utf8").trim(), "SELECT 'mine';");

  // A remote that cannot be reached is an error, not a hang; the bar keeps working after it.
  git(scripts, "remote", "set-url", "origin", join(dir, "missing.git"));
  await refresh(page);
  await page.waitFor(`${toast}.includes('missing.git')`, 30_000);
  await page.eval(`document.querySelector('.toast').click()`);
  await page.waitFor(`document.querySelector('.toast') === null`);
  await page.eval(`${barButton("Push")}.click()`);
  await page.waitFor(`${toast}.length > 0`, 30_000);
  await page.eval(`document.querySelector('.toast').click()`);
  // The remote moves on meanwhile, so the restored address is proven by a new count.
  writeFileSync(join(other, "zeta.sql"), "SELECT 'z';\n");
  git(other, "add", "zeta.sql");
  git(other, "commit", "-q", "-m", "zeta");
  git(other, "push", "-q");
  git(scripts, "remote", "set-url", "origin", remote);
  await refresh(page);
  await page.waitFor(`${sync} === '↑1 ↓2'`, 30_000);

  // A script saved from its editor shows as changed once the window looks again.
  await page.eval(`${scriptRow("beta")}.id = 'e2e-beta'`);
  await page.clickSel("#e2e-beta", { clickCount: 2 });
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('beta.sql')`, 10_000);
  const tab = `[...document.querySelectorAll('.query-tab')].find(t => t.style.display !== 'none')`;
  await page.eval(`${tab}.querySelector('.cm-content').focus()`);
  await page.key("End", "End", 35, CTRL);
  await page.type("-- edited");
  await page.waitFor(`[...${tab}.querySelectorAll('.cm-line')].some(l => l.textContent.includes('-- edited'))`);
  await page.key("s", "KeyS", 83, CTRL);
  await page.waitFor(`document.querySelector('.tab.active')?.textContent.startsWith('beta.sql —')`, 10_000);
  await refresh(page);
  await page.waitFor(`${badgeOf("beta")} === 'M'`);
  assert.ok(readFileSync(join(scripts, "beta.sql"), "utf8").includes("-- edited"));
}
