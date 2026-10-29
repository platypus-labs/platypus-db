# PlatypusDB site

A static, single-page presentation of PlatypusDB: plain HTML and CSS with one small script for
the screenshot tour and the fade-in of a few blocks. No build step, nothing loaded from other
sites.

```
index.html      the page
style.css       styles; dark first, colors from apps/desktop/src/theme.ts (Platypus Dark and Light)
site.js         screenshot tour as tabs (without it every screenshot shows, one under the other)
fonts/          Inter and JetBrains Mono, copied from apps/desktop/src/assets/fonts
img/            logo, icon, database logos (databases.svg, from simple-icons as in the app's
                DriverPicker) and the screenshots and GIFs from docs/screenshots
.nojekyll       tells GitHub Pages to serve the folder as it is
```

## Preview

Open `index.html` in a browser. To test it as it will be served (absolute paths, MIME types),
serve the folder instead, for example:

```bash
python -m http.server -d site 8000   # then http://localhost:8000
```

The page follows the system's light or dark mode. The GIFs show only when the system does not ask
for less motion; otherwise the PNG of the same screen shows.

## Update

The text follows the README and [docs/FEATURES.md](../docs/FEATURES.md), the source of truth for features. When the screenshots or GIFs in
`docs/screenshots/` change, copy them into `img/screenshots/` again.

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
