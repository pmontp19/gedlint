// gedlint web viewer (issue 15). Vanilla JS, no framework, no build step
// beyond the wasm embedding done by scripts/build-web.sh. All engine work
// happens in worker.js; this file owns state, rendering and file I/O.
'use strict';

(() => {
  const $ = (sel) => document.querySelector(sel);
  const TEXT = new TextDecoder('utf-8', { fatal: false });
  const PAGE_LIMIT = 200;

  const state = {
    worker: null,
    ready: false,
    busy: false,
    fileName: null,
    bytes: null,       // original bytes of the current file
    normBytes: null,   // lone-CR normalized, for line addressing (lazy)
    lines: null,       // Uint8Array[] of normBytes without terminators (lazy)
    lineText: new Map(), // lineno -> decoded text, for search
    report: null,
    reportRaw: null,
    registry: new Map(), // code -> rule meta
    edits: [],
    normalizedEndings: false,
    appliedBanner: null, // {appliedGroups, postponed, normalizedEndings, fileName}
    baselineText: null,  // loaded baseline file text, null = none
    baselineName: null,  // file name it was loaded from
    baselineMatch: null, // {known: number[], baselined, resolved, resolved_total}
    filters: { sev: 'all', cats: new Set(), q: '' },
    shown: PAGE_LIMIT,
    shownKnown: PAGE_LIMIT,
  };

  // ---------------------------------------------------------------------
  // Worker RPC
  // ---------------------------------------------------------------------

  let rpcId = 0;
  const pending = new Map();

  function call(type, extra, transfer) {
    return new Promise((resolve, reject) => {
      const id = ++rpcId;
      pending.set(id, { resolve, reject });
      state.worker.postMessage(Object.assign({ type, id }, extra), transfer || []);
    });
  }

  state.worker = new Worker('worker.js');
  state.worker.onmessage = (ev) => {
    const m = ev.data;
    if (m.type === 'ready') {
      state.ready = true;
      setStatus('');
      $('#example').disabled = false;
      loadRegistry();
      return;
    }
    if (m.type === 'boot-error') {
      setStatus('The checking engine failed to load: ' + m.error, 'error');
      return;
    }
    if (m.type === 'phase') {
      setStatus(cap(m.label) + '\u2026');
      return;
    }
    const p = pending.get(m.id);
    if (!p) return;
    pending.delete(m.id);
    if (m.ok) p.resolve(m);
    else p.reject(new Error(m.error));
  };
  state.worker.onerror = (e) => setStatus('Worker error: ' + (e.message || 'unknown'), 'error');

  async function loadRegistry() {
    const m = await call('registry');
    for (const r of JSON.parse(m.json)) state.registry.set(r.code, r);
  }

  // ---------------------------------------------------------------------
  // Status
  // ---------------------------------------------------------------------

  function setStatus(text, kind) {
    const el = $('#status');
    el.textContent = text;
    el.classList.toggle('error', kind === 'error');
    el.hidden = !text;
  }

  const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);

  // ---------------------------------------------------------------------
  // File loading
  // ---------------------------------------------------------------------

  function loadBytes(bytes, name) {
    if (!state.ready || state.busy) return;
    state.busy = true;
    state.fileName = name;
    state.appliedBanner = null;
    state.normBytes = null;
    state.lines = null;
    state.lineText.clear();
    setStatus('Analyzing ' + name + '\u2026');
    check(bytes)
      .then(() => {
        state.busy = false;
        $('#results').hidden = false;
        $('#settings').hidden = false;
        $('#results').scrollIntoView({ behavior: 'smooth', block: 'start' });
      })
      .catch((e) => {
        state.busy = false;
        setStatus('Analysis failed: ' + e.message, 'error');
      });
  }

  function configText() {
    const presets = [];
    if ($('#preset-hispanic').checked) presets.push('hispanic-naming');
    if ($('#preset-hygiene').checked) presets.push('hygiene');
    if (!presets.length) return '';
    return '[lints]\npresets = ["recommended", "' + presets.join('", "') + '"]\n';
  }

  async function check(bytes) {
    state.bytes = bytes;
    // The line cache belongs to the previous bytes: after a repair (or a
    // re-check of different bytes) snippet rows and search would render
    // against the old line numbering otherwise.
    state.normBytes = null;
    state.lines = null;
    state.lineText.clear();
    const cfg = configText();
    const m = await call('check', { data: bytes.buffer.slice(0), config: cfg });
    const parsed = JSON.parse(m.json);
    if (parsed.error) throw new Error(parsed.error);
    state.report = parsed;
    state.reportRaw = m.json;
    // The baseline comparison pairs with this run: same bytes, same
    // config (the known flags index this report's diagnostics).
    await matchBaseline(bytes, cfg);
    await refreshEdits();
    renderAll();
    setStatus('');
  }

  // Compare the current run against the loaded baseline, in the engine.
  // No baseline loaded: nothing to do, findings render unsplit.
  async function matchBaseline(bytes, cfg) {
    state.baselineMatch = null;
    if (state.baselineText === null) return;
    const m = await call('baseline-match', {
      data: bytes.buffer.slice(0),
      baseline: state.baselineText,
      config: cfg,
    });
    const parsed = JSON.parse(m.json);
    if (parsed.error) throw new Error(parsed.error);
    if (parsed.known.length !== state.report.diagnostics.length) {
      throw new Error('the comparison is out of step with this run; check the file again');
    }
    state.baselineMatch = parsed;
  }

  async function refreshEdits() {
    // Same config string check() lints with (configText reads the same
    // checkboxes): the repair list must follow them, and applyRepairs()
    // indexes its mask into exactly this list.
    const m = await call('edits', { data: state.bytes.buffer.slice(0), config: configText() });
    const parsed = JSON.parse(m.json);
    if (parsed.error) throw new Error(parsed.error);
    state.edits = parsed.edits;
    state.normalizedEndings = parsed.normalized_endings;
  }

  function readFile(file) {
    if (state.busy || !state.ready) return;
    const reader = new FileReader();
    reader.onprogress = (e) => {
      if (e.lengthComputable) setStatus('Reading ' + file.name + '\u2026 ' + Math.round((e.loaded / e.total) * 100) + '%');
    };
    reader.onerror = () => setStatus('Could not read the file.', 'error');
    reader.onload = () => loadBytes(new Uint8Array(reader.result), file.name);
    reader.readAsArrayBuffer(file);
  }

  function loadExample() {
    const bin = atob(EXAMPLE_GED_B64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    loadBytes(bytes, 'example.ged');
  }

  // ---------------------------------------------------------------------
  // Baseline (issue 52): record what you have already seen, then work
  // through only what is new. Everything is computed by the engine in the
  // worker; this side only downloads and uploads text.
  // ---------------------------------------------------------------------

  function baselineFileName() {
    return state.fileName.replace(/(\.ged)?$/i, '') + '.baseline.json';
  }

  async function saveBaseline() {
    if (state.busy || !state.report) return;
    state.busy = true;
    setStatus('Writing the baseline\u2026');
    try {
      const m = await call('baseline', { data: state.bytes.buffer.slice(0), config: configText() });
      const parsed = JSON.parse(m.json);
      if (parsed.error) throw new Error(parsed.error);
      // Download, and adopt what was saved: from now on the run is
      // compared against this snapshot. Re-saving after fixing is the
      // ratchet: findings that vanished drop out of the file.
      state.baselineText = m.json;
      state.baselineName = baselineFileName();
      download(m.json, state.baselineName, 'application/json');
      await matchBaseline(state.bytes, configText());
      renderAll();
      setStatus('Baseline saved as ' + state.baselineName);
    } catch (e) {
      setStatus('Saving the baseline failed: ' + e.message, 'error');
    } finally {
      state.busy = false;
    }
  }

  function loadBaselineFile(file) {
    if (state.busy || !state.ready || !state.report) return;
    const reader = new FileReader();
    reader.onerror = () => setStatus('Could not read the baseline file.', 'error');
    reader.onload = () => adoptBaseline(String(reader.result), file.name);
    reader.readAsText(file);
  }

  async function adoptBaseline(text, name) {
    state.busy = true;
    setStatus('Matching ' + name + '\u2026');
    try {
      state.baselineText = text;
      state.baselineName = name;
      await matchBaseline(state.bytes, configText());
      renderAll();
      setStatus('Baseline loaded: ' + name);
    } catch (e) {
      state.baselineText = null;
      state.baselineName = null;
      state.baselineMatch = null;
      renderAll();
      setStatus('Could not use that baseline: ' + e.message, 'error');
    } finally {
      state.busy = false;
    }
  }

  function removeBaseline() {
    state.baselineText = null;
    state.baselineName = null;
    state.baselineMatch = null;
    renderAll();
    setStatus('');
  }

  // ---------------------------------------------------------------------
  // Line addressing (mirrors the engine: normalize lone CR, split on LF,
  // drop the terminator; spans are byte offsets into what is left)
  // ---------------------------------------------------------------------

  function getLines() {
    if (state.lines) return state.lines;
    const b = state.bytes;
    let norm = b;
    for (let i = 0; i < b.length; i++) {
      if (b[i] === 0x0D && (i + 1 === b.length || b[i + 1] !== 0x0A)) {
        norm = new Uint8Array(b.length);
        norm.set(b.subarray(0, i));
        let w = i;
        for (let j = i; j < b.length; j++) {
          if (b[j] === 0x0D && (j + 1 === b.length || b[j + 1] !== 0x0A)) {
            norm[w++] = 0x0A;
          } else {
            norm[w++] = b[j];
          }
        }
        break;
      }
    }
    state.normBytes = norm;
    const out = [];
    let start = 0;
    for (let i = 0; i <= norm.length; i++) {
      if (i === norm.length || norm[i] === 0x0A) {
        let end = i;
        if (end > start && norm[end - 1] === 0x0D) end--; // CRLF: CR belongs to the terminator
        out.push(norm.subarray(start, end));
        start = i + 1;
      }
    }
    state.lines = out;
    return out;
  }

  function lineTextAt(no) {
    if (state.lineText.has(no)) return state.lineText.get(no);
    const lines = getLines();
    const text = no >= 1 && no <= lines.length ? TEXT.decode(lines[no - 1]) : '';
    state.lineText.set(no, text);
    return text;
  }

  // ---------------------------------------------------------------------
  // Rendering: summary
  // ---------------------------------------------------------------------

  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => (
    { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
  ));

  function headValue(tag) {
    // Cheap HEAD scan for CHAR/SOUR: the first "1 TAG value" before the
    // first level-0 record after HEAD.
    const lines = getLines();
    let inHead = false;
    for (let i = 0; i < lines.length && i < 60; i++) {
      const t = TEXT.decode(lines[i]);
      if (/^0 /.test(t)) {
        if (inHead) break;
        inHead = /^0 HEAD/.test(t);
      } else if (inHead && t.startsWith('1 ' + tag + ' ')) {
        return t.slice(2 + tag.length + 1).trim();
      }
    }
    return null;
  }

  function healthBadge(rep) {
    const s = rep.summary;
    if (s.errors > 0) return ['Structural errors', 'error'];
    if (s.warnings > 0) return ['Import warnings', 'warn'];
    if (s.infos > 0) return ['Minor notes', 'info'];
    return ['Clean', 'clean'];
  }

  function renderSummary() {
    const rep = state.report;
    const [label, cls] = healthBadge(rep);
    const bom = state.bytes.length >= 3 && state.bytes[0] === 0xEF && state.bytes[1] === 0xBB && state.bytes[2] === 0xBF;
    let charset = headValue('CHAR') || 'not declared';
    if (bom) charset += ' with BOM';
    const sour = headValue('SOUR') || 'unknown';
    const s = rep.summary;
    const sevPill = (key, n, label2) =>
      `<button type="button" class="pill sev-${key}${state.filters.sev === key ? ' on' : ''}" data-sev="${key}">` +
      `<span class="dot"></span>${label2} <b>${n}</b></button>`;

    const total = s.errors + s.warnings + s.infos;
    const people = rep.individuals + (rep.individuals === 1 ? ' person' : ' people');
    const fams = rep.families + (rep.families === 1 ? ' family' : ' families');
    $('#summary').innerHTML =
      `<div class="sum-row1">
         <span class="health ${cls}">${label}</span>
         <span class="sum-file" title="${esc(state.fileName)}">${esc(state.fileName)}</span>
         <span class="sum-actions">
           <button id="dl-report" class="btn btn-ghost btn-small" type="button">Download report (JSON)</button>
           <button id="new-file" class="btn btn-ghost btn-small" type="button">Check another file</button>
         </span>
       </div>
       <dl class="sum-meta">
         <div><dt>Format</dt><dd>GEDCOM ${esc(rep.version)}</dd></div>
         <div><dt>Characters</dt><dd>${esc(charset)}</dd></div>
         <div><dt>Exported by</dt><dd>${esc(sour)}</dd></div>
         <div><dt>Size</dt><dd>${rep.lines} lines &middot; ${people} &middot; ${fams}</dd></div>
       </dl>
       <div class="sum-pills" role="group" aria-label="Counts by severity">
         ${sevPill('error', s.errors, 'Errors')}
         ${sevPill('warning', s.warnings, 'Warnings')}
         ${sevPill('info', s.infos, 'Notes')}
         ${state.edits.length || state.normalizedEndings
           ? `<a class="pill repairs-link" href="#repairs"><span class="dot"></span>Repairs available <b>${state.edits.length}</b></a>` : ''}
       </div>
       ${total === 0 ? '<p class="allclean">No findings. This file should import cleanly.</p>' : ''}`;

    $('#dl-report').addEventListener('click', downloadReport);
    $('#new-file').addEventListener('click', () => $('#file').click());
    for (const b of document.querySelectorAll('#summary [data-sev]')) {
      b.addEventListener('click', () => {
        state.filters.sev = state.filters.sev === b.dataset.sev ? 'all' : b.dataset.sev;
        renderFindings();
        renderSummary();
      });
    }
  }

  // ---------------------------------------------------------------------
  // Rendering: baseline card
  // ---------------------------------------------------------------------

  function renderBaseline() {
    const sec = $('#baseline');
    if (!state.report) {
      sec.hidden = true;
      sec.innerHTML = '';
      return;
    }
    const total = state.report.diagnostics.length;
    const o = state.baselineMatch;

    if (!o) {
      if (!total) {
        sec.hidden = true;
        sec.innerHTML = '';
        return;
      }
      sec.hidden = false;
      sec.innerHTML =
        `<div class="bl-row">
           <div class="bl-main">
             <h2 id="baseline-title">A thousand findings is a lot to face at once</h2>
             <p class="hint">Save a baseline and this page will remember the ${total === 1 ? 'finding' : `<b>${total}</b> findings`} you
             have already seen. From then on only what is <em>new</em> asks for your attention, and the ones you fix
             are counted. Keep the downloaded file next to your tree and load it on your next visit.</p>
           </div>
           <div class="bl-actions">
             <button id="bl-save" class="btn btn-primary btn-small" type="button">Save baseline</button>
             <button id="bl-load" class="btn btn-ghost btn-small" type="button">Load a baseline</button>
           </div>
         </div>`;
      $('#bl-save').addEventListener('click', saveBaseline);
      $('#bl-load').addEventListener('click', () => $('#baseline-file').click());
      return;
    }

    const known = o.baselined;
    const fresh = state.report.diagnostics.length - known;
    const fixed = o.resolved_total;
    let headline;
    if (fixed > 0) {
      headline = `<p class="bl-fixed">You fixed <b>${fixed}</b> ${fixed === 1 ? 'finding' : 'findings'} since your baseline.` +
        (fresh ? ` <b>${fresh}</b> ${fresh === 1 ? 'is' : 'are'} new.` : ' Nothing new.') + `</p>`;
    } else if (fresh === 0) {
      headline = '<p class="bl-fixed">No new findings since your baseline.</p>';
    } else {
      headline = `<p class="bl-fixed"><b>${fresh}</b> new ${fresh === 1 ? 'finding' : 'findings'} since your baseline.</p>`;
    }
    const resolvedList = o.resolved.length
      ? `<ul class="bl-resolved">${o.resolved.map((e) => {
          const meta = state.registry.get(e.code);
          return `<li><span class="code">${esc(e.code)}</span> &times;${e.count}` +
            (meta ? ` ${esc(meta.title)}` : '') + '</li>';
        }).join('')}</ul>`
      : '';
    sec.hidden = false;
    sec.innerHTML =
      `<div class="bl-row">
         <div class="bl-main">
           <h2 id="baseline-title">Baseline: ${esc(state.baselineName || '')}</h2>
           ${headline}
           <p class="hint">${fresh} new &middot; ${known} already seen${o.resolved.length ? ` &middot; ${fixed} fixed` : ''}.
           ${fixed || fresh ? ' Update the baseline to record where you are now; fixed findings drop out of it.' : ''}</p>
           ${resolvedList}
         </div>
         <div class="bl-actions">
           <button id="bl-save" class="btn btn-primary btn-small" type="button">Update baseline</button>
           <button id="bl-load" class="btn btn-ghost btn-small" type="button">Load another baseline</button>
           <button id="bl-remove" class="btn btn-ghost btn-small" type="button">Stop comparing</button>
         </div>
       </div>`;
    $('#bl-save').addEventListener('click', saveBaseline);
    $('#bl-load').addEventListener('click', () => $('#baseline-file').click());
    $('#bl-remove').addEventListener('click', removeBaseline);
  }

  // ---------------------------------------------------------------------
  // Rendering: findings explorer
  // ---------------------------------------------------------------------

  function catCounts() {
    const out = new Map();
    for (const d of state.report.diagnostics) out.set(d.category, (out.get(d.category) || 0) + 1);
    return out;
  }

  function renderFilters() {
    const s = state.report.summary;
    const sevs = [
      ['all', 'All', s.errors + s.warnings + s.infos],
      ['error', 'Errors', s.errors],
      ['warning', 'Warnings', s.warnings],
      ['info', 'Notes', s.infos],
    ];
    $('#sev-pills').innerHTML = sevs.map(([k, l, n]) =>
      `<button type="button" class="seg-btn${state.filters.sev === k ? ' on' : ''}" data-sev="${k}">${l} <b>${n}</b></button>`
    ).join('');
    for (const b of document.querySelectorAll('#sev-pills [data-sev]')) {
      b.addEventListener('click', () => {
        state.filters.sev = b.dataset.sev;
        state.shown = PAGE_LIMIT;
        renderFindings();
        renderSummary();
      });
    }

    const cats = ['correctness', 'suspicious', 'style', 'upgrade'];
    const counts = catCounts();
    $('#cat-chips').innerHTML = cats.filter((c) => counts.has(c)).map((c) =>
      `<button type="button" class="chip${state.filters.cats.has(c) ? ' on' : ''}" data-cat="${c}">${c} <b>${counts.get(c)}</b></button>`
    ).join('');
    for (const b of document.querySelectorAll('#cat-chips [data-cat]')) {
      b.addEventListener('click', () => {
        const c = b.dataset.cat;
        if (state.filters.cats.has(c)) state.filters.cats.delete(c);
        else state.filters.cats.add(c);
        state.shown = PAGE_LIMIT;
        renderFindings();
      });
    }
  }

  function findingMatches(d) {
    if (state.filters.sev !== 'all' && d.severity.toLowerCase() !== state.filters.sev) return false;
    if (state.filters.cats.size && !state.filters.cats.has(d.category)) return false;
    const q = state.filters.q;
    if (!q) return true;
    const hay = (d.message + ' ' + d.code + ' ' + d.category + ' ' + (lineTextAt(d.line) || '')).toLowerCase();
    return hay.includes(q);
  }

  function severityClass(sev) {
    return { ERROR: 'error', WARN: 'warning', INFO: 'info' }[sev] || 'info';
  }

  function renderSnippet(d) {
    if (!d.line) return '';
    const lines = getLines();
    if (d.line > lines.length) return '';
    const raw = lines[d.line - 1];
    // col/len are byte offsets into the on-disk line (RFC 014 section 1):
    // slice bytes, then decode the three pieces. Never JS string indices.
    const col = Math.min(d.col, raw.length);
    const end = d.len ? Math.min(d.col + d.len, raw.length) : col;
    const parts = [
      { t: TEXT.decode(raw.subarray(0, col)) },
      d.len ? { t: TEXT.decode(raw.subarray(col, end)), mark: true } : { t: '' },
      { t: TEXT.decode(raw.subarray(end)) },
    ];
    const rows = [];
    const from = Math.max(1, d.line - 1);
    const to = Math.min(lines.length, d.line + 1);
    for (let no = from; no <= to; no++) {
      if (no === d.line) {
        const inner = parts.map((p) => (p.mark ? '<mark>' + esc(p.t) + '</mark>' : esc(p.t))).join('');
        rows.push(`<div class="sn-row hit"><span class="sn-no">${no}</span><code>${inner || ' '}</code></div>`);
      } else {
        const t = esc(lineTextAt(no));
        rows.push(`<div class="sn-row"><span class="sn-no">${no}</span><code>${t || ' '}</code></div>`);
      }
    }
    return `<pre class="snippet">${rows.join('')}</pre>`;
  }

  function fixTag(code) {
    const meta = state.registry.get(code);
    if (!meta || !meta.fixable) return '';
    const label = meta.fixable === 'safe' ? 'Safe repair available' : 'Repair available, needs review';
    return `<span class="fix-tag ${meta.fixable === 'safe' ? 'safe' : 'maybe'}">${label}</span>`;
  }

  function renderFinding(d) {
    const meta = state.registry.get(d.code) || {};
    const title = meta.title || d.code;
    const sev = severityClass(d.severity);
    const lineLabel = d.line ? 'Line ' + d.line : 'Whole file';
    return `<article class="finding ${sev}">
      <header class="f-head">
        <span class="code">${esc(d.code)}</span>
        <span class="sev-label">${esc(sev)}</span>
        <span class="cat">${esc(d.category)}</span>
        ${fixTag(d.code)}
        <span class="lineno">${lineLabel}</span>
      </header>
      <h3 class="f-title">${esc(title)}</h3>
      <p class="f-msg">${esc(d.message)}</p>
      ${renderSnippet(d)}
      ${meta.why ? `<details class="explain">
        <summary>Why it matters and what to do</summary>
        <p>${esc(meta.why)}</p>
        <p class="remedy"><strong>What to do:</strong> ${esc(meta.remedy || '')}</p>
      </details>` : ''}
    </article>`;
  }

  function renderFindings() {
    renderFilters();
    const all = state.report.diagnostics;
    const listEl = $('#findings');
    const title = $('#explorer-title');
    const staticMore = $('#show-more');

    if (state.baselineMatch) {
      renderSplitFindings();
      return;
    }
    const list = all.filter(findingMatches);
    state.shown = Math.min(state.shown, list.length);
    const slice = list.slice(0, state.shown);
    listEl.innerHTML = slice.map(renderFinding).join('');
    title.innerHTML = list.length === all.length
      ? `Findings <span class="count">${all.length}</span>`
      : `Findings <span class="count">${list.length}</span> of ${all.length}`;
    if (!list.length) {
      listEl.innerHTML = '<p class="empty">No findings match the current filters.</p>';
    }
    const rest = list.length - slice.length;
    staticMore.hidden = !rest;
    if (rest) staticMore.textContent = 'Show ' + Math.min(rest, PAGE_LIMIT) + ' more findings (' + rest + ' remaining)';
  }

  // Baseline view: new findings first, everything you have already seen
  // collapsed under its own header. Never hidden: the collapsed group
  // carries its count in the summary, and the filters apply to both.
  function renderSplitFindings() {
    const all = state.report.diagnostics;
    const flags = state.baselineMatch.known;
    const paired = all.map((d, i) => ({ d, known: flags[i] === 1 }));
    const filtered = paired.filter((p) => findingMatches(p.d));
    const fresh = filtered.filter((p) => !p.known);
    const seen = filtered.filter((p) => p.known);
    const totalCount = all.length - state.baselineMatch.baselined;

    state.shown = Math.min(state.shown, fresh.length);
    state.shownKnown = Math.min(state.shownKnown, seen.length);
    const freshSlice = fresh.slice(0, state.shown);
    const seenSlice = seen.slice(0, state.shownKnown);

    const moreBtn = (group, listLen, sliceLen) => {
      const rest = listLen - sliceLen;
      if (rest <= 0) return '';
      return `<button class="btn btn-ghost wide bl-more" data-group="${group}" type="button">` +
        'Show ' + Math.min(rest, PAGE_LIMIT) + ' more (' + rest + ' remaining)</button>';
    };

    let html = '';
    html += `<h3 class="grp-head grp-new">New since your baseline <span class="count">${fresh.length}</span></h3>`;
    html += freshSlice.map((p) => renderFinding(p.d)).join('');
    if (!fresh.length) {
      html += '<p class="empty">Nothing new since your baseline. Everything this run found is in the group below.</p>';
    } else {
      html += moreBtn('new', fresh.length, freshSlice.length);
    }
    html += `<details class="known-group">
      <summary>Findings you have already seen <span class="count">${seen.length}</span></summary>
      <div class="known-inner">
        ${seenSlice.map((p) => renderFinding(p.d)).join('')}
        ${seen.length ? '' : '<p class="empty">Nothing you have already seen matches the current filters.</p>'}
        ${moreBtn('known', seen.length, seenSlice.length)}
      </div>
    </details>`;

    $('#findings').innerHTML = html;
    $('#show-more').hidden = true;
    for (const b of document.querySelectorAll('#findings .bl-more')) {
      b.addEventListener('click', () => {
        if (b.dataset.group === 'known') state.shownKnown += PAGE_LIMIT;
        else state.shown += PAGE_LIMIT;
        renderSplitFindings();
      });
    }
    $('#explorer-title').innerHTML =
      `Findings <span class="count">${all.length}</span>` +
      (totalCount ? ` <span class="bl-newnote">${totalCount} new</span>` : ' <span class="bl-newnote">none new</span>');
  }

  function renderAll() {
    renderSummary();
    renderBaseline();
    state.shown = PAGE_LIMIT;
    state.shownKnown = PAGE_LIMIT;
    renderFindings();
    renderRepairs();
    renderBanner();
  }

  // ---------------------------------------------------------------------
  // Rendering: repairs
  // ---------------------------------------------------------------------

  function editWeight(e) {
    // Mirrors fix.rs: a rejoin stands for every CONC line it swallowed.
    return e.code === 'E101' ? e.end - e.start : 1;
  }

  function renderRepairs() {
    const sec = $('#repairs');
    if (!state.edits.length && !state.normalizedEndings) {
      sec.hidden = true;
      return;
    }
    sec.hidden = false;
    const listEl = $('#repair-list');
    listEl.innerHTML = state.edits.map((e, i) => {
      const safe = e.applicability === 'safe';
      const range = e.start === e.end ? 'line ' + e.start : 'lines ' + e.start + '\u2013' + e.end;
      const preview = e.replacement_preview ? `<pre class="r-preview">after: ${esc(e.replacement_preview)}</pre>` : '';
      return `<label class="repair ${safe ? 'safe' : 'maybe'}">
        <input type="checkbox" data-i="${i}" ${safe ? 'checked' : ''}>
        <span class="r-body">
          <span class="r-head"><span class="code">${esc(e.code)}</span> ${range}
            <span class="tag">${safe ? 'safe' : 'needs review'}</span></span>
          <span class="r-note">${esc(e.note)}</span>
          ${preview}
        </span>
      </label>`;
    }).join('') || '<p class="empty">Only line-ending normalization applies to this file.</p>';
    for (const cb of listEl.querySelectorAll('input[type=checkbox]')) {
      cb.addEventListener('change', updateRepairFoot);
    }
    updateRepairFoot();
  }

  function selectedMask() {
    const mask = new Uint8Array(state.edits.length);
    for (const cb of document.querySelectorAll('#repair-list input[type=checkbox]')) {
      if (cb.checked) mask[Number(cb.dataset.i)] = 1;
    }
    return mask;
  }

  function updateRepairFoot() {
    const boxes = [...document.querySelectorAll('#repair-list input[type=checkbox]')];
    const safeBoxes = boxes.filter((b) => b.checked && state.edits[Number(b.dataset.i)].applicability === 'safe');
    const anyBoxes = boxes.filter((b) => b.checked);
    $('#select-safe').textContent = safeBoxes.length ? 'Unselect safe repairs' : 'Select all safe repairs';
    $('#repair-count').textContent = boxes.length ? anyBoxes.length + ' of ' + boxes.length + ' selected' : '';
    // A file whose only repair is CR normalization has no checkboxes.
    $('#apply').textContent = boxes.length
      ? 'Apply ' + anyBoxes.length + ' repair' + (anyBoxes.length === 1 ? '' : 's')
      : 'Normalize line endings';
    $('#apply').disabled = !anyBoxes.length && !boxes.length && !state.normalizedEndings;
  }

  async function applyRepairs() {
    if (state.busy) return;
    state.busy = true;
    setStatus('Applying repairs\u2026');
    try {
      const mask = selectedMask();
      const m = await call('apply', {
        data: state.bytes.buffer.slice(0),
        mask: mask.buffer,
        config: configText(), // the config the listed edits were computed under
      });
      const summary = JSON.parse(m.json);
      if (summary.error) throw new Error(summary.error);
      state.appliedBanner = {
        applied: summary.applied,
        postponed: summary.postponed,
        normalizedEndings: summary.normalized_endings,
        bytes: m.file,
      };
      // Re-check the repaired bytes so the improvement is visible.
      await check(m.file);
      state.busy = false;
    } catch (e) {
      state.busy = false;
      setStatus('Applying repairs failed: ' + e.message, 'error');
    }
  }

  function renderBanner() {
    const el = $('#applied-banner');
    const b = state.appliedBanner;
    if (!b) {
      el.hidden = true;
      el.innerHTML = '';
      return;
    }
    const groups = new Map();
    for (const e of b.applied) {
      const g = groups.get(e.code) || { count: 0, note: e.note };
      g.count += editWeight(e);
      groups.set(e.code, g);
    }
    const total = [...groups.values()].reduce((n, g) => n + g.count, 0)
      + (b.applied.length === 0 && b.normalizedEndings ? 1 : 0);
    const postponed = b.postponed.reduce((n, e) => n + editWeight(e), 0);
    el.hidden = false;
    el.className = 'card banner ok';
    el.innerHTML =
      `<h2>Applied ${total} repair${total === 1 ? '' : 's'}</h2>
       <ul class="applied-list">
         ${[...groups.entries()].map(([code, g]) =>
           `<li><span class="code">${esc(code)}</span> ${esc(g.note)} &times; ${g.count}</li>`).join('')}
         ${b.normalizedEndings ? '<li><span class="code">style</span> normalized classic Mac CR line endings to LF</li>' : ''}
       </ul>
       ${postponed ? `<p class="muted">${postponed} repair${postponed === 1 ? '' : 's'} postponed (they overlap one just applied; run the repairs again to pick them up).</p>` : ''}
       <div class="sum-actions">
         <button id="dl-repaired" class="btn btn-primary btn-small" type="button">Download repaired file</button>
       </div>`;
    $('#dl-repaired').addEventListener('click', () => {
      const name = state.fileName.replace(/(\.ged)?$/i, '') + '-repaired.ged';
      download(b.bytes, name, 'text/plain');
    });
  }

  // ---------------------------------------------------------------------
  // Downloads
  // ---------------------------------------------------------------------

  function download(bytes, name, mime) {
    const blob = new Blob([bytes], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 10000);
  }

  function downloadReport() {
    const name = state.fileName.replace(/(\.ged)?$/i, '') + '.report.json';
    download(state.reportRaw, name, 'application/json');
  }

  // ---------------------------------------------------------------------
  // Wiring
  // ---------------------------------------------------------------------

  const dz = $('#dropzone');
  const fileInput = $('#file');
  const baselineInput = $('#baseline-file');

  // A .json dropped or picked where a tree belongs is almost always a
  // baseline: route it there instead of linting it as a GEDCOM.
  function routeFile(f) {
    if (/\.json$/i.test(f.name)) {
      if (state.report) loadBaselineFile(f);
      else setStatus('Check a GEDCOM file first, then load its baseline here.', 'error');
      return;
    }
    readFile(f);
  }

  $('#browse').addEventListener('click', () => fileInput.click());
  dz.addEventListener('click', (e) => {
    if (e.target.closest('button')) return;
    fileInput.click();
  });
  dz.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      fileInput.click();
    }
  });
  fileInput.addEventListener('change', () => {
    if (fileInput.files.length) routeFile(fileInput.files[0]);
    fileInput.value = '';
  });
  baselineInput.addEventListener('change', () => {
    if (baselineInput.files.length) loadBaselineFile(baselineInput.files[0]);
    baselineInput.value = '';
  });

  ['dragenter', 'dragover'].forEach((t) =>
    dz.addEventListener(t, (e) => {
      e.preventDefault();
      dz.classList.add('over');
    }));
  ['dragleave', 'drop'].forEach((t) =>
    dz.addEventListener(t, (e) => {
      e.preventDefault();
      dz.classList.remove('over');
    }));
  dz.addEventListener('drop', (e) => {
    const f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
    if (f) routeFile(f);
  });

  // Page-level drop: keep a stray file from navigating the tab away.
  window.addEventListener('dragover', (e) => e.preventDefault());
  window.addEventListener('drop', (e) => e.preventDefault());

  $('#example').addEventListener('click', loadExample);

  $('#search').addEventListener('input', (e) => {
    state.filters.q = e.target.value.trim().toLowerCase();
    state.shown = PAGE_LIMIT;
    renderFindings();
  });

  $('#show-more').addEventListener('click', () => {
    state.shown += PAGE_LIMIT;
    renderFindings();
  });

  $('#select-safe').addEventListener('click', () => {
    const boxes = [...document.querySelectorAll('#repair-list input[type=checkbox]')];
    const allSafeOn = boxes.every((b) => !state.edits[Number(b.dataset.i)] || state.edits[Number(b.dataset.i)].applicability !== 'safe' || b.checked);
    for (const b of boxes) {
      if (state.edits[Number(b.dataset.i)].applicability === 'safe') b.checked = !allSafeOn;
    }
    updateRepairFoot();
  });

  $('#apply').addEventListener('click', applyRepairs);

  const recheck = () => {
    if (state.bytes && !state.busy) {
      state.busy = true;
      state.appliedBanner = null;
      setStatus('Re-checking\u2026');
      check(state.bytes)
        .then(() => { state.busy = false; })
        .catch((e) => { state.busy = false; setStatus('Re-check failed: ' + e.message, 'error'); });
    }
  };
  $('#preset-hispanic').addEventListener('change', recheck);
  $('#preset-hygiene').addEventListener('change', recheck);

  $('#copy-action').addEventListener('click', async (e) => {
    const btn = e.currentTarget;
    const text = $('#action-snippet').textContent;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const ta = document.createElement('textarea');
      ta.value = text;
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      ta.remove();
    }
    const old = btn.textContent;
    btn.textContent = 'Copied';
    setTimeout(() => { btn.textContent = old; }, 1500);
  });
})();
