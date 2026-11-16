(function () {
  'use strict';
  const $ = id => document.getElementById('explorer-' + id);
  let mounted = false, catalog = [], worker, serial = 0, initialized, loadedKey = '', pending = new Map(), generation = 0;
  let timezone = 'Europe/Zurich', catalogErrors = [], lastFigures = null, lastOptions = null;
  let theme = (() => { try { return localStorage.getItem('explorer_theme') === 'light' ? 'light' : 'dark'; } catch (_) { return 'dark'; } })();
  const PALETTE = {
    dark: { bg: '#000', text: '#ddd', grid: '#292929', zero: '#444', hoverBg: '#171717', hoverText: '#fff', empty: '#aaa' },
    light: { bg: '#fff', text: '#222', grid: '#e5e5e5', zero: '#999', hoverBg: '#fff', hoverText: '#111', empty: '#666' },
  };
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
    // Mean lines, rings and cell shading are computed per theme: redraw.
    if (lastOptions) {
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
    status('Reading saved sessions…'); loadedKey = '';
    const response = await fetch('/admin/analysis/catalog');
    if (!response.ok) throw new Error('Could not retrieve sessions (' + response.status + ').');
    const data = await response.json(); catalog = data.sessions; timezone = data.timezone; catalogErrors = data.errors;
    options($('participants'), [...new Set(catalog.map(s => s.participant))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true })));
    options($('dates'), [...new Set(catalog.map(s => s.date))].sort());
    single($('paradigm'), [['', 'All trial sets'], ...new Set(catalog.flatMap(s => s.paradigms))], '');
    $('timezone').textContent = 'Dates: ' + timezone + '. Ctrl/Cmd selects multiple; Shift selects a range. All selects every visible option.';
    updateSessions();
    status(catalog.length ? `${catalog.length} saved sessions. Select data and press Plot.` + (catalogErrors.length ? ` ${catalogErrors.length} unreadable files; see data notes below.` : '') : 'No saved gameplay sessions found. New recordings will appear after Refresh.');
    showNotes(catalogErrors.map(e => e.path + ': ' + e.error));
  }
  function updateSessions() {
    generation++; $('draw').disabled = false;
    const people = new Set(selected($('participants'))), dates = new Set(selected($('dates'))), search = $('search').value.trim().toLowerCase();
    const from = $('from').value, to = $('to').value, paradigm = $('paradigm').value;
    const sessions = catalog.filter(s => people.has(s.participant) && dates.has(s.date) && (!search || s.participant.toLowerCase().includes(search))
      && (!from || s.date >= from) && (!to || s.date <= to) && (!$('hr-only').checked || s.has_hr) && (!paradigm || s.paradigms.includes(paradigm)));
    const entries = sessions.map(s => [s.id, `${s.participant} · ${s.date} · ${s.time || ''} · ${s.trials} trials${s.has_hr ? ' · HR' : ''} · ${s.id.split('/').pop()}`]);
    options($('sessions'), entries);
    options($('group-a'), entries, sessions.slice(0, Math.ceil(sessions.length / 2)).map(s => s.id));
    options($('group-b'), entries, sessions.slice(Math.ceil(sessions.length / 2)).map(s => s.id));
    loadedKey = ''; $('selection-count').textContent = sessions.length + ' matching sessions';
  }
  function axes() {
    const p = $('plot').value, labels = AdminAnalysis.LABELS;
    const entries = keys => keys.map(k => [k, labels[k] || k.toUpperCase()]);
    let x = ['level'], y = ['mistakes'], style = [['points', 'Points'], ['line', 'Lines + points']];
    if (['mistakes_level', 'mistakes_object', 'trials', 'first_choice'].includes(p)) { x = ['level', 'date', 'participant']; y = []; }
    if (p === 'first_choice') y = ['kind', 'region'];
    if (['duration', 'chain', 'scatter'].includes(p)) {
      x = ['round', 'level', 'counter', 'chain_pos', 'duration', 'mistakes', 't'];
      y = p === 'duration' ? ['duration'] : p === 'chain' ? ['chain_pos', 'chain_delta'] : ['duration', 'mistakes', 'chain_pos', 'chain_delta', 'level', 'round', 'attempts', 'first_correct'];
    }
    if (p === 'histogram') { x = []; y = ['duration', 'mistakes']; style = [['histogram', 'Histogram'], ['box', 'Box plot'], ['violin', 'Violin plot']]; }
    if (p === 'hr_summary') { x = ['level', 'counter']; y = ['hr']; }
    if (p === 'timeline') { x = []; y = []; }
    single($('x'), x.length ? entries(x) : [['', 'Preset axis']]);
    single($('y'), y.length ? entries(y) : [['', 'Selected measure']]);
    single($('style'), style);
    $('x').disabled = x.length <= 1; $('y').disabled = y.length <= 1;
    $('style-wrap').hidden = !['duration', 'chain', 'scatter', 'histogram'].includes(p);
    $('measure-wrap').hidden = !['mistakes_level', 'mistakes_object'].includes(p);
    $('color-wrap').hidden = !['duration', 'chain', 'scatter', 'histogram'].includes(p);
    $('matrix-wrap').hidden = p !== 'mistakes_object';
  }
  function settings() {
    return { plot: $('plot').value, x: $('x').value || undefined, y: $('y').value || undefined, style: $('style').value,
      measure: $('measure').value, color: $('color').value, weight: $('weight').value,
      kind: $('kind').value, congruency: $('congruency').value, completion: $('completion').value,
      config: $('config').value, levels: selected($('levels')).map(Number), individual: $('individual').checked,
      objects: selected($('objects')),
      matrix: $('matrix').checked,
      compare: $('compare').checked, groupA: selected($('group-a')), groupB: selected($('group-b')),
      labelOverrides: readOverrides(), swap: $('swap').checked, theme,
    };
  }
  function readOverrides() { try { return JSON.parse(localStorage.getItem('analysis_congruency') || '{}'); } catch (_) { return {}; } }
  async function plot() {
    const run = ++generation;
    const ids = selected($('sessions'));
    if (!ids.length) { status('Select at least one session.', true); return; }
    if ($('from').value && $('to').value && $('from').value > $('to').value) { status('The start date must precede the end date.', true); return; }
    const key = JSON.stringify(ids.slice().sort());
    $('draw').disabled = true;
    try {
      if (key !== loadedKey) {
        const documents = [];
        for (let batch = 0; batch < ids.length; batch += 100) {
          let offset = 0;
          do {
            status(`Loading selected data… ${documents.length} files`);
            const response = await fetch('/admin/analysis/data', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ sessions: ids.slice(batch, batch + 100), offset }) });
            if (!response.ok) throw new Error('Could not load data (' + response.status + '). Refresh and try again.');
            const data = await response.json();
            if (run !== generation) return;
            documents.push(...data.documents); offset = data.next_offset;
          } while (offset !== null);
        }
        status('Preparing plot data…');
        const data = await request({ type: 'load', documents, sessions: catalog });
        if (run !== generation) return;
        const oldLevels = selected($('levels')).filter(v => v !== ''), oldConfig = $('config').value;
        const retainedLevels = oldLevels.filter(v => data.levels.includes(Number(v)));
        options($('levels'), data.levels, retainedLevels.length ? retainedLevels : undefined);
        single($('config'), [['', 'All configurations'], ...data.configs.map(c => [c, c])], data.configs.includes(oldConfig) ? oldConfig : '');
        const oldObjects = selected($('objects')), retainedObjects = oldObjects.filter(key => data.objects.some(obj => obj.key === key));
        options($('objects'), data.objects.map(obj => [obj.key, `L${obj.level} · ${obj.kind} ${obj.object} · ${obj.config}`]), retainedObjects.length ? retainedObjects : undefined);
        renderLabels(data.objects || []);
        loadedKey = key;
      }
      const o = settings();
      if (!o.levels.length) throw new Error('Select at least one level.');
      if (!o.objects.length) throw new Error('Select at least one object.');
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
      status(`${result.sessions} sessions · ${result.count} object presentations. ${o.weight === 'participant' ? 'Group summaries weight participants equally.' : 'Group summaries weight sessions equally.'}`);
      showNotes([...catalogErrors.map(e => e.path + ': ' + e.error), ...result.warnings]);
    } catch (error) { status(error.message, true); }
    finally { if (run === generation) $('draw').disabled = false; }
  }
  function showNotes(notes) {
    $('notes-text').textContent = [...new Set(notes)].join('\n'); $('notes').hidden = !notes.length;
  }
  function renderLabels(objects) {
    const overrides = readOverrides(), body = $('labels-body'); body.replaceChildren();
    for (const obj of objects) {
      const row = document.createElement('tr');
      for (const value of [obj.config, 'Level ' + obj.level, obj.kind + ' ' + obj.object]) { const td = document.createElement('td'); td.textContent = value; row.append(td); }
      const td = document.createElement('td'), select = document.createElement('select'); select.setAttribute('aria-label', `Congruency level ${obj.level} object ${obj.object}`);
      single(select, ['unknown', 'no rule', 'congruent', 'incongruent'], overrides[obj.key] || obj.congruency);
      select.addEventListener('change', () => { const current = readOverrides(); current[obj.key] = select.value; localStorage.setItem('analysis_congruency', JSON.stringify(current)); });
      td.append(select); row.append(td); body.append(row);
    }
  }
  async function render(result, o) {
    lastFigures = result; lastOptions = o; const c = PALETTE[o.theme === 'light' ? 'light' : 'dark'];
    for (const node of $('plots').querySelectorAll('.plot')) Plotly.purge(node);
    $('plots').replaceChildren(); $('plots').classList.toggle('compare', o.compare);
    for (const panel of result.panels) {
      const outer = document.createElement('div'); outer.className = 'plot-panel';
      const title = document.createElement('h3'); title.textContent = panel.title; outer.append(title);
      const plot = document.createElement('div'); plot.className = 'plot'; outer.append(plot);
      $('plots').append(outer);
      const traces = panel.traces, axis = { gridcolor: c.grid, zerolinecolor: c.zero, linecolor: c.zero, automargin: true };
      let xlabel = panel.x, ylabel = panel.y, xaxis = panel.axis || {}, yaxis = panel.yaxis || {};
      let shapes = panel.shapes || [];
      let extraLayout = { ...panel.layout };
      if (o.swap && !panel.matrix) {
        for (const t of traces) {
          [t.x, t.y] = [t.y, t.x];
          const errorX = t.error_x, errorY = t.error_y; delete t.error_x; delete t.error_y;
          if (errorX) t.error_y = errorX;
          if (errorY) t.error_x = errorY;
          if (t.yaxis === 'y2') { t.xaxis = 'x2'; delete t.yaxis; }
          if (['box', 'violin', 'bar'].includes(t.type)) t.orientation = 'h';
        }
        if (extraLayout.yaxis2) { extraLayout.xaxis2 = { ...extraLayout.yaxis2, overlaying: 'x' }; delete extraLayout.yaxis2; }
        [xlabel, ylabel] = [ylabel, xlabel]; [xaxis, yaxis] = [yaxis, xaxis];
        shapes = shapes.map(s => ({ ...s, x0: s.y0, x1: s.y1, y0: s.x0, y1: s.x1, xref: s.yref || 'x', yref: s.xref || 'y' }));
      }
      const topLegend = panel.matrix || o.plot === 'timeline';
      const layout = { paper_bgcolor: c.bg, plot_bgcolor: c.bg, font: { color: c.text, family: 'system-ui, sans-serif', size: 12 },
        height: panel.height || 570, margin: { t: topLegend ? 85 : 30, l: 70, r: 25, b: topLegend ? 150 : 110 },
        hoverlabel: { bgcolor: c.hoverBg, bordercolor: c.zero, font: { color: c.hoverText } }, legend: { orientation: 'h', y: topLegend ? 1.05 : -0.25, yanchor: topLegend ? 'bottom' : 'top' },
        xaxis: { ...axis, title: xlabel, ...xaxis }, yaxis: { ...axis, title: ylabel, ...yaxis }, shapes, ...extraLayout };
      if (!traces.some(t => (t.x?.length || t.y?.length))) layout.annotations = [{ text: 'No data for this plot and selection', xref: 'paper', yref: 'paper', x: .5, y: .5, showarrow: false, font: { color: c.empty } }];
      await Plotly.newPlot(plot, traces, layout, { responsive: true, displaylogo: false, toImageButtonOptions: { filename: o.plot, format: 'png', scale: 2 } });
      const tools = document.createElement('div'); tools.className = 'plot-tools';
      const csv = document.createElement('button'); csv.textContent = 'Download CSV'; csv.onclick = () => downloadCSV(panel.rows, o.plot + '.csv');
      const full = document.createElement('button'); full.textContent = 'Fullscreen'; full.onclick = async () => { try { await outer.requestFullscreen(); } catch (_) {} Plotly.Plots.resize(plot); };
      tools.append(csv, full); outer.append(tools);
      if (panel.note) { const note = document.createElement('p'); note.className = 'plot-note'; note.textContent = panel.note; outer.append(note); }
    }
    $('export-settings').hidden = false;
  }
  function download(content, name, type) {
    const url = URL.createObjectURL(new Blob([content], { type })); const a = document.createElement('a'); a.href = url; a.download = name; a.click(); setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  function downloadCSV(rows, name) {
    const keys = [...new Set(rows.flatMap(r => Object.keys(r)))].filter(k => rows.every(r => typeof r[k] !== 'object' || r[k] === null));
    const quote = value => { let s = String(value ?? ''); if (typeof value === 'string' && /^\s*[=+@\-\t\r]/.test(s)) s = "'" + s; return '"' + s.replace(/"/g, '""') + '"'; };
    download([keys.map(quote).join(','), ...rows.map(r => keys.map(k => quote(r[k])).join(','))].join('\r\n'), name, 'text/csv;charset=utf-8');
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
      <details><summary>Compare two groups</summary><label class="check"><input id="explorer-compare" type="checkbox">Show A and B side by side</label>
        <div class="explorer-controls"><label class="wide">Group A<select id="explorer-group-a" multiple aria-label="Group A sessions"></select></label><label class="wide">Group B<select id="explorer-group-b" multiple aria-label="Group B sessions"></select></label></div></details>
      <div class="explorer-controls">
        <label class="wide">Plot<select id="explorer-plot"></select></label><label>X axis<select id="explorer-x"></select></label><label>Y axis / signal<select id="explorer-y"></select></label>
        <label id="explorer-measure-wrap">Measure<select id="explorer-measure"><option value="mistakes">Raw mistakes</option><option value="per_object">Mistakes / presented object</option><option value="percent">Incorrect checks (%)</option><option value="first_correct">Correct first checks (%)</option></select></label>
        <label id="explorer-style-wrap">Style<select id="explorer-style"></select></label>
        <label id="explorer-color-wrap">Colour by<select id="explorer-color"><option value="participant">Participant</option><option value="session">Session</option><option value="kind">Object type</option><option value="congruency">Congruency</option><option value="level">Level</option><option value="first_status">First-check outcome</option></select></label>
      </div>
      <details><summary>Filter by level, object or configuration</summary><div class="explorer-controls">
        <label>Levels<select id="explorer-levels" multiple size="3" aria-label="Levels"><option value="" disabled>Load sessions first</option></select><button data-all="levels" type="button">All levels</button></label>
        <label>Object type<select id="explorer-kind"><option value="">All types</option><option>corner</option><option>face</option></select></label>
        <label>Objects<select id="explorer-objects" multiple size="3" aria-label="Objects"></select><button data-all="objects" type="button">All objects</button></label>
        <label>Congruency<select id="explorer-congruency"><option value="">All</option><option>congruent</option><option>incongruent</option><option>no rule</option><option>unknown</option></select></label>
        <label>Level status<select id="explorer-completion"><option value="">All</option><option value="complete">Completed</option><option value="incomplete">Unfinished / unknown</option></select></label>
        <label>Configuration<select id="explorer-config"><option value="">All configurations</option></select></label>
        <label>Summary weighting<select id="explorer-weight"><option value="participant">Equal per participant</option><option value="session">Equal per session</option></select></label>
      </div></details>
      <div class="explorer-controls">
        <label class="check"><input id="explorer-individual" type="checkbox" checked>Individual observations</label>
        <label class="check"><input id="explorer-swap" type="checkbox">Swap axes</label>
        <label class="check" id="explorer-matrix-wrap"><input id="explorer-matrix" type="checkbox" checked>First-choice matrix</label>
        <button id="explorer-draw" type="button" class="primary">Plot</button><button id="explorer-export-settings" type="button" hidden>Save selection</button>
      </div>
      <output id="explorer-status" aria-live="polite"></output>
      <details id="explorer-notes" hidden><summary>Data notes</summary><output id="explorer-notes-text"></output></details>
      <details><summary>Congruency labels for older recordings</summary><p class="hint">Load sessions with Plot, then label each object here. Labels are saved in this browser and included in selection exports. Unknown labels stay unknown. For future recordings, set congruency in the trial editor.</p><div class="table-wrap"><table><thead><tr><th>Configuration</th><th>Level</th><th>Object</th><th>Congruency</th></tr></thead><tbody id="explorer-labels-body"></tbody></table></div></details>
      <div id="explorer-plots" class="plots"></div>
      <p class="hint">The three reference plots, gameplay summaries and HR views are computed here in your browser. Model-probability plots from the ZIP require the original model code.</p>`;
    $('back').onclick = () => { panel.hidden = true; document.querySelector('#admin-flow > .landing').hidden = false; applyTheme(); };
    $('theme').onclick = toggleTheme;
    $('refresh').onclick = () => refresh().catch(e => status(e.message, true));
    for (const id of ['participants', 'dates', 'from', 'to', 'paradigm', 'hr-only']) $(id).addEventListener('change', updateSessions);
    $('search').addEventListener('input', updateSessions);
    $('sessions').addEventListener('change', () => { generation++; loadedKey = ''; $('draw').disabled = false; });
    $('plot').addEventListener('change', axes);
    $('draw').onclick = plot;
    for (const button of panel.querySelectorAll('[data-all]')) button.onclick = () => {
      for (const opt of $(button.dataset.all).options) opt.selected = true;
      if (['participants', 'dates'].includes(button.dataset.all)) { if (button.dataset.all === 'participants') $('search').value = ''; if (button.dataset.all === 'dates') { $('from').value = ''; $('to').value = ''; } updateSessions(); }
    };
    $('export-settings').onclick = () => download(JSON.stringify({ sessions: selected($('sessions')), timezone, options: settings(), notes: lastFigures?.warnings || [] }, null, 2), 'analysis-selection.json', 'application/json');
    document.addEventListener('fullscreenchange', () => { for (const node of panel.querySelectorAll('.plot')) Plotly.Plots.resize(node); });
  }
  document.getElementById('btn-explorer').addEventListener('click', open);
})();
