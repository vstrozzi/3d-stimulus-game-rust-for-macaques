(function () {
  'use strict';
  const $ = id => document.getElementById('explorer-' + id);
  let mounted = false, catalog = [], worker, serial = 0, initialized, loadedKey = '', pending = new Map(), generation = 0;
  let timezone = 'Europe/Zurich', catalogErrors = [], lastFigures = null, lastOptions = null;
  // Comparison groups are kept apart from the filters: a session stays in its group until it is unticked.
  const groups = { a: new Set(), b: new Set() };
  let matching = new Set();
  // White like the analysis figures unless this browser chose the dark page.
  let theme = (() => { try { return localStorage.getItem('explorer_theme') === 'dark' ? 'dark' : 'light'; } catch (_) { return 'light'; } })();
  function applyTheme() {
    const panel = document.getElementById('data-explorer');
    panel.classList.toggle('light', theme === 'light');
    document.body.classList.toggle('explorer-light', theme === 'light' && !panel.hidden);
    const button = $('theme'); if (button) button.textContent = theme === 'light' ? 'Dark page' : 'White page';
  }
  async function toggleTheme() {
    theme = theme === 'light' ? 'dark' : 'light';
    try { localStorage.setItem('explorer_theme', theme); } catch (_) {}
    applyTheme();
    if (lastOptions) {                                    // colours are computed per theme: redraw
      const o = { ...lastOptions, theme };
      try { await render(await request({ type: 'figure', options: o }), o); } catch (error) { status(error.message, true); }
    }
  }
  const script = src => new Promise((resolve, reject) => {
    const el = document.createElement('script'); el.src = src; el.onload = resolve; el.onerror = () => { el.remove(); reject(new Error('Could not load ' + src)); }; document.head.append(el);
  });
  async function resources() {
    if (!initialized) initialized = (async () => {
      if (!window.AdminAnalysis) await script('./admin_analysis.js');
      if (!window.Plotly) await script('./vendor/plotly.min.js');
      worker = new Worker('./admin_analysis_worker.js');
      worker.onmessage = ({ data }) => { const p = pending.get(data.id); if (p) { pending.delete(data.id); data.error ? p.reject(new Error(data.error)) : p.resolve(data.result); } };
      worker.onerror = () => { for (const p of pending.values()) p.reject(new Error('Analysis worker failed. Close and reopen the explorer.')); pending.clear(); loadedKey = ''; initialized = null; worker.terminate(); };
    })().catch(error => { initialized = null; throw error; });
    return initialized;
  }
  function request(data) { return new Promise((resolve, reject) => { const id = ++serial; pending.set(id, { resolve, reject }); worker.postMessage({ ...data, id }); }); }
  function status(message, error = false) { $('status').textContent = message; $('status').className = error ? 'error' : ''; }
  const selected = el => [...el.selectedOptions].map(o => o.value);
  function options(el, entries, choices) {
    el.replaceChildren(...entries.map(entry => {
      const [value, label] = Array.isArray(entry) ? entry : [entry, entry];
      const opt = new Option(label, String(value)); opt.selected = choices ? choices.includes(String(value)) : true; return opt;
    }));
  }
  function single(el, entries, value) {
    options(el, entries, []); el.value = value ?? String(Array.isArray(entries[0]) ? entries[0][0] : entries[0]);
  }
  async function refresh() {
    status('Reading saved sessions…'); loadedKey = ''; $('draw').disabled = true;   // updateSessions() enables it again
    const response = await fetch('/admin/analysis/catalog').catch(error => { $('draw').disabled = false; throw error; });
    if (!response.ok) $('draw').disabled = false;
    if (!response.ok) throw new Error('Could not retrieve sessions (' + response.status + ').');
    const data = await response.json(); catalog = data.sessions; timezone = data.timezone; catalogErrors = data.errors;
    options($('participants'), [...new Set(catalog.map(s => s.participant))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })));
    options($('dates'), [...new Set(catalog.map(s => s.date))].sort());
    single($('paradigm'), [['', 'All trial sets'], ...new Set(catalog.flatMap(s => s.paradigms))], '');
    $('timezone').textContent = 'Dates: ' + timezone + '. Ctrl/Cmd selects multiple; Shift selects a range. All selects every visible option.';
    updateSessions();
    status(catalog.length ? `${catalog.length} saved sessions. Select data and press Plot.` + (catalogErrors.length ? ` ${catalogErrors.length} problems; see data notes below.` : '') : 'No saved gameplay sessions found. New recordings will appear after Refresh.');
    showNotes(catalogErrors.map(e => e.path + ': ' + e.error));
  }
  function updateSessions() {
    generation++; $('draw').disabled = false;
    const people = new Set(selected($('participants'))), dates = new Set(selected($('dates'))), search = $('search').value.trim().toLowerCase();
    const from = $('from').value, to = $('to').value, paradigm = $('paradigm').value;
    const sessions = catalog.filter(s => people.has(s.participant) && dates.has(s.date) && (!search || s.participant.toLowerCase().includes(search))
      && (!from || s.date >= from) && (!to || s.date <= to) && (!$('hr-only').checked || s.has_hr) && (!paradigm || s.paradigms.includes(paradigm)));
    options($('sessions'), sessions.map(s => [s.id, sessionText(s)]));
    matching = new Set(sessions.map(s => s.id));
    const known = new Set(catalog.map(s => s.id));
    for (const g of Object.values(groups)) for (const id of g) if (!known.has(id)) g.delete(id);   // deleted after a refresh
    renderGroups();
    loadedKey = ''; $('selection-count').textContent = sessions.length + ' matching sessions';
  }
  const sessionText = s => `${s.label} · ${s.date} · ${s.time || ''} · ${s.trials} trials${s.has_hr ? ' · HR' : ''} · ${s.id.split('/').pop()}`;
  // Each list shows the sessions that pass the filters above plus its own name filter, and always its ticked sessions.
  function renderGroups() {
    for (const [g, other] of [['a', 'b'], ['b', 'a']]) {
      const list = $('group-' + g), top = list.scrollTop, find = $('find-' + g).value.trim().toLowerCase();
      const rows = catalog.filter(s => groups[g].has(s.id) || (matching.has(s.id) && (!find || sessionText(s).toLowerCase().includes(find) || s.id.toLowerCase().includes(find))));
      list.replaceChildren(...rows.map(s => {
        const row = document.createElement('label'), box = document.createElement('input'), text = document.createElement('span');
        const taken = groups[other].has(s.id);
        box.type = 'checkbox'; box.value = s.id; box.checked = groups[g].has(s.id); box.disabled = taken;
        text.textContent = sessionText(s);
        row.className = 'group-row' + (box.checked ? ' in-' + g : '') + (taken ? ' taken' : '');
        row.title = taken ? `Already in group ${other.toUpperCase()}` : s.id;
        row.append(box, text);
        if (taken) { const tag = document.createElement('b'); tag.className = 'group-tag tag-' + other; tag.textContent = other.toUpperCase(); row.append(tag); }
        return row;
      }));
      if (!rows.length) { const empty = document.createElement('p'); empty.className = 'hint'; empty.textContent = 'No session matches the filters.'; list.append(empty); }
      list.scrollTop = top;
      const hidden = [...groups[g]].filter(id => !matching.has(id)).length;
      $('count-' + g).textContent = `${groups[g].size} ticked` + (hidden ? ` (${hidden} outside the filters, kept)` : '');
    }
  }
  function toggleGroup(g, id, on) {
    if (on) groups[g].add(id); else groups[g].delete(id);
    generation++; $('draw').disabled = false; renderGroups();
  }
  function addShown(g) {
    const other = g === 'a' ? 'b' : 'a';
    for (const box of $('group-' + g).querySelectorAll('input')) if (!groups[other].has(box.value)) groups[g].add(box.value);
    generation++; renderGroups();
  }
  // Compare mode plots exactly the two groups, whatever the session filters currently show.
  const plotIds = () => $('compare').checked ? [...new Set([...groups.a, ...groups.b])] : selected($('sessions'));
  const NUMERIC = ['y', 'trial_nr_global', 'level_nr', 'chain__step', 'clock__trial_duration', 'wrong', 'nr_attempts', 'object_rep_nr', 'pos_in_round', 'clock__trial_start_time'];
  function axes() {
    const p = $('plot').value, labels = AdminAnalysis.LABELS;
    const entries = keys => keys.map(k => [k, labels[k] || k]);
    let x = [], y = [], style = [['points', 'Points'], ['line', 'Lines + points']];
    if (p === 'first_choice') y = [['kind', 'Corner or face'], ['position', 'Position (0–5)']];
    if (p === 'duration') x = entries(['y', 'trial_nr_global', 'level_nr', 'chain__step', 'pos_in_round']);
    if (p === 'chain') x = entries(['trial_nr_global', 'y']);
    if (p === 'scatter') { x = entries(NUMERIC); y = entries(['clock__trial_duration', ...NUMERIC.filter(k => k !== 'clock__trial_duration')]); }
    if (p === 'histogram') { y = entries(['clock__trial_duration', 'wrong', 'nr_attempts']); style = [['histogram', 'Histogram'], ['box', 'Box plot'], ['violin', 'Violin plot']]; }
    if (p === 'hr_summary') x = [['level_nr', 'Level'], ['trial', 'Trial']];
    single($('x'), x.length ? x : [['', 'As in the figure']]);
    single($('y'), y.length ? y : [['', 'As in the figure']]);
    single($('style'), style);
    $('x').disabled = x.length <= 1; $('y').disabled = y.length <= 1;
    $('style-wrap').hidden = !['duration', 'chain', 'scatter', 'histogram'].includes(p);
    $('color-wrap').hidden = !['duration', 'chain', 'scatter', 'histogram'].includes(p);
    $('matrix-wrap').hidden = p !== 'mistakes_object';
    $('swap-wrap').hidden = ['mistakes_object', 'timeline'].includes(p);
  }
  function settings() {
    return { plot: $('plot').value, x: $('x').value || undefined, y: $('y').value || undefined, style: $('style').value, color: $('color').value,
      kind: $('kind').value, congruency: $('congruency').value, completion: $('completion').value,
      levels: selected($('levels')).map(Number), individual: $('individual').checked, matrix: $('matrix').checked,
      compare: $('compare').checked, groupA: [...groups.a], groupB: [...groups.b],
      nameA: $('name-a').value.trim(), nameB: $('name-b').value.trim(), swap: $('swap').checked, theme };
  }
  async function plot() {
    const run = ++generation;
    const ids = plotIds();
    if (!ids.length) { status($('compare').checked ? 'Tick sessions for groups A and B.' : 'Select at least one session.', true); return; }
    if ($('from').value && $('to').value && $('from').value > $('to').value) { status('The start date must precede the end date.', true); return; }
    const key = JSON.stringify(ids.slice().sort());
    $('draw').disabled = true;
    try {
      if (key !== loadedKey) {
        status('Building the tables with build_dataset.py (the first time for a session takes a few seconds)…');
        const response = await fetch('/admin/analysis/data', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ sessions: ids }) });
        if (!response.ok) throw new Error('Could not load data (' + response.status + '). Refresh and try again.');
        const payload = await response.json();
        if (run !== generation) return;
        status('Preparing plot data…');
        const meta = await request({ type: 'load', payload });
        if (run !== generation) return;
        const old = selected($('levels')).filter(v => v !== ''), keep = old.filter(v => meta.levels.includes(Number(v)));
        options($('levels'), meta.levels, keep.length ? keep : undefined);
        loadedKey = key;
      }
      const o = settings();
      if (!o.levels.length) throw new Error('Select at least one level.');
      if (o.compare) {
        const selection = new Set(ids);
        o.groupA = o.groupA.filter(s => selection.has(s)); o.groupB = o.groupB.filter(s => selection.has(s));
        if (!o.groupA.length || !o.groupB.length) throw new Error('Choose sessions for both comparison groups.');
        if (o.groupA.some(s => o.groupB.includes(s))) throw new Error('A session cannot belong to both comparison groups.');
      }
      status('Rendering…');
      const result = await request({ type: 'figure', options: o });
      if (run !== generation) return;
      await render(result, o);
      status(`${result.sessions} sessions · ${result.count} trials.`);
      showNotes([...catalogErrors.map(e => e.path + ': ' + e.error), ...result.warnings]);
    } catch (error) { status(error.message, true); }
    finally { if (run === generation) $('draw').disabled = false; }
  }
  function showNotes(notes) {
    $('notes-text').textContent = [...new Set(notes)].join('\n'); $('notes').hidden = !notes.length;
  }
  // Grid plots: size dots so each participant's spot fits inside its cell at the
  // current size and zoom (Plotly marker sizes are in pixels).
  function fitDots(plot, fit) {
    const xa = plot._fullLayout.xaxis, px = xa._length / Math.abs(xa.range[1] - xa.range[0]);
    const dot = Math.max(2.5, Math.min(18, px * fit.step * 0.8)), sizes = { dot, ring: dot * 1.45 };
    const markers = [], markerSizes = [], texts = [], textSizes = [], opacity = [];
    plot.data.forEach((t, i) => {
      if (sizes[t.meta]) { markers.push(i); markerSizes.push(+sizes[t.meta].toFixed(1)); }
      if (t.meta === 'num') { texts.push(i); textSizes.push(+Math.max(4, dot * 0.58).toFixed(1)); opacity.push(dot >= 9 ? 1 : 0); }
    });
    const jobs = [];
    if (markers.length && markers.some((i, k) => plot.data[i].marker.size !== markerSizes[k])) jobs.push(Plotly.restyle(plot, { 'marker.size': markerSizes }, markers));
    if (texts.length && texts.some((i, k) => plot.data[i].textfont.size !== textSizes[k] || plot.data[i].opacity !== opacity[k])) jobs.push(Plotly.restyle(plot, { 'textfont.size': textSizes, opacity }, texts));
    return Promise.all(jobs);
  }
  async function render(result, o) {
    lastFigures = result; lastOptions = o;
    const c = AdminAnalysis.palette(o.theme);
    for (const node of $('plots').querySelectorAll('.plot')) Plotly.purge(node);
    $('plots').replaceChildren(); $('plots').classList.toggle('compare', !!o.compare && !result.panels.some(p => p.stacked));
    for (const panel of result.panels) {
      const outer = document.createElement('div'); outer.className = 'plot-panel';
      const title = document.createElement('h3');
      title.innerHTML = (panel.letter ? `<b class="panel-letter">${panel.letter}</b> ` : '') + panel.title;   // title is escaped by the analysis
      outer.append(title);
      const plot = document.createElement('div'); plot.className = 'plot'; outer.append(plot);
      $('plots').append(outer);
      const traces = panel.traces, axis = { gridcolor: c.GRID, zerolinecolor: c.FAINT, automargin: true };
      let xlabel = panel.x, ylabel = panel.y, xaxis = panel.axis || {}, yaxis = panel.yaxis || {};
      let shapes = panel.shapes || [];
      const extraLayout = { ...panel.layout };
      if (o.swap && !panel.noSwap) {
        for (const t of traces) {
          [t.x, t.y] = [t.y, t.x];
          const errorX = t.error_x, errorY = t.error_y; delete t.error_x; delete t.error_y;
          if (errorX) t.error_y = errorX;
          if (errorY) t.error_x = errorY;
          if (['box', 'violin', 'bar'].includes(t.type)) t.orientation = 'h';
        }
        [xlabel, ylabel] = [ylabel, xlabel]; [xaxis, yaxis] = [yaxis, xaxis];
        shapes = shapes.map(s => ({ ...s, x0: s.y0, x1: s.y1, y0: s.x0, y1: s.x1, xref: s.yref || 'x', yref: s.xref || 'y' }));
      }
      const margin = panel.margin || { t: 20, l: 70, r: 25, b: 110 };
      let height = panel.height || 520;
      if (panel.aspect) height = Math.round((plot.clientWidth - margin.l - margin.r) * panel.aspect + margin.t + margin.b);
      const layout = { paper_bgcolor: c.SURF, plot_bgcolor: c.SURF, font: { color: c.INK, family: 'Arial, Helvetica, "DejaVu Sans", sans-serif', size: 12 },
        height, margin, hoverlabel: { bgcolor: c.SURF, bordercolor: c.MUTED, font: { color: c.INK } },
        legend: { orientation: 'h', y: -0.25, yanchor: 'top', font: { size: 11 } },
        xaxis: { ...axis, title: { text: xlabel || '' }, ...xaxis }, yaxis: { ...axis, title: { text: ylabel || '' }, ...yaxis }, shapes, ...extraLayout };
      const hasData = t => t.x ? t.x.some((v, i) => v != null && (!t.y || t.y[i] != null)) : (t.y || []).some(v => v != null);
      if (!traces.some(hasData)) layout.annotations = [...(layout.annotations || []), { text: 'No data for this plot and selection', xref: 'paper', yref: 'paper', x: .5, y: .5, showarrow: false, font: { color: c.MUTED } }];
      await Plotly.newPlot(plot, traces, layout, { responsive: true, displaylogo: false, toImageButtonOptions: { filename: o.plot, format: 'png', scale: 3 } });
      if (panel.fit) { plot.on('plotly_relayout', () => fitDots(plot, panel.fit)); await fitDots(plot, panel.fit); }
      nameTips(plot, panel.names);
      const tools = document.createElement('div'); tools.className = 'plot-tools';
      const csv = document.createElement('button'); csv.textContent = 'Download plotted values (CSV)'; csv.onclick = () => downloadCSV(panel.rows, o.plot + '.csv');
      const full = document.createElement('button'); full.textContent = 'Fullscreen'; full.onclick = async () => { try { await outer.requestFullscreen(); } catch (_) {} Plotly.Plots.resize(plot); };
      tools.append(csv, full); outer.append(tools);
      if (panel.note) { const note = document.createElement('p'); note.className = 'plot-note'; note.textContent = panel.note; outer.append(note); }
    }
    $('export-settings').hidden = false;
  }
  // Shortened participant names (legend, strip rows, timeline key) show the full id on hover. Plotly's drag and
  // legend-toggle layers sit above the text, so the check uses the label's box instead of the label's own events.
  function nameTips(plot, names) {
    const full = Object.fromEntries(Object.entries(names || {}).filter(([shown, id]) => shown !== id));
    if (!Object.keys(full).length) return;
    const tip = document.createElement('div'); tip.className = 'name-tip'; tip.hidden = true; plot.parentElement.append(tip);
    let nodes = [];
    const collect = () => { nodes = [...plot.querySelectorAll('text, tspan')].filter(n => Object.hasOwn(full, n.textContent.trim())); };
    collect(); plot.on('plotly_afterplot', collect);
    plot.addEventListener('mousemove', e => {
      const hit = nodes.find(n => { const r = n.getBoundingClientRect(); return r.width && e.clientX >= r.left - 2 && e.clientX <= r.right + 2 && e.clientY >= r.top - 2 && e.clientY <= r.bottom + 2; });
      tip.hidden = !hit;
      if (hit) { tip.textContent = full[hit.textContent.trim()]; tip.style.left = e.clientX + 12 + 'px'; tip.style.top = e.clientY + 14 + 'px'; }
    });
    plot.addEventListener('mouseleave', () => { tip.hidden = true; });
  }
  function download(content, name, type) {
    const url = URL.createObjectURL(content instanceof Blob ? content : new Blob([content], { type })); const a = document.createElement('a'); a.href = url; a.download = name; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  function downloadCSV(rows, name) {
    const keys = [...new Set(rows.flatMap(r => Object.keys(r)))].filter(k => rows.every(r => typeof r[k] !== 'object' || r[k] === null));
    const quote = value => { let s = String(value ?? ''); if (typeof value === 'string' && /^\s*[=+@\-\t\r]/.test(s)) s = "'" + s; return '"' + s.replace(/"/g, '""') + '"'; };
    download([keys.map(quote).join(','), ...rows.map(r => keys.map(k => quote(r[k])).join(','))].join('\r\n'), name, 'text/csv;charset=utf-8');
  }
  async function downloadTables() {
    const ids = plotIds();
    if (!ids.length) { status('Select at least one session.', true); return; }
    $('tables').disabled = true; status('Building trials.csv and positions.csv with build_dataset.py…');
    try {
      const response = await fetch('/admin/analysis/tables', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ sessions: ids }) });
      if (!response.ok) throw new Error('Could not build the tables (' + response.status + ').');
      download(await response.blob(), 'parsed_tables.zip');
      status(`Downloaded trials.csv and positions.csv for ${ids.length} sessions.`);
    } catch (error) { status(error.message, true); }
    finally { $('tables').disabled = false; }
  }
  async function open() {
    document.querySelector('#admin-flow > .landing').hidden = true;
    const panel = document.getElementById('data-explorer'); panel.hidden = false;
    if (!mounted) { mount(panel); mounted = true; }
    applyTheme();
    try {
      await resources();
      if (!$('plot').options.length) { single($('plot'), Object.entries(AdminAnalysis.PLOTS), 'mistakes_level'); axes(); await refresh(); }
    } catch (error) { status(error.message, true); }
  }
  function mount(panel) {
    panel.innerHTML = `
      <header><h2>Data Explorer</h2><div><button id="explorer-theme" type="button"></button><button id="explorer-back" type="button">Back</button></div></header>
      <div class="explorer-controls">
        <label class="wide">Participants<input id="explorer-search" type="search" placeholder="Search names"><select id="explorer-participants" multiple aria-label="Participants"></select><button data-all="participants" type="button">All participants</button></label>
        <label class="wide">Dates<select id="explorer-dates" multiple aria-label="Dates"></select><button data-all="dates" type="button">All dates</button></label>
        <label>From<input id="explorer-from" type="date"></label><label>To<input id="explorer-to" type="date"></label>
        <button id="explorer-refresh" type="button">Refresh</button>
      </div>
      <p class="hint" id="explorer-timezone"></p>
      <details><summary>Sessions · <span id="explorer-selection-count"></span></summary><div class="explorer-controls"><label class="wide">Sessions<select id="explorer-sessions" multiple aria-label="Sessions"></select></label>
        <label>Trial set<select id="explorer-paradigm"></select></label><label class="check"><input id="explorer-hr-only" type="checkbox">With HR only</label><button data-all="sessions" type="button">All matching sessions</button></div>
      </details>
      <details><summary>Compare two groups (panels A and B)</summary><label class="check"><input id="explorer-compare" type="checkbox">Show A and B side by side</label>
        <p class="hint">Tick sessions into each group. Ticked sessions stay in their group (and stay highlighted) when you change any filter, until you untick them. Compare mode plots the two groups, not the session list above.</p>
        <div class="explorer-controls group-picks">${['a', 'b'].map(g => `
          <div class="group-pick">
            <label>Group ${g.toUpperCase()} name<input id="explorer-name-${g}" type="text" placeholder="${g === 'a' ? 'e.g. pilot_Gymer' : 'e.g. kjpp'}" maxlength="60"></label>
            <input id="explorer-find-${g}" type="search" placeholder="Filter names, e.g. kjpp" aria-label="Filter group ${g.toUpperCase()} list">
            <div class="group-tools"><b class="group-tag tag-${g}">${g.toUpperCase()}</b><span id="explorer-count-${g}"></span><button data-add="${g}" type="button">Tick all shown</button><button data-clear="${g}" type="button">Untick all</button></div>
            <div id="explorer-group-${g}" class="group-list" role="group" aria-label="Group ${g.toUpperCase()} sessions"></div>
          </div>`).join('')}</div></details>
      <div class="explorer-controls">
        <label class="wide">Plot<select id="explorer-plot"></select></label><label>X axis<select id="explorer-x"></select></label><label>Y axis<select id="explorer-y"></select></label>
        <label id="explorer-style-wrap">Style<select id="explorer-style"></select></label>
        <label id="explorer-color-wrap">Colour by<select id="explorer-color"><option value="participant_id">Participant</option><option value="level_nr">Level</option><option value="object_type">Object type</option><option value="features_congruent">Congruency</option><option value="first">First-check outcome</option></select></label>
      </div>
      <details><summary>Filter by level, object type, congruency or level status</summary><div class="explorer-controls">
        <label>Levels<select id="explorer-levels" multiple size="3" aria-label="Levels"><option value="" disabled>Load sessions first</option></select><button data-all="levels" type="button">All levels</button></label>
        <label>Object type<select id="explorer-kind"><option value="">All types</option><option value="corner">Corner (ty. I)</option><option value="face">Face (ty. II)</option></select></label>
        <label>Congruency<select id="explorer-congruency"><option value="">All</option><option value="congruent">Congruent</option><option value="incongruent">Incongruent</option><option value="no rule">No colour rule (L0–4)</option></select></label>
        <label>Level status<select id="explorer-completion"><option value="">All</option><option value="complete">Completed</option><option value="incomplete">Not finished (time limit)</option></select></label>
      </div></details>
      <div class="explorer-controls">
        <label class="check"><input id="explorer-individual" type="checkbox" checked>Individual participants</label>
        <label class="check" id="explorer-swap-wrap"><input id="explorer-swap" type="checkbox">Swap axes</label>
        <label class="check" id="explorer-matrix-wrap"><input id="explorer-matrix" type="checkbox" checked>First-check strip</label>
        <button id="explorer-draw" type="button" class="primary">Plot</button>
        <button id="explorer-tables" type="button" title="trials.csv and positions.csv, exactly as build_dataset.py writes them">Download parsed tables</button>
        <button id="explorer-export-settings" type="button" hidden>Save selection</button>
      </div>
      <output id="explorer-status" aria-live="polite"></output>
      <details id="explorer-notes" hidden><summary>Data notes</summary><output id="explorer-notes-text"></output></details>
      <div id="explorer-plots" class="plots"></div>
      <p class="hint">Values come from the analysis scripts (build_dataset.py and make_MAIN_figures.py), run on the server; the browser only draws them.</p>`;
    $('back').onclick = () => { panel.hidden = true; document.querySelector('#admin-flow > .landing').hidden = false; applyTheme(); };
    $('theme').onclick = toggleTheme;
    $('refresh').onclick = () => refresh().catch(e => status(e.message, true));
    for (const id of ['participants', 'dates', 'from', 'to', 'paradigm', 'hr-only']) $(id).addEventListener('change', updateSessions);
    $('search').addEventListener('input', updateSessions);
    $('sessions').addEventListener('change', () => { generation++; loadedKey = ''; $('draw').disabled = false; });
    $('plot').addEventListener('change', axes);
    for (const g of ['a', 'b']) {
      $('find-' + g).addEventListener('input', renderGroups);
      $('group-' + g).addEventListener('change', e => toggleGroup(g, e.target.value, e.target.checked));
      panel.querySelector(`[data-add="${g}"]`).onclick = () => addShown(g);
      panel.querySelector(`[data-clear="${g}"]`).onclick = () => { groups[g].clear(); generation++; renderGroups(); };
    }
    $('draw').onclick = plot;
    $('tables').onclick = downloadTables;
    for (const button of panel.querySelectorAll('[data-all]')) button.onclick = () => {
      for (const opt of $(button.dataset.all).options) opt.selected = !opt.disabled;
      if (['participants', 'dates'].includes(button.dataset.all)) { if (button.dataset.all === 'participants') $('search').value = ''; if (button.dataset.all === 'dates') { $('from').value = ''; $('to').value = ''; } updateSessions(); }
    };
    $('export-settings').onclick = () => download(JSON.stringify({ sessions: selected($('sessions')), timezone, options: settings(), notes: lastFigures?.warnings || [] }, null, 2), 'analysis-selection.json', 'application/json');
    document.addEventListener('fullscreenchange', () => { for (const node of panel.querySelectorAll('.plot')) Plotly.Plots.resize(node); });
  }
  document.getElementById('btn-explorer').addEventListener('click', open);
})();
