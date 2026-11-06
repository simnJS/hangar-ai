/* ═══════════════════════════════════════════════════════════════
   Hangar.AI site: the motion

   Scroll-driven, with GSAP and ScrollTrigger (vendor/, 3.15.0) and
   nothing else. The page is written in its final state and reads
   whole without this file: everything here starts from that state,
   moves only transform and opacity, and gives it back on exit.

     full    960 px and wider, motion allowed: tilts, pins, scrubs
     light   narrower, motion allowed: the same story, no pin, no tilt
     reduce  prefers-reduced-motion: plain fades, nothing travels

   index.html adds `motion` to <html> before the first paint when
   motion is allowed, and takes it back after 3 s if this file never
   ran; styles.css only hides the hero's copy under it (until
   `motion-ready`), so a page without script shows everything.

   Every number worth tuning is in TUNE below.
   ═══════════════════════════════════════════════════════════════ */
(function () {
  'use strict';

  var root = document.documentElement;
  var gsap = window.gsap;
  var ScrollTrigger = window.ScrollTrigger;
  if (!gsap || !ScrollTrigger) {
    root.classList.remove('motion');
    return;
  }
  gsap.registerPlugin(ScrollTrigger);

  var TUNE = {
    scrub: 0.6,            // seconds a scrubbed tween takes to catch up with the scroll
    heroTilt: 18,          // deg the hero window leans back at the top of the page
    heroScale: 0.88,       // and how small it starts
    heroLift: 60,          // px it rises by
    heroFlatAt: 'top 14%', // where its top is once it lies flat
    glowDrift: 28,         // % of its height the glow drifts down across the hero
    chatPin: 1.4,          // viewport heights the chat section stays pinned
    tilt: 10,              // deg the other windows lean back as they come in
    reveal: 36,            // px a card rises as it fades in (half on a phone)
    lineStagger: 0.09,     // s between two lines of a heading
    countFor: 1.1          // s a figure takes to count up
  };

  var Q = function (sel, ctx) {
    return Array.prototype.slice.call((ctx || document).querySelectorAll(sel));
  };
  var allowed = window.matchMedia('(prefers-reduced-motion: no-preference)').matches;

  /* ── the hero's entrance ───────────────────────────────────────
     Once, at load, timed rather than scrolled: the headline's words
     rise in turn, the rest follows. styles.css held the copy hidden
     until now, so nothing shows before it moves. */

  function heroIntro() {
    var copy = document.querySelector('.hero__copy');
    var title = copy && copy.querySelector('.hero__title');
    if (!copy || !title) return;
    var html = title.innerHTML;
    var words = splitWords(title);
    var rest = Q('.hero__copy > *').filter(function (el) { return el !== title; });
    gsap.timeline({
      defaults: { ease: 'power3.out' },
      onComplete: function () {
        title.innerHTML = html;
        title.removeAttribute('aria-label');
        gsap.set(rest, { clearProps: 'transform,opacity' });
      }
    })
      .from(rest[0], { y: 10, opacity: 0, duration: 0.6 }, 0)
      .from(words, { yPercent: 55, opacity: 0, duration: 0.9, stagger: 0.055 }, 0.08)
      .from(rest.slice(1), { y: 16, opacity: 0, duration: 0.7, stagger: 0.07 }, 0.4);
  }

  // Each word in a span of its own; the &nbsp; pairs stay together.
  function splitWords(el) {
    el.setAttribute('aria-label', el.textContent);
    var parts = el.textContent.split(' ');
    el.textContent = '';
    return parts.map(function (word, i) {
      if (i) el.appendChild(document.createTextNode(' '));
      var span = document.createElement('span');
      span.className = 'w';
      span.setAttribute('aria-hidden', 'true');
      span.textContent = word;
      el.appendChild(span);
      return span;
    });
  }

  /* ── headings: masked line reveals ─────────────────────────────
     A title is cut into its lines when it comes into view (so the
     lines are the ones the visitor's width and font give), each line
     rises out of its own mask, and the title gets its text back. */

  function headings(conditions, cleanups) {
    Q('.sec__title').forEach(function (el) {
      if (ScrollTrigger.isInViewport(el, 0.1) || el.children.length) return;
      var html = el.innerHTML;
      gsap.set(el, { opacity: 0 });
      ScrollTrigger.create({
        trigger: el,
        start: 'top 88%',
        once: true,
        onEnter: function () {
          if (conditions.reduce) {
            gsap.to(el, { opacity: 1, duration: 0.6, ease: 'none' });
            return;
          }
          var lines = splitLines(el);
          gsap.set(el, { opacity: 1 });
          gsap.from(lines, {
            yPercent: 105,
            duration: 0.95,
            ease: 'power4.out',
            stagger: TUNE.lineStagger,
            onComplete: function () { restore(); }
          });
        }
      });
      function restore() {
        el.innerHTML = html;
        el.removeAttribute('aria-label');
      }
      cleanups.push(restore);
    });
  }

  function splitLines(el) {
    var text = el.textContent;
    el.setAttribute('aria-label', text);
    var words = splitWords(el);
    // One read pass, then one write pass.
    var tops = words.map(function (w) { return w.offsetTop; });
    var lines = [];
    words.forEach(function (w, i) {
      if (!i || tops[i] - tops[i - 1] > 2) lines.push([]);
      lines[lines.length - 1].push(w.textContent);
    });
    el.textContent = '';
    return lines.map(function (line) {
      var mask = document.createElement('span');
      mask.className = 'rl';
      mask.setAttribute('aria-hidden', 'true');
      var inner = document.createElement('span');
      inner.textContent = line.join(' ');
      mask.appendChild(inner);
      el.appendChild(mask);
      return inner;
    });
  }

  /* ── reveals: cards and short texts fade up as they come in ──── */

  var REVEAL = [
    '.head .sec__lede', '.head .eyebrow', '.split__copy .eyebrow', '.split__copy .sec__lede',
    '.split__copy .fine', '.duo > .card', '.facts .fold', '.phones__note', '.reach__card',
    '.themes', '.exp > .card', '.perf__group', '.perf__item', '.perf__note', '.specs .fold',
    '.plat', '.after', '.qa details'
  ];

  function reveals(conditions, extra) {
    var rise = conditions.reduce ? 0 : conditions.full ? TUNE.reveal : TUNE.reveal / 2;
    var els = Q(REVEAL.join(',')).concat(extra).filter(function (el) {
      return !ScrollTrigger.isInViewport(el, 0.05);
    });
    if (!els.length) return;
    gsap.set(els, { opacity: 0, y: rise });
    ScrollTrigger.batch(els, {
      start: 'top 90%',
      once: true,
      onEnter: function (batch) {
        gsap.to(batch, {
          opacity: 1,
          y: 0,
          duration: conditions.reduce ? 0.5 : 0.8,
          ease: conditions.reduce ? 'none' : 'power3.out',
          stagger: 0.08,
          overwrite: true
        });
      }
    });
  }

  /* ── the hero: the window leans back, then lies flat ─────────── */

  function heroScroll(conditions) {
    var win = document.querySelector('.shot--hero .window');
    if (win) {
      var from = conditions.full
        ? { transformPerspective: 1400, y: TUNE.heroLift, rotateX: TUNE.heroTilt, scale: TUNE.heroScale }
        : { y: TUNE.heroLift / 2, scale: 0.96 };
      gsap.fromTo(win, from, {
        y: 0, rotateX: 0, scale: 1, ease: 'none',
        scrollTrigger: { trigger: win, start: 0, end: TUNE.heroFlatAt, scrub: TUNE.scrub }
      });
    }
    var hero = document.querySelector('.hero');
    var glow = document.querySelector('.hero__glow');
    if (glow) {
      gsap.to(glow, {
        yPercent: TUNE.glowDrift, opacity: 0.55, ease: 'none',
        scrollTrigger: { trigger: hero, start: 'top top', end: 'bottom top', scrub: true }
      });
    }
    var copy = document.querySelector('.hero__copy');
    if (copy && conditions.full) {
      gsap.to(copy, {
        y: -50, opacity: 0.25, ease: 'none',
        scrollTrigger: { trigger: hero, start: 'top top', end: '+=560', scrub: TUNE.scrub }
      });
    }
  }

  /* ── the chat view: its tasks panel, part by part ──────────────
     The panel is one capture (img/tasks, 580 × 1400 at its widest).
     Over it go veils of the capture's own background, one per part,
     and the five progress bars, redrawn in the capture's colours
     exactly where it has them, so they can fill. The panel comes in
     part by part; then, pinned on a wide screen, each point of the
     copy lights the part of the panel it is about. */

  var PANEL = { w: 580, h: 1400, bg: '#1a1c1e', track: '#2c2e2f', fill: '#3e8bff' };
  var PARTS = [[0, 172], [172, 505], [505, 1148], [1148, 1400]]; // Tasks, Needs you, In progress, Review
  var BARS = [[26, 88, 234], [42, 655, 256], [42, 859, 170], [42, 1063, 64], [42, 1289, 406]]; // x, y, width; 12 high

  function chat(conditions, cleanups) {
    var section = document.getElementById('chat');
    var panel = section && section.querySelector('.window--panel');
    if (!panel) return [];
    var split = section.querySelector('.split');
    var points = Q('.points li', section);

    var fx = document.createElement('div');
    fx.className = 'tfx';
    fx.setAttribute('aria-hidden', 'true');
    var fills = BARS.map(function (b) {
      var bar = document.createElement('i');
      bar.className = 'tfx__bar';
      place(bar, b[0] - 0.5, b[1] - 0.5, b[2] + 1, 13);
      bar.style.background = PANEL.track;
      var fill = document.createElement('i');
      fill.className = 'tfx__fill';
      fill.style.background = PANEL.fill;
      bar.appendChild(fill);
      fx.appendChild(bar);
      return fill;
    });
    var veils = PARTS.map(function (p) {
      var veil = document.createElement('i');
      veil.className = 'tfx__veil';
      place(veil, 0, p[0], PANEL.w, p[1] - p[0]);
      veil.style.background = PANEL.bg;
      fx.appendChild(veil);
      return veil;
    });
    panel.appendChild(fx);
    cleanups.push(function () { fx.remove(); });

    // Coming in: the frame rises, its parts appear top to bottom.
    var come = gsap.timeline({
      scrollTrigger: { trigger: panel, start: 'top 92%', end: 'top 30%', scrub: TUNE.scrub }
    });
    come
      .fromTo(panel, { y: conditions.full ? 70 : 30 }, { y: 0, ease: 'none', duration: 1 }, 0)
      .fromTo(veils, { opacity: 1 }, { opacity: 0, ease: 'none', duration: 0.3, stagger: 0.18 }, 0.1);

    var fillFrom = { scaleX: 0, transformOrigin: '0 50%' };
    var roomy = split && conditions.full &&
      window.innerHeight - 52 > split.offsetHeight + 24;
    if (!roomy) {
      come.fromTo(fills, fillFrom, { scaleX: 1, ease: 'power1.out', duration: 0.4, stagger: 0.06 }, 0.55);
      return points; // revealed with the rest
    }

    // Pinned: one point at a time, and the part of the panel it is about.
    var dim = 0.62;
    var lit = gsap.timeline({
      defaults: { ease: 'power1.inOut', duration: 1 },
      scrollTrigger: {
        trigger: split,
        start: function () { return 'center ' + (52 + (window.innerHeight - 52) / 2) + 'px'; },
        end: '+=' + Math.round(TUNE.chatPin * 100) + '%',
        pin: true,
        scrub: TUNE.scrub,
        invalidateOnRefresh: true
      }
    });
    lit
      // 1. tasks with their progress: In progress and Review lit, the bars fill
      .fromTo([points[1], points[2]], { opacity: 1 }, { opacity: 0.3, immediateRender: false }, 0)
      .fromTo([veils[0], veils[1]], { opacity: 0 }, { opacity: dim, immediateRender: false }, 0)
      .fromTo(fills, fillFrom, { scaleX: 1, ease: 'power2.out', duration: 1.2, stagger: 0.12 }, 0.3)
      // 2. what waits for you: Needs you lit
      .to(points[0], { opacity: 0.3 }, 2.2)
      .to(points[1], { opacity: 1 }, 2.2)
      .to(veils[1], { opacity: 0 }, 2.2)
      .to([veils[2], veils[3]], { opacity: dim }, 2.2)
      // 3. previews and kits: the whole panel again
      .to(points[1], { opacity: 0.3 }, 3.6)
      .to(points[2], { opacity: 1 }, 3.6)
      .to(veils, { opacity: 0 }, 3.6)
      .to([points[0], points[1]], { opacity: 1 }, 4.8)
      .to({}, { duration: 0.4 });
    return [];
  }

  function place(el, x, y, w, h) {
    el.style.left = (x / PANEL.w) * 100 + '%';
    el.style.top = (y / PANEL.h) * 100 + '%';
    el.style.width = (w / PANEL.w) * 100 + '%';
    el.style.height = (h / PANEL.h) * 100 + '%';
  }

  /* ── the delivery: its four stages, driven by the scroll ───────
     Written done; the scroll walks it from pending to landed and
     back. A stage runs over its stretch of the card's way up the
     screen, and the check and the review take the longest. */

  var STAGES = [[0.04, 0.18], [0.22, 0.46], [0.5, 0.74], [0.78, 0.9]];

  function delivery(conditions, cleanups) {
    var box = document.getElementById('delivery');
    if (!box) return;
    var steps = Q('.dstep', box);
    var states = steps.map(function () { return ''; });
    function render(p) {
      steps.forEach(function (step, i) {
        var s = p < STAGES[i][0] ? 'pending' : p < STAGES[i][1] ? 'running' : 'done';
        if (s === states[i]) return;
        if (s === 'done' && states[i] === 'running') {
          gsap.fromTo(step.querySelector('.dstep__mark'), { scale: 0.6 }, { scale: 1, duration: 0.35, ease: 'back.out(3)' });
        }
        states[i] = s;
        step.dataset.state = s;
      });
    }
    var st = ScrollTrigger.create({
      trigger: box,
      start: conditions.full ? 'top 82%' : 'top 88%',
      end: conditions.full ? 'bottom 38%' : 'bottom 45%',
      onUpdate: function (self) { render(self.progress); }
    });
    // Once every trigger has measured again (a resize, a font), the
    // card shows the stage its place on the screen says.
    function settle() { render(st.progress); }
    settle();
    ScrollTrigger.addEventListener('refresh', settle);
    cleanups.push(function () {
      ScrollTrigger.removeEventListener('refresh', settle);
      steps.forEach(function (step) { step.dataset.state = 'done'; });
    });
  }

  /* ── the other windows lean back as they come in ─────────────── */

  function windows(conditions) {
    [['#orchestrator .shot .window', 0], ['#kits .shot .window', -6]].forEach(function (w) {
      var el = document.querySelector(w[0]);
      if (!el) return;
      var from = conditions.full
        ? { transformPerspective: 1600, y: 80, rotateX: TUNE.tilt, rotateY: w[1], scale: 0.93 }
        : { y: 36, scale: 0.97 };
      gsap.fromTo(el, from, {
        y: 0, rotateX: 0, rotateY: 0, scale: 1, ease: 'none',
        scrollTrigger: { trigger: el, start: 'top bottom', end: 'top 30%', scrub: TUNE.scrub }
      });
    });
  }

  /* ── Hangar Mobile: the phones come in one after the other ────── */

  function phones(conditions) {
    var items = Q('.phones__item');
    if (!items.length) return [];
    if (!conditions.full) return items; // a fade, with the rest
    gsap.fromTo(items, {
      transformPerspective: 1200,
      y: 120,
      z: -220,
      rotateY: function (i) { return (1.5 - i) * 9; },
      opacity: 0
    }, {
      y: 0, z: 0, rotateY: 0, opacity: 1, ease: 'none', stagger: 0.14,
      scrollTrigger: { trigger: '.phones', start: 'top 96%', end: 'top 38%', scrub: TUNE.scrub }
    });
    return [];
  }

  /* ── performance: figures count up, bars go from before to after ─
     Each figure gets a bar under it, drawn here: the release before
     is the whole bar, this one the part it is of it. A figure whose
     "before" is no number (no echo within a minute) gets none. */

  var UNITS = { '%': 1, s: 1, ms: 0.001, MB: 1, MiB: 1, GiB: 1024 };

  var figures = Q('.perf__item').map(function (item) {
    var now = item.querySelector('.perf__now');
    var unit = now && now.querySelector('small');
    var text = now && now.firstChild;
    if (!text || text.nodeType !== 3 || !unit) return null;
    // The number in a box of its own, held at its final width while it
    // counts, so the unit after it never moves.
    var num = document.createElement('span');
    num.className = 'perf__num';
    now.insertBefore(num, text);
    num.appendChild(text);
    var value = parseFloat(text.nodeValue);
    var was = (item.querySelector('.perf__was') || {}).textContent || '';
    var m = was.match(/([\d.]+)\s*(%|ms|s|MB|MiB|GiB)\s*$/);
    var ratio = m && UNITS[unit.textContent] && UNITS[m[2]]
      ? (value * UNITS[unit.textContent]) / (parseFloat(m[1]) * UNITS[m[2]])
      : null;
    var bar = null;
    if (ratio !== null) {
      bar = document.createElement('span');
      bar.className = 'perf__bar';
      bar.setAttribute('aria-hidden', 'true');
      bar.innerHTML = '<i class="perf__bar-was"></i><i class="perf__bar-now"></i>';
      bar.lastChild.style.transform = 'scaleX(' + Math.max(ratio, 0.012).toFixed(4) + ')';
      item.appendChild(bar);
    }
    return {
      item: item, num: num, text: text, value: value, final: text.nodeValue,
      decimals: (text.nodeValue.split('.')[1] || '').length, bar: bar,
      ratio: ratio === null ? 0 : Math.max(ratio, 0.012)
    };
  }).filter(Boolean);

  function performance(conditions, cleanups) {
    if (conditions.reduce) return;
    var widths = figures.map(function (f) { return f.num.getBoundingClientRect().width; });
    figures.forEach(function (f, i) {
      var show = { v: 0 };
      f.num.style.minWidth = widths[i] + 'px';
      if (f.bar) gsap.set([f.bar.firstChild, f.bar.lastChild], { scaleX: 0 });
      f.text.nodeValue = (0).toFixed(f.decimals);
      ScrollTrigger.create({
        trigger: f.item,
        start: 'top 96%',
        once: true,
        onEnter: function () {
          var tl = gsap.timeline({ delay: (i % 4) * 0.08 });
          if (f.bar) tl.to(f.bar.firstChild, { scaleX: 1, duration: 0.5, ease: 'power2.out' }, 0);
          tl.to(show, {
            v: f.value, duration: TUNE.countFor, ease: 'power3.out',
            onUpdate: function () { f.text.nodeValue = show.v.toFixed(f.decimals); },
            onComplete: function () { f.text.nodeValue = f.final; }
          }, f.bar ? 0.35 : 0);
          if (f.bar) tl.to(f.bar.lastChild, { scaleX: f.ratio, duration: TUNE.countFor, ease: 'power3.out' }, 0.35);
        }
      });
    });
    cleanups.push(function () {
      figures.forEach(function (f) {
        f.text.nodeValue = f.final;
        f.num.style.minWidth = '';
        if (f.bar) gsap.set(f.bar.lastChild, { scaleX: f.ratio });
      });
    });
  }

  /* ── links within the page glide to their section ──────────────
     The page's own smooth scrolling is off while this runs (it would
     fight ScrollTrigger); a link to a question still jumps, since
     main.js opens it. Any wheel, touch or key stops the glide. */

  var glide = null;
  function stopGlide() {
    if (glide) { glide.kill(); glide = null; }
  }
  ['wheel', 'touchstart', 'keydown'].forEach(function (type) {
    window.addEventListener(type, stopGlide, { passive: true });
  });

  document.addEventListener('click', function (e) {
    if (!allowed || e.defaultPrevented || e.button || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    var a = e.target.closest && e.target.closest('a[href^="#"]');
    var id = a && a.getAttribute('href').slice(1);
    if (!id || id === 'main') return;
    var target = document.getElementById(id);
    if (!target || target.tagName === 'DETAILS' || target.closest('details')) return;
    e.preventDefault();
    var pad = parseFloat(getComputedStyle(root).scrollPaddingTop) || 0;
    var to = id === 'top' ? 0 : target.getBoundingClientRect().top + window.scrollY - pad;
    to = gsap.utils.clamp(0, ScrollTrigger.maxScroll(window), to);
    var at = { y: window.scrollY };
    stopGlide();
    glide = gsap.to(at, {
      y: to,
      duration: gsap.utils.clamp(0.5, 1.3, Math.abs(to - at.y) / 2800),
      ease: 'power3.inOut',
      onUpdate: function () { window.scrollTo(0, at.y); },
      onComplete: function () {
        glide = null;
        if (!target.hasAttribute('tabindex') && !/^(A|BUTTON|INPUT|SELECT|TEXTAREA)$/.test(target.tagName)) {
          target.setAttribute('tabindex', '-1');
        }
        target.focus({ preventScroll: true });
      }
    });
    if (window.location.hash !== '#' + id) history.pushState(null, '', '#' + id);
  });

  /* ── one set per kind of screen ─────────────────────────────── */

  if (allowed) heroIntro();
  root.classList.add('motion-ready');

  var mm = gsap.matchMedia();
  mm.add({
    full: '(min-width: 960px) and (prefers-reduced-motion: no-preference)',
    light: '(max-width: 959px) and (prefers-reduced-motion: no-preference)',
    reduce: '(prefers-reduced-motion: reduce)'
  }, function (context) {
    var c = context.conditions;
    var cleanups = [];
    var extra = [];
    if (!c.reduce) {
      // In the page's order, the pin first of all: what comes after it
      // is measured with the room it takes.
      heroScroll(c);
      extra = extra.concat(chat(c, cleanups));
      windows(c);
      delivery(c, cleanups);
      extra = extra.concat(phones(c));
    }
    headings(c, cleanups);
    reveals(c, extra);
    performance(c, cleanups);
    return function () {
      cleanups.forEach(function (fn) { fn(); });
    };
  });

  // Inter swaps in after the first layout; measure again once it has.
  if (document.fonts && document.fonts.ready) {
    document.fonts.ready.then(function () { ScrollTrigger.refresh(); });
  }
  window.addEventListener('load', function () { ScrollTrigger.refresh(); });
})();
