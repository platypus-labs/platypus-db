# PlatypusDB site

A static presentation of PlatypusDB, a home page and five pages (features, databases, compare,
download, docs): plain HTML and CSS with one small script for the tour tabs, the fade-in of a few
blocks and the name of the visitor's system on the download button. The pages under `docs/` and
`docs.html` are generated from the Markdown in `docs/` (see Update); the others are written by hand.
No build tool, nothing loaded from other sites.

```
index.html      the home page
features.html   every feature, area by area
databases.html  the supported databases and how each connects
compare.html    size, license and features next to other SQL clients
download.html   installers per system and building from source
docs.html       getting started and the index of the docs; generated
docs/           the user guide (guide/, with databases/), features, shortcuts, agents, plugins and
                the connections file as pages; generated, do not edit by hand
style.css       styles; dark only, colors from apps/desktop/src/theme.ts (Platypus Dark)
site.js         tour as tabs (without it every screenshot shows, one under the other)
fonts/          Inter and JetBrains Mono, copied from apps/desktop/src/assets/fonts
img/            icon, the platypus (mark.png, from docs/brand/logo-only-1024.png), database
                logos (databases.svg, from simple-icons as in the app's DriverPicker) and
                the logos of other SQL clients for Compare (clients.svg, from simple-icons; the
                app icons in clients/ from tableplus.com, heidisql.com and the pgAdmin repository) and
                the screenshots and GIFs from docs/screenshots that the page shows
.nojekyll       tells GitHub Pages to serve the folder as it is
```

## Preview

Open `index.html` in a browser. To test it as it will be served (absolute paths, MIME types),
serve the folder instead, for example:

```bash
python -m http.server -d site 8000   # then http://localhost:8000
```

The page is dark only. The window at the top is drawn in HTML and CSS: its query types itself
and the result appears, all in CSS animations. When the system asks for less motion the query is
already typed; the GIFs of the tour play anyway, since they are what the tour shows.

## Update

The text follows the README and [docs/user/FEATURES.md](../docs/user/FEATURES.md), the source of truth for features. When the screenshots or GIFs in
`docs/screenshots/` change, run `node scripts/copy-site-screenshots.mjs`: it copies the ones the pages show into `img/screenshots/`.

The docs pages come from `docs/user/guide/**/*.md`, FEATURES.md, SHORTCUTS.md, AGENTS.md, PLUGINS.md
and CONNECTIONS_FILE.md. After changing any of them, run, from the root:

```bash
node scripts/build-site-docs.mjs        # writes docs.html and docs/; the same bytes every run
node scripts/copy-site-screenshots.mjs  # the pictures those pages show
node scripts/check-links.mjs
```

`node scripts/build-site-docs.mjs --check` writes nothing and exits 1 when the pages are not what
the documents produce. The Markdown is read by the parser of the app's help, which drops raw HTML;
all text is escaped and only `http`, `https` and `mailto` links are kept. A link to another
document that has a page becomes a link to that page; one to a file the site does not carry (the
README, the developer documents, source folders) goes to GitHub, which answers 404 until the
repository is public. The menu of a page and the index in `docs.html` list the pages by themselves.

## Publish on GitHub Pages

`.github/workflows/pages.yml` publishes `site/` as it is. It runs only when started by hand
(*Actions › Pages › Run workflow*, or `gh workflow run pages.yml`), never on a push. It first runs
`node scripts/build-site-docs.mjs --check` and `node scripts/check-links.mjs`, so a site that is
out of step with the documents or has a broken link is not published. Its permissions are
`contents: read` for the check and `pages: write` and `id-token: write` for the deploy job only.

Not done yet, and not done by the workflow: *Settings › Pages › Build and deployment › Source*
must be set to *GitHub Actions*, once. Pages on a private repository needs a paid GitHub plan, and
the published site is public even when the repository is not; the repository is private today, so
the workflow has not been run.

**Domain: to be decided.** Until then the site is at `https://<owner>.github.io/platypus-db/`.
The pages use relative addresses, so they work under that path and under a domain of their own.
A custom domain needs a `CNAME` file in `site/` (or the domain set in the Pages settings), a DNS
record, and the addresses in the repository's *Website* field and in the README updated.
