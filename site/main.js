/* ═══════════════════════════════════════════════════════════════
   Hangar.AI site

   Small jobs around a static page; the page reads fine without them.

     1. themes: repaint the page, and show the hero in the matching scheme
     2. the section bar, which marks where you are
     3. the download for the system you are on
     4. the delivery, which plays its four stages once
     5. the short lists, folded to their titles on a phone
   ═══════════════════════════════════════════════════════════════ */
(function () {
  'use strict';

  var root = document.documentElement;
  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ── 1. themes ─────────────────────────────────────────────────
     Eight of the app's 33, with the chrome tokens the web app's
     deriveChromeTokens() computed for them, copied as they came out
     (crates/hangar-theme holds them now, with the same values). */

  var THEMES = [
    {id:"graphite",name:"Graphite",light:false,bg:"#121316",sunken:"#0c0c0e",chrome:"#1b1c1f",bar:"#202124",raised:"#232426",border:"#2c2d30",borderStrong:"#414245",sep:"#090a0b",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#c8c9ce",fg2:"#9c9da2",fg3:"#8c8d91",accent:"#3e8bff",accentHi:"#68a5ff",accentFg:"#0b192e",accentSoft:"rgba(62, 139, 255, 0.14)",working:"#5fd27f",waiting:"#e7c06b",yours:"#6fd4df",danger:"#ff7b72",segTrack:"#121316",segThumb:"#3d3e41",shadow:"rgba(0,0,0,0.45)",fgHi:"#ecedf0"},
    {id:"tokyo-night",name:"Tokyo Night",light:false,bg:"#1a1b26",sunken:"#111219",chrome:"#23242f",bar:"#282933",raised:"#2a2b35",border:"#33343e",borderStrong:"#484951",sep:"#0d0e13",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#c0caf5",fg2:"#9aa3c6",fg3:"#8a91b2",accent:"#7aa2f7",accentHi:"#97b6f9",accentFg:"#161d2c",accentSoft:"rgba(122, 162, 247, 0.14)",working:"#9ece6a",waiting:"#e0af68",yours:"#0db9d7",danger:"#f77e95",segTrack:"#1a1b26",segThumb:"#424454",shadow:"rgba(0,0,0,0.45)",fgHi:"#d9dff9"},
    {id:"catppuccin-mocha",name:"Catppuccin Mocha",light:false,bg:"#1e1e2e",sunken:"#14141e",chrome:"#272736",bar:"#2c2c3b",raised:"#2e2e3d",border:"#373745",borderStrong:"#4b4b58",sep:"#0f0f17",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#cdd6f4",fg2:"#a3aac4",fg3:"#9399b3",accent:"#cba6f7",accentHi:"#d6baf9",accentFg:"#251e2c",accentSoft:"rgba(203, 166, 247, 0.14)",working:"#a6e3a1",waiting:"#f9e2af",yours:"#94e2d5",danger:"#f38ba8",segTrack:"#1e1e2e",segThumb:"#47495a",shadow:"rgba(0,0,0,0.45)",fgHi:"#e1e6f8"},
    {id:"dracula",name:"Dracula",light:false,bg:"#282a36",sunken:"#1a1b23",chrome:"#31333e",bar:"#353742",raised:"#373944",border:"#40414c",borderStrong:"#53555e",sep:"#14151b",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#f8f8f2",fg2:"#c6c7c5",fg3:"#b3b4b4",accent:"#bd93f9",accentHi:"#ccabfa",accentFg:"#221a2d",accentSoft:"rgba(189, 147, 249, 0.14)",working:"#50fa7b",waiting:"#f1fa8c",yours:"#8be9fd",danger:"#ff9999",segTrack:"#282a36",segThumb:"#565860",shadow:"rgba(0,0,0,0.45)",fgHi:"#fbfbf7"},
    {id:"nord",name:"Nord",light:false,bg:"#2e3440",sunken:"#1e222a",chrome:"#363c48",bar:"#3b404b",raised:"#3d424d",border:"#454a55",borderStrong:"#585d66",sep:"#171a20",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#d8dee9",fg2:"#bbc1cc",fg3:"#a8aeb9",accent:"#88c0d0",accentHi:"#a2ceda",accentFg:"#182325",accentSoft:"rgba(136, 192, 208, 0.14)",working:"#b1c89d",waiting:"#ebcb8b",yours:"#b3c0e1",danger:"#e1b6ba",segTrack:"#2e3440",segThumb:"#565b66",shadow:"rgba(0,0,0,0.45)",fgHi:"#e8ebf2"},
    {id:"rose-pine",name:"Rosé Pine",light:false,bg:"#191724",sunken:"#100f17",chrome:"#22202d",bar:"#272531",raised:"#292733",border:"#32313c",borderStrong:"#474550",sep:"#0d0c12",line:"rgba(255, 255, 255, 0.07)",lineStrong:"rgba(255, 255, 255, 0.12)",fg:"#e0def4",fg2:"#b0aec2",fg3:"#9e9caf",accent:"#c4a7e7",accentHi:"#d1baec",accentFg:"#231e2a",accentSoft:"rgba(196, 167, 231, 0.14)",working:"#7ba6b7",waiting:"#f6c177",yours:"#9cd8cb",danger:"#ed7d9c",segTrack:"#191724",segThumb:"#464452",shadow:"rgba(0,0,0,0.45)",fgHi:"#ecebf8"},
    {id:"github-light",name:"GitHub Light",light:true,bg:"#ffffff",sunken:"#ffffff",chrome:"#f5f5f5",bar:"#f0f0f0",raised:"#ededed",border:"#e3e3e3",borderStrong:"#cccccc",sep:"#dbdbdb",line:"rgba(0, 0, 0, 0.08)",lineStrong:"rgba(0, 0, 0, 0.14)",fg:"#24292f",fg2:"#595c61",fg3:"#686c71",accent:"#0969da",accentHi:"#085cc0",accentFg:"#ffffff",accentSoft:"rgba(9, 105, 218, 0.14)",working:"#116329",waiting:"#9b4708",yours:"#166a6d",danger:"#ba1f29",segTrack:"#dedede",segThumb:"#ffffff",shadow:"rgba(0,0,0,0.12)",fgHi:"#1b1f23"},
    {id:"catppuccin-latte",name:"Catppuccin Latte",light:true,bg:"#eff1f5",sunken:"#f5f6f9",chrome:"#e5e7eb",bar:"#e1e3e6",raised:"#dee0e4",border:"#d5d6da",borderStrong:"#bfc1c4",sep:"#cecfd3",line:"rgba(0, 0, 0, 0.08)",lineStrong:"rgba(0, 0, 0, 0.14)",fg:"#4c4f69",fg2:"#53566f",fg3:"#60627a",accent:"#8839ef",accentHi:"#712fc6",accentFg:"#ffffff",accentSoft:"rgba(136, 57, 239, 0.14)",working:"#28641b",waiting:"#8f4207",yours:"#0f6166",danger:"#b00d30",segTrack:"#d0d2d5",segThumb:"#f5f6f9",shadow:"rgba(0,0,0,0.12)",fgHi:"#393b4f"}
  ];

  var TOKENS = {
    bg: '--bg', sunken: '--bg-sunken', chrome: '--chrome', bar: '--chrome-bar', raised: '--raised',
    border: '--border', borderStrong: '--border-strong', sep: '--sep', line: '--line',
    lineStrong: '--line-strong', fgHi: '--fg-hi', fg: '--fg', fg2: '--fg-secondary', fg3: '--fg-muted',
    accent: '--accent', accentHi: '--accent-hi', accentFg: '--accent-fg', accentSoft: '--accent-soft',
    working: '--state-working', waiting: '--state-waiting', yours: '--state-yours', danger: '--danger',
    segTrack: '--seg-track', segThumb: '--seg-thumb', shadow: '--shadow'
  };

  // Until a visitor picks one, the page follows the system: Graphite, or
  // GitHub Light on a light system (styles.css, data-theme="auto").
  var prefersLight = window.matchMedia('(prefers-color-scheme: light)').matches;
  var currentTheme = prefersLight ? 'github-light' : 'graphite';

  function applyTheme(theme) {
    Object.keys(TOKENS).forEach(function (key) {
      root.style.setProperty(TOKENS[key], theme[key]);
    });
    root.style.colorScheme = theme.light ? 'light' : 'dark';
    root.dataset.theme = theme.id;
    // The hero has a capture of the app in each scheme.
    root.dataset.scheme = theme.light ? 'light' : 'dark';
    currentTheme = theme.id;
    Array.prototype.forEach.call(document.querySelectorAll('meta[name="theme-color"]'), function (meta) {
      meta.setAttribute('content', theme.bar);
    });
  }

  (function swatches() {
    var host = document.getElementById('swatches');
    if (!host) return;
    THEMES.forEach(function (theme) {
      var button = document.createElement('button');
      button.type = 'button';
      button.className = 'sw';
      button.setAttribute('aria-pressed', String(theme.id === currentTheme));
      var strip = document.createElement('span');
      strip.className = 'sw__strip';
      strip.setAttribute('aria-hidden', 'true');
      [theme.bg, theme.raised, theme.fg, theme.accent, theme.working, theme.waiting, theme.yours].forEach(function (c) {
        var i = document.createElement('i');
        i.style.background = c;
        strip.appendChild(i);
      });
      button.appendChild(strip);
      button.appendChild(document.createTextNode(theme.name));
      button.addEventListener('click', function () {
        applyTheme(theme);
        Array.prototype.forEach.call(host.children, function (el) {
          el.setAttribute('aria-pressed', String(el === button));
        });
      });
      host.appendChild(button);
    });
  })();

  /* ── 2. the section bar ──────────────────────────────────────── */

  var links = {};
  Array.prototype.forEach.call(document.querySelectorAll('#nav a'), function (a) {
    links[a.getAttribute('href').slice(1)] = a;
  });

  if ('IntersectionObserver' in window) {
    var spy = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) {
        if (!entry.isIntersecting) return;
        Object.keys(links).forEach(function (id) {
          if (id === entry.target.id) links[id].setAttribute('aria-current', 'true');
          else links[id].removeAttribute('aria-current');
        });
      });
    }, { rootMargin: '-45% 0px -50% 0px' });
    Object.keys(links).concat(['top']).forEach(function (id) {
      var section = document.getElementById(id);
      if (section) spy.observe(section);
    });
  }

  // A link to a question opens it.
  function openTarget() {
    var target = window.location.hash && document.getElementById(window.location.hash.slice(1));
    if (target && target.tagName === 'DETAILS') target.open = true;
  }
  window.addEventListener('hashchange', openTarget);
  openTarget();

  /* ── 3. the download for this system ─────────────────────────── */

  (function platform() {
    var ua = navigator.userAgent;
    var touchMac = /Macintosh/.test(ua) && navigator.maxTouchPoints > 1;
    var os = /Android|iPhone|iPad|iPod/.test(ua) || touchMac ? null
      : /Mac OS X|Macintosh/.test(ua) ? 'macos'
      : /Windows/.test(ua) ? 'windows'
      : /Linux|X11|CrOS/.test(ua) ? 'linux'
      : null;
    if (!os) return;

    var card = document.querySelector('.plat[data-os="' + os + '"]');
    if (!card) return;
    card.classList.add('is-you');
    var title = card.querySelector('.plat__os');
    if (title) {
      var you = document.createElement('span');
      you.className = 'plat__you';
      you.textContent = 'This computer';
      title.appendChild(you);
    }

    var file = card.querySelector('.plat__get');
    var get = document.getElementById('get');
    if (get && file) {
      get.href = file.href;
      get.textContent = 'Download for ' + { windows: 'Windows', macos: 'macOS', linux: 'Linux' }[os];
    }

    var note = document.getElementById('getnote');
    if (note && os === 'windows') {
      note.appendChild(document.createTextNode(' SmartScreen warns on first run because the installer is not signed yet: '));
      var why = document.createElement('a');
      why.href = '#faq-smartscreen';
      why.textContent = 'here is what to do';
      note.appendChild(why);
      note.appendChild(document.createTextNode('.'));
    }
  })();

  /* ── 4. the delivery ───────────────────────────────────────────
     Written in its final state, so nothing moves without script or
     for anyone who asked for less motion. */

  (function delivery() {
    var box = document.getElementById('delivery');
    if (!box || reduced || !('IntersectionObserver' in window)) return;
    var steps = box.querySelectorAll('.dstep');
    Array.prototype.forEach.call(steps, function (s) { s.dataset.state = 'pending'; });

    var played = false;
    var watch = new IntersectionObserver(function (entries) {
      if (played || !entries.some(function (e) { return e.isIntersecting; })) return;
      played = true;
      watch.disconnect();
      var i = 0;
      (function next() {
        if (i >= steps.length) return;
        var step = steps[i];
        step.dataset.state = 'running';
        setTimeout(function () {
          step.dataset.state = 'done';
          i += 1;
          setTimeout(next, 120);
        }, i === 1 || i === 2 ? 1100 : 650);
      })();
    }, { threshold: 0.6 });
    watch.observe(box);
  })();

  /* ── 5. the short lists ────────────────────────────────────────
     The facts under the orchestrator and the list under "Everything
     else" are written open. On a phone they fold to their titles, a
     tap away each; they open again when the screen grows. */

  (function folds() {
    var folds = document.querySelectorAll('details.fold');
    if (!folds.length) return;
    var roomy = window.matchMedia('(min-width: 600px)');
    function set() {
      Array.prototype.forEach.call(folds, function (d) { d.open = roomy.matches; });
    }
    set();
    if (roomy.addEventListener) roomy.addEventListener('change', set);
    else if (roomy.addListener) roomy.addListener(set);

    // Folding moved what a link to a section points at: go back to it.
    if (!roomy.matches && window.location.hash) {
      var target = document.getElementById(window.location.hash.slice(1));
      if (target) {
        try { target.scrollIntoView({ block: 'start', behavior: 'instant' }); }
        catch (e) { target.scrollIntoView(true); }
      }
    }
  })();
})();
