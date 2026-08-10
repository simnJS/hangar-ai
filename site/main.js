/* ═══════════════════════════════════════════════════════════════
   Hangar.AI site

   Four small jobs, none of them the demo: that is the app, built
   for the browser, living in site/demo.

     · the theme switcher, which repaints the page and tells the
       embedded app to switch with it
     · the handshake that hides the stage's loading state once the
       app inside reports it has mounted
     · the prompt chips, which type into the demo's first pane
     · scroll furniture: reveal, nav highlighting, platform hint
   ═══════════════════════════════════════════════════════════════ */
(function () {
  'use strict';

  var reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ── 1. themes ─────────────────────────────────────────────
     Copied verbatim from src/themes.ts. Eight of the thirty-two. */

  var THEMES = [
    { id:'tokyo-night', name:'Tokyo Night', app:'dark', accent:'#7aa2f7', bg:'#1a1b26', fg:'#c0caf5',
      red:'#f7768e', green:'#9ece6a', yellow:'#e0af68', magenta:'#bb9af7', cyan:'#7dcfff' },
    { id:'catppuccin-mocha', name:'Catppuccin Mocha', app:'dark', accent:'#cba6f7', bg:'#1e1e2e', fg:'#cdd6f4',
      red:'#f38ba8', green:'#a6e3a1', yellow:'#f9e2af', magenta:'#f5c2e7', cyan:'#94e2d5' },
    { id:'dracula', name:'Dracula', app:'dark', accent:'#bd93f9', bg:'#282a36', fg:'#f8f8f2',
      red:'#ff5555', green:'#50fa7b', yellow:'#f1fa8c', magenta:'#ff79c6', cyan:'#8be9fd' },
    { id:'one-dark', name:'One Dark', app:'dark', accent:'#61afef', bg:'#282c34', fg:'#abb2bf',
      red:'#e06c75', green:'#98c379', yellow:'#e5c07b', magenta:'#c678dd', cyan:'#56b6c2' },
    { id:'nord', name:'Nord', app:'dark', accent:'#88c0d0', bg:'#2e3440', fg:'#d8dee9',
      red:'#bf616a', green:'#a3be8c', yellow:'#ebcb8b', magenta:'#b48ead', cyan:'#88c0d0' },
    { id:'gruvbox-dark', name:'Gruvbox Dark', app:'dark', accent:'#fabd2f', bg:'#282828', fg:'#ebdbb2',
      red:'#cc241d', green:'#98971a', yellow:'#d79921', magenta:'#b16286', cyan:'#689d6a' },
    { id:'kanagawa', name:'Kanagawa', app:'dark', accent:'#7e9cd8', bg:'#1f1f28', fg:'#dcd7ba',
      red:'#c34043', green:'#76946a', yellow:'#c0a36e', magenta:'#957fb8', cyan:'#6a9589' },
    { id:'github-light', name:'GitHub Light', app:'light', accent:'#0969da', bg:'#ffffff', fg:'#24292f',
      red:'#cf222e', green:'#116329', yellow:'#4d2d00', magenta:'#8250df', cyan:'#1b7c83' }
  ];

  function rgb(hex) {
    var h = hex.replace('#', '');
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }
  function hex(c) {
    return '#' + c.map(function (v) {
      return Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, '0');
    }).join('');
  }
  /** t = 0 keeps a, t = 1 becomes b. */
  function mix(a, b, t) {
    var x = rgb(a), y = rgb(b);
    return hex([x[0] + (y[0] - x[0]) * t, x[1] + (y[1] - x[1]) * t, x[2] + (y[2] - x[2]) * t]);
  }
  function luminance(c) {
    var v = rgb(c).map(function (n) {
      n /= 255;
      return n <= 0.03928 ? n / 12.92 : Math.pow((n + 0.055) / 1.055, 2.4);
    });
    return 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
  }
  function alpha(c, a) {
    var v = rgb(c);
    return 'rgba(' + v[0] + ',' + v[1] + ',' + v[2] + ',' + a + ')';
  }

  /** Everything the page paints with, derived from three colours. */
  function applyTheme(t) {
    var dark = t.app === 'dark';
    var s = document.documentElement.style;

    s.setProperty('--bg', dark ? mix(t.bg, '#000000', 0.24) : mix(t.bg, t.fg, 0.05));
    s.setProperty('--surface', t.bg);
    s.setProperty('--raised', mix(t.bg, t.fg, 0.08));
    s.setProperty('--line', mix(t.bg, t.fg, 0.2));
    s.setProperty('--line-soft', mix(t.bg, t.fg, 0.11));

    s.setProperty('--ink', t.fg);
    s.setProperty('--ink-hi', dark ? mix(t.fg, '#ffffff', 0.34) : mix(t.fg, '#000000', 0.4));
    s.setProperty('--dim', mix(t.fg, t.bg, 0.36));
    s.setProperty('--faint', mix(t.fg, t.bg, 0.58));

    s.setProperty('--acc', t.accent);
    // Dark ink on the accent whenever black beats white for contrast.
    s.setProperty('--acc-ink', luminance(t.accent) > 0.179 ? mix(t.bg, '#000000', 0.5) : '#ffffff');
    s.setProperty('--acc-soft', alpha(t.accent, dark ? 0.15 : 0.1));

    s.setProperty('--red', t.red);
    s.setProperty('--green', t.green);
    s.setProperty('--yellow', t.yellow);
    s.setProperty('--magenta', t.magenta);
    s.setProperty('--cyan', t.cyan);

    document.documentElement.dataset.theme = t.id;
    document.documentElement.style.colorScheme = t.app;

    var meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', s.getPropertyValue('--bg').trim() || t.bg);

    toDemo({ type: 'hangar-demo:theme', id: t.id });
  }

  function buildSwatches() {
    var host = document.getElementById('swatches');
    if (!host) return;

    THEMES.forEach(function (t, i) {
      var b = document.createElement('button');
      b.type = 'button';
      b.className = 'sw';
      b.setAttribute('aria-pressed', i === 0 ? 'true' : 'false');
      b.innerHTML =
        '<span class="sw__strip">' +
          '<i style="background:' + t.bg + '"></i>' +
          '<i style="background:' + t.accent + '"></i>' +
          '<i style="background:' + t.magenta + '"></i>' +
          '<i style="background:' + t.green + '"></i>' +
          '<i style="background:' + t.yellow + '"></i>' +
          '<i style="background:' + t.red + '"></i>' +
          '<i style="background:' + t.fg + '"></i>' +
        '</span>' +
        '<span class="sw__name">' + t.name + '<span class="sw__kind">' + t.app + '</span></span>';

      b.addEventListener('click', function () {
        applyTheme(t);
        Array.prototype.forEach.call(host.children, function (el) {
          el.setAttribute('aria-pressed', String(el === b));
        });
      });

      host.appendChild(b);
    });
  }

  /* ── 2. the embedded app ───────────────────────────────────
     It reports when it has mounted. Until then the stage shows a
     loading state. A blank rectangle tells nobody anything. */

  var stage = document.querySelector('.stage');
  var frame = document.getElementById('demoframe');
  var boot = document.getElementById('stageboot');
  var ready = false;

  function toDemo(message) {
    if (!frame || !frame.contentWindow) return;
    frame.contentWindow.postMessage(message, '*');
  }

  function markReady() {
    if (ready || !stage) return;
    ready = true;
    stage.classList.add('is-ready');
    var current = document.documentElement.dataset.theme;
    if (current && current !== 'tokyo-night') toDemo({ type: 'hangar-demo:theme', id: current });
  }

  function failed() {
    if (ready || !boot) return;
    boot.innerHTML =
      '<b>The demo did not start.</b>' +
      '<a class="stage__bootlink" href="demo/index.html" target="_blank" rel="noopener" ' +
      'style="opacity:1">Open it in a tab ↗</a>';
  }

  window.addEventListener('message', function (event) {
    var data = event.data;
    if (!data || data.type !== 'hangar-demo:ready') return;
    markReady();
  });

  if (stage && frame) {
    // Offer the escape hatch once it has clearly taken a while…
    setTimeout(function () { if (!ready) stage.classList.add('is-slow'); }, 5000);
    // …and say so plainly if the frame never even loaded.
    setTimeout(failed, 12000);

    frame.addEventListener('load', function () {
      // A build without the bridge would otherwise sit behind the overlay
      // forever, which is worse than showing it a moment early.
      setTimeout(markReady, 3000);
    });
  }

  /* ── 3. prompt chips ─────────────────────────────────────── */

  var prompts = document.getElementById('prompts');
  if (prompts) {
    prompts.addEventListener('click', function (event) {
      var chip = event.target.closest ? event.target.closest('.chip') : null;
      if (!chip) return;
      toDemo({ type: 'hangar-demo:type', text: chip.dataset.send });
      chip.classList.add('is-sent');
      setTimeout(function () { chip.classList.remove('is-sent'); }, 900);
      if (frame) frame.scrollIntoView({ block: 'center', behavior: reduced ? 'auto' : 'smooth' });
    });
  }

  /* ── 4. scroll furniture ─────────────────────────────────── */

  var bar = document.getElementById('topbar');
  if (bar) {
    var onScroll = function () {
      bar.classList.toggle('is-stuck', window.scrollY > 8);
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
  }

  if ('IntersectionObserver' in window) {
    var reveal = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        e.target.classList.add('is-in');
        reveal.unobserve(e.target);
      });
    }, { rootMargin: '0px 0px -12% 0px', threshold: 0.06 });

    document.querySelectorAll('.reveal').forEach(function (el) { reveal.observe(el); });

    var links = {};
    document.querySelectorAll('.topnav a').forEach(function (a) {
      links[a.getAttribute('href').slice(1)] = a;
    });

    var spy = new IntersectionObserver(function (entries) {
      entries.forEach(function (e) {
        if (!e.isIntersecting) return;
        Object.keys(links).forEach(function (id) {
          links[id].classList.toggle('is-here', id === e.target.id);
        });
      });
    }, { rootMargin: '-45% 0px -50% 0px' });

    Object.keys(links).forEach(function (id) {
      var section = document.getElementById(id);
      if (section) spy.observe(section);
    });
  } else {
    document.querySelectorAll('.reveal').forEach(function (el) { el.classList.add('is-in'); });
  }

  /* Highlights the card for the machine you are on, and names it on the
     hero button. The three downloads are otherwise identical to scan. */
  (function platform() {
    var ua = navigator.userAgent;
    var os = /Mac/i.test(ua) ? 'macos' : /Linux|X11|CrOS/i.test(ua) ? 'linux' : /Win/i.test(ua) ? 'windows' : null;
    if (!os) return;

    var card = document.querySelector('.plat[data-os="' + os + '"]');
    if (card) card.classList.add('is-you');

    var get = document.getElementById('mainget');
    if (get) get.textContent = 'Download for ' + ({ windows: 'Windows', macos: 'macOS', linux: 'Linux' })[os];
  })();

  buildSwatches();
})();
