/* Pure browser analysis, also loaded by the worker and Node verification. */
(function (root) {
  'use strict';
  const TAU = 2 * Math.PI;
  const COLORS = ['#5799ed', '#f56b3c', '#21b98a', '#eeaf23', '#dc75ab', '#9a80f6', '#71cbd0', '#cba57e'];
  const STATUS = { correct: '#19a66c', position: '#f0a900', kind: '#ed4337', unknown: '#999', none: '#999' };
  const finite = v => typeof v === 'number' && Number.isFinite(v);
  const mean = a => a.length ? a.reduce((s, v) => s + v, 0) / a.length : null;
  const sd = a => a.length > 1 ? Math.sqrt(a.reduce((s, v) => s + (v - mean(a)) ** 2, 0) / (a.length - 1)) : null;
  const median = a => { const b = a.slice().sort((x, y) => x - y); return b.length ? (b[Math.floor((b.length - 1) / 2)] + b[Math.floor(b.length / 2)]) / 2 : null; };
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
    const trials = [], packets = [], waves = [], warnings = new Set();
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
      const info = d.hr_info || {};
      const deviceName = String(info.device?.name || info.device || 'unknown');
      const optical = /sense|verity/i.test(deviceName) || (d.waveforms || []).includes('hr_ppg') || !!d.hr_ppg?.length;
      for (const [index, p] of (d.hr_meas || []).entries()) {
        const timed = finite(p.beat_elapsed_secs);
        const arrival = p.arrival_elapsed_secs;
        packets.push({ ...p, session: trial.session, participant: trial.participant, date: trial.date,
          trial: trial.id, packet: index, level, config: trial.config,
          device: deviceName,
          timing: timed ? 'synchronized beat' : 'packet arrival (approximate)',
          sync_status: info.sync_info?.status || 'unsynchronized',
          sync_precision_ms: info.sync_info?.precision_ms ?? null,
          t: timed ? p.beat_elapsed_secs : arrival,
          valid: finite(p.hr) && p.hr > 0 && (optical || (p.rr_ms || []).some(r => finite(r) && r > 0)),
          connection_events: index === 0 ? info.connection_events || [] : [],
        });
      }
      for (const key of ['hr_ecg', 'hr_ppg']) for (const frame of d[key] || []) waves.push({ ...frame, signal: key, session: trial.session, participant: trial.participant, level, config: trial.config });
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
      if (t) { p.file_trial = p.file_trial || p.trial; p.trial = t.id; p.level = t.level; p.config = t.config; }
    };
    for (const p of packets) assignContext(p);
    const rr = [];
    for (const p of packets) {
      let back = 0;
      for (let i = (p.rr_ms || []).length - 1; i >= 0; i--) {
        const value = p.rr_ms[i];
        if (finite(value) && value > 0 && finite(p.t)) rr.push({ ...p, rr: value, t: p.t - back, interval_index: i });
        back += finite(value) ? value / 1000 : 0;
      }
    }
    rr.sort((a, b) => a.session.localeCompare(b.session) || a.t - b.t);
    for (const p of rr) assignContext(p);
    if (trials.some(t => t.remove_completed === null)) warnings.add('Some older logs omit remove_completed_chains and other fixed settings.');
    return { trials, packets, rr, waves, warnings: [...warnings] };
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
    return { ...dataset, trials, packets: dataset.packets.filter(p => ids.has(p.trial)), rr: dataset.rr.filter(p => ids.has(p.trial)),
      waves: dataset.waves.filter(w => trials.some(t => t.session === w.session && t.level === w.level && t.config === w.config)) };
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
    participant: 'Participant', date: 'Session date', hr: 'Heart rate (bpm)', rr: 'RR interval (ms)', config: 'Configuration', kind: 'Object type', congruency: 'Congruency', session: 'Session' };
  const PLOTS = {
    mistakes_level: 'Mistakes per level', mistakes_object: 'Mistakes per object + first choices', timeline: 'Object timeline',
    trials: 'Trials / presentations per level', first_choice: 'First-choice distribution', duration: 'Trial duration',
    chain: 'Chain progression', scatter: 'Custom scatter / line', hr: 'Heart-rate timeline', rr: 'RR timeline',
    histogram: 'Distribution / histogram', hr_summary: 'Heart rate by level', event_hr: 'Event-aligned heart rate',
    poincare: 'RR Poincaré plot', waveform: 'ECG / optical signal', hrv: 'RR variability by level',
  };
  function groups(dataset, options) {
    if (options.compare) return ['A', 'B'].map(name => ({ name: 'Group ' + name, data: filterSessions(dataset, new Set(options['group' + name] || [])) }));
    return [{ name: 'Selection', data: dataset }];
  }
  function filterSessions(d, ids) { return Object.fromEntries(Object.entries(d).map(([k, v]) => [k, k === 'warnings' ? v : v.filter(x => ids.has(x.session))])); }
  function line(name, x, y, color, extra = {}) { return { type: 'scatter', mode: 'lines+markers', name, x, y, line: { color, width: 1.5 }, marker: { color, size: 6 }, connectgaps: false, ...extra }; }
  function escaped(value) { return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])); }
  function hover(t) { return `${escaped(t.participant)} · ${escaped(t.session)}<br>Level ${t.level}, object ${t.object}, round ${t.round}<br>Chain ${t.chain_pos} · ${escaped(t.first_status)}<br>${escaped(t.congruency)} · config ${escaped(t.config)}`; }
  function levelFigure(d, o, byObject = false) {
    const measure = o.plot === 'trials' ? 'trials' : o.measure;
    const xkey = o.x || 'level';
    const observations = [];
    for (const rows of groupBy(d.trials, t => `${t.session}|${t.level_run}|${byObject ? t.object : ''}`).values()) {
      observations.push({ ...rows[0], value: metric(rows, measure), finished: rows[0].level_complete, n_presentations: rows.length });
    }
    const traces = [];
    const split = byObject ? t => t.kind + ' · ' + t.congruency : () => 'Level total';
    const subjectColors = new Map([...new Set(observations.map(t => t.participant))].map((person, i) => [person, COLORS[i % COLORS.length]]));
    const categories = [...groupBy(observations, split)];
    for (const [categoryIndex, [category, cat]] of categories.entries()) {
      const shiftX = x => byObject && finite(x) ? x + (categoryIndex - (categories.length - 1) / 2) * Math.min(.12, .5 / Math.max(1, categories.length - 1)) : x;
      const color = byObject ? cat[0].kind === 'corner' ? '#f56b3c' : '#5799ed' : '#fff';
      const xvalues = [...new Set(cat.map(t => t[xkey]))].sort((a, b) => typeof a === 'number' ? a - b : String(a).localeCompare(String(b)));
      // First average repeat sessions within participant, then across people.
      const values = xvalues.map(x => {
        const atX = cat.filter(t => t[xkey] === x && finite(t.value));
        return [...groupBy(atX, t => o.weight === 'session' ? t.session : t.participant).values()].map(rows => mean(rows.map(t => t.value)));
      });
      const means = values.map(mean), errors = values.map(sd);
      if (o.individual) {
        for (const [session, rows] of groupBy(cat, t => t.session)) {
          const person = rows[0].participant;
          const repeats = [...new Set(cat.filter(t => t.participant === person).map(t => t.session))];
          const label = person + (repeats.length > 1 ? ' · ' + rows[0].date + ' #' + (repeats.indexOf(session) + 1) : '');
          const sorted = rows.slice().sort((a, b) => xvalues.indexOf(a[xkey]) - xvalues.indexOf(b[xkey]));
          const c = byObject ? color : subjectColors.get(person);
          traces.push(line(label + (byObject ? ' · ' + category : ''), sorted.map(t => shiftX(t[xkey])), sorted.map(t => t.value), c, {
            mode: byObject ? 'markers' : 'lines+markers', opacity: 0.65, text: sorted.map(hover),
            marker: { size: 7, color: c, symbol: sorted.map(t => t.finished ? 'circle' : 'circle-open') }, hovertemplate: '%{text}<br>%{y}<extra>%{fullData.name}</extra>' }));
        }
      }
      traces.push(line(`${category}, mean ± SD`, xvalues.map(shiftX), means, color, { line: { color, width: 3 },
        marker: { size: 9, color, symbol: byObject ? (category.includes('incongruent') ? 'triangle-up' : category.includes('no rule') ? 'diamond' : 'circle') : 'circle' },
        error_y: { type: 'data', array: errors, visible: true, color },
        customdata: values.map(v => v.length), hovertemplate: '%{x}: %{y:.3g}<br>n = %{customdata}<extra>%{fullData.name}</extra>' }));
    }
    if (byObject) {
      const total = levelFigure(d, { ...o, individual: false }, false);
      traces.push(...total.traces.map(t => ({ ...t, marker: { ...t.marker, symbol: 'square' } })));
    }
    const axisValues = [...new Set(observations.map(t => t[xkey]))].sort((a, b) => typeof a === 'number' ? a - b : String(a).localeCompare(String(b)));
    return { traces, rows: observations.map(t => ({ participant: t.participant, session: t.session, level: t.level, object: byObject ? t.object : '', kind: t.kind, congruency: t.congruency, value: t.value, finished: t.finished, config: t.config })), x: LABELS[xkey], y: LABELS[measure] || measure,
      axis: { tickmode: 'array', tickvals: axisValues, ticktext: axisValues.map(x => `${escaped(x)}<br>n = ${new Set(observations.filter(t => t[xkey] === x && finite(t.value)).map(t => o.weight === 'session' ? t.session : t.participant)).size}`) },
      note: 'Open observations mark unfinished levels. n counts participants or sessions with a known value. Repeated sessions are averaged within participant by default; SD is sample standard deviation.' };
  }
  function firstMatrix(d, o = {}) {
    const rows = d.trials.filter(t => t.first_sighting);
    const traces = [];
    const objectLabel = t => `L${t.level} · ${t.kind} ${t.object} · ${t.congruency}`;
    const columns = [...new Set(rows.map(objectLabel))];
    const progress = new Map([...groupBy(d.trials, t => t.session)].map(([session, a]) => [session, a.reduce((n, t) => Math.max(n, t.level * 1000 + (t.complete ? 100 : 0) + t.chain_pos), 0)]));
    const participants = [...new Set(rows.map(t => t.session))].sort((a, b) => progress.get(a) - progress.get(b));
    const rowLabel = t => t.participant + ' · ' + t.date + ' #' + (participants.indexOf(t.session) + 1);
    for (const kind of ['corner', 'face']) {
      const share = columns.map(column => {
        const a = rows.filter(t => objectLabel(t) === column);
        const proportions = [...groupBy(a, t => o.weight === 'session' ? t.session : t.participant).values()].map(b => b.filter(t => t.first_kind === kind).length / b.length);
        return 100 * mean(proportions);
      });
      traces.push({ type: 'bar', name: 'Share ' + kind, x: columns, y: share, yaxis: 'y2', marker: { color: kind === 'corner' ? '#f56b3c' : '#5799ed' }, hovertemplate: '%{x}<br>%{y:.1f}%<extra>%{fullData.name}</extra>' });
    }
    for (const kind of ['corner', 'face', 'unknown', 'none']) {
      const a = rows.filter(t => t.first_kind === kind);
      traces.push({ type: 'scatter', mode: 'markers', name: 'First choice: ' + kind,
        x: a.map(objectLabel),
        y: a.map(rowLabel),
        text: a.map(hover), marker: { color: kind === 'corner' ? '#f56b3c' : kind === 'face' ? '#5799ed' : '#999', symbol: kind === 'corner' ? 'triangle-up' : kind === 'face' ? 'square' : 'x', size: 9 },
        hovertemplate: '%{text}<extra>%{fullData.name}</extra>' });
    }
    return { traces, rows: rows.map(({ checks, ...t }) => t), x: 'Level / object / congruency', y: 'Participant / session', height: Math.max(540, new Set(rows.map(t => t.session)).size * 22 + 300),
      yaxis: { domain: [0, .72], categoryorder: 'array', categoryarray: participants.map(id => rowLabel(rows.find(t => t.session === id))) },
      layout: { barmode: 'stack', yaxis2: { domain: [.83, 1], title: 'First choice (%)', range: [0, 100], gridcolor: '#222' } },
      note: 'First check on the first presentation of each object. Orange triangles: corner; blue squares: face; crosses: no/unknown check. Bars show choice proportions; any unfilled share represents no/unknown checks. Rows are ordered by progress.' };
  }
  function timeline(d) {
    const traces = [], sessions = [...new Set(d.trials.map(t => t.session))];
    const side = Math.ceil(Math.sqrt(sessions.length || 1));
    const xpos = t => `L${t.level} · ${t.kind} ${t.object} · ${t.congruency}`;
    const categories = [...new Set(d.trials.map(xpos))];
    const coordinate = t => {
      const i = sessions.indexOf(t.session);
      return [categories.indexOf(xpos(t)) + ((i % side + 0.5) / side - 0.5) * 0.7,
        t.round + (Math.floor(i / side) / side - 0.5) * 0.65];
    };
    for (const [status, color] of Object.entries(STATUS)) {
      const a = d.trials.filter(t => t.first_status === status), xy = a.map(coordinate);
      traces.push({ type: 'scatter', mode: 'markers', name: { correct: 'First check correct', position: 'Wrong position, right kind', kind: 'Wrong kind', none: 'No check', unknown: 'Unknown' }[status],
        x: xy.map(v => v[0]), y: xy.map(v => v[1]), text: a.map(hover),
        marker: { color, size: Math.max(4, Math.min(10, 24 / side)), symbol: ['none', 'unknown'].includes(status) ? 'x' : 'circle' }, hovertemplate: '%{text}<extra></extra>' });
    }
    for (const [label, key, symbol] of [['Chain completed', 'complete', 'circle-open'], ['Last saved trial (timeout)', 'timeout', 'square-open']]) {
      const a = d.trials.filter(t => t[key]), xy = a.map(coordinate);
      traces.push({ type: 'scatter', mode: 'markers', name: label, x: xy.map(v => v[0]), y: xy.map(v => v[1]), text: a.map(hover),
        marker: { color: '#fff', size: Math.max(8, Math.min(16, 34 / side)), symbol, line: { width: 1.5 } }, hovertemplate: '%{text}<extra>%{fullData.name}</extra>' });
    }
    return { traces, rows: d.trials.map(({ checks, ...t }) => t), x: 'Level / object', y: 'Round',
      axis: { tickmode: 'array', tickvals: categories.map((_, i) => i), ticktext: categories, tickangle: -45 },
      shapes: categories.map((_, i) => ({ type: 'rect', x0: i - 0.45, x1: i + 0.45, y0: 0.5, y1: d.trials.reduce((n, t) => Math.max(n, t.round), 1) + 0.5, fillcolor: i % 2 ? '#151515' : '#222', line: { width: 0 }, layer: 'below' })),
      note: 'One dot per object presentation; first-check outcome. Each session has a fixed position within a cell. Rings mark completion; squares mark the last saved timeout trial. Hover identifies the participant.',
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
      traces.push({ type: 'bar', name: byRegion && !['none', 'unknown'].includes(kind) ? 'Door ' + kind : kind, x: xs, y, marker: { color: byRegion ? COLORS[index % COLORS.length] : kind === 'corner' ? '#f56b3c' : kind === 'face' ? '#5799ed' : '#777' } });
    }
    return { traces, rows, x: LABELS[xkey], y: 'First choices (%)', layout: { barmode: 'stack' } };
  }
  function series(d, field) {
    const input = field === 'rr' ? d.rr : d.packets;
    return input.filter(p => field === 'rr' || p.valid).filter(p => finite(p.t) && finite(p[field]));
  }
  function lowerBound(a, t) { let lo = 0, hi = a.length; while (lo < hi) { const mid = (lo + hi) >>> 1; if (a[mid].t < t) lo = mid + 1; else hi = mid; } return lo; }
  function hrTimeline(d, o, field) {
    const traces = [], rows = series(d, field); let i = 0;
    for (const [session, a] of groupBy(rows, p => p.session)) {
      const x = [], y = [], text = []; let start = 0, sum = 0;
      for (let j = 0; j < a.length; j++) {
        const p = a[j], prev = a[j - 1];
        if (prev && p.t - prev.t > (field === 'rr' ? 3 : 5)) { x.push(null); y.push(null); text.push(''); }
        x.push(p.t);
        sum += p[field];
        while (start < j && a[start].t < p.t - 10) sum -= a[start++][field];
        y.push(o.smooth ? sum / (j - start + 1) : p[field]);
        text.push(`${escaped(p.participant)}<br>${escaped(p.timing)} · ${escaped(p.sync_status)}<br>Sync precision: ${p.sync_precision_ms ?? 'unknown'} ms`);
      }
      traces.push(line(a[0].participant + ' · ' + session.split('/').pop(), x, y, COLORS[i++ % COLORS.length], { mode: field === 'rr' ? 'markers' : 'lines', text, hovertemplate: '%{x:.2f}s · %{y:.2f}<br>%{text}<extra></extra>' }));
    }
    const events = d.trials.flatMap(t => t.checks.map(c => ({ ...c, participant: t.participant, session: t.session, complete: t.complete && c === t.checks[t.checks.length - 1] })));
    for (const status of ['correct', 'position', 'kind']) {
      const a = events.filter(e => e.status === status && finite(e.t));
      traces.push({ type: 'scatter', mode: 'markers', name: status + ' check', x: a.map(e => e.t), y: a.map(() => 0.02), yaxis: 'y2',
        marker: { color: STATUS[status], size: 6, symbol: 'line-ns-open' }, text: a.map(e => escaped(e.participant)), hovertemplate: '%{x:.2f}s · %{text}<extra>%{fullData.name}</extra>' });
    }
    const completed = events.filter(e => e.complete && finite(e.t));
    traces.push({ type: 'scatter', mode: 'markers', name: 'Chain completed', x: completed.map(e => e.t), y: completed.map(() => .02), yaxis: 'y2', marker: { color: '#fff', symbol: 'circle-open', size: 12 }, text: completed.map(e => escaped(e.participant)), hovertemplate: '%{x:.2f}s · %{text}<extra>Chain completed</extra>' });
    const disconnects = d.packets.flatMap(p => p.connection_events.filter(e => /disconnect/i.test(e.event)).map(e => ({ t: e.elapsed_secs, participant: p.participant })));
    traces.push({ type: 'scatter', mode: 'markers', name: 'Sensor disconnected', x: disconnects.map(e => e.t), y: disconnects.map(() => .04), yaxis: 'y2', marker: { color: '#aaa', symbol: 'x', size: 8 }, text: disconnects.map(e => escaped(e.participant)), hovertemplate: '%{x:.2f}s · %{text}<extra>Sensor disconnected</extra>' });
    const shapes = [...groupBy(d.trials.filter(t => finite(t.t)), t => t.session + '/' + t.level).values()].map(a => ({ type: 'line', x0: a[0].t, x1: a[0].t, y0: 0, y1: 1, yref: 'paper', line: { color: '#444', dash: 'dot', width: 1 } }));
    return { traces, rows: rows.map(p => ({ session: p.session, participant: p.participant, level: p.level, t: p.t, [field]: p[field], timing: p.timing, sync_status: p.sync_status })), x: LABELS.t, y: LABELS[field], shapes,
      layout: { yaxis2: { overlaying: 'y', range: [0, 1], visible: false } }, note: 'Smoothing uses a trailing 10-second window. Gaps stay open. Packet arrival times are approximate; synchronized beats use their recorded sensor mapping.' };
  }
  function distribution(d, o) {
    const field = o.y || 'duration', input = ['hr', 'rr'].includes(field) ? series(d, field) : d.trials;
    const traces = []; let i = 0;
    for (const [label, a] of groupBy(input, t => t[o.color || 'participant'])) {
      const values = a.map(t => t[field]).filter(finite), color = COLORS[i++ % COLORS.length];
      traces.push(o.style === 'box' || o.style === 'violin' ? { type: o.style, name: String(label), y: values, [o.style === 'box' ? 'boxpoints' : 'points']: o.individual ? 'all' : false, marker: { color }, line: { color }, ...(o.style === 'violin' ? { box: { visible: true } } : {}) }
        : { type: 'histogram', name: String(label), x: values, histnorm: 'probability density', opacity: 0.6, marker: { color }, nbinsx: 30 });
    }
    return { traces, rows: input.map(t => ({ participant: t.participant, session: t.session, level: t.level, value: t[field] })), x: ['box', 'violin'].includes(o.style) ? 'Group' : LABELS[field], y: ['box', 'violin'].includes(o.style) ? LABELS[field] : 'Probability density', layout: ['box', 'violin'].includes(o.style) ? {} : { barmode: 'overlay' } };
  }
  function hrSummary(d, o) {
    const traces = [], rows = series(d, 'hr').filter(p => p.phase === 'trial' || p.phase === 'file bucket (timing unknown)'); let i = 0;
    const observations = [...groupBy(rows, p => p.session + '/' + p.level).values()].map(a => ({ participant: a[0].participant, session: a[0].session, level: a[0].level, value: o.style === 'median' ? median(a.map(p => p.hr)) : mean(a.map(p => p.hr)) }));
    if (['box', 'violin'].includes(o.style)) {
      for (const [level, a] of groupBy(observations, p => p.level)) traces.push({ type: o.style, name: 'Level ' + level, y: a.map(p => p.value), [o.style === 'box' ? 'boxpoints' : 'points']: 'all', marker: { color: COLORS[i++ % COLORS.length] } });
    } else {
      if (o.individual) for (const [session, a] of groupBy(observations, p => p.session)) traces.push(line(a[0].participant + ' · ' + a[0].session.split('/').pop(), a.map(p => p.level), a.map(p => p.value), COLORS[i++ % COLORS.length]));
      const levels = [...new Set(observations.map(p => p.level))].sort((a, b) => a - b);
      const values = levels.map(l => {
        const a = observations.filter(p => p.level === l);
        return o.weight === 'session' ? a.map(p => p.value) : [...groupBy(a, p => p.participant).values()].map(b => mean(b.map(p => p.value)));
      });
      traces.push(line('Mean ± SD', levels, values.map(mean), '#fff', { error_y: { type: 'data', array: values.map(sd), visible: true }, line: { color: '#fff', width: 3 } }));
    }
    return { traces, rows: observations, x: 'Level', y: 'Heart rate (bpm)', note: 'Uses samples within recorded trial boundaries. Pre-game and between-trial samples are excluded when those boundaries are available. Older files without boundaries use their file membership.' };
  }
  function eventHR(d, o) {
    const observations = [], traces = [], events = [];
    const bySession = groupBy(series(d, 'hr'), p => p.session);
    const sums = new Map([...bySession].map(([key, a]) => { const p = [0]; for (const v of a) p.push(p[p.length - 1] + v.hr); return [key, p]; }));
    for (const t of d.trials) for (const c of t.checks) {
      if (o.event === 'completion' ? t.complete && c === t.checks[t.checks.length - 1] : o.event === 'correct' ? c.correct === true : c.correct === false) events.push({ ...c, session: t.session, participant: t.participant });
    }
    for (let sec = -10; sec <= 20; sec++) {
      const points = [];
      for (const e of events) {
        const a = bySession.get(e.session) || [], prefix = sums.get(e.session);
        const lo = lowerBound(a, e.t + sec - 0.5), hi = lowerBound(a, e.t + sec + 0.5);
        if (hi > lo) points.push({ participant: e.participant, session: e.session, value: (prefix[hi] - prefix[lo]) / (hi - lo) });
      }
      const units = [...groupBy(points, p => o.weight === 'session' ? p.session : p.participant).values()].map(a => mean(a.map(p => p.value)));
      observations.push({ seconds: sec, hr: mean(units), sd: sd(units), n: units.length });
    }
    traces.push(line('Mean ± SD', observations.map(p => p.seconds), observations.map(p => p.hr), '#fff', { error_y: { type: 'data', array: observations.map(p => p.sd), visible: true }, customdata: observations.map(p => p.n), hovertemplate: '%{x}s · %{y:.2f} bpm<br>n = %{customdata}<extra></extra>' }));
    return { traces, rows: observations, x: 'Time from event (s)', y: 'Heart rate (bpm)', note: 'Aligned to recorded check time, in 1-second bins. Nearby events may overlap; packet-arrival recordings have approximate alignment. n counts participants (or sessions), not events.' };
  }
  function poincare(d) {
    const traces = [], rows = []; let i = 0;
    for (const [session, a] of groupBy(series(d, 'rr'), p => p.session)) {
      const pairs = [];
      for (let j = 1; j < a.length; j++) if (a[j].t - a[j - 1].t <= Math.max(3, a[j].rr / 1000 + 0.5) && !a[j].connection_events.some(e => /disconnect/i.test(e.event))) {
        pairs.push({ session, participant: a[j].participant, rr: a[j - 1].rr, next_rr: a[j].rr });
      }
      rows.push(...pairs);
      traces.push({ type: 'scattergl', mode: 'markers', name: a[0]?.participant + ' · ' + session.split('/').pop(), x: pairs.map(p => p.rr), y: pairs.map(p => p.next_rr), marker: { color: COLORS[i++ % COLORS.length], size: 5, opacity: 0.6 } });
    }
    return { traces, rows, x: 'RR interval (ms)', y: 'Next RR interval (ms)', note: 'Raw recorded intervals; gaps and session boundaries are excluded. No automatic artifact correction.' };
  }
  function waveform(d, o) {
    const traces = [], rows = []; let color = 0;
    for (const [key, a] of groupBy(d.waves, f => f.session + '/' + f.signal)) {
      const first = a[0], channels = first.signal === 'hr_ecg' ? ['uV'] : ['ppg0', 'ppg1', 'ppg2', 'ambient'];
      const frames = a.filter(f => finite(f.first_elapsed_secs)).sort((a, b) => a.first_elapsed_secs - b.first_elapsed_secs);
      for (const channel of channels) {
        const x = [], y = []; let previous = null;
        for (const f of frames) {
          const values = f[channel] || [];
          if (previous !== null && f.first_elapsed_secs - previous > 2 * f.dt_ms / 1000) { x.push(null); y.push(null); }
          for (let j = 0; j < values.length; j++) { x.push(f.first_elapsed_secs + j * f.dt_ms / 1000); y.push(values[j]); }
          previous = x[x.length - 1];
        }
        rows.push(...x.map((t, j) => ({ session: first.session, signal: first.signal, channel, t, value: y[j] })));
        traces.push({ type: 'scattergl', mode: 'lines', name: `${first.participant} · ${channel} · ${key.split('/').slice(-2).join('/')}`, x, y, line: { color: COLORS[color++ % COLORS.length], width: 1 }, connectgaps: false });
      }
    }
    return { traces, rows, x: 'Synchronized session time (s)', y: 'ECG (µV) / optical signal (raw units)', note: 'Only signal frames with a recorded clock mapping are shown. Select a session/level to inspect full-resolution signals.' };
  }
  function hrv(d, o) {
    const rows = [];
    for (const a of groupBy(series(d, 'rr').filter(p => p.phase === 'trial' || p.phase === 'file bucket (timing unknown)'), p => p.session + '/' + p.level).values()) {
      const diffs = [];
      for (let i = 1; i < a.length; i++) if (a[i].t - a[i - 1].t <= Math.max(3, a[i].rr / 1000 + 0.5)) diffs.push((a[i].rr - a[i - 1].rr) ** 2);
      rows.push({ participant: a[0].participant, session: a[0].session, level: a[0].level, n: a.length,
        duration: a[a.length - 1].t - a[0].t, rmssd: diffs.length ? Math.sqrt(mean(diffs)) : null, sdnn: sd(a.map(p => p.rr)) });
    }
    const traces = [...groupBy(rows, r => r.session)].map(([session, a], i) => line(a[0].participant + ' · ' + session.split('/').pop(), a.map(p => p.level), a.map(p => p[o.y || 'rmssd']), COLORS[i % COLORS.length], { text: a.map(p => `${p.n} intervals · ${p.duration.toFixed(1)} seconds`), hovertemplate: '%{y:.2f} ms<br>%{text}<extra></extra>' }));
    return { traces, rows, x: 'Level', y: (o.y || 'rmssd').toUpperCase() + ' (ms)', note: 'Descriptive RR variability from uncorrected intervals. Recording duration and interval count are shown on hover; short or interrupted recordings are not directly comparable.' };
  }
  function figure(dataset, options) {
    const d = filter(dataset, options), o = options;
    const make = d => {
      switch (o.plot) {
        case 'mistakes_level': case 'trials': return levelFigure(d, o);
        case 'mistakes_object': return levelFigure(d, o, true);
        case 'timeline': return timeline(d);
        case 'first_choice': return firstChoice(d, o);
        case 'duration': return scatter(d, { ...o, y: 'duration' });
        case 'chain': return scatter(d, { ...o, y: o.y || 'chain_pos' });
        case 'scatter': return scatter(d, o);
        case 'hr': case 'rr': return hrTimeline(d, o, o.plot);
        case 'histogram': return distribution(d, o);
        case 'hr_summary': return hrSummary(d, o);
        case 'event_hr': return eventHR(d, o);
        case 'poincare': return poincare(d);
        case 'waveform': return waveform(d, o);
        case 'hrv': return hrv(d, o);
        default: throw new Error('Unknown plot');
      }
    };
    const panels = groups(d, o).map(g => ({ ...make(g.data), title: g.name }));
    if (o.plot === 'mistakes_object' && o.matrix !== false) for (const g of groups(d, o)) panels.push({ ...firstMatrix(g.data, o), title: g.name + ' · First check on first sighting', matrix: true });
    const warnings = [...d.warnings];
    if (d.trials.some(t => t.congruency === 'unknown')) warnings.push('Some objects have unknown congruency. Add labels below or in the trial editor; labels are never guessed from level number.');
    if ([...groupBy(d.trials, t => t.level).values()].some(a => new Set(a.map(t => t.config)).size > 1)) warnings.push('The selection combines different configurations for the same level. Use the Configuration filter for a like-for-like comparison.');
    if (['hr', 'rr', 'event_hr', 'poincare', 'hrv', 'hr_summary'].includes(o.plot) && d.packets.some(p => p.timing !== 'synchronized beat')) warnings.push('Some HR data uses packet arrival time, not synchronized beat time. Event alignment is approximate for these recordings.');
    if (o.plot === 'waveform' && d.waves.some(w => !finite(w.first_elapsed_secs))) warnings.push('Unsynchronized waveform frames cannot be placed on session time and are omitted.');
    return { panels, warnings, count: d.trials.length, sessions: new Set(d.trials.map(t => t.session)).size };
  }
  const api = { normalize, figure, classify, metric, congruency, kindOfDoor, hash, mean, sd, PLOTS, LABELS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.AdminAnalysis = api;
})(typeof self !== 'undefined' ? self : globalThis);
