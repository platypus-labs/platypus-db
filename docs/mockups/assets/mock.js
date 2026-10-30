// Group R mockups: icons, the app shell around each page, and the mockup bar (theme, notes).
// One icon family: 24px grid, 1.6 stroke, round caps, as the redesign proposes.

const ICONS = {
  database: '<ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v14c0 1.7 3.6 3 8 3s8-1.3 8-3V5"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>',
  table: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 10h18M3 15h18M9 10v10"/>',
  view: '<path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  fn: '<path d="M14 4h-1a3 3 0 0 0-3 3v10a3 3 0 0 1-3 3H6M7 11h8"/>',
  folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
  schema: '<path d="M12 3 3 8l9 5 9-5-9-5z"/><path d="m3 13 9 5 9-5"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  x: '<path d="M6 6l12 12M18 6 6 18"/>',
  play: '<path d="M7 4v16l13-8z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="1.5"/>',
  save: '<path d="M5 3h11l4 4v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M8 3v5h8M8 21v-7h8v7"/>',
  open: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v1H7l-4 9z"/><path d="M3 19l3-9h17l-3 9z"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
  history: '<path d="M3 12a9 9 0 1 0 3-6.7L3 8"/><path d="M3 3v5h5M12 7v5l3 2"/>',
  right: '<path d="m9 6 6 6-6 6"/>',
  down: '<path d="m6 9 6 6 6-6"/>',
  left: '<path d="m15 6-6 6 6 6"/>',
  filter: '<path d="M3 5h18l-7 8v6l-4 2v-8z"/>',
  refresh: '<path d="M21 12a9 9 0 0 1-15.5 6.3L3 16M3 12a9 9 0 0 1 15.5-6.3L21 8"/><path d="M21 3v5h-5M3 21v-5h5"/>',
  download: '<path d="M12 4v12m-5-5 5 5 5-5M4 20h16"/>',
  upload: '<path d="M12 20V8m-5 5 5-5 5 5M4 4h16"/>',
  key: '<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9M17 6l3 3M15 8l2 2"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  more: '<circle cx="5" cy="12" r="1.2"/><circle cx="12" cy="12" r="1.2"/><circle cx="19" cy="12" r="1.2"/>',
  check: '<path d="m5 12 5 5 9-10"/>',
  alert: '<path d="M12 3 2 20h20z"/><path d="M12 10v4M12 17.5v.01"/>',
  error: '<circle cx="12" cy="12" r="9"/><path d="M12 7v6M12 16.5v.01"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>',
  lock: '<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>',
  shield: '<path d="M12 3 4 6v6c0 5 3.5 8 8 9 4.5-1 8-4 8-9V6z"/>',
  bot: '<rect x="4" y="8" width="16" height="12" rx="3"/><path d="M12 4v4M9 14h.01M15 14h.01M2 13v3M22 13v3"/>',
  users: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0"/><path d="M16 4.5a3.5 3.5 0 0 1 0 7M18 14a6 6 0 0 1 3.5 6"/>',
  activity: '<path d="M3 12h4l3-8 4 16 3-8h4"/>',
  chart: '<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>',
  erd: '<rect x="3" y="3" width="7" height="6" rx="1"/><rect x="14" y="15" width="7" height="6" rx="1"/><rect x="14" y="3" width="7" height="6" rx="1"/><path d="M10 6h4M17.5 9v6"/>',
  file: '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
  code: '<path d="m8 8-4 4 4 4M16 8l4 4-4 4M14 5l-4 14"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3"/>',
  copy: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3"/>',
  columns: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>',
  server: '<rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/>',
  plug: '<path d="M9 2v5M15 2v5M6 7h12v4a6 6 0 0 1-12 0zM12 17v5"/>',
  palette: '<path d="M12 3a9 9 0 1 0 0 18c1.2 0 2-.8 2-2 0-.5-.2-1-.5-1.3-.3-.4-.5-.8-.5-1.3 0-1.1.9-2 2-2h2.4A4.6 4.6 0 0 0 22 9.8C22 6 17.5 3 12 3z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10" cy="7" r="1"/><circle cx="15" cy="7" r="1"/>',
  keyboard: '<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10"/>',
  globe: '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>',
  terminal: '<path d="m5 8 4 4-4 4M12 17h7"/>',
  external: '<path d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/>',
  list: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
  bell: '<path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4z"/><path d="M10 21h4"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>',
  compare: '<circle cx="6" cy="6" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5V15a3 3 0 0 0 3 3h6.5M18 15.5V9a3 3 0 0 0-3-3H8.5"/>',
  zap: '<path d="M13 2 4 14h7l-1 8 9-12h-7z"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  panel: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>',
  panelr: '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M15 4v16"/>',
  sliders: '<path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"/><circle cx="16" cy="6" r="2"/><circle cx="10" cy="12" r="2"/><circle cx="18" cy="18" r="2"/>',
  cpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9 2v4M15 2v4M9 18v4M15 18v4M2 9h4M2 15h4M18 9h4M18 15h4"/>',
  sparkle: '<path d="M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5 18 18M6 18l2.5-2.5M15.5 8.5 18 6"/>',
  layers: '<path d="M12 3 3 8l9 5 9-5-9-5z"/><path d="m3 13 9 5 9-5"/>',
  wand: '<path d="m4 20 12-12M14 4v2M18 8h2M17 3l1 1M20 6l1 1"/>',
  note: '<path d="M5 3h10l4 4v14H5z"/><path d="M9 9h6M9 13h6M9 17h3"/>',
  pin: '<path d="M12 17v5M8 3h8l-1 6 3 3v2H6v-2l3-3z"/>',
  github: '<path d="M9 19c-4 1.5-4-2-6-2.5m12 5v-3.5a3 3 0 0 0-.9-2.4c3-.3 6.1-1.5 6.1-6.6A5.2 5.2 0 0 0 18.8 5 4.8 4.8 0 0 0 18.7 1.4S17.5 1 15 2.7a13.4 13.4 0 0 0-6 0C6.5 1 5.3 1.4 5.3 1.4A4.8 4.8 0 0 0 5.2 5a5.2 5.2 0 0 0-1.4 3.6c0 5.1 3.1 6.3 6.1 6.6a3 3 0 0 0-.9 2.4V22"/>',
  windows: '<path d="M3 5.5 10.5 4.5v7H3zM12 4.3 21 3v8.5h-9zM3 13h7.5v7L3 19zM12 13h9v8l-9-1.3z"/>',
  apple: '<path d="M16.5 3c0 1.5-1.2 3-2.8 3 0-1.6 1.3-3 2.8-3zM19 16.5c-.8 1.9-2 3.9-3.6 3.9-1.4 0-1.8-.8-3.4-.8s-2.1.8-3.4.8C6.9 20.4 4 16.3 4 12.4 4 9 6.2 7.3 8.3 7.3c1.4 0 2.4.9 3.6.9 1.1 0 2.4-1 4-1 .6 0 2.6.1 3.8 2-3.3 1.8-2.7 6.4-.7 7.3z"/>',
  minus: '<path d="M5 12h14"/>',
  maximize: '<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>',
  pause: '<rect x="6" y="5" width="4" height="14" rx="1"/><rect x="14" y="5" width="4" height="14" rx="1"/>',
  grid: '<rect x="3" y="3" width="8" height="8" rx="1.5"/><rect x="13" y="3" width="8" height="8" rx="1.5"/><rect x="3" y="13" width="8" height="8" rx="1.5"/><rect x="13" y="13" width="8" height="8" rx="1.5"/>',
  cloud: '<path d="M7 18a4.5 4.5 0 0 1-.6-8.96A6 6 0 0 1 18 10a4 4 0 0 1 0 8z"/>',
  ban: '<circle cx="12" cy="12" r="9"/><path d="m5.6 5.6 12.8 12.8"/>',
  linux: '<path d="M12 3c-2.2 0-3.5 1.8-3.5 4.5 0 2-1.5 3.5-2.5 5.5S5 17 7 18.5 10 21 12 21s3-1 5-2.5 1-3.5 0-5.5-2.5-3.5-2.5-5.5C14.5 4.8 14.2 3 12 3z"/>',
};

function icons(root = document) {
  root.querySelectorAll("i[data-i]").forEach((el) => {
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("class", `i ${el.className}`.trim());
    svg.setAttribute("aria-hidden", "true");
    svg.innerHTML = ICONS[el.dataset.i] || ICONS.x;
    el.replaceWith(svg);
  });
}

const LOGO = "../assets/img/platypus-mark.png";

// Default navigator: project "Shop" with three connections, driver icons in place of dots.
const NAVIGATOR = `
  <div class="side-head"><span class="title">Shop</span>
    <button class="btn icon sm" title="Nuova connessione"><i data-i="plus" class="sm"></i></button>
    <button class="btn icon sm" title="Importa"><i data-i="download" class="sm"></i></button>
    <button class="btn icon sm" title="Altro"><i data-i="more" class="sm"></i></button></div>
  <div class="input search filter"><i data-i="search" class="sm"></i>Filtra oggetti…</div>
  <div class="tree">
    <div class="node l1"><span class="tw"><i data-i="down" class="sm"></i></span>
      <span class="drv pg">Pg<span class="mark" style="background:var(--ok)"></span><span class="ping ok"></span></span>
      <span>Shop dev</span><span class="badge dev">DEV</span></div>
    <div class="node l2"><span class="tw"><i data-i="down" class="sm"></i></span><i data-i="schema" class="sm ic-folder"></i>public</div>
    <div class="node l3"><span class="tw"><i data-i="down" class="sm"></i></span><i data-i="folder" class="sm ic-folder"></i>Tabelle<span class="cnt">4</span></div>
    <div class="node l4"><span class="tw"></span><i data-i="table" class="sm ic-table"></i>customers</div>
    <div class="node l4"><span class="tw"></span><i data-i="table" class="sm ic-table"></i>order_items</div>
    <div class="node l4 sel"><span class="tw"></span><i data-i="table" class="sm ic-table"></i>orders</div>
    <div class="node l4"><span class="tw"></span><i data-i="table" class="sm ic-table"></i>products</div>
    <div class="node l3"><span class="tw"><i data-i="right" class="sm"></i></span><i data-i="folder" class="sm ic-folder"></i>Viste<span class="cnt">1</span></div>
    <div class="node l3"><span class="tw"><i data-i="right" class="sm"></i></span><i data-i="folder" class="sm ic-folder"></i>Funzioni<span class="cnt">3</span></div>
    <div class="node l1"><span class="tw"><i data-i="right" class="sm"></i></span>
      <span class="drv pg">Pg<span class="mark" style="background:var(--error)"></span><span class="ping off"></span></span>
      <span>Shop prod</span><span class="badge prod">PROD</span></div>
    <div class="node l1"><span class="tw"><i data-i="right" class="sm"></i></span>
      <span class="drv duck">Dk<span class="ping off"></span></span><span>analytics.duckdb</span></div>
    <div class="node l1"><span class="tw"><i data-i="right" class="sm"></i></span>
      <span class="drv my">My<span class="ping err"></span></span><span>Legacy CRM</span><span class="cnt" style="color:var(--error)">errore</span></div>
  </div>
  <div class="side-sec">
    <div class="side-head"><span class="title" style="font-weight:600">Script</span>
      <button class="btn icon sm" title="Nuovo script"><i data-i="plus" class="sm"></i></button></div>
    <div class="tree" style="flex:none;padding-bottom:var(--sp-3)">
      <div class="node l1"><span class="tw"></span><i data-i="file" class="sm ic-folder"></i>revenue_by_city.sql</div>
      <div class="node l1"><span class="tw"></span><i data-i="file" class="sm ic-folder"></i>cleanup_orders.sql</div>
    </div>
  </div>`;

function shell() {
  const b = document.body;
  if (!b.dataset.shell) return;
  const main = document.querySelector(".main");
  const sideTpl = document.getElementById("side");
  const railTpl = document.getElementById("rail");
  const crumb = b.dataset.crumb || '<b>Shop</b><span>/</span><span class="dot ok"></span><span>Shop dev</span>';
  const app = document.createElement("div");
  app.className = "app" + (b.dataset.shell === "noside" ? " no-side" : "") + (b.dataset.env ? ` env-${b.dataset.env}` : "");
  app.innerHTML = `
    <div class="top">
      <img class="logo" src="${LOGO}" alt="">
      <button class="btn icon sm" title="Mostra o nascondi il navigator"><i data-i="panel" class="sm"></i></button>
      <div class="crumb">${crumb}</div>
      <div class="omni"><i data-i="search" class="sm"></i>Cerca tabelle, query, comandi<kbd>Ctrl K</kbd></div>
      <button class="btn icon sm" title="Attività"><i data-i="activity" class="sm"></i></button>
      <button class="btn icon sm" title="Agenti"><i data-i="bot" class="sm"></i></button>
      <button class="btn icon sm" title="Impostazioni"><i data-i="settings" class="sm"></i></button>
      <div class="win"><span>&#x2013;</span><span>&#x25A1;</span><span>&#x2715;</span></div>
    </div>
    ${railTpl ? `<nav class="rail" aria-label="Progetti">${railTpl.innerHTML}</nav>` : `    <nav class="rail" aria-label="Progetti">
      <div class="sq all" title="Tutte le connessioni"><i data-i="database"></i></div>
      <div class="div"></div>
      <div class="sq on" style="background:#c46a14" title="Shop">SH<span class="tx"></span></div>
      <div class="sq" style="background:#1d7f74" title="Analytics">AN</div>
      <div class="sq" style="background:#4b5bb8" title="Clienti">CL</div>
      <div class="sq add" title="Nuovo progetto"><i data-i="plus"></i></div>
      <div class="foot"><button class="btn icon" title="Aiuto"><i data-i="keyboard"></i></button></div>
    </nav>`}
    <aside class="side">${sideTpl ? sideTpl.innerHTML : NAVIGATOR}</aside>`;
  app.appendChild(main);
  b.prepend(app);
}

function mockBar() {
  const bar = document.createElement("div");
  bar.className = "mk-bar";
  const root = document.documentElement;
  const saved = localStorageGet("mk-theme");
  if (saved) root.dataset.theme = saved;
  bar.innerHTML = `<a href="${b().dataset.index || "../index.html"}">&#8592; Indice</a>
    <button class="btn sm ghost" data-act="theme"><i data-i="sun" class="sm"></i>Tema</button>
    <button class="btn sm ghost" data-act="notes"><i data-i="note" class="sm"></i>Note</button>`;
  bar.addEventListener("click", (e) => {
    const act = e.target.closest("button")?.dataset.act;
    if (act === "theme") {
      root.dataset.theme = root.dataset.theme === "light" ? "dark" : "light";
      localStorageSet("mk-theme", root.dataset.theme);
    }
    if (act === "notes") document.body.classList.toggle("mk-hide-notes");
  });
  document.body.appendChild(bar);
}

function b() { return document.body; }
function localStorageGet(k) { try { return localStorage.getItem(k); } catch { return null; } }
function localStorageSet(k, v) { try { localStorage.setItem(k, v); } catch { /* private window */ } }

shell();
mockBar();
icons();
