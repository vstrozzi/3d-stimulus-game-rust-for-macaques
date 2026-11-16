/* Pure browser analysis, also loaded by the worker and Node verification. */
(function (root) {
  'use strict';
  // Matplotlib's default colour cycle (tab10), assigned automatically in order.
  const COLORS = ['#1f77b4', '#ff7f0e', '#2ca02c', '#d62728', '#9467bd', '#8c564b', '#e377c2', '#7f7f7f', '#bcbd22', '#17becf'];
  const STATUS = { correct: '#19a66c', position: '#f0a900', kind: '#ed4337', unknown: '#999', none: '#999' };
  // Mean lines, rings and cell shading follow the page theme (dark or white).
  const ink = o => o?.theme === 'light' ? '#111' : '#fff';
  const finite = v => typeof v === 'number' && Number.isFinite(v);
  const mean = a => a.length ? a.reduce((s, v) => s + v, 0) / a.length : null;
  const sd = a => a.length > 1 ? Math.sqrt(a.reduce((s, v) => s + (v - mean(a)) ** 2, 0) / (a.length - 1)) : null;
  const groupBy = (a, fn) => { const m = new Map(); for (const v of a) { const k = fn(v); if (!m.has(k)) m.set(k, []); m.get(k).push(v); } return m; };
  const mod = (x, n) => ((x % n) + n) % n;
  function stable(v) {
    if (Array.isArray(v)) return '[' + v.map(stable).join(',') + ']';
    if (v && typeof v === 'object') return '{' + Object.keys(v).sort().map(k => JSON.stringify(k) + ':' + stable(v[k])).join(',') + '}';
    return JSON.stringify(v);
  }
  function hash(v) { let h = 2166136261; for (const c of stable(v) || '') h = Math.imul(h ^ c.charCodeAt(0), 16777619); return (h >>> 0).toString(16).padStart(8, '0'); }
  const kindOfDoor = d => !Number.isInteger(d) ? 'unknown' : d % 2 === 0 ? 'corner' : 'face';
  function congruency(c) {
    const label = c.analysis_congruency ?? c.congruency ?? c._congruency;
    return ['congruent', 'incongruent', 'no rule'].includes(label) ? label : 'unknown';
  }
  function classify(row, cfg, orient) {
    const s = row.state_read || row;
    const threshold = cfg.cosine_alignment_threshold;
    const alignment = s.current_alignment;
    let door = Number.isInteger(row.chosen_door) ? row.chosen_door : null;
    // Six base doors: centers start at start_orient + pi/2 + pi/6.
    // The camera faces the origin, and BaseDoor.normal faces inward.
    if (door === null && finite(orient) && finite(s.camera_x) && finite(s.camera_z) && Math.hypot(s.camera_x, s.camera_z) > 0) {
      door = mod(Math.round((Math.atan2(s.camera_z, s.camera_x) - orient - Math.PI / 2) / (Math.PI / 3) - 0.5), 6);
    }
    const correct = typeof row.correct === 'boolean' ? row.correct : finite(alignment) && finite(threshold) ? alignment > threshold : null;
    let sameKind = door === null ? null : kindOfDoor(door) === kindOfDoor(cfg.target_door);
    // Older logs may have only the unsigned angular error. Door parity is
    // invariant to the sign, so kind can still be recovered; region cannot.
    if (sameKind === null && finite(alignment)) sameKind = Math.round(Math.acos(Math.max(-1, Math.min(1, alignment))) / (Math.PI / 3)) % 2 === 0;
    return {
      t: finite(row.check_input_event_elapsed_secs) ? row.check_input_event_elapsed_secs : s.present_elapsed_secs,
      correct, door, chosen_kind: door === null ? (sameKind === null ? 'unknown' : sameKind ? kindOfDoor(cfg.target_door) : kindOfDoor(cfg.target_door) === 'face' ? 'corner' : 'face') : kindOfDoor(door),
      status: correct === true ? 'correct' : correct === null || sameKind === null ? 'unknown' : sameKind ? 'position' : 'kind',
    };
  }
  function normalize(documents, catalog) {
    const sessionMap = new Map(catalog.map(s => [s.id, s]));
    // A re-timed file can be uploaded again after midnight under another day
    // folder. Session + original basename is its durable identity.
    const docs = [...new Map(documents.map(d => [d.session + '/' + d.path.split('/').pop(), d])).values()];
    const summaries = new Map();
    const levelKey = d => {
      const match = ('/' + d.path).match(/\/(level_\d+)\/([^/]+)\//);
      return d.session + '/' + (match ? match[1] + '/' + match[2] : d.path.split('/').slice(0, -1).join('/'));
    };
    for (const doc of docs) if (doc.kind === 'level') summaries.set(levelKey(doc), doc.data);
    const trials = [], packets = [], warnings = new Set();
    for (const document of docs) {
      if (document.kind !== 'trial') continue;
      const d = document.data, session = sessionMap.get(document.session) || { participant: 'unknown', date: 'unknown' };
      const summary = summaries.get(levelKey(document)) || {};
      const cfg = d.trial_config || {}, levelCfg = summary.level_config || {};
      const frames = d.check_frames || Object.values(d.frames || {}).filter(f => f.commands_sent?.check);
      const checks = (d.check_events || frames).map(f => classify(f, cfg, d.start_orient));
      const unknownChecks = checks.filter(c => c.correct === null).length;
      if (unknownChecks) warnings.add('Some check outcomes are unknown because their alignment or threshold is missing.');
      if (!checks.length && d.nr_attempts > 0) warnings.add('Some older trials have check counts but no check events; their mistakes cannot be reconstructed.');
      const first = checks[0] || null;
      const level = Number(d.level_index), object = Number(d.active_chain);
      const trial = {
        id: document.session + '/' + document.path.split('/').pop(), session: document.session,
        participant: session.participant, date: session.date, level, level_run: levelKey(document), object,
        config: hash(levelCfg.objects ? levelCfg : { level, config: cfg }),
        paradigm: d.session_info?.paradigm || 'unknown', kind: kindOfDoor(cfg.target_door),
        congruency: congruency(cfg), chain_pos: d.trial_index_in_chain,
        counter: d.trial_run_counter, started: d.timestamp_start,
        duration: d.elapsed_time_no_anim, animation_duration: d.elapsed_time_anim,
        attempts: d.nr_attempts, checks, first_status: first?.status || (d.nr_attempts > 0 ? 'unknown' : 'none'),
        first_correct: first ? first.correct : null, first_kind: first?.chosen_kind || 'none', first_region: first?.door ?? null,
        mistakes: unknownChecks || checks.length !== d.nr_attempts ? null : checks.filter(c => c.correct === false).length,
        complete: d.trial_chain_completed === true, level_complete: summary.level_completed === 'completed',
        timeout: d.outcome === 'timeout', level_status: summary.level_completed || 'unknown',
        outcome: d.outcome, t: d.hr_info?.trial_start_elapsed_secs ?? checks[0]?.t ?? null,
        start_t: d.hr_info?.trial_start_elapsed_secs ?? null, end_t: d.hr_info?.trial_end_elapsed_secs ?? null,
        remove_completed: levelCfg.fixed?.remove_completed_chains ?? null,
        partial: d.partial_trial === true,
      };
      trials.push(trial);
      // Only what the mean-HR plot needs: the bpm value, its time and validity.
      const info = d.hr_info || {};
      // Chest straps repeat the last bpm with no RR when the strap is off;
      // optical sensors never send RR, so for them any positive bpm counts.
      const optical = /sense|verity/i.test(String(info.device?.name || info.device || ''));
      for (const [index, p] of (d.hr_meas || []).entries()) {
        const timed = finite(p.beat_elapsed_secs);
        packets.push({ session: trial.session, trial: trial.id, packet: index, level, config: trial.config, hr: p.hr,
          timing: timed ? 'synchronized beat' : 'packet arrival (approximate)',
          t: timed ? p.beat_elapsed_secs : p.arrival_elapsed_secs,
          valid: finite(p.hr) && p.hr > 0 && (optical || (p.rr_ms || []).some(r => finite(r) && r > 0)),
        });
      }
    }
    trials.sort((a, b) => a.session.localeCompare(b.session) || a.counter - b.counter || String(a.started).localeCompare(String(b.started)));
    const rounds = new Map(), prevPos = new Map();
    for (const t of trials) {
      const key = t.level_run + '/' + t.object;
      t.round = (rounds.get(key) || 0) + 1; rounds.set(key, t.round);
      const previous = prevPos.get(key);
      t.chain_delta = previous === undefined ? null : t.chain_pos - previous;
      prevPos.set(key, t.chain_pos);
      t.first_sighting = t.round === 1;
    }
    for (const run of groupBy(trials, t => t.level_run).values()) {
      const last = run[run.length - 1];
      last.timeout = last.timeout || last.level_status === 'timeout';
    }
    packets.sort((a, b) => a.session.localeCompare(b.session) || a.t - b.t || a.packet - b.packet);
    // Trial files contain packets since the preceding save, including gaps
    // and late-arriving beats. Assign by recorded time, not by file bucket.
    const intervals = groupBy(trials.filter(t => finite(t.start_t) && finite(t.end_t)), t => t.session);
    for (const a of intervals.values()) a.sort((x, y) => x.start_t - y.start_t);
    const assignContext = p => {
      const a = intervals.get(p.session) || [];
      let lo = 0, hi = a.length;
      while (lo < hi) { const mid = (lo + hi) >>> 1; if (a[mid].start_t <= p.t) lo = mid + 1; else hi = mid; }
      const t = a[lo - 1];
      p.phase = !a.length ? 'file bucket (timing unknown)' : !t ? 'before first trial' : p.t <= t.end_t ? 'trial' : 'between trials';
      if (t) { p.trial = t.id; p.level = t.level; p.config = t.config; }
    };
    for (const p of packets) assignContext(p);
    if (trials.some(t => t.remove_completed === null)) warnings.add('Some older logs omit remove_completed_chains and other fixed settings.');
    return { trials, packets, warnings: [...warnings] };
  }
  function filter(dataset, options) {
    if (options.labelOverrides) dataset = { ...dataset, trials: dataset.trials.map(t => {
      const label = options.labelOverrides[`${t.config}|${t.level}|${t.object}`];
      return ['unknown', 'no rule', 'congruent', 'incongruent'].includes(label) ? { ...t, congruency: label } : t;
    }) };
    const keep = t => (!options.levels?.length || options.levels.includes(t.level)) && (!options.config || t.config === options.config)
      && (!options.objects?.length || options.objects.includes(`${t.config}|${t.level}|${t.object}`))
      && (!options.kind || t.kind === options.kind) && (!options.congruency || t.congruency === options.congruency)
      && (!options.completion || (options.completion === 'complete' ? t.level_complete : !t.level_complete));
    const trials = dataset.trials.filter(keep), ids = new Set(trials.map(t => t.id));
    return { ...dataset, trials, packets: dataset.packets.filter(p => ids.has(p.trial)) };
  }
  function metric(rows, measure) {
    if (measure === 'trials') return rows.length;
    if (measure === 'duration') return mean(rows.map(t => t.duration).filter(finite));
    if (measure === 'first_correct') { const v = rows.map(t => t.first_correct).filter(v => v !== null); return v.length ? 100 * mean(v.map(Number)) : null; }
    if (!rows.length || rows.some(t => t.mistakes === null)) return null;
    const mistakes = rows.reduce((s, t) => s + t.mistakes, 0);
    if (measure === 'per_object') return mistakes / new Set(rows.map(t => t.object)).size;
    if (measure === 'percent') { const n = rows.reduce((s, t) => s + t.attempts, 0); return n ? 100 * mistakes / n : null; }
    return mistakes;
  }
  const LABELS = { mistakes: 'Mistakes (incorrect checks)', per_object: 'Mistakes / presented object', percent: 'Incorrect checks (%)',
    trials: 'Object presentations', duration: 'Active trial duration (s)', first_correct: 'Correct first checks (%)',
    level: 'Level', object: 'Object', round: 'Object presentation number (round)', chain_pos: 'Chain position (0-based)',
    chain_delta: 'Change from previous chain position', counter: 'Session trial number', t: 'Session time (s)',
    participant: 'Participant', date: 'Session date', hr: 'Mean heart rate (bpm)', config: 'Configuration', kind: 'Object type', congruency: 'Congruency', session: 'Session' };
  const PLOTS = {
    mistakes_level: 'Mistakes per level', mistakes_object: 'Mistakes per object + first choices', timeline: 'Object timeline',
    trials: 'Trials / presentations per level', first_choice: 'First-choice distribution', duration: 'Trial duration',
    chain: 'Chain progression', scatter: 'Custom scatter / line', histogram: 'Distribution / histogram',
    hr_summary: 'Mean heart rate',
  };

  function groups(dataset, options) {
    if (options.compare) return ['A', 'B'].map(name => ({ name: 'Group ' + name, data: filterSessions(dataset, new Set(options['group' + name] || [])) }));
    return [{ name: 'Selection', data: dataset }];
  }
  function filterSessions(d, ids) { return Object.fromEntries(Object.entries(d).map(([k, v]) => [k, k === 'warnings' ? v : v.filter(x => ids.has(x.session))])); }
  function line(name, x, y, color, extra = {}) { return { type: 'scatter', mode: 'lines+markers', name, x, y, line: { color, width: 1.5 }, marker: { color, size: 6 }, connectgaps: false, ...extra }; }
  function escaped(value) { return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])); }
  function hover(t) { return `${escaped(t.participant)} · ${escaped(t.session)}<br>Level ${t.level}, object ${t.object}, round ${t.round}<br>Chain ${t.chain_pos} · ${escaped(t.first_status)}<br>${escaped(t.congruency)} · config ${escaped(t.config)}`; }
  // Participants cycle through tab10; after ten the line style changes.
  const PEOPLE = COLORS;
  const DASHES = ['solid', 'dash', 'dot', 'dashdot'];
  const KIND_COLOR = { corner: '#ef6a3a', face: '#2f7fd8', unknown: '#888' };
  const CONG_SYMBOL = { 'no rule': 'diamond', congruent: 'circle', incongruent: 'triangle-up', unknown: 'square' };
  const CONG_ORDER = { 'no rule': 0, congruent: 1, incongruent: 2, unknown: 3 };
  const byKindCong = (a, b) => (a.kind === b.kind ? 0 : a.kind === 'corner' ? -1 : 1) || CONG_ORDER[a.congruency] - CONG_ORDER[b.congruency];
  const sortValues = a => a.sort((x, y) => typeof x === 'number' ? x - y : String(x).localeCompare(String(y)));
  const legendOnly = (name, marker, extra = {}) => ({ type: 'scatter', mode: 'markers', name, x: [null], y: [null], marker, hoverinfo: 'skip', ...extra });
  // One value per session x level run (x object): the plotted observations.
  function observe(d, o, byObject) {
    const measure = o.plot === 'trials' ? 'trials' : o.measure;
    const observations = [...groupBy(d.trials, t => `${t.session}|${t.level_run}|${byObject ? t.object : ''}`).values()]
      .map(rows => ({ ...rows[0], value: metric(rows, measure), finished: rows[0].level_complete, n_presentations: rows.length }));
    return { measure, observations };
  }
  // Average repeat sessions within participant first (default), then across.
  function summarize(rows, o) {
    const values = [...groupBy(rows.filter(t => finite(t.value)), t => o.weight === 'session' ? t.session : t.participant).values()].map(a => mean(a.map(t => t.value)));
    return { mean: mean(values), sd: sd(values), n: values.length };
  }
  function countAxis(observations, xkey, o) {
    const values = sortValues([...new Set(observations.map(t => t[xkey]))]);
    return { tickmode: 'array', zeroline: false, tickvals: values, ticktext: values.map(x => `${escaped(x)}<br><span style="font-size:10px">n = ${new Set(observations.filter(t => t[xkey] === x && finite(t.value)).map(t => o.weight === 'session' ? t.session : t.participant)).size}</span>`) };
  }
  const exportRows = (observations, byObject) => observations.map(t => ({ participant: t.participant, session: t.session, level: t.level, object: byObject ? t.object : '', kind: t.kind, congruency: t.congruency, value: t.value, finished: t.finished, config: t.config }));
  // Reference figure 3: one line per participant, mean ± SD beside them.
  function levelFigure(d, o) {
    const xkey = o.x || 'level', { measure, observations } = observe(d, o, false), traces = [];
    const xvalues = sortValues([...new Set(observations.map(t => t[xkey]))]);
    const numeric = xvalues.every(finite), offset = numeric && o.individual ? 0.18 : 0;
    if (o.individual) {
      const people = [...new Set(observations.map(t => String(t.participant)))].sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
      const bySession = [...groupBy(observations, t => t.session)].sort((a, b) => people.indexOf(String(a[1][0].participant)) - people.indexOf(String(b[1][0].participant)) || a[0].localeCompare(b[0]));
      for (const [session, rows] of bySession) {
        const person = String(rows[0].participant), i = people.indexOf(person), color = PEOPLE[i % PEOPLE.length];
        const repeats = [...new Set(observations.filter(t => String(t.participant) === person).map(t => t.session))];
        const label = person + (repeats.length > 1 ? ' · ' + rows[0].date + ' #' + (repeats.indexOf(session) + 1) : '');
        const sorted = rows.slice().sort((a, b) => xvalues.indexOf(a[xkey]) - xvalues.indexOf(b[xkey]));
        traces.push(line(label, sorted.map(t => t[xkey]), sorted.map(t => t.value), color, {
          line: { color, width: 1.5, dash: DASHES[Math.floor(i / PEOPLE.length) % DASHES.length] }, text: sorted.map(hover),
          marker: { size: 7, color: sorted.map(t => t.finished ? color : '#fff0'), line: { color, width: 1.5 }, symbol: 'circle' },
          hovertemplate: '%{text}<br>%{y}<extra>%{fullData.name}</extra>' }));
      }
    }
    if (o.individual) traces.push(legendOnly('Level not finished', { color: '#fff0', size: 8, line: { color: '#888', width: 1.5 } }));
    const stats = xvalues.map(x => summarize(observations.filter(t => t[xkey] === x), o));
    traces.push(line('Level total, mean ± SD', xvalues.map(x => numeric ? x + offset : x), stats.map(s => s.mean), ink(o), {
      line: { color: ink(o), width: 3 }, marker: { size: 9, color: ink(o), line: { color: o.theme === 'light' ? '#fff' : '#000', width: 1.5 } },
      error_y: { type: 'data', array: stats.map(s => s.sd), visible: true, color: ink(o), thickness: 1.5, width: 5 },
      customdata: stats.map(s => s.n), hovertemplate: '%{x}: %{y:.3g}<br>n = %{customdata}<extra>%{fullData.name}</extra>' }));
    return { traces, rows: exportRows(observations, false), x: LABELS[xkey], y: LABELS[measure] || measure, axis: countAxis(observations, xkey, o),
      layout: { legend: { orientation: 'h', y: -0.2, yanchor: 'top', x: 0.5, xanchor: 'center', title: { text: o.individual ? 'Participant' : '', side: 'top' } } },
      note: 'Open circles mark unfinished levels (time limit). The mean ± SD is drawn just right of each level; n counts participants (or sessions) with a known value. Repeated sessions are averaged within participant by default.' };
  }
  // Reference figure 2: mistakes per object by type and congruency, with the
  // first check on first sighting underneath, aligned to the same levels.
  function objectFigure(d, o) {
    const { measure, observations } = observe(d, o, true), light = o.theme === 'light', traces = [], shapes = [];
    const levels = sortValues([...new Set(observations.map(t => t.level))]);
    const category = t => t.kind + ' · ' + t.congruency;
    const perLevel = new Map(levels.map(L => [L, [...new Map(observations.filter(t => t.level === L).map(t => [category(t), t])).values()].sort(byKindCong).map(category)]));
    const spot = (L, cat) => { const a = perLevel.get(L), w = Math.min(0.15, 0.6 / Math.max(1, a.length)); return L - 0.1 + (a.indexOf(cat) - (a.length - 1) / 2) * w; };
    const people = [...new Set(observations.map(t => t.session))];
    for (const [cat, rows] of [...groupBy(observations, category)].sort((a, b) => byKindCong(a[1][0], b[1][0]))) {
      const { kind, congruency } = rows[0], color = KIND_COLOR[kind] || '#888', symbol = CONG_SYMBOL[congruency];
      if (o.individual) traces.push({ type: 'scatter', mode: 'markers', showlegend: false, name: cat,
        x: rows.map(t => spot(t.level, cat) - 0.035 + 0.01 * (people.indexOf(t.session) % 5 - 2) / 2), y: rows.map(t => t.value), text: rows.map(hover), opacity: 0.4,
        marker: { size: 6, symbol: rows.map(t => t.finished ? symbol : symbol + '-open'), color, line: { color, width: 1 } }, hovertemplate: '%{text}<br>%{y}<extra></extra>' });
      const xs = levels.filter(L => rows.some(t => t.level === L)), stats = xs.map(L => summarize(rows.filter(t => t.level === L), o));
      traces.push({ type: 'scatter', mode: 'markers', showlegend: false, name: cat + ', mean ± SD', x: xs.map(L => spot(L, cat) + 0.02), y: stats.map(s => s.mean),
        marker: { size: 9, symbol, color, line: { color: light ? '#fff' : '#000', width: 1 } }, error_y: { type: 'data', array: stats.map(s => s.sd), visible: true, color, thickness: 1.5, width: 3 },
        customdata: stats.map(s => s.n), hovertemplate: `${escaped(cat)}<br>Level %{x:.0f}: %{y:.3g}<br>n = %{customdata}<extra></extra>` });
    }
    // Level total uses one observation per session x level (all objects).
    const totals = observe(d, o, false).observations, totalStats = levels.map(L => summarize(totals.filter(t => t.level === L), o));
    traces.push(line('Level total, mean ± SD', levels.map(L => L + 0.36), totalStats.map(s => s.mean), ink(o), {
      line: { color: ink(o), width: 2 }, marker: { size: 8, symbol: 'square', color: ink(o) },
      error_y: { type: 'data', array: totalStats.map(s => s.sd), visible: true, color: ink(o), thickness: 1.5, width: 4 },
      customdata: totalStats.map(s => s.n), hovertemplate: 'Level %{x:.0f}: %{y:.3g}<br>n = %{customdata}<extra>Level total</extra>' }));
    const grey = light ? '#777' : '#aaa';
    traces.push(legendOnly('Corner object', { color: KIND_COLOR.corner, size: 9 }), legendOnly('Face object', { color: KIND_COLOR.face, size: 9 }),
      legendOnly('No colour rule (L0–4)', { color: grey, size: 9, symbol: 'diamond' }), legendOnly('Congruent (L5–8)', { color: grey, size: 9 }),
      legendOnly('Incongruent (L5–8)', { color: grey, size: 9, symbol: 'triangle-up' }));
    if (o.individual) traces.push(legendOnly('Participant', { color: grey, size: 7, opacity: 0.45 }), legendOnly('Participant, level not finished', { color: '#fff0', size: 7, line: { color: grey, width: 1 } }));
    const band = light ? '#f1f1ef' : '#141414';
    for (const L of levels) if (L % 2 === 0) shapes.push({ type: 'rect', xref: 'x', yref: 'paper', x0: L - 0.5, x1: L + 0.5, y0: 0, y1: 1, fillcolor: band, line: { width: 0 }, layer: 'below' });
    const rows = exportRows(observations, true);
    let height = 560, yaxis = {}, layout = {};
    if (o.matrix !== false) {
      const first = d.trials.filter(t => t.first_sighting);
      const progress = new Map([...groupBy(d.trials, t => t.session)].map(([s, a]) => [s, a.reduce((n, t) => Math.max(n, t.level * 1000 + (t.level_complete ? 500 : 0) + (t.complete ? 100 : 0) + t.chain_pos), 0)]));
      const sessions = [...new Set(first.map(t => t.session))].sort((a, b) => progress.get(b) - progress.get(a) || a.localeCompare(b));
      const R = sessions.length, rowY = s => R - sessions.indexOf(s), shareY = R + 1.25;
      const repeats = groupBy(sessions, s => first.find(t => t.session === s).participant);
      const label = s => { const p = first.find(t => t.session === s).participant, r = repeats.get(p); return escaped(r.length > 1 ? `${p} #${r.indexOf(s) + 1}` : p); };
      const colKey = t => `${t.level}|${t.kind}|${t.congruency}|${t.object}`;
      const columns = [...new Map(first.map(t => [colKey(t), t])).values()].sort((a, b) => a.level - b.level || byKindCong(a, b) || a.object - b.object);
      const colX = new Map();
      for (const [L, cols] of groupBy(columns, t => t.level)) { const w = Math.min(0.2, 0.8 / cols.length); cols.forEach((t, j) => colX.set(colKey(t), L + (j - (cols.length - 1) / 2) * w)); }
      for (const [kind, name, color, symbol] of [['corner', 'Checked a corner first', KIND_COLOR.corner, 'triangle-up'], ['face', 'Checked a face first', KIND_COLOR.face, 'square'], [null, 'No check', '#888', 'x-thin-open']]) {
        const a = first.filter(t => kind ? t.first_kind === kind : !['corner', 'face'].includes(t.first_kind));
        traces.push({ type: 'scatter', mode: 'markers', name, legendgroup: 'first', yaxis: 'y2', x: a.map(t => colX.get(colKey(t))), y: a.map(t => rowY(t.session)), text: a.map(hover),
          marker: { color, symbol, size: 7, line: { color, width: 1 } }, hovertemplate: '%{text}<extra>%{fullData.name}</extra>' });
      }
      const shareText = [];
      for (const c of columns) {
        const a = first.filter(t => colKey(t) === colKey(c)), x = colX.get(colKey(c)), half = Math.min(0.08, 0.32 / Math.max(1, (perLevel.get(c.level) || [1]).length));
        const share = kind => 100 * mean([...groupBy(a, t => o.weight === 'session' ? t.session : t.participant).values()].map(b => b.filter(t => t.first_kind === kind).length / b.length));
        const corner = share('corner'), face = share('face'), base = shareY - 0.45, h = 0.9;
        shapes.push({ type: 'rect', xref: 'x', yref: 'y2', x0: x - half, x1: x + half, y0: base, y1: base + h * corner / 100, fillcolor: KIND_COLOR.corner, line: { width: 0 } },
          { type: 'rect', xref: 'x', yref: 'y2', x0: x - half, x1: x + half, y0: base + h * corner / 100, y1: base + h * (corner + face) / 100, fillcolor: KIND_COLOR.face, line: { width: 0 } });
        shareText.push([x, `Level ${c.level}, ${escaped(c.kind)} ${c.object} · ${escaped(c.congruency)}<br>corner first ${corner.toFixed(0)}% · face first ${face.toFixed(0)}% · n = ${a.length}`]);
      }
      traces.push({ type: 'scatter', mode: 'markers', showlegend: false, yaxis: 'y2', x: shareText.map(v => v[0]), y: shareText.map(() => shareY), text: shareText.map(v => v[1]), marker: { size: 12, opacity: 0 }, hovertemplate: '%{text}<extra></extra>' });
      traces.push(legendOnly('Share corner (bar)', { color: KIND_COLOR.corner, size: 10, symbol: 'square' }, { legendgroup: 'first' }));
      rows.push(...first.map(t => ({ table: 'first check on first sighting', participant: t.participant, session: t.session, level: t.level, object: t.object, kind: t.kind, congruency: t.congruency, first_kind: t.first_kind, first_status: t.first_status })));
      // Keep the top plot ~380 px and give each participant row ~17 px.
      const top = 380, bottom = R * 17 + 40, gap = 70;
      height = top + bottom + gap + 100;
      const plotH = top + bottom + gap, split = bottom / plotH;
      yaxis = { domain: [split + gap / plotH, 1] };
      layout.yaxis2 = { domain: [0, split], range: [0.4, shareY + 0.6], tickmode: 'array', tickvals: [...sessions.map(rowY), shareY], ticktext: [...sessions.map(label), 'corner share'],
        showgrid: false, zeroline: false, tickfont: { size: 10 }, title: { text: 'First check on<br>first sighting' } };
    }
    layout.legend = { orientation: 'v', x: 1.02, xanchor: 'left', y: 1, yanchor: 'top', tracegroupgap: 18 };
    return { traces, rows, x: 'Level', y: LABELS[measure] || measure, noSwap: true, height, shapes, yaxis, layout,
      axis: { ...countAxis(observations, 'level', o), range: [levels[0] - 0.6, levels.at(-1) + 0.6], showgrid: false, zeroline: false },
      margin: { t: 30, l: 90, r: 230, b: 40 },
      note: 'Top: mistakes per object, mean ± SD across participants (colour = object type, shape = congruency); faded points are participants, open when the level was not finished; black squares are the level total. Bottom: first check on the first presentation of each object, one row per participant (furthest progress on top); the top bar is the share who checked a corner first.' };
  }
  // Object timeline laid out like the reference figure: one grey cell per
  // object x round, objects grouped under "Level N", every session at a fixed
  // spot of a small grid inside each cell (furthest progress bottom left).
  // Dot sizes are fitted to the drawn cell size in the explorer (trace.meta).
  function timeline(d, o = {}) {
    const MAX_ROUND = 15, CELL = 0.86, LEVEL_GAP = 0.7, light = o.theme === 'light';
    const fill = { band: light ? '#f7f7f5' : '#0c0c0c', cell: light ? '#e6e6e3' : '#232323', text: light ? '#666' : '#999' };
    const progress = new Map([...groupBy(d.trials, t => t.session)].map(([s, a]) => [s, a.reduce((n, t) => Math.max(n, t.level * 1000 + (t.level_complete ? 500 : 0) + (t.complete ? 100 : 0) + t.chain_pos), 0)]));
    const sessions = [...progress.keys()].sort((a, b) => progress.get(b) - progress.get(a) || a.localeCompare(b));
    const gridCols = Math.max(1, Math.ceil(Math.sqrt(sessions.length))), gridRows = Math.max(1, Math.ceil(sessions.length / gridCols));
    const slot = new Map(sessions.map((s, i) => [s, [((i % gridCols) + 0.5) / gridCols - 0.5, (Math.floor(i / gridCols) + 0.5) / gridRows - 0.5]]));
    const congOrder = { 'no rule': 0, congruent: 1, incongruent: 2, unknown: 3 };
    const colKey = t => `${t.level}|${t.kind}|${t.congruency}|${t.object}`;
    const columns = [...new Map(d.trials.map(t => [colKey(t), { key: colKey(t), level: t.level, kind: t.kind, congruency: t.congruency, object: t.object }])).values()]
      .sort((a, b) => a.level - b.level || (a.kind === b.kind ? 0 : a.kind === 'corner' ? -1 : 1) || congOrder[a.congruency] - congOrder[b.congruency] || a.object - b.object);
    let x = 0, previous = null;
    for (const c of columns) { if (previous !== null && c.level !== previous) x += LEVEL_GAP; c.x = x++; previous = c.level; }
    const colOf = new Map(columns.map(c => [c.key, c]));
    const overflow = d.trials.some(t => t.round > MAX_ROUND);
    const topRow = Math.min(MAX_ROUND, d.trials.reduce((n, t) => Math.max(n, t.round), 1));
    const overflowY = topRow + 1.35, top = overflow ? overflowY + 0.85 : topRow + 0.5;
    const coordinate = t => {
      const [dx, dy] = slot.get(t.session), c = colOf.get(colKey(t));
      return t.round > MAX_ROUND ? [c.x + dx * CELL, overflowY - 0.45 + dy * 0.2] : [c.x + dx * CELL, t.round + dy * CELL];
    };
    const shapes = [], annotations = [], traces = [];
    for (const [level, cols] of groupBy(columns, c => c.level)) {
      const x0 = cols[0].x - 0.5, x1 = cols.at(-1).x + 0.5;
      shapes.push({ type: 'rect', x0, x1, y0: 0.45, y1: top, fillcolor: fill.band, line: { width: 0 }, layer: 'below' });
      shapes.push({ type: 'line', x0: x0 + 0.1, x1: x1 - 0.1, yref: 'paper', y0: 1.005, y1: 1.005, line: { color: fill.text, width: 1 } });
      annotations.push({ x: (x0 + x1) / 2, y: 1.005, xref: 'x', yref: 'paper', yanchor: 'bottom', showarrow: false, text: `<b>Level ${escaped(level)}</b>` });
      for (const c of cols) {
        const a = d.trials.filter(t => colKey(t) === c.key);
        const reached = Math.min(MAX_ROUND, a.reduce((n, t) => Math.max(n, t.round), 0));
        for (let r = 1; r <= reached; r++) shapes.push({ type: 'rect', x0: c.x - CELL / 2, x1: c.x + CELL / 2, y0: r - CELL / 2, y1: r + CELL / 2, fillcolor: fill.cell, line: { width: 0 }, layer: 'below' });
        const late = a.filter(t => t.round > MAX_ROUND);
        if (late.length) {
          shapes.push({ type: 'rect', x0: c.x - CELL / 2, x1: c.x + CELL / 2, y0: overflowY - 0.7, y1: overflowY + 0.7, fillcolor: fill.cell, line: { width: 0 }, layer: 'below' });
          const known = late.filter(t => t.first_correct !== null);
          annotations.push({ x: c.x, y: overflowY + 0.2, showarrow: false, font: { size: 9, color: fill.text },
            text: `${new Set(late.map(t => t.session)).size} p.<br>${late.length} tr.<br>${known.length ? Math.round(100 * known.filter(t => t.first_correct).length / known.length) + '%' : '–'}` });
        }
      }
    }
    const statusName = { correct: 'First check correct', position: 'Wrong position, right kind (corner/face)', kind: 'Wrong kind (corner vs. face)', none: 'No check', unknown: 'Unknown' };
    for (const [status, color] of Object.entries(STATUS)) {
      const a = d.trials.filter(t => t.first_status === status && t.round <= MAX_ROUND), xy = a.map(coordinate);
      traces.push({ type: 'scatter', mode: 'markers', name: statusName[status], meta: 'dot', x: xy.map(v => v[0]), y: xy.map(v => v[1]), text: a.map(hover),
        marker: { color, size: 6, symbol: ['none', 'unknown'].includes(status) ? 'x-thin-open' : 'circle', line: { color, width: 1.2 } }, hovertemplate: '%{text}<extra></extra>' });
    }
    for (const [label, key, symbol, meta] of [['Chain completed', 'complete', 'circle-open', 'ring'], ['Last trial (time limit)', 'timeout', 'square-open', 'stop']]) {
      const a = d.trials.filter(t => t[key] && (key === 'timeout' || t.round <= MAX_ROUND)), xy = a.map(coordinate);
      traces.push({ type: 'scatter', mode: 'markers', name: label, meta, x: xy.map(v => v[0]), y: xy.map(v => v[1]), text: a.map(t => hover(t) + (t.round > MAX_ROUND ? `<br>Stopped in round ${t.round}` : '')),
        marker: { color: ink(o), size: 9, symbol, line: { width: 1.3, color: ink(o) } }, hovertemplate: '%{text}<extra>%{fullData.name}</extra>' });
    }
    // Key: where each session sits inside a cell.
    const repeats = groupBy(sessions, s => d.trials.find(t => t.session === s).participant);
    const keyLabel = s => { const p = d.trials.find(t => t.session === s).participant, r = repeats.get(p); return r.length > 1 ? `${p} #${r.indexOf(s) + 1}` : p; };
    const longest = sessions.reduce((n, s) => Math.max(n, keyLabel(s).length), 3);
    const keyW = Math.max(2.4, gridCols * Math.max(0.8, 0.17 * longest)), keyH = Math.max(1.6, gridRows * 0.6), keyX = (columns.at(-1)?.x ?? 0) + 1.2 + keyW / 2, keyY = top - 0.7 - keyH / 2;
    shapes.push({ type: 'rect', x0: keyX - keyW / 2, x1: keyX + keyW / 2, y0: keyY - keyH / 2, y1: keyY + keyH / 2, fillcolor: fill.cell, line: { width: 0 }, layer: 'below' });
    annotations.push({ x: keyX, y: keyY + keyH / 2, yanchor: 'bottom', showarrow: false, font: { size: 10, color: fill.text }, text: 'Participant position<br>in a cell' });
    annotations.push({ x: keyX, y: keyY - keyH / 2, yanchor: 'top', showarrow: false, font: { size: 9, color: fill.text }, text: 'furthest progress<br>bottom left' });
    traces.push({ type: 'scatter', mode: 'text', showlegend: false, hoverinfo: 'skip', x: sessions.map(s => keyX + slot.get(s)[0] * keyW * 0.92), y: sessions.map(s => keyY + slot.get(s)[1] * keyH * 0.9),
      text: sessions.map(s => escaped(keyLabel(s))), textfont: { size: 10, color: ink(o) } });
    const short = { congruent: 'cong.', incongruent: 'incong.', unknown: '?', 'no rule': '' };
    const yTicks = Array.from({ length: topRow }, (_, i) => i + 1);
    return { traces, rows: d.trials.map(({ checks, ...t }) => t), x: '', y: 'Round', noSwap: true,
      fit: { cols: gridCols, rows: gridRows, w: CELL, h: CELL },
      height: Math.max(420, 200 + (top - 0.45) * Math.max(30, gridRows * 12)),
      margin: { t: 95, b: 70, l: 60, r: 20 },
      axis: { tickmode: 'array', tickvals: columns.map(c => c.x), ticktext: columns.map(c => escaped(c.kind) + (short[c.congruency] ? `<br><span style="font-size:9px">${short[c.congruency]}</span>` : '')),
        tickangle: 0, tickfont: { size: 10 }, showgrid: false, zeroline: false, showline: false, range: [-0.7, keyX + keyW / 2 + 0.3] },
      yaxis: { tickmode: 'array', tickvals: overflow ? [...yTicks, overflowY] : yTicks, ticktext: overflow ? [...yTicks, '>15'] : yTicks, showgrid: false, zeroline: false, range: [0.4, top + 0.05] },
      shapes, layout: { annotations },
      note: 'Each cell is one object in one round; each participant has a fixed position inside the cell (key on the right). Row >15: participants (p.), trials (tr.) and % correct first checks after round 15; a square there marks a stop after round 15. Hover a dot to identify it.',
    };
  }
  function scatter(d, o) {
    const traces = [], rows = d.trials;
    let i = 0;
    for (const [label, a] of groupBy(rows, t => t[o.color || 'participant'])) {
      for (const [session, b] of groupBy(a, t => t.session)) {
        const sorted = b.slice().sort((a, b) => a.counter - b.counter);
        traces.push(line(String(label), sorted.map(t => t[o.x || 'round']), sorted.map(t => t[o.y || 'duration']), COLORS[i % COLORS.length], {
          mode: o.style === 'line' ? 'lines+markers' : 'markers', legendgroup: String(label), showlegend: session === a[0].session, text: sorted.map(hover), hovertemplate: '%{text}<br>%{x}, %{y}<extra></extra>' }));
      }
      i++;
    }
    return { traces, rows: rows.map(({ checks, ...t }) => t), x: LABELS[o.x || 'round'], y: LABELS[o.y || 'duration'] };
  }
  function firstChoice(d, o) {
    const traces = [], rows = [];
    const first = d.trials.filter(t => t.first_sighting), xkey = o.x || 'level';
    const xs = [...new Set(first.map(t => t[xkey]))].sort((a, b) => a - b);
    const byRegion = o.y === 'region';
    const choice = t => byRegion ? t.first_region === null ? t.first_kind === 'none' ? 'none' : 'unknown' : String(t.first_region) : t.first_kind;
    const categories = byRegion ? ['0', '1', '2', '3', '4', '5', 'unknown', 'none'] : ['corner', 'face', 'unknown', 'none'];
    for (const [index, kind] of categories.entries()) {
      const y = xs.map(x => {
        const a = first.filter(t => t[xkey] === x);
        const proportions = [...groupBy(a, t => o.weight === 'session' ? t.session : t.participant).values()].map(b => b.filter(t => choice(t) === kind).length / b.length);
        const p = 100 * mean(proportions);
        rows.push({ [xkey]: x, choice: kind, breakdown: byRegion ? 'region' : 'kind', percent: p, n: proportions.length }); return p;
      });
      traces.push({ type: 'bar', name: byRegion && !['none', 'unknown'].includes(kind) ? 'Door ' + kind : kind, x: xs, y, marker: { color: byRegion ? COLORS[index % COLORS.length] : KIND_COLOR[kind] || '#777' } });
    }
    return { traces, rows, x: LABELS[xkey], y: 'First choices (%)', layout: { barmode: 'stack' } };
  }
  function distribution(d, o) {
    const field = o.y || 'duration', input = d.trials;
    if (!['duration', 'mistakes'].includes(field)) throw new Error('Choose duration or mistakes for the gameplay distribution.');
    const traces = []; let i = 0;
    for (const [label, a] of groupBy(input, t => t[o.color || 'participant'])) {
      const values = a.map(t => t[field]).filter(finite), color = COLORS[i++ % COLORS.length];
      traces.push(o.style === 'box' || o.style === 'violin' ? { type: o.style, name: String(label), y: values, [o.style === 'box' ? 'boxpoints' : 'points']: o.individual ? 'all' : false, marker: { color }, line: { color }, ...(o.style === 'violin' ? { box: { visible: true } } : {}) }
        : { type: 'histogram', name: String(label), x: values, histnorm: 'probability density', opacity: 0.6, marker: { color }, nbinsx: 30 });
    }
    return { traces, rows: input.map(t => ({ participant: t.participant, session: t.session, level: t.level, value: t[field] })), x: ['box', 'violin'].includes(o.style) ? 'Group' : LABELS[field], y: ['box', 'violin'].includes(o.style) ? LABELS[field] : 'Probability density', layout: ['box', 'violin'].includes(o.style) ? {} : { barmode: 'overlay' } };
  }
  function hrSummary(d, o) {
    const xkey = o.x === 'counter' ? 'counter' : 'level';
    const eligible = d.packets.filter(p => p.valid && finite(p.hr) && (p.phase === 'trial' || p.phase === 'file bucket (timing unknown)'));
    const byTrial = groupBy(eligible, p => p.trial);
    const selected = groupBy(d.trials, t => xkey === 'counter' ? t.id : t.session + '/' + t.level);
    const observations = [...selected.values()].map(trials => {
      const first = trials[0], packets = trials.flatMap(t => byTrial.get(t.id) || []);
      return { participant: first.participant, session: first.session, level: first.level,
        ...(xkey === 'counter' ? { counter: first.counter, object: first.object } : {}),
        value: mean(packets.map(p => p.hr)), n_readings: packets.length };
    });
    const traces = [];
    if (o.individual) for (const [session, rows] of groupBy(observations, p => p.session)) {
      const sorted = rows.slice().sort((a, b) => a[xkey] - b[xkey]);
      traces.push(line(rows[0].participant + ' · ' + session.split('/').pop(), sorted.map(p => p[xkey]), sorted.map(p => p.value), COLORS[traces.length % COLORS.length], {
        customdata: sorted.map(p => [p.level, p.n_readings]),
        hovertemplate: '%{x}: %{y:.2f} bpm<br>Level %{customdata[0]} · %{customdata[1]} readings<extra>%{fullData.name}</extra>',
      }));
    }
    const xs = [...new Set(observations.map(p => p[xkey]))].sort((a, b) => a - b);
    const values = xs.map(x => {
      const rows = observations.filter(p => p[xkey] === x && finite(p.value));
      return [...groupBy(rows, p => o.weight === 'session' ? p.session : p.participant).values()].map(a => mean(a.map(p => p.value)));
    });
    traces.push(line('Mean ± SD', xs, values.map(mean), ink(o), {
      error_y: { type: 'data', array: values.map(sd), visible: true, color: ink(o) }, line: { color: ink(o), width: 3 },
      customdata: values.map(a => a.length), hovertemplate: '%{x}: %{y:.2f} bpm<br>n = %{customdata}<extra>Mean ± SD</extra>',
    }));
    return { traces, rows: observations, x: LABELS[xkey], y: 'Mean heart rate (bpm)',
      note: 'Each point is the arithmetic mean of valid HR readings within the selected trial or level. Missing readings stay blank. Recorded trial boundaries exclude pre-game and between-trial readings; older files without boundaries use file membership. Group summaries average sessions within participant by default.' };
  }
  function figure(dataset, options) {
    const d = filter(dataset, options), o = options;
    const make = d => {
      switch (o.plot) {
        case 'mistakes_level': case 'trials': return levelFigure(d, o);
        case 'mistakes_object': return objectFigure(d, o);
        case 'timeline': return timeline(d, o);
        case 'first_choice': return firstChoice(d, o);
        case 'duration': return scatter(d, { ...o, y: 'duration' });
        case 'chain': return scatter(d, { ...o, y: o.y || 'chain_pos' });
        case 'scatter': return scatter(d, o);
        case 'histogram': return distribution(d, o);
        case 'hr_summary': return hrSummary(d, o);
        default: throw new Error('Unknown plot');
      }
    };
    const panels = groups(d, o).map(g => ({ ...make(g.data), title: `${g.name}  (n = ${new Set(g.data.trials.map(t => t.participant)).size})` }));
    // Side-by-side groups share one y scale, as in the reference figures.
    if (o.compare && ['mistakes_level', 'trials', 'mistakes_object', 'hr_summary'].includes(o.plot)) {
      const ys = panels.flatMap(p => p.traces.filter(t => !t.yaxis || t.yaxis === 'y').flatMap(t => (t.y || []).map((y, i) => [y, t.error_y?.array?.[i] || 0])))
        .filter(([y]) => finite(y)).flatMap(([y, e]) => [y - (finite(e) ? e : 0), y + (finite(e) ? e : 0)]);
      if (ys.length) {
        const lo = Math.min(...ys), hi = Math.max(...ys), pad = (hi - lo || 1) * 0.06;
        for (const p of panels) p.yaxis = { ...p.yaxis, range: [o.plot === 'hr_summary' ? lo - pad : Math.min(0, lo - pad), hi + pad] };
      }
    }
    const warnings = [...d.warnings];
    if (d.trials.some(t => t.congruency === 'unknown')) warnings.push('Some objects have unknown congruency. Add labels below or in the trial editor; labels are never guessed from level number.');
    if ([...groupBy(d.trials, t => t.level).values()].some(a => new Set(a.map(t => t.config)).size > 1)) warnings.push('The selection combines different configurations for the same level. Use the Configuration filter for a like-for-like comparison.');
    if (o.plot === 'hr_summary' && d.packets.some(p => p.timing !== 'synchronized beat')) warnings.push('Some HR recordings use packet arrival time; assignment to trial boundaries is approximate.');
    return { panels, warnings, count: d.trials.length, sessions: new Set(d.trials.map(t => t.session)).size };
  }
  const api = { normalize, figure, classify, metric, congruency, kindOfDoor, hash, mean, sd, PLOTS, LABELS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.AdminAnalysis = api;
})(typeof self !== 'undefined' ? self : globalThis);
