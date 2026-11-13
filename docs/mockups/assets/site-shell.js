// Group R mockups: the current site's header and footer, for the inner pages.
// Load before mock.js, which turns the <i data-i> placeholders into icons.

(function siteShell() {
  const page = document.body.dataset.page || "";
  const GH = "https://github.com/platypus-labs/platypus-db";
  const link = (href, label, id) => `<a href="${href}"${id === page ? ' class="on" aria-current="page" style="color:var(--hi)"' : ""}>${label}</a>`;
  const gh = '<svg class="icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38v-1.49c-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.5 7.5 0 0 1 4 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 16 8c0-4.42-3.58-8-8-8z"/></svg>';

  const head = document.createElement("header");
  head.className = "nav-wrap";
  head.innerHTML = `<div class="nav">
      <a class="brand" href="home.html"><img src="base/img/mark.png" alt="" width="26" height="26"><span>PlatypusDB</span></a>
      <nav class="nav-links" aria-label="Sections">
        ${link("home.html#tour", "Tour", "")}${link("features.html", "Features", "features")}${link("databases.html", "Databases", "databases")}${link("compare.html", "Compare", "compare")}${link("docs.html", "Docs", "docs")}
      </nav>
      <div class="nav-actions">
        <a class="button ghost" href="${GH}">${gh}<span>GitHub</span></a>
        <a class="button primary" href="download.html">Download</a>
      </div></div>`;
  document.body.prepend(head);

  const foot = document.createElement("footer");
  foot.className = "footer";
  foot.innerHTML = `<div class="footer-grid">
      <div><a class="brand" href="home.html"><img src="base/img/mark.png" alt="" width="26" height="26"><span>PlatypusDB</span></a>
        <p>A small, fast desktop SQL client, built with Rust and Tauri.</p></div>
      <nav><h2>Docs</h2><ul><li><a href="docs.html">Getting started</a></li><li><a href="features.html">Features</a></li><li><a href="docs.html">Shortcuts</a></li><li><a href="docs.html">Agents and the CLI</a></li></ul></nav>
      <nav><h2>Project</h2><ul><li><a href="${GH}">Benchmarks</a></li><li><a href="${GH}">Roadmap</a></li><li><a href="${GH}">Extending</a></li><li><a href="${GH}">Releasing</a></li></ul></nav>
      <nav><h2>Get it</h2><ul><li><a href="download.html">Download</a></li><li><a href="${GH}">Source code</a></li><li><a href="${GH}/issues">Issues</a></li></ul></nav>
    </div>
    <div class="footer-bottom"><span>PlatypusDB · AGPL-3.0</span><span>Inter and JetBrains Mono under the SIL Open Font License.</span></div>`;
  document.body.appendChild(foot);
})();
