# PlatypusDB site

A static presentation of PlatypusDB, a home page and five pages (features, databases, compare,
download, docs): plain HTML and CSS with one small script for the tour tabs, the fade-in of a few
blocks and the name of the visitor's system on the download button. No build step, nothing loaded from other
sites.

```
index.html      the home page
features.html   every feature, area by area
databases.html  the supported databases and how each connects
compare.html    size, license and features next to other SQL clients
download.html   installers per system and building from source
docs.html       getting started, and an index of the Markdown docs on GitHub
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

The text follows the README and [docs/FEATURES.md](../docs/FEATURES.md), the source of truth for features. When the screenshots or GIFs in
`docs/screenshots/` change, run `node scripts/copy-site-screenshots.mjs`: it copies the ones the pages show into `img/screenshots/`.

## Publish on GitHub Pages

Without a workflow, GitHub Pages serves the root of a branch or its `/docs` folder, not `/site`
(and `docs/` here holds the project's documents). So the site goes on its own branch, with the
content of `site/` at its root:

```bash
git push origin "$(git subtree split --prefix site main)":refs/heads/gh-pages
```

Then *Settings › Pages › Build and deployment*: Source *Deploy from a branch*, branch
`gh-pages`, folder `/ (root)`. After changing the site on `main`, run the same command again:
the split gives the same history plus the new commits, so the push is a fast-forward.

The site is then at `https://<owner>.github.io/platypus-db/`. Pages on a private repository
needs a paid GitHub plan, and the published site is public even when the repository is not.
