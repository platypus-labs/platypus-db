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

// The download button names the visitor's system.
(function () {
  var p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  var os = /win/i.test(p) ? "Windows" : /mac/i.test(p) ? "macOS" : /linux/i.test(p) ? "Linux" : "";
  var button = document.getElementById("download-os");
  if (os && button) button.textContent = "Download for " + os;
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

// The link of the section in view is marked in the navigation.
(function () {
  // Only the links to sections of this page; the others open pages of their own.
  var links = document.querySelectorAll('.nav-links a[href^="#"]');
  if (!("IntersectionObserver" in window) || !links.length) return;
  var byId = {};
  links.forEach(function (a) { byId[a.getAttribute("href").slice(1)] = a; });
  var watcher = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      if (!e.isIntersecting) return;
      links.forEach(function (a) { a.removeAttribute("aria-current"); });
      byId[e.target.id].setAttribute("aria-current", "true");
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
    dbeaver: { name: "DBeaver CE", tile: "DB", color: "#c9a27a", ratio: "6×", download: "122 MiB", disk: "206 MiB", runtime: "Java, JRE bundled", license: "Apache 2.0, free", point: "Your DBeaver connections", pointText: "Import a workspace or a .dbp project export, with its folders, projects and saved user names." },
    datagrip: { name: "DataGrip", tile: "DG", color: "#21d789", ratio: "54×", download: "1,047 MiB", disk: "not measured", runtime: "Java, JetBrains Runtime bundled", license: "Proprietary; free for non-commercial use", point: "Your DataGrip connections", pointText: "Import them with their folders. No account and no license to renew." },
    tableplus: { name: "TablePlus", tile: "TP", color: "#f0a14b", ratio: "3×", download: "60 MiB", disk: "180 MiB", runtime: "Native (GTK on Linux)", license: "Proprietary, free tier", point: "Open source", pointText: "AGPL-3.0: free for any use, at work too, with no license to buy." },
    beekeeper: { name: "Beekeeper Studio", tile: "BK", color: "#fad83b", ratio: "18×", download: "346 MiB", disk: "1,105 MiB", runtime: "Electron", license: "GPLv3 community edition, paid edition", point: "Oracle in the free app", pointText: "Oracle comes with the free download, with PL/SQL blocks and real cancel." }
  };
  function show(id) {
    var c = clients[id];
    document.querySelectorAll("[data-vs]").forEach(function (el) {
      var key = el.getAttribute("data-vs");
      if (key === "tile") {
        el.textContent = c.tile;
        el.style.setProperty("--c", c.color);
      } else {
        el.textContent = c[key];
      }
    });
    pick.querySelectorAll("button").forEach(function (b) {
      b.setAttribute("aria-selected", b.getAttribute("data-client") === id ? "true" : "false");
    });
  }
  pick.addEventListener("click", function (e) {
    var b = e.target.closest("button[data-client]");
    if (b) show(b.getAttribute("data-client"));
  });
})();
