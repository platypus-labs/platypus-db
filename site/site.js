// The tour as tabs: one window at a time; the arrows and Home/End move between them.
// Without this script every screenshot shows, one under the other.
(function () {
  var list = document.querySelector(".tour [role=tablist]");
  if (!list) return;
  var tabs = Array.prototype.slice.call(list.querySelectorAll("[role=tab]"));

  function select(index, focus) {
    tabs.forEach(function (tab, i) {
      var on = i === index;
      tab.setAttribute("aria-selected", on ? "true" : "false");
      tab.tabIndex = on ? 0 : -1;
      document.getElementById(tab.getAttribute("aria-controls")).hidden = !on;
    });
    if (focus) {
      tabs[index].focus();
      tabs[index].scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }

  tabs.forEach(function (tab, i) {
    tab.addEventListener("click", function () {
      select(i, false);
    });
  });

  list.addEventListener("keydown", function (e) {
    var at = tabs.indexOf(document.activeElement);
    if (at < 0) return;
    var next = { ArrowRight: at + 1, ArrowDown: at + 1, ArrowLeft: at - 1, ArrowUp: at - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (next === undefined) return;
    e.preventDefault();
    select((next + tabs.length) % tabs.length, true);
  });

  // A column beside the shots on wide screens, a row above them on narrow ones (style.css).
  var narrow = window.matchMedia("(max-width: 1000px)");
  function orient() {
    list.setAttribute("aria-orientation", narrow.matches ? "horizontal" : "vertical");
  }
  if (narrow.addEventListener) narrow.addEventListener("change", orient);
  orient();

  list.hidden = false;
  select(0, false);
})();

// Blocks marked `reveal` fade in the first time they come into view.
(function () {
  var blocks = document.querySelectorAll(".reveal");
  if (!("IntersectionObserver" in window)) {
    blocks.forEach(function (b) { b.classList.add("seen"); });
    return;
  }
  var watcher = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (!e.isIntersecting) return;
      e.target.classList.add("seen");
      watcher.unobserve(e.target);
    });
  }, { rootMargin: "0px 0px -10% 0px" });
  blocks.forEach(function (b) { watcher.observe(b); });
})();

// The download button names the visitor's system, with its logo. On Windows it downloads the
// installer (data-windows, written by scripts/site-release.mjs); macOS and Linux have several
// files, so it opens their list on the download page.
(function () {
  var p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  var os = /win/i.test(p) ? "Windows" : /mac/i.test(p) ? "macOS" : /linux/i.test(p) ? "Linux" : "";
  if (!os) return;
  var logos = {
    Windows: "M3 5.5 10.5 4.5v7H3zM12 4.3 21 3v8.5h-9zM3 13h7.5v7L3 19zM12 13h9v8l-9-1.3z",
    macOS: "M16.5 3c0 1.5-1.2 3-2.8 3 0-1.6 1.3-3 2.8-3zM19 16.5c-.8 1.9-2 3.9-3.6 3.9-1.4 0-1.8-.8-3.4-.8s-2.1.8-3.4.8C6.9 20.4 4 16.3 4 12.4 4 9 6.2 7.3 8.3 7.3c1.4 0 2.4.9 3.6.9 1.1 0 2.4-1 4-1 .6 0 2.6.1 3.8 2-3.3 1.8-2.7 6.4-.7 7.3z",
    Linux: "M12 3c-2.2 0-3.5 1.8-3.5 4.5 0 2-1.5 3.5-2.5 5.5S5 17 7 18.5 10 21 12 21s3-1 5-2.5 1-3.5 0-5.5-2.5-3.5-2.5-5.5C14.5 4.8 14.2 3 12 3z"
  };
  var group = document.querySelector('.dl-groups [data-os="' + os + '"]');
  var button = document.getElementById("download-os");
  if (button) {
    button.innerHTML = '<svg class="i" viewBox="0 0 24 24" aria-hidden="true"><path d="' + logos[os] + '"/></svg>';
    button.appendChild(document.createTextNode("Download for " + os));
    var windows = button.getAttribute("data-windows");
    if (os === "Windows" && windows) button.href = windows;
    else button.href = (group ? "" : "download.html") + "#" + os.toLowerCase();
  }
  // On the download page, the files of that system come first and are marked.
  if (group) {
    group.classList.add("yours");
    group.parentNode.insertBefore(group, group.parentNode.firstChild);
  }
})();

// The app window types its query again every few seconds while it is in view.
(function () {
  var app = document.querySelector(".app");
  if (!app || window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  var visible = true;
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(function (entries) {
      visible = entries[0].isIntersecting;
    }).observe(app);
  }
  setInterval(function () {
    if (!visible || document.hidden) return;
    // Restarting a CSS animation: take the class off, force a layout, put it back.
    app.classList.remove("play");
    void app.offsetWidth;
    app.classList.add("play");
  }, 14000);
})();

// A soft light follows the pointer over the cards.
(function () {
  document.querySelectorAll(".card").forEach(function (card) {
    card.addEventListener("pointermove", function (e) {
      var r = card.getBoundingClientRect();
      card.style.setProperty("--x", e.clientX - r.left + "px");
      card.style.setProperty("--y", e.clientY - r.top + "px");
    });
  });
})();

// The link of the section in view is marked in the navigation and in the bar of the areas.
(function () {
  // Only the links to sections of this page; the others open pages of their own.
  var links = document.querySelectorAll('.nav-links a[href^="#"], .pill-nav a[href^="#"], .rail a[href^="#"]');
  if (!("IntersectionObserver" in window) || !links.length) return;
  var byId = {};
  links.forEach(function (a) {
    var id = a.getAttribute("href").slice(1);
    (byId[id] = byId[id] || []).push(a);
  });
  var watcher = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (!e.isIntersecting) return;
      links.forEach(function (a) { a.removeAttribute("aria-current"); });
      byId[e.target.id].forEach(function (a) { a.setAttribute("aria-current", "true"); });
    });
  }, { rootMargin: "-45% 0px -50% 0px" });
  Object.keys(byId).forEach(function (id) {
    var section = document.getElementById(id);
    if (section) watcher.observe(section);
  });
})();

// Compare: the head to head shows one client at a time, picked with the buttons above it.
(function () {
  var pick = document.querySelector(".vs-pick");
  if (!pick) return;
  var clients = {
    dbeaver: { name: "DBeaver CE", logo: "dbeaver", color: "#c9a27a", ratio: "4×", download: "122 MiB", disk: "206 MiB", runtime: "Java, JRE bundled", license: "Apache 2.0, free", point: "Your DBeaver connections", pointText: "Import a workspace or a .dbp project export, with its folders, projects and saved user names." },
    datagrip: { name: "DataGrip", logo: "datagrip", color: "#e6e6e6", ratio: "35×", download: "1,047 MiB", disk: "not published", runtime: "Java, JetBrains Runtime bundled", license: "Proprietary; free for non-commercial use", point: "Your DataGrip connections", pointText: "Import them with their folders. No account and no license to renew." },
    tableplus: { name: "TablePlus", image: "img/clients/tableplus.png", color: "#f0a14b", ratio: "2×", download: "60 MiB", disk: "180 MiB", runtime: "Native (GTK on Linux)", license: "Proprietary, free tier", point: "Open source", pointText: "AGPL-3.0: free for any use, at work too, with no license to buy." },
    beekeeper: { name: "Beekeeper Studio", logo: "beekeeperstudio", color: "#fad83b", ratio: "12×", download: "346 MiB", disk: "1,105 MiB", runtime: "Electron", license: "GPLv3 community edition, paid edition", point: "Oracle in the free app", pointText: "Oracle comes with the free download, with PL/SQL blocks and real cancel." }
  };
  function show(id) {
    var c = clients[id];
    document.querySelectorAll("[data-vs]").forEach(function (el) {
      var key = el.getAttribute("data-vs");
      if (key === "tile") {
        el.innerHTML = c.image
          ? '<img src="' + c.image + '" alt="" width="34" height="34">'
          : '<svg aria-hidden="true"><use href="img/clients.svg#' + c.logo + '"/></svg>';
        el.classList.toggle("img", !!c.image);
        el.style.setProperty("--c", c.color);
      } else {
        el.textContent = c[key];
      }
    });
    pick.querySelectorAll("button").forEach(function (b) {
      b.setAttribute("aria-selected", b.getAttribute("data-client") === id ? "true" : "false");
    });
  }
  var buttons = Array.prototype.slice.call(pick.querySelectorAll("button[data-client]"));
  function select(b) {
    buttons.forEach(function (x) { x.tabIndex = x === b ? 0 : -1; });
    show(b.getAttribute("data-client"));
  }
  pick.addEventListener("click", function (e) {
    var b = e.target.closest("button[data-client]");
    if (b) select(b);
  });
  // The arrows and Home/End move between the clients, as in the tour.
  pick.addEventListener("keydown", function (e) {
    var at = buttons.indexOf(document.activeElement);
    if (at < 0) return;
    var next = { ArrowRight: at + 1, ArrowDown: at + 1, ArrowLeft: at - 1, ArrowUp: at - 1, Home: 0, End: buttons.length - 1 }[e.key];
    if (next === undefined) return;
    e.preventDefault();
    var b = buttons[(next + buttons.length) % buttons.length];
    b.focus();
    select(b);
  });
  select(buttons[0]);
})();

// Features: the share of the areas read so far, in the rail beside them.
(function () {
  var rail = document.querySelector(".rail");
  var areas = document.querySelector(".areas");
  if (!rail || !areas) return;
  var bar = rail.querySelector(".rail-bar s");
  var count = rail.querySelector(".rail-pct");
  var queued = false;
  function update() {
    queued = false;
    var r = areas.getBoundingClientRect();
    // 0 % when the areas reach the middle of the window, 100 % when their end does.
    var span = r.height;
    var read = window.innerHeight / 2 - r.top;
    var pct = Math.max(0, Math.min(100, Math.round((read / span) * 100)));
    bar.style.width = pct + "%";
    count.textContent = ("00" + pct).slice(-3) + "%";
  }
  function queue() {
    if (!queued) {
      queued = true;
      requestAnimationFrame(update);
    }
  }
  window.addEventListener("scroll", queue, { passive: true });
  window.addEventListener("resize", queue);
  update();
})();

// Tooltips: an element with data-tip shows its text on hover and on focus. One box, placed by
// the script, so a table that scrolls sideways does not cut it.
(function () {
  var targets = document.querySelectorAll("[data-tip]");
  if (!targets.length) return;
  var tip = document.createElement("div");
  tip.className = "tip";
  tip.setAttribute("role", "tooltip");
  tip.id = "tip";
  tip.hidden = true;
  document.body.appendChild(tip);
  var current = null;
  function show(el) {
    current = el;
    tip.textContent = el.getAttribute("data-tip");
    tip.hidden = false;
    var r = el.getBoundingClientRect();
    var w = tip.offsetWidth;
    var left = Math.max(8, Math.min(window.innerWidth - w - 8, r.left + r.width / 2 - w / 2));
    var top = r.top - tip.offsetHeight - 8;
    if (top < 8) top = r.bottom + 8;
    tip.style.left = left + "px";
    tip.style.top = top + "px";
    el.setAttribute("aria-describedby", "tip");
  }
  function hide(el) {
    current = null;
    tip.hidden = true;
    el.removeAttribute("aria-describedby");
  }
  targets.forEach(function (el) {
    el.addEventListener("mouseenter", function () { show(el); });
    el.addEventListener("mouseleave", function () { hide(el); });
    el.addEventListener("focus", function () { show(el); });
    el.addEventListener("blur", function () { hide(el); });
  });
  // The box follows its cell when the page or the table scrolls (a focused cell scrolls into view).
  function follow() {
    if (current && !tip.hidden) show(current);
  }
  window.addEventListener("scroll", follow, { passive: true });
  document.addEventListener("scroll", follow, { passive: true, capture: true });
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape") tip.hidden = true;
  });
})();

// Docs: the menu starts closed on a narrow screen (it stays open without this script).
(function () {
  var menu = document.querySelector(".docs-nav");
  if (menu && window.matchMedia("(max-width: 900px)").matches) menu.removeAttribute("open");
})();
