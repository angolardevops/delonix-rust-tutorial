/* Delonix Rust Tutorial — interacção: tema, realce de código, copiar, TOC, menu e pesquisa (Ctrl+K). */
(function () {
  'use strict';
  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var root = document.documentElement;

  /* ── Tema: auto → claro → escuro ─────────────────────────────────────────────────────── */
  var themeBtn = $('#theme-btn');
  function themeLabel() { var t = root.getAttribute('data-theme'); return t === 'dark' ? '☾' : t === 'light' ? '☀' : '◐'; }
  themeBtn.textContent = themeLabel();
  themeBtn.addEventListener('click', function () {
    var order = ['auto', 'light', 'dark'], cur = root.getAttribute('data-theme') || 'auto';
    var next = order[(order.indexOf(cur) + 1) % 3];
    root.setAttribute('data-theme', next);
    try { localStorage.setItem('dxr-theme', next); } catch (e) {}
    themeBtn.textContent = themeLabel();
    themeBtn.title = 'Tema: ' + next;
  });

  /* ── Código: realce, botão copiar, etiqueta da linguagem ─────────────────────────────── */
  $$('pre > code').forEach(function (code) {
    var pre = code.parentNode, m = /language-(\w+)/.exec(code.className || ''), lang = m ? m[1] : '';
    if (window.hljs && lang && lang !== 'plaintext' && lang !== 'text') {
      try { window.hljs.highlightElement(code); } catch (e) {}
    }
    if (pre.closest('.term')) return;
    var holder = pre.closest('.code-fig') || pre;
    var btn = document.createElement('button');
    btn.className = 'copy'; btn.type = 'button'; btn.textContent = 'Copiar'; btn.setAttribute('aria-label', 'Copiar código');
    btn.addEventListener('click', function (ev) {
      ev.preventDefault(); ev.stopPropagation();
      var done = function () { btn.textContent = 'Copiado ✓'; btn.classList.add('ok'); setTimeout(function () { btn.textContent = 'Copiar'; btn.classList.remove('ok'); }, 1600); };
      if (navigator.clipboard) navigator.clipboard.writeText(code.textContent).then(done, function () {});
    });
    holder.appendChild(btn);
    if (lang) { var tag = document.createElement('span'); tag.className = 'lang-tag'; tag.textContent = lang; holder.appendChild(tag); }
  });

  /* ── Progresso de leitura + TOC (scrollspy) ──────────────────────────────────────────── */
  var bar = $('#progress'), links = $$('.toc a'), heads = links.map(function (a) { return document.getElementById(a.getAttribute('href').slice(1)); });
  function onScroll() {
    var h = document.documentElement, max = h.scrollHeight - h.clientHeight;
    bar.style.width = (max > 0 ? (h.scrollTop / max) * 100 : 0) + '%';
    var cur = -1;
    for (var i = 0; i < heads.length; i++) { if (heads[i] && heads[i].getBoundingClientRect().top < 110) cur = i; }
    links.forEach(function (a, i) { a.classList.toggle('on', i === cur); });
  }
  window.addEventListener('scroll', onScroll, { passive: true }); onScroll();
  var active = $('.sidebar a.active'); if (active && active.scrollIntoView) active.scrollIntoView({ block: 'center' });

  /* ── Menu móvel ──────────────────────────────────────────────────────────────────────── */
  var menu = $('.menu-btn');
  menu.addEventListener('click', function () { document.body.classList.toggle('menu-open'); });
  $('#scrim').addEventListener('click', function () { document.body.classList.remove('menu-open'); });
  $$('.sidebar a').forEach(function (a) { a.addEventListener('click', function () { document.body.classList.remove('menu-open'); }); });

  /* ── Pesquisa (Ctrl/⌘ + K, ou «/») ───────────────────────────────────────────────────── */
  var pal = $('#palette'), input = $('#pal-q'), list = $('#pal-list'), count = $('#pal-count');
  var index = null, loading = null, results = [], sel = 0, lastFocus = null;
  var isMac = /Mac|iPhone|iPad/.test(navigator.platform);
  $('#kbd-hint').textContent = isMac ? '⌘ K' : 'Ctrl K';

  function norm(s) { return s.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, ''); }
  function esc(s) { return s.replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); }

  function load() {
    if (index) return Promise.resolve(index);
    if (!loading) loading = fetch('search-index.json').then(function (r) { return r.json(); }).then(function (d) {
      d.forEach(function (e) { e._h = norm(e.h); e._t = norm(e.t); e._x = norm(e.x); });
      index = d; return d;
    });
    return loading;
  }

  function search(q) {
    var toks = norm(q).split(/[^a-z0-9_]+/).filter(Boolean);
    if (!toks.length) return [];
    var out = [];
    index.forEach(function (e) {
      var score = 0;
      for (var i = 0; i < toks.length; i++) {
        var t = toks[i], s = 0;
        if (e._h.indexOf(t) >= 0) s += e._h === t ? 14 : (e._h.split(/[^a-z0-9_]+/).indexOf(t) >= 0 ? 9 : 6);
        if (e._t.indexOf(t) >= 0) s += 3;
        var pos = e._x.indexOf(t);
        if (pos >= 0) {
          var occ = 0, at = -1; while (occ < 8 && (at = e._x.indexOf(t, at + 1)) >= 0) occ++;   // frequência (com tecto)
          s += 2 + (e._x.indexOf(' ' + t) >= 0 ? 1 : 0) + occ * 0.9;
        }
        if (!s) return;               // todos os termos têm de aparecer (AND)
        score += s;
      }
      if (e._h.indexOf(norm(q).trim()) >= 0) score += 8;   // a frase inteira no título
      if (e.p === 'index') score -= 6;                      // a página inicial é só um mapa: não deve ganhar a conteúdo
      out.push({ e: e, s: score });
    });
    out.sort(function (a, b) { return b.s - a.s; });
    return out.slice(0, 14).map(function (r) { return r.e; });
  }

  function mark(text, toks) {
    var n = norm(text), ranges = [];
    toks.forEach(function (t) { var i = -1; while ((i = n.indexOf(t, i + 1)) >= 0) { ranges.push([i, i + t.length]); } });
    ranges.sort(function (a, b) { return a[0] - b[0]; });
    var out = '', pos = 0;
    ranges.forEach(function (r) { if (r[0] < pos) return; out += esc(text.slice(pos, r[0])) + '<mark>' + esc(text.slice(r[0], r[1])) + '</mark>'; pos = r[1]; });
    return out + esc(text.slice(pos));
  }

  function excerpt(e, toks) {
    var n = e._x, at = -1;
    for (var i = 0; i < toks.length && at < 0; i++) at = n.indexOf(toks[i]);
    var start = Math.max(0, at - 50), s = e.x.slice(start, start + 170);
    return (start > 0 ? '…' : '') + s + (start + 170 < e.x.length ? '…' : '');
  }

  function render(q) {
    var toks = norm(q).split(/[^a-z0-9_]+/).filter(Boolean);
    results = q.trim() ? search(q) : [];
    sel = 0;
    if (!q.trim()) {
      list.innerHTML = '<li class="pal-empty">Escreve para pesquisar em todos os capítulos, excertos de código e saídas reais.<br><small>Experimenta: <code>pivot_root</code>, <code>digest</code>, <code>cgroup</code>, <code>typestate</code></small></li>';
      count.textContent = ''; return;
    }
    if (!results.length) { list.innerHTML = '<li class="pal-empty">Nada encontrado para «' + esc(q) + '».</li>'; count.textContent = '0 resultados'; return; }
    list.innerHTML = results.map(function (e, i) {
      var href = e.p + '.html' + (e.a ? '#' + e.a : '');
      var head = e.h || e.t;
      return '<li role="option" data-i="' + i + '" aria-selected="' + (i === 0) + '"><a href="' + href + '">' +
        '<div class="r-top"><span class="h">' + mark(head, toks) + '</span><span class="r-page">' + esc(e.t) + '</span></div>' +
        (e.x ? '<div class="r-txt">' + mark(excerpt(e, toks), toks) + '</div>' : '') + '</a></li>';
    }).join('');
    count.textContent = results.length + (results.length === 1 ? ' resultado' : ' resultados');
  }

  function select(i) {
    var items = $$('#pal-list li[role=option]'); if (!items.length) return;
    sel = (i + items.length) % items.length;
    items.forEach(function (li, k) { li.setAttribute('aria-selected', k === sel); });
    items[sel].scrollIntoView({ block: 'nearest' });
  }

  function open() {
    lastFocus = document.activeElement; pal.hidden = false; document.body.style.overflow = 'hidden';
    input.value = ''; render(''); input.focus(); load();
  }
  function close() { pal.hidden = true; document.body.style.overflow = ''; if (lastFocus && lastFocus.focus) lastFocus.focus(); }

  input.addEventListener('input', function () { load().then(function () { render(input.value); }); });
  input.addEventListener('keydown', function (ev) {
    if (ev.key === 'ArrowDown') { ev.preventDefault(); select(sel + 1); }
    else if (ev.key === 'ArrowUp') { ev.preventDefault(); select(sel - 1); }
    else if (ev.key === 'Enter') { var a = $$('#pal-list li[role=option] a')[sel]; if (a) { close(); window.location.href = a.getAttribute('href'); } }
  });
  list.addEventListener('mousemove', function (ev) { var li = ev.target.closest && ev.target.closest('li[role=option]'); if (li) select(+li.getAttribute('data-i')); });
  list.addEventListener('click', function () { close(); });
  pal.addEventListener('mousedown', function (ev) { if (ev.target === pal) close(); });
  $('#search-open').addEventListener('click', open);

  document.addEventListener('keydown', function (ev) {
    var k = ev.key.toLowerCase();
    if ((ev.ctrlKey || ev.metaKey) && k === 'k') { ev.preventDefault(); pal.hidden ? open() : close(); }
    else if (k === 'escape' && !pal.hidden) { ev.preventDefault(); close(); }
    else if (k === '/' && pal.hidden && !/input|textarea|select/i.test((document.activeElement || {}).tagName || '')) { ev.preventDefault(); open(); }
  });
})();
