// Help in the app: the user guide that comes with the app opens from the command palette and the
// "?" menu of the top bar, reads offline with an index and links between its pages (a heading
// of the page, another page, a page in a folder, a page that is missing, a file outside the
// guide, an address that opens in the browser, one that is refused), never runs the HTML of a
// page, and the "?" of the connection form, Settings > Agents and Settings > Safety open the
// matching page. "What's new" shows the changelog section of the running version, and "Report a
// bug" opens the project's issue page. The pages are the scenario's own (the test hook
// `__platypus.setGuide`), so the links do not depend on the text of docs/user/guide; a last part walks
// the guide that ships.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { DatabaseSync } from "node:sqlite";
import { CTRL } from "../cdp.mjs";
import { editConnection } from "../conn-helpers.mjs";

const NAME = "Help E2E";
const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..", "..", "..");

export function setup({ dir }) {
  const path = join(dir, "help.db");
  const db = new DatabaseSync(path);
  db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY)");
  db.close();
  const toml = `version = 4

[[connections]]
id = "00000000-0000-4000-8000-0000000f0d01"
name = "${NAME}"
driver = "sqlite"
database = '${path}'
save_password = false
`;
  writeFileSync(join(dir, "connections.toml"), toml);
}

const filler = Array.from({ length: 60 }, (_, i) => `Paragraph ${i + 1} of the connect page.`).join("\n\n");

const PAGES = {
  README: `# Guide index

- [Connect](connect.md)
- [PostgreSQL](databases/postgres.md)
- [A page that is not there](nothing.md)
- [Outside the guide](../FEATURES.md#grid)
- [The site](https://example.org/page?a=1&b=2)
- [A script](javascript:window.__pwned=1)

<script>window.__pwned = 1</script>
<img src=x onerror="window.__pwned = 1">

Text with <b onmouseover="window.__pwned = 1">tags</b> inside.
`,
  connect: `# Connect to a database

${filler}

## SSH tunnels

Open the **Network** tab.

| Setting | Meaning |
|---|---|
| Host | the server |

![A screenshot](../../screenshots/palette.png)

See [the tunnels](#ssh-tunnels) above, [PostgreSQL](databases/postgres.md#tls) and [the index](README.md).
`,
  agents: "# Agents\n\nThe agents page.\n",
  safety: "# Safety\n\nThe safety page.\n",
  "databases/README": "# Databases\n\nOne page per database.\n",
  "databases/postgres": `# PostgreSQL\n\n${filler}\n\n## TLS\n\nBack to [connect](../connect.md#ssh-tunnels) or the [index](../README.md).\n`,
};

const dialog = `document.querySelector('.guide-dialog:not(.whats-new)')`;
const reader = `${dialog}.querySelector('.guide-reader')`;
const pageOf = `${reader}.dataset.page`;
const link = (text) =>
  `[...${reader}.querySelectorAll('a')].find(a => a.textContent === ${JSON.stringify(text)})`;
const opened = `window.__platypus.openedLinks.urls`;

async function openPalette(page, text) {
  await page.key("k", "KeyK", 75, CTRL);
  await page.waitFor(`document.querySelector('.palette input') !== null`);
  await page.type(text);
  await page.waitFor(`document.querySelector('.palette input').value === ${JSON.stringify(text)}`);
  await page.sleep(300);
}

async function closeGuide(page) {
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`${dialog} === null`);
}

const menuEntries = `[...document.querySelectorAll('.context-menu button')].map(b => b.textContent.trim())`;

async function helpMenu(page, entry) {
  await page.eval(`document.querySelector('.help-button').click()`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  await page.eval(`[...document.querySelectorAll('.context-menu button')].find(b => b.textContent.trim() === ${JSON.stringify(entry)}).click()`);
}

export const ready = `document.querySelector('.sidebar-header') !== null && window.__platypus !== undefined`;

export default async function (page) {
  await page.eval(`window.__platypus.setGuide(${JSON.stringify(PAGES)})`);

  // The palette finds the guide by "help" and by its name.
  await openPalette(page, "help");
  const entries = await page.eval(`[...document.querySelectorAll('.palette [role=option] .palette-label')].map(l => l.textContent)`);
  for (const wanted of ["Help: User guide", "Help: Keyboard shortcuts", "Help: What's new", "Help: Report a bug…"]) {
    assert.ok(entries.includes(wanted), `${wanted} in the palette: ${entries}`);
  }
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.palette') === null`);
  await openPalette(page, "user guide");
  await page.key("Enter", "Enter", 13);
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(pageOf), "README");
  assert.equal(await page.eval(`${reader}.querySelector('h1').textContent`), "Guide index");

  // Nothing of the page's HTML runs or shows: no script, no handler, no stray tag text.
  assert.equal(await page.eval(`window.__pwned`), undefined);
  assert.equal(await page.eval(`${reader}.querySelectorAll('script, img[onerror], [onmouseover]').length`), 0);
  const text = await page.eval(`${reader}.textContent`);
  assert.ok(!text.includes("<") && text.includes("Text with tags inside."), text);

  // The index lists the pages in reading order (the guide's own, not alphabetical), with the folder's pages under Databases.
  const index = await page.eval(`[...${dialog}.querySelectorAll('.guide-index button')].map(b => b.dataset.page)`);
  assert.deepEqual(index, ["README", "connect", "safety", "agents", "databases/README", "databases/postgres"]);
  assert.equal(await page.eval(`${dialog}.querySelector('.guide-index [aria-current=page]').dataset.page`), "README");

  // A refused address is plain text; the others are links.
  assert.equal(await page.eval(`${reader}.querySelector('a[data-href^="javascript"]')`), null);
  assert.ok((await page.eval(`${reader}.textContent`)).includes("A script"));

  // A link to another page, then to a heading of the same page (the reader scrolls to it).
  await page.eval(`${link("Connect")}.click()`);
  await page.waitFor(`${pageOf} === 'connect'`);
  assert.equal(await page.eval(`${reader}.scrollTop`), 0);
  assert.equal(await page.eval(`${reader}.querySelectorAll('table th').length`), 2, "the table is a table");
  // A screenshot the guide does not carry is left out; its description stays.
  assert.equal(await page.eval(`${reader}.querySelectorAll('img').length`), 0);
  assert.ok((await page.eval(`${reader}.textContent`)).includes("A screenshot"));
  await page.eval(`${link("the tunnels")}.click()`);
  await page.waitFor(`${reader}.scrollTop > 100`);
  assert.equal(await page.eval(pageOf), "connect");

  // A link into a page of the folder, at its heading; the link back climbs out of the folder.
  await page.eval(`${link("PostgreSQL")}.click()`);
  await page.waitFor(`${pageOf} === 'databases/postgres'`);
  await page.waitFor(`${reader}.scrollTop > 100`);
  // Read a little further down, so that Back must return here and not to the heading.
  await page.eval(`${reader}.scrollTop += 7`);
  const left = await page.eval(`${reader}.scrollTop`);
  await page.eval(`${link("connect")}.click()`);
  await page.waitFor(`${pageOf} === 'connect'`);
  await page.waitFor(`${reader}.scrollTop > 100`);

  // Back follows the way taken: connect, postgres, connect <- postgres <- connect <- README.
  await page.eval(`${dialog}.querySelector('.guide-head button').click()`);
  await page.waitFor(`${pageOf} === 'databases/postgres'`);
  assert.equal(await page.eval(`${reader}.scrollTop`), left, "Back returns to where the reader was");
  await page.eval(`${dialog}.querySelector('.guide-head button').click()`);
  await page.waitFor(`${pageOf} === 'connect'`);
  await page.eval(`${link("the index")}.click()`);
  await page.waitFor(`${pageOf} === 'README'`);

  // A page that is missing, and a file outside the guide: a plain message, the dialog stays.
  await page.eval(`${link("A page that is not there")}.click()`);
  await page.waitFor(`${reader}.querySelector('.guide-missing') !== null`);
  assert.match(await page.eval(`${reader}.textContent`), /docs\/user\/guide\/nothing\.md/);
  assert.ok(await page.eval(`${dialog} !== null`));
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page=README]').click()`);
  await page.waitFor(`${reader}.querySelector('h1') !== null`);
  await page.eval(`${link("Outside the guide")}.click()`);
  await page.waitFor(`${reader}.querySelector('.guide-missing') !== null`);
  assert.match(await page.eval(`${reader}.textContent`), /docs\/user\/FEATURES\.md/);

  // An address opens in the browser (here: collected), not in the window.
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page=README]').click()`);
  await page.waitFor(`${reader}.querySelector('h1') !== null`);
  await page.eval(`${link("The site")}.click()`);
  await page.waitFor(`${opened}.length === 1`);
  assert.equal(await page.eval(`${opened}[0]`), "https://example.org/page?a=1&b=2");
  assert.equal(await page.eval(`${dialog} !== null && ${pageOf}`), "README", "the guide stays where it was");

  // The index opens a page directly; Escape closes the guide.
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="databases/postgres"]').click()`);
  await page.waitFor(`${pageOf} === 'databases/postgres'`);
  await closeGuide(page);

  // The "?" menu of the top bar: the same commands, and what they do.
  await page.eval(`document.querySelector('.help-button').click()`);
  await page.waitFor(`document.querySelector('.context-menu') !== null`);
  assert.deepEqual(await page.eval(menuEntries), ["User guide", "Keyboard shortcuts", "What's new", "Report a bug…"]);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.context-menu') === null`);

  await helpMenu(page, "Report a bug…");
  await page.waitFor(`${opened}.length === 2`);
  assert.equal(await page.eval(`${opened}[1]`), "https://github.com/platypus-labs/platypus-db/issues");

  await helpMenu(page, "Keyboard shortcuts");
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  await page.waitFor(`document.querySelector('.settings-body .shortcuts-scope, .settings-body table') !== null`);
  assert.equal(await page.eval(`document.querySelector('.settings-head h3').textContent`), "Keyboard shortcuts");
  assert.equal(await page.eval(`document.querySelector('.settings-head .help-link')`), null, "no \"?\" on a page with no guide page");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  // What's new: the changelog section of the running version.
  const version = /^version = "([^"]+)"/m.exec(readFileSync(join(repo, "Cargo.toml"), "utf8"))[1];
  await helpMenu(page, "What's new");
  await page.waitFor(`document.querySelector('.whats-new .guide-reader h3, .whats-new .guide-missing') !== null`);
  const heading = await page.eval(`document.querySelector('.whats-new h2').textContent`);
  assert.equal(heading, `What's new in ${version}`);
  const changelog = readFileSync(join(repo, "CHANGELOG.md"), "utf8");
  const section = new RegExp(`^## \\[${version.replaceAll(".", "\\.")}\\][^\\n]*\\n([\\s\\S]*?)(?=^## )`, "m").exec(changelog);
  const shown = await page.eval(`document.querySelector('.whats-new .guide-reader').textContent`);
  if (section) {
    const first = /^### (.+)$/m.exec(section[1])[1];
    assert.ok(shown.includes(first), `the section of ${version} shows "${first}"`);
    assert.ok(!shown.includes("not released yet"), "no note about the unreleased changes");
    assert.ok(!/## \[/.test(shown), "only one section");
  } else {
    assert.ok(shown.includes("not released yet"), shown);
  }
  // Markdown shows as elements, never as the marks it is written with.
  assert.ok(!shown.includes("**") && !shown.includes("](") && !shown.includes("###"), shown.slice(0, 300));
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.whats-new') === null`);

  // The "?" of the connection form lands on the connect page; Escape closes the guide only.
  await editConnection(page, NAME);
  await page.eval(`(() => { const help = document.querySelector('.connection-dialog .help-link'); help.focus(); help.click(); })()`);
  await page.waitFor(`${dialog} !== null`);
  assert.equal(await page.eval(pageOf), "connect");
  assert.equal(await page.eval(`${reader}.querySelector('h1').textContent`), "Connect to a database");
  const onTop = await page.eval(`(() => { const r = ${dialog}.getBoundingClientRect(); return document.elementFromPoint(r.x + r.width / 2, r.y + r.height / 2).closest('.guide-dialog') !== null; })()`);
  assert.ok(onTop, "the guide is above the form");
  await closeGuide(page);
  assert.ok(await page.eval(`document.querySelector(".connection-dialog") !== null`), "the form stays open");
  // The focus went back to the form, which answers to a second Escape.
  assert.ok(await page.eval(`document.activeElement?.classList.contains('help-link')`), "the focus is back on the \"?\"");
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.connection-dialog') === null`);

  // Settings: the "?" of Agents and Safety, none elsewhere.
  await page.key(",", "Comma", 188, CTRL);
  await page.waitFor(`document.querySelector('.modal.settings') !== null`);
  for (const [section, wanted] of [["Agents", "agents"], ["Safety", "safety"]]) {
    await page.settingsTab(section);
    await page.waitFor(`document.querySelector('.settings-head .help-link') !== null`);
    await page.eval(`document.querySelector('.settings-head .help-link').click()`);
    await page.waitFor(`${dialog} !== null`);
    assert.equal(await page.eval(pageOf), wanted);
    await closeGuide(page);
    assert.ok(await page.eval(`document.querySelector(".modal.settings") !== null`), "Settings stay open");
  }
  await page.settingsTab("Appearance");
  assert.equal(await page.eval(`document.querySelector('.settings-head .help-link')`), null);
  await page.key("Escape", "Escape", 27);
  await page.waitFor(`document.querySelector('.modal.settings') === null`);

  // The guide that really ships: every page of its index opens, its screenshots load, the
  // reference stays out of the index, and a link to the reference opens it in the dialog.
  await page.eval(`window.__platypus.setGuide(null)`);
  await helpMenu(page, "User guide");
  await page.waitFor(`${dialog} !== null`);
  assert.ok(await page.eval(`document.activeElement === ${reader}`), "the reader has the focus");
  const real = await page.eval(`[...${dialog}.querySelectorAll('.guide-index button')].map(b => b.dataset.page)`);
  for (const wanted of ["README", "getting-started", "connect", "safety", "agents", "databases/README", "databases/postgresql"]) {
    assert.ok(real.includes(wanted), `${wanted} in the index: ${real}`);
  }
  assert.ok(!real.some((key) => key.endsWith(".md")), `the reference is not in the index: ${real}`);
  for (const key of real) {
    await page.eval(`${dialog}.querySelector('.guide-index button[data-page="${key}"]').click()`);
    await page.waitFor(`${pageOf} === ${JSON.stringify(key)} && ${reader}.querySelector('h1') !== null`);
    assert.equal(await page.eval(`${reader}.querySelector('.guide-no-image')?.textContent ?? null`), null, `an image of ${key} is not carried`);
  }
  // The second picture of the page is below the fold, and pictures load when they come near.
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="connect"]').click()`);
  await page.waitFor(`${pageOf} === 'connect' && ${reader}.querySelectorAll('img').length === 2`);
  await page.eval(`[...${reader}.querySelectorAll('img')].at(-1).scrollIntoView()`);
  await page.waitFor(`[...${reader}.querySelectorAll('img')].every(i => i.complete && i.naturalWidth > 0)`);
  await page.eval(`${dialog}.querySelector('.guide-index button[data-page="README"]').click()`);
  await page.waitFor(`${pageOf} === 'README'`);
  await page.eval(`${link("FEATURES.md")}.click()`);
  await page.waitFor(`${pageOf} === 'docs/user/FEATURES.md' && ${reader}.querySelector('h1') !== null`);
  assert.equal(await page.eval(`${reader}.querySelector('.guide-missing')`), null);
  await closeGuide(page);

  // With no guide in the build (an empty one here), the dialog says so instead of failing.
  await page.eval(`window.__platypus.setGuide({})`);
  await helpMenu(page, "User guide");
  await page.waitFor(`${dialog} !== null`);
  assert.ok((await page.eval(`${dialog}.textContent`)).includes("The guide is not included in this build."));
  assert.ok(await page.eval(`${reader}.querySelector(".guide-missing") !== null`));
  await closeGuide(page);
}
