// The screenshot tour as tabs: one window at a time; the arrows and Home/End move between them.
// Without this script every screenshot shows, one under the other.
(function () {
  var list = document.querySelector(".gallery [role=tablist]");
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
  var narrow = window.matchMedia("(max-width: 960px)");
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
