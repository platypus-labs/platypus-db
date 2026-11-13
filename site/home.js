// The home page (index.html), on top of site.js: the hero title, the chapters that follow the
// scroll in the pinned window, the rail, and the platypus (its hop, its eye on the pointer, its
// facts about the sample database, its phone). Without this script the page reads top to bottom
// with the first chapter and the first title.
(function () {
  var show = document.getElementById("tour");
  var win = document.getElementById("hwin");
  if (!show || !win) return;
  var chaps = Array.prototype.slice.call(document.querySelectorAll(".chap"));
  var shots = Array.prototype.slice.call(document.querySelectorAll("#shots img"));
  var plat = document.getElementById("plat");
  var rail = document.getElementById("rail");
  var still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var step = -1;
  var ticking = false;
  var pinned = false;
  // The rare phone animation (work in progress): false turns it off.
  var PHONE = true;

  function store(key, value) {
    try {
      if (value === undefined) return localStorage.getItem(key);
      localStorage.setItem(key, value);
    } catch (e) {
      // Storage off: the first visit every time.
    }
    return null;
  }

  // ---- Hero title: the speed one on a first visit, then another at random, never twice in a row.
  var TITLES = [
    ["Put the phone down.", "The query's done."], // speed of queries
    ["Blink once.", "It's already open."], // 0.7 s to a window
    ["Twenty databases.", "One tiny window."], // database support
    ["20 MiB.", "Not a typo."], // download size
    ["Scroll a million rows", "like it's ten."], // the grid
    ["Edit production.", "Keep your job."] // safety
  ];
  (function () {
    var h1 = document.getElementById("hero-title");
    var last = store("home-title");
    var title = 0;
    if (last !== null) {
      title = Math.floor(Math.random() * (TITLES.length - 1));
      if (title >= +last) title += 1;
    }
    h1.children[0].textContent = TITLES[title][0];
    h1.children[1].textContent = TITLES[title][1];
    store("home-title", String(title));
  })();

  // ---- The platypus talks about the sample database of the welcome screen
  // (samples/wildlife.sql): every fact is true of that data.
  var FACTS = [
    "Psst. The app ships a <b>sample database</b> about us. Welcome screen, <i>Try the sample database</i>.",
    "The sample database tags <b>300</b> of us, <code>PT-0001</code> to <code>PT-0300</code>. I'm not in it. I'm the logo.",
    "<b>15,000</b> sightings in the sample data. Nobody asked for our consent.",
    "One sighting in six has <code>platypus_id IS NULL</code>. Some of us value our privacy.",
    "A quarter of us have no nickname. <code>WHERE nickname IS NULL</code>. Ouch.",
    "Longest river in the sample: the Snowy, <b>352 km</b>. <code>SELECT max(length_km) FROM rivers</code>",
    "Males weigh 1.5 to 2.2 kg in <code>measurements</code>. I'm not heavy, I'm disk-shaped.",
    "Five behaviours on record: foraging, swimming, resting, burrowing, grooming. Querying? Working on it.",
    "12 researchers, <b>2,000</b> surveys. They still haven't found my burrow.",
    "Run <code>Most seen platypuses.sql</code> on the sample to see who's famous. Spoiler: not me.",
    "Bills from 50 to 61.9 mm in the sample. Mine is a perfect 61.9."
  ];
  var say = document.getElementById("say");
  var fact = 0;
  var typing = 0;
  var sayTimer = 0;

  // Types the text out, tags kept whole.
  function speak(html, label) {
    clearInterval(typing);
    clearTimeout(sayTimer);
    var parts = html.match(/<[^>]+>|[^<]/g);
    var head = "<small>" + label + "</small>";
    var n = 0;
    say.classList.add("on");
    if (still) {
      say.innerHTML = head + html;
      return;
    }
    typing = setInterval(function () {
      n += 2;
      while (n < parts.length && parts[n - 1].charAt(0) === "<") n++;
      say.innerHTML = head + parts.slice(0, n).join("") + (n < parts.length ? '<span class="caret"></span>' : "");
      if (n >= parts.length) clearInterval(typing);
    }, 24);
  }

  function nextFact() {
    speak(FACTS[fact], "Platypus fact #" + (fact + 1));
    fact = (fact + 1) % FACTS.length;
    sayTimer = setTimeout(function () {
      if (pinned) nextFact();
    }, 9000);
  }

  function hush() {
    clearInterval(typing);
    clearTimeout(sayTimer);
    say.classList.remove("on");
  }

  // ---- The hop: on a new chapter, or a click on the platypus.
  function hop() {
    if (still) return;
    plat.classList.remove("hopping");
    void plat.offsetWidth;
    plat.classList.add("hopping");
  }
  plat.addEventListener("click", hop);
  plat.addEventListener("keydown", function (e) {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    hop();
  });

  // ---- The phone: now and then, while the window is pinned.
  var phoning = false;
  function usePhone() {
    if (still || phoning) return;
    phoning = true;
    hush();
    plat.classList.add("phoning");
    setTimeout(function () {
      // Not if the visitor has left the tour meanwhile: nothing would take the bubble away.
      if (pinned) speak("Wait, done already? <b>A million rows, first page in 287 ms.</b>", "Query finished");
    }, 2300);
    setTimeout(function () {
      plat.classList.remove("phoning");
      phoning = false;
      sayTimer = setTimeout(function () {
        if (pinned) nextFact();
      }, 2500);
    }, 3500);
  }
  // The chance comes every 6 s, only while the window is pinned.
  var phoneTimer = 0;
  function phoneChances(on) {
    clearInterval(phoneTimer);
    if (!on || !PHONE || still) return;
    phoneTimer = setInterval(function () {
      if (!phoning && !document.hidden && Math.random() < 0.12) usePhone();
    }, 6000);
  }

  // ---- Rail: one tick per section and per chapter.
  var marks = Array.prototype.slice.call(document.querySelectorAll("[data-rail]"));
  function chapterLength() {
    return window.innerHeight * 0.7;
  }
  function showTop() {
    return show.getBoundingClientRect().top + window.scrollY;
  }
  function goTo(el) {
    var c = chaps.indexOf(el);
    var y = c >= 0 ? showTop() + c * chapterLength() + 2 : el.getBoundingClientRect().top + window.scrollY;
    if (el.id === "top") y = 0;
    window.scrollTo({ top: Math.max(0, y), behavior: still ? "auto" : "smooth" });
  }
  marks.forEach(function (el) {
    var li = document.createElement("li");
    var a = document.createElement("a");
    var label = document.createElement("span");
    a.href = el.id ? "#" + el.id : "#";
    a.setAttribute("aria-label", el.getAttribute("data-rail"));
    label.textContent = el.getAttribute("data-rail");
    a.appendChild(label);
    a.addEventListener("click", function (e) {
      e.preventDefault();
      goTo(el);
    });
    li.appendChild(a);
    rail.appendChild(li);
    el.railTick = a;
  });

  // ---- Scroll: the window sliding in, the chapter, the rail.
  function update() {
    ticking = false;
    // Every measure first, then every change: no layout forced in between.
    var y = window.scrollY;
    var top = showTop();
    var stageIn = win.parentNode;
    var shift = window.innerWidth > 1100 ? -(stageIn.clientWidth - win.offsetWidth) / 2 + 24 : 0;
    var showEnd = top + show.offsetHeight;
    var half = window.innerHeight * 0.5;
    var past = marks.filter(function (el) {
      return chaps.indexOf(el) < 0 && el.getBoundingClientRect().top < half;
    });
    // 0 at the top of the page, 1 once the stage is pinned: the window slides from the centre to the right.
    var enter = Math.min(1, Math.max(0, y / Math.max(1, top)));
    stageIn.style.setProperty("--shift", shift + "px");
    stageIn.style.setProperty("--enter", enter.toFixed(3));

    var s = Math.min(chaps.length - 1, Math.max(0, Math.floor((y - top) / chapterLength())));
    var nowPinned = y >= top - 1 && y < showEnd - window.innerHeight * 0.6;
    if (nowPinned !== pinned) {
      pinned = nowPinned;
      phoneChances(pinned);
      if (!pinned) hush();
      else if (!phoning) nextFact();
    }
    if (s !== step) {
      if (step >= 0) hop();
      step = s;
      chaps.forEach(function (c, i) {
        c.classList.toggle("on", i === s);
      });
      shots.forEach(function (img, i) {
        img.classList.toggle("on", i === s);
      });
    }

    // The chapter while the stage is pinned, otherwise the last section past the middle of the screen.
    var current = pinned ? chaps[step] : past.pop() || marks[0];
    marks.forEach(function (el) {
      el.railTick.classList.toggle("on", el === current);
    });
  }
  function onScroll() {
    if (ticking) return;
    ticking = true;
    window.requestAnimationFrame(update);
  }
  window.addEventListener("scroll", onScroll, { passive: true });
  window.addEventListener("resize", onScroll);
  update();

  // ---- The parts follow the pointer, each by its depth, and the eye looks at it.
  if (!still) {
    window.addEventListener("pointermove", function (e) {
      var r = plat.getBoundingClientRect();
      var mx = ((e.clientX - (r.left + r.width * 0.36)) / window.innerWidth) * 2;
      var my = ((e.clientY - (r.top + r.height * 0.3)) / window.innerHeight) * 2;
      // The platypus is mirrored: a move to the right is a move to its left.
      plat.style.setProperty("--mx", (-Math.max(-1, Math.min(1, mx))).toFixed(3));
      plat.style.setProperty("--my", Math.max(-1, Math.min(1, my)).toFixed(3));
    });
  }
})();
