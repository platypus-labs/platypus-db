// Builds the documentation pages of the site (site/docs.html and site/docs/) from the Markdown of
// the guide (docs/guide/**) and of the reference documents the guide links to. The Markdown is
// parsed by the parser of the app's help (apps/desktop/src/help/markdown.ts), which drops raw
// HTML; every text is escaped here and only http, https and mailto links survive. Run it after
// changing any of those documents, then copy the pictures:
//
//   node scripts/build-site-docs.mjs            writes the pages
//   node scripts/build-site-docs.mjs --check    exits 1 when the pages on disk are not what the
//                                               documents produce (nothing is written)
//   node scripts/copy-site-screenshots.mjs      copies the pictures the pages show
//
// The output is the same bytes every time: no dates, no order that depends on the disk.
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, posix, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseMarkdown, plainText } from "../apps/desktop/src/help/markdown.ts";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const siteDir = join(root, "site");
export const REPO = "https://github.com/PaoloCisly/platypus-db";

/** Reference documents turned into pages next to the guide: source, page. */
const EXTRA = [
  ["docs/FEATURES.md", "docs/features.html"],
  ["docs/SHORTCUTS.md", "docs/shortcuts.html"],
  ["docs/AGENTS.md", "docs/agents.html"],
  ["docs/PLUGINS.md", "docs/plugins.html"],
  ["docs/CONNECTIONS_FILE.md", "docs/connections-file.html"],
];
/** The order of the guide's task pages in the menu; a page not listed follows, by name. */
const GUIDE_ORDER = ["connect", "queries", "edit-data", "import-export", "safety", "projects-sharing", "agents", "troubleshooting"];
/** Documents that stay on GitHub, in the menu's last group. */
const ON_GITHUB = [
  ["Benchmarks", "docs/BENCHMARKS.md"],
  ["Extending", "docs/EXTENDING.md"],
  ["Development", "docs/DEVELOPMENT.md"],
  ["Roadmap", "docs/ROADMAP.md"],
  ["Releasing", "docs/RELEASING.md"],
  ["README", "README.md"],
];

/** The page (path below site/) that a source document becomes. */
export function outputOf(src) {
  const extra = EXTRA.find(([s]) => s === src);
  if (extra) return extra[1];
  if (src === "docs/guide/getting-started.md") return "docs.html";
  const below = src.slice("docs/guide/".length).replace(/\.md$/, "");
  return `docs/guide/${below === "README" ? "index" : below.replace(/\/README$/, "/index")}.html`;
}

/** The documents, as repository paths, in a fixed order. */
export function sourcesIn(dir = root) {
  const guide = readdirSync(join(dir, "docs", "guide"), { recursive: true })
    .map((f) => `docs/guide/${String(f).replaceAll("\\", "/")}`)
    .filter((f) => f.endsWith(".md"))
    .sort();
  return [...guide, ...EXTRA.map(([s]) => s)];
}

const ESCAPES = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
/** `text` made safe for element content and for a quoted attribute. */
export const esc = (text) => String(text).replace(/[&<>"']/g, (c) => ESCAPES[c]);

/** The relative address from the page `from` to the file `to`, both below site/. */
function relativeUrl(from, to) {
  return posix.relative(posix.dirname(from), to) || posix.basename(to);
}

/** The prefix (`../`, ...) that leads from the page `out` to the root of the site. */
const rootOf = (out) => "../".repeat(out.split("/").length - 1);

/**
 * Where a link written in the document `src` (shown on the page `out`) goes. `pages` maps the
 * sources that have a page to it. Returns the address, or `null` for a link that must not be
 * one: any scheme but http, https and mailto, or a path leaving the repository.
 */
export function resolveHref(href, src, out, pages) {
  const probe = href.replace(/[\u0000- \u007f-\u009f]/g, "");
  if (probe === "") return null;
  if (probe.startsWith("#")) return probe;
  if (/^[a-z][a-z0-9+.-]*:/i.test(probe)) return /^(https?|mailto):/i.test(probe) ? probe : null;
  if (probe.startsWith("/") || probe.includes("\\")) return null;
  const [path, ...anchor] = probe.split("#");
  const file = (path ?? "").split("?")[0];
  let decoded;
  try {
    decoded = decodeURI(file);
  } catch {
    return null;
  }
  const target = posix.normalize(posix.join(posix.dirname(src), decoded));
  if (target.startsWith("..") || target.startsWith("/")) return null;
  const hash = anchor.length > 0 ? `#${anchor.join("#")}` : "";
  if (pages.has(target)) return relativeUrl(out, pages.get(target)) + hash;
  if (target.startsWith("docs/screenshots/")) return `${rootOf(out)}img/screenshots/${target.slice("docs/screenshots/".length)}`;
  // Anything else the site does not carry stays on GitHub; a name without extension is a folder.
  return `${REPO}/${/\.[a-z0-9]+$/i.test(target) ? "blob" : "tree"}/main/${target}${hash}`;
}

/** The source of a picture in the document `src`, shown on `out`; `null` when the site has not got it. */
export function resolveImage(source, src, out) {
  const probe = source.replace(/[\u0000- ]/g, "");
  if (probe === "" || /^[a-z][a-z0-9+.-]*:/i.test(probe) || probe.startsWith("//") || probe.includes("\\")) return null;
  let target;
  try {
    target = posix.normalize(posix.join(posix.dirname(src), decodeURI(probe.split(/[?#]/)[0])));
  } catch {
    return null;
  }
  if (!target.startsWith("docs/screenshots/")) return null;
  return `${rootOf(out)}img/screenshots/${target.slice("docs/screenshots/".length)}`;
}

/** Renders inline nodes to HTML. `ctx` is `{ src, out, pages }`. */
export function renderInline(nodes, ctx) {
  return nodes
    .map((n) => {
      switch (n.kind) {
        case "text":
          return esc(n.text);
        case "code":
          return `<code>${esc(n.text)}</code>`;
        case "strong":
          return `<strong>${renderInline(n.children, ctx)}</strong>`;
        case "em":
          return `<em>${renderInline(n.children, ctx)}</em>`;
        case "link": {
          const inner = renderInline(n.children, ctx);
          const href = resolveHref(n.href, ctx.src, ctx.out, ctx.pages);
          return href === null ? inner : `<a href="${esc(href)}">${inner}</a>`;
        }
        case "image": {
          const source = resolveImage(n.src, ctx.src, ctx.out);
          return source === null ? esc(n.alt) : `<img src="${esc(source)}" alt="${esc(n.alt)}" loading="lazy">`;
        }
        default:
          return "";
      }
    })
    .join("");
}

const ALIGN = { left: "left", center: "center", right: "right" };

/** Renders blocks to HTML, one block per line. */
export function renderBlocks(blocks, ctx) {
  return blocks
    .map((b) => {
      switch (b.kind) {
        case "heading": {
          const level = Math.min(Math.max(b.level, 1), 6);
          return `<h${level} id="${esc(b.anchor)}">${renderInline(b.children, ctx)}</h${level}>`;
        }
        case "paragraph":
          return `<p>${renderInline(b.children, ctx)}</p>`;
        case "list": {
          const tag = b.ordered ? "ol" : "ul";
          const start = b.ordered && b.start !== 1 ? ` start="${Number(b.start) | 0}"` : "";
          const items = b.items.map((item) => {
            // An item that is one paragraph is written without it.
            const inner = item.length === 1 && item[0].kind === "paragraph" ? renderInline(item[0].children, ctx) : renderBlocks(item, ctx);
            return `<li>${inner}</li>`;
          });
          return `<${tag}${start}>\n${items.join("\n")}\n</${tag}>`;
        }
        case "quote":
          return `<blockquote>\n${renderBlocks(b.children, ctx)}\n</blockquote>`;
        case "code": {
          const lang = /^[\w+-]+$/.test(b.lang) ? ` class="language-${esc(b.lang)}"` : "";
          return `<pre><code${lang}>${esc(b.text)}</code></pre>`;
        }
        case "table": {
          const cell = (tag, inline, i) => {
            const align = ALIGN[b.align[i]] ? ` style="text-align:${ALIGN[b.align[i]]}"` : "";
            return `<${tag}${align}>${renderInline(inline, ctx)}</${tag}>`;
          };
          const head = `<thead><tr>${b.header.map((h, i) => cell("th", h, i)).join("")}</tr></thead>`;
          const rows = b.rows.map((r) => `<tr>${r.map((c, i) => cell("td", c, i)).join("")}</tr>`).join("\n");
          return `<div class="table-wrap"><table class="grid">\n${head}\n<tbody>\n${rows}\n</tbody>\n</table></div>`;
        }
        case "rule":
          return "<hr>";
        default:
          return "";
      }
    })
    .join("\n");
}

/** The text of the first heading of `blocks`, or `fallback`. */
const titleOf = (blocks, fallback) => {
  const h = blocks.find((b) => b.kind === "heading");
  return h ? plainText(h.children) : fallback;
};

/** About 160 characters of the first paragraph that is not a back link, for the description. */
export function describe(blocks) {
  const p = blocks.find((b) => b.kind === "paragraph" && !plainText(b.children).startsWith("←"));
  const text = p ? plainText(p.children).replace(/\s+/g, " ").trim() : "";
  if (text.length <= 160) return text;
  return `${text.slice(0, 157).replace(/\s+\S*$/, "")}…`;
}

/** The menu on the left: the page `out` is the current one. */
function menu(out, entries) {
  const link = (label, href, sub = false) => {
    const here = entries.byLabel.get(label) === out;
    const cls = [sub ? "sub" : "", here ? "on" : ""].filter(Boolean).join(" ");
    return `<a href="${esc(href)}"${cls ? ` class="${cls}"` : ""}${here ? ' aria-current="page"' : ""}>${esc(label)}</a>`;
  };
  const group = (title, items) => `<h6>${esc(title)}</h6>\n${items.join("\n")}`;
  const page = (label, sub) => link(label, relativeUrl(out, entries.byLabel.get(label)), sub);
  const inDatabases = out.startsWith("docs/guide/databases/");
  const parts = [
    group("Start", [page(entries.start, false), page(entries.guide, false)]),
    group("Guide", [...entries.tasks.map((l) => page(l, false)), page(entries.databases, false), ...(inDatabases ? entries.dbPages.map((l) => page(l, true)) : [])]),
    group("Reference", entries.reference.map((l) => page(l, false))),
    group("On GitHub", ON_GITHUB.map(([label, path]) => `<a href="${REPO}/blob/main/${path}">${esc(label)}</a>`)),
  ];
  return parts.join("\n");
}

const NAV_PAGES = [
  ["index.html#tour", "Tour"],
  ["features.html", "Features"],
  ["databases.html", "Databases"],
  ["compare.html", "Compare"],
  ["docs.html", "Docs"],
];

/** The whole page. `body` is HTML, `toc` the headings `[anchor, text]` of the right column. */
function layout({ out, title, description, body, toc, menuHtml, extra }) {
  const up = rootOf(out);
  const nav = NAV_PAGES.map(([href, label]) => `<a href="${up}${href}"${label === "Docs" ? ' aria-current="page"' : ""}>${label}</a>`).join("\n        ");
  const crumbs = out === "docs.html" ? "Start / Getting started" : `Docs / ${esc(title)}`;
  const tocHtml = toc.map(([anchor, text]) => `<a href="#${esc(anchor)}">${esc(text)}</a>`).join("\n    ");
  const f = (path, label) => `<li><a href="${up}${path}">${label}</a></li>`;
  const g = (path, label) => `<li><a href="${REPO}/blob/main/docs/${path}">${label}</a></li>`;
  return `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>PlatypusDB · ${esc(title)}</title>
  <meta name="description" content="${esc(description)}">
  <meta name="color-scheme" content="dark">
  <meta name="theme-color" content="#0b0d0f">
  <link rel="icon" type="image/png" href="${up}img/icon.png">
  <link rel="preload" href="${up}fonts/inter-latin-wght-normal.woff2" as="font" type="font/woff2" crossorigin>
  <link rel="stylesheet" href="${up}style.css">
  <script>document.documentElement.classList.add("js");</script>
</head>
<body>
  <a class="skip" href="#main">Skip to content</a>

  <header class="nav-wrap">
    <div class="nav">
      <a class="brand" href="${up}index.html" aria-label="PlatypusDB, home">
        <img src="${up}img/mark.png" alt="" width="26" height="26">
        <span>PlatypusDB</span>
      </a>
      <nav class="nav-links" aria-label="Sections">
        ${nav}
      </nav>
      <div class="nav-actions">
        <a class="button ghost" href="${REPO}">
          <svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><use href="#gh"/></svg><span>GitHub</span>
        </a>
        <a class="button primary" href="${up}download.html">Download</a>
      </div>
    </div>
  </header>

  <main class="docs" id="main">
  <nav aria-label="Docs">
    <details class="docs-nav" open>
    <summary>Documentation menu</summary>
    ${menuHtml}
    </details>
  </nav>

  <article>
    <div class="crumbs">${crumbs}</div>
${body}${extra}
  </article>

  <aside class="toc-r">
    <div class="lbl">On this page</div>
    ${tocHtml}
  </aside>
</main>

  <footer class="footer">
    <div class="footer-grid">
      <div>
        <a class="brand" href="${up}index.html"><img src="${up}img/mark.png" alt="" width="26" height="26"><span>PlatypusDB</span></a>
        <p>A small, fast desktop SQL client, built with Rust and Tauri.</p>
      </div>
      <nav aria-labelledby="f-docs">
        <h2 id="f-docs">Docs</h2>
        <ul>
          ${f("docs.html", "Getting started")}
          ${f("docs/guide/index.html", "User guide")}
          ${f("docs/features.html", "Features")}
          ${f("docs/shortcuts.html", "Shortcuts")}
          ${f("docs/agents.html", "Agents and the CLI")}
        </ul>
      </nav>
      <nav aria-labelledby="f-project">
        <h2 id="f-project">Project</h2>
        <ul>
          ${g("BENCHMARKS.md", "Benchmarks")}
          ${g("ROADMAP.md", "Roadmap")}
          ${g("EXTENDING.md", "Extending")}
          ${g("RELEASING.md", "Releasing")}
        </ul>
      </nav>
      <nav aria-labelledby="f-get">
        <h2 id="f-get">Get it</h2>
        <ul>
          <li><a href="${REPO}/releases">Releases</a></li>
          <li><a href="${REPO}">Source code</a></li>
          <li><a href="${REPO}/issues">Issues</a></li>
        </ul>
      </nav>
    </div>
    <div class="footer-bottom">
      <span>PlatypusDB · AGPL-3.0</span>
      <span>Inter and JetBrains Mono under the <a href="${up}fonts/OFL-Inter.txt">SIL Open Font License</a>. Database logos from simple-icons (CC0).</span>
    </div>
  </footer>

  <svg class="sprite" width="0" height="0" aria-hidden="true">
    <symbol id="gh" viewBox="0 0 16 16"><path d="M8 0C3.58 0 0 3.580 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z"/></symbol>
  </svg>

  <script src="${up}site.js" defer></script>
</body>
</html>
`;
}

/** The index of every generated page, shown under Getting started on docs.html. */
function indexSection(out, entries) {
  const item = (label) => `<li><a href="${esc(relativeUrl(out, entries.byLabel.get(label)))}">${esc(label)}</a>${entries.blurbs.get(label) ? ` — ${esc(entries.blurbs.get(label))}` : ""}</li>`;
  const list = (title, labels) => `<h3>${esc(title)}</h3>\n<ul>\n${labels.map(item).join("\n")}\n</ul>`;
  return `\n<h2 id="all-pages">All the pages</h2>\n${list("User guide", [entries.guide, ...entries.tasks, entries.databases])}\n${list("Databases", entries.dbPages)}\n${list("Reference", entries.reference)}`;
}

/** The short names of the database pages, from the links in the first column of the databases page's table. */
function databaseNames(blocks, pages) {
  const names = new Map();
  const table = blocks.find((b) => b.kind === "table");
  for (const row of table?.rows ?? []) {
    const link = row[0]?.find((n) => n.kind === "link");
    const src = link && posix.join("docs/guide/databases", link.href);
    if (src && pages.has(src)) names.set(src, plainText(link.children));
  }
  return names;
}

/**
 * Every generated file as `{ path: html }`, from the documents read by `read(src)`.
 * `sources` are the repository paths of the documents.
 */
export function buildSite(sources, read) {
  const pages = new Map(sources.map((s) => [s, outputOf(s)]));
  const parsed = new Map(sources.map((s) => [s, parseMarkdown(read(s))]));
  const dbNames = databaseNames(parsed.get("docs/guide/databases/README.md") ?? [], pages);
  const label = (s) => dbNames.get(s) ?? titleOf(parsed.get(s), s);
  const byLabel = new Map();
  const blurbs = new Map();
  for (const s of sources) {
    if (byLabel.has(label(s))) throw new Error(`two pages are called "${label(s)}": ${s}`);
    byLabel.set(label(s), pages.get(s));
    blurbs.set(label(s), describe(parsed.get(s)));
  }
  const guideTasks = sources.filter((s) => /^docs\/guide\/[^/]+\.md$/.test(s) && !/\/(README|getting-started)\.md$/.test(s));
  const rank = (s) => {
    const i = GUIDE_ORDER.indexOf(s.slice("docs/guide/".length, -3));
    return i < 0 ? GUIDE_ORDER.length : i;
  };
  guideTasks.sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
  const dbOrder = [...dbNames.keys()];
  const dbRank = (s) => (dbOrder.includes(s) ? dbOrder.indexOf(s) : dbOrder.length);
  const entries = {
    byLabel,
    blurbs,
    start: label("docs/guide/getting-started.md"),
    guide: label("docs/guide/README.md"),
    tasks: guideTasks.map(label),
    databases: label("docs/guide/databases/README.md"),
    dbPages: sources.filter((s) => /^docs\/guide\/databases\/(?!README)[^/]+\.md$/.test(s)).sort((a, b) => dbRank(a) - dbRank(b) || a.localeCompare(b)).map(label),
    reference: EXTRA.map(([s]) => label(s)),
  };
  const files = {};
  for (const s of sources) {
    const out = pages.get(s);
    const blocks = parsed.get(s);
    const ctx = { src: s, out, pages };
    const toc = blocks.filter((b) => b.kind === "heading" && b.level === 2).map((b) => [b.anchor, plainText(b.children)]);
    const isStart = out === "docs.html";
    if (isStart) toc.push(["all-pages", "All the pages"]);
    files[out] = layout({
      out,
      title: titleOf(blocks, s),
      description: describe(blocks) || label(s),
      body: renderBlocks(blocks, ctx),
      extra: isStart ? indexSection(out, entries) : "",
      toc,
      menuHtml: menu(out, entries),
    });
  }
  return files;
}

/** Pages below site/docs/ on disk, as paths below site/. */
function generatedOnDisk() {
  const dir = join(siteDir, "docs");
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { recursive: true })
    .map((f) => `docs/${String(f).replaceAll("\\", "/")}`)
    .filter((f) => f.endsWith(".html"))
    .sort();
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const files = buildSite(sourcesIn(), (s) => readFileSync(join(root, s), "utf8"));
  if (process.argv.includes("--check")) {
    const stale = [];
    for (const [path, html] of Object.entries(files)) {
      const file = join(siteDir, path);
      if (!existsSync(file) || readFileSync(file, "utf8").replaceAll("\r\n", "\n") !== html) stale.push(path);
    }
    for (const path of generatedOnDisk()) if (!(path in files)) stale.push(`${path} (no document for it)`);
    if (stale.length > 0) {
      console.error(`out of step with the documents: ${stale.join(", ")}\nrun: node scripts/build-site-docs.mjs`);
      process.exit(1);
    }
    console.log(`site docs ok: ${Object.keys(files).length} pages`);
  } else {
    for (const [path, html] of Object.entries(files)) {
      mkdirSync(dirname(join(siteDir, path)), { recursive: true });
      writeFileSync(join(siteDir, path), html);
    }
    const gone = generatedOnDisk().filter((p) => !(p in files));
    for (const p of gone) rmSync(join(siteDir, p));
    console.log(`wrote ${Object.keys(files).length} pages${gone.length > 0 ? `, removed ${gone.join(", ")}` : ""}`);
  }
}
