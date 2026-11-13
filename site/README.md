# PlatypusDB site

A static presentation of PlatypusDB, a home page and five pages (features, databases, compare,
download, docs): plain HTML and CSS with one small script for the tour tabs, the fade-in of a few
blocks and the name of the visitor's system on the download button. The pages under `docs/` and
`docs.html` are generated from the Markdown in `docs/` (see Update); the others are written by hand.
No build tool, nothing loaded from other sites.

```
index.html      the home page, with home.css and home.js (the hero, the tour, the platypus)
features.html   every feature, area by area
databases.html  the supported databases and how each connects
compare.html    size, license and features next to other SQL clients
download.html   installers per system and building from source
docs.html       getting started and the index of the docs; generated
docs/           the user guide (guide/, with databases/), features, shortcuts, agents, plugins and
                the connections file as pages; generated, do not edit by hand
style.css       styles; dark only, colors from apps/desktop/src/theme.ts (Platypus Dark)
site.js         tour as tabs on the other pages, the visitor's system on the download buttons, the
                navigation's background, the light under the pointer on the cards
home.css        the home page only, on top of style.css
home.js         the home page only: the hero title (the speed one on a first visit, then another
                at random), the chapters of the tour following the scroll, the rail, and the
                platypus built from img/parts/ (hop, eye, facts about the sample database, and
                a rare phone animation, PHONE in the script)
fonts/          Inter and JetBrains Mono, copied from apps/desktop/src/assets/fonts
img/            icon (icon.png, from docs/brand/app-icon.png), the platypus (mark.png, from
                docs/brand/logo-only-1024.png; both written by scripts/brand-assets.mjs), database
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

The pages are dark only. On the home page the app window stays in place while the tour's
chapters change with the scroll, and the platypus sits on it: its layers drop in, it hops on each
chapter, follows the pointer and tells facts about the sample database (`samples/wildlife.sql`;
keep them true if that file changes). The mock it was built from is
`docs/mockups/site/home-v2.html`. When the system asks for less motion nothing moves; the GIFs of
the tour play anyway, since they are what the tour shows.

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

*Settings › Pages › Build and deployment › Source* is set to *GitHub Actions* (done once, on
2026-11-13, when the repository became public). Run the workflow again after changing the site.

**Domain: to be decided.** Until then the site is at
[platypus-labs.github.io/platypus-db](https://platypus-labs.github.io/platypus-db/), also the
repository's *Website*.
The pages use relative addresses, so they work under that path and under a domain of their own.
A custom domain needs a `CNAME` file in `site/` (or the domain set in the Pages settings), a DNS
record, and the addresses in the repository's *Website* field and in the README updated.
