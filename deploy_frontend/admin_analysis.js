/* Drawing for the admin explorer. The numbers come from the analysis team's
 * scripts on the server (build_dataset.py + the data part of make_MAIN_figures.py);
 * this file only groups them per figure exactly as make_MAIN_figures.py does and
 * turns them into Plotly traces in the same style. Loaded by the worker and Node tests. */
(function (root) {
  'use strict';
  // ── make_MAIN_figures.py style ──
  const COLORS = ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#6250d6', '#e34948'];
  const TYPE_COL = { 1: '#e8692e', 2: '#2a78d6' };             // 1 corner object, 2 face object
  const CHOICE_COL = { corner: TYPE_COL[1], face: TYPE_COL[2] };
  const GREEN = '#1a9a5c', RED = '#d7362b', YELLOW = '#f0a500';
  const CONG_MK = { True: 'circle', False: 'triangle-up', '': 'diamond' };  // congruent / incongruent / no colour rule
  const CONG_MS = { True: 9, False: 10, '': 8 };
  const TY = { 1: '(ty. I)', 2: '(ty. II)' };
  const DASH = '5px,2.5px';                                       // 9th+ participant: same hues, dashed
  const CAP = 15, CELL = 0.88;
  const THEMES = {
    light: { INK: '#222222', MUTED: '#6f6f6f', GRID: '#e8e8e8', FAINT: '#d9d9d9', SURF: '#ffffff', BAND: '#f5f5f3', TILE: '#ececea' },
    dark: { INK: '#e6e6e6', MUTED: '#9a9a9a', GRID: '#262626', FAINT: '#3a3a3a', SURF: '#000000', BAND: '#101010', TILE: '#262626' },
  };
  const palette = theme => THEMES[theme === 'light' ? 'light' : 'dark'];

  const finite = v => typeof v === 'number' && Number.isFinite(v);
  const mean = a => a.length ? a.reduce((s, v) => s + v, 0) / a.length : null;
  const std = a => { if (a.length < 2) return null; const m = mean(a); return Math.sqrt(a.reduce((s, v) => s + (v - m) ** 2, 0) / (a.length - 1)); };  // pandas ddof=1
  const groupBy = (a, fn) => { const m = new Map(); for (const v of a) { const k = fn(v); if (!m.has(k)) m.set(k, []); m.get(k).push(v); } return m; };
  const cmp = (a, b) => a < b ? -1 : a > b ? 1 : 0;                       // Python ordering
  const cmpTuple = (a, b) => { for (let i = 0; i < a.length; i++) { const c = cmp(a[i], b[i]); if (c) return c; } return 0; };
  const short = p => String(p).replace('kjpp_vrsafe_bl_pilot_', 'pilot ').replace('kjpp_vrsafe_', '');
  const escaped = v => String(v ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const LABEL_MAX = 18;
  const label = p => { const s = short(p); return s.length > LABEL_MAX ? s.slice(0, 9) + '…' + s.slice(-8) : s; };
  function rng(seed) { let s = seed >>> 0; return () => { s = (s + 0x6D2B79F5) >>> 0; let t = s; t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }
  const legendOnly = (name, marker, extra = {}) => ({ type: 'scatter', mode: 'markers', name, x: [null], y: [null], marker, hoverinfo: 'skip', ...extra });
  const trialId = (session, counter) => session + '|' + counter;
  /** Top of a y axis that starts at 0 (ylim(0, None)): highest value or error bar, plus 5 %. */
  const top = traces => { const v = traces.filter(t => !t.yaxis || t.yaxis === 'y').flatMap(t => (t.y || []).map((y, i) => finite(y) ? y + (finite(t.error_y?.array?.[i]) ? t.error_y.array[i] : 0) : null)).filter(finite); return v.length ? Math.max(1, ...v) * 1.05 : 1; };
  const K = (...parts) => parts.join('\u0000');

  // ── data ──
  /** Server payload -> rows (one per trial, in the scripts' time order) + heart-rate readings. */
  function load(payload) {
    const T = payload.rows.map(r => Object.fromEntries(payload.columns.map((c, i) => [c, r[i]])));
    for (const t of T) {
      t.id = trialId(t.session, t.trial_run_counter);
      t.slot = [t.object_type_nr, t.cong_key, t.object_id_order_by_type];   // same object for everyone
      t.slotKey = t.slot.join('|');
    }
    const ids = new Set(T.map(t => t.id));
    // Heart rate: assign each reading to the trial whose recorded boundaries contain it
    // (a file holds every reading since the previous save); older files: file membership.
    const packets = [];
    for (const h of payload.heart || []) {
      const optical = /sense|verity/i.test(h.device || '');
      for (const [index, [beat, arrival, hr, rr]] of h.readings.entries()) {
        const timed = finite(beat);
        packets.push({ session: h.session, trial: trialId(h.session, h.trial_run_counter), packet: index, hr,
          t: timed ? beat : arrival, timing: timed ? 'synchronized beat' : 'packet arrival (approximate)',
          // Chest straps repeat the last bpm without RR when the strap is off.
          valid: finite(hr) && hr > 0 && (optical || rr === true) });
      }
    }
    const intervals = groupBy((payload.heart || []).filter(h => finite(h.start) && finite(h.end)), h => h.session);
    for (const a of intervals.values()) a.sort((x, y) => x.start - y.start);
    for (const p of packets) {
      const a = intervals.get(p.session) || [];
      let lo = 0, hi = a.length;
      while (lo < hi) { const mid = (lo + hi) >>> 1; if (a[mid].start <= p.t) lo = mid + 1; else hi = mid; }
      const h = a[lo - 1];
      p.phase = !a.length ? 'file bucket (timing unknown)' : !h ? 'before first trial' : p.t <= h.end ? 'trial' : 'between trials';
      if (h) p.trial = trialId(h.session, h.trial_run_counter);
    }
    return { T, packets: packets.filter(p => ids.has(p.trial)), participants: payload.participants || [], warnings: payload.warnings || [] };
  }
  function filter(d, o) {
    const cong = { congruent: 'True', incongruent: 'False', 'no rule': '' };
    const T = d.T.filter(t => (!o.levels?.length || o.levels.includes(t.level_nr))
      && (!o.kind || t.object_type === o.kind)
      && (!o.congruency || t.features_congruent === cong[o.congruency])
      && (!o.completion || (o.completion === 'complete' ? t.level_done : !t.level_done))
      && (!o.sessions || o.sessions.has(t.session)));
    const ids = new Set(T.map(t => t.id)), people = new Set(T.map(t => t.participant_id));
    return { ...d, T, packets: d.packets.filter(p => ids.has(p.trial)), participants: d.participants.filter(p => people.has(p)) };
  }
  /** make_MAIN_figures.load(): O = one row per participant x level x object (first row = first sighting). */
  function objects(T) {
    return [...groupBy(T, t => K(t.participant_id, t.level_nr, t.object_id)).values()].map(a => ({
      participant_id: a[0].participant_id, level_nr: a[0].level_nr, object_id: a[0].object_id,
      wrong: a.reduce((s, t) => s + t.wrong, 0), typ: a[0].object_type_nr, cong: a[0].features_congruent,
      slot: a[0].slot, slotKey: a[0].slotKey, done: a[0].level_done, first_choice: a[0].first_attempt_type,
    }));
  }
  /** Progress order: levels completed (desc), furthest level (desc), trials (asc). */
  function progressOrder(T) {
    const prog = new Map();
    for (const a of groupBy(T, t => t.participant_id).values()) {
      const levels = [...groupBy(a, t => t.level_nr).values()].map(b => b[0]);
      prog.set(a[0].participant_id, { done: levels.filter(t => t.level_done).length, reached: Math.max(...levels.map(t => t.level_nr)), trials: a.length });
    }
    return [...prog.keys()].sort(cmp).sort((a, b) => prog.get(b).done - prog.get(a).done || prog.get(b).reached - prog.get(a).reached || prog.get(a).trials - prog.get(b).trials);
  }
  const xaxisStyle = c => ({ showline: true, linecolor: c.INK, linewidth: 1, ticks: 'outside', tickcolor: c.INK, ticklen: 4, showgrid: false, zeroline: false });
  const yaxisStyle = c => ({ showline: true, linecolor: c.INK, linewidth: 1, ticks: 'outside', tickcolor: c.INK, ticklen: 4, showgrid: true, gridcolor: c.GRID, zeroline: false, rangemode: 'tozero' });
  const nTicks = (levels, n, c) => ({ tickmode: 'array', tickvals: levels, ticktext: levels.map(L => `${L}<br><span style="font-size:10px;color:${c.MUTED}">n = ${n(L)}</span>`) });

  // ── 1. mistakes per level (also trials per level) ──
  function mistakesPerLevel(d, o) {
    const c = palette(o.theme), shift = 0.2, traces = [], rows = [];
    const value = o.plot === 'trials' ? a => a.length : a => a.reduce((s, t) => s + t.wrong, 0);
    for (const a of groupBy(d.T, t => K(t.participant_id, t.level_nr)).values())
      rows.push({ participant_id: a[0].participant_id, level_nr: a[0].level_nr, value: value(a), level_done: a[0].level_done });
    const M = groupBy(rows, r => r.participant_id);
    if (o.individual !== false) d.participants.forEach((pid, i) => {
      const pts = (M.get(pid) || []).slice().sort((a, b) => a.level_nr - b.level_nr), color = COLORS[i % 8];
      traces.push({ type: 'scatter', mode: 'lines+markers', name: escaped(label(pid)),
        x: pts.map(p => p.level_nr), y: pts.map(p => p.value), opacity: 0.8,
        line: { color, width: 1.4, dash: i < 8 ? 'solid' : DASH },
        marker: { size: 6, color: pts.map(p => p.level_done ? color : c.SURF), line: { color, width: 1.3 } },
        hovertemplate: `${escaped(pid)}<br>Level %{x}: %{y}<extra></extra>` });
    });
    const levels = [...new Set(rows.map(r => r.level_nr))].sort((a, b) => a - b);
    const st = levels.map(L => { const v = rows.filter(r => r.level_nr === L).map(r => r.value); return { L, mean: mean(v), sd: std(v), n: v.length }; });
    traces.push({ type: 'scatter', mode: 'lines+markers', name: 'Mean ± SD', legend: 'legend2', x: st.map(s => s.L + shift), y: st.map(s => s.mean),
      line: { color: c.INK, width: 2.4 }, marker: { size: 8, color: c.INK, line: { color: c.SURF, width: 1.2 } },
      error_y: { type: 'data', array: st.map(s => s.sd), visible: true, color: c.INK, thickness: 1.4, width: 4 },
      customdata: st.map(s => s.n), hovertemplate: 'mean %{y:.2f}<br>n = %{customdata}<extra>Mean ± SD</extra>' });
    traces.push(legendOnly('Level not finished (time limit)', { size: 6, color: c.SURF, line: { color: c.MUTED, width: 1.3 } }, { legend: 'legend2' }));
    return { traces, rows, y: AXES[o.plot === 'trials' ? 'trials' : 'mistakes_level'].y, x: AXES.mistakes_level.x,
      axis: { ...xaxisStyle(c), ...nTicks(levels, L => st.find(s => s.L === L).n, c), range: [Math.min(0, ...levels) - 0.4, Math.max(8, ...levels) + 0.5] },
      yaxis: { ...yaxisStyle(c), range: [0, top(traces)] },
      // Equal columns as wide as the longest name (≈7 px per character plus the line sample), so long test names never overlap.
      layout: { legend: { orientation: 'h', x: 0.5, xanchor: 'center', y: -0.2, yanchor: 'top', entrywidthmode: 'pixels',
        entrywidth: Math.max(82, 44 + 7 * Math.max(0, ...d.participants.map(p => label(p).length))), title: { text: 'Participant', side: 'top center' } },
        legend2: { orientation: 'h', x: 1, xanchor: 'right', y: 1.02, yanchor: 'bottom' } },
      note: 'Mistake: a wrong check (in a won trial, the checks before the winning one; otherwise every check). Open markers: level not finished (time limit). Mean ± SD across participants, drawn just right of each level; n = participants who played the level.' };
  }

  // ── 2. mistakes per object, with the first-check strip ──
  function mistakesPerObject(d, o) {
    const c = palette(o.theme), span = 0.64, center = -0.08, totX = 0.37, traces = [], shapes = [];
    const O = objects(d.T), order = progressOrder(d.T), npmax = o.npmax || Math.max(1, order.length);
    const rowOf = new Map(order.map((p, i) => [p, npmax - 1 - i])), meanY = npmax + 0.25;
    const random = rng(1), strip = o.matrix !== false;
    const levels = [...new Set(O.map(r => r.level_nr))].sort((a, b) => a - b);
    const points = new Map(), means = new Map(), choices = { corner: [], face: [], none: [] };
    for (const L of levels) {
      const dl = O.filter(r => r.level_nr === L);
      const slots = [...new Map(dl.map(r => [r.slotKey, r.slot])).values()].sort(cmpTuple), kk = slots.length, w = span / kk;
      slots.forEach((s, i) => {
        const x = L + center + (kk > 1 ? (i - (kk - 1) / 2) * w : 0), a = dl.filter(r => r.slotKey === s.join('|'));
        const key = `${a[0].typ}|${a[0].cong}`, jit = kk > 1 ? w * 0.28 : 0.08;
        if (!points.has(key)) { points.set(key, []); means.set(key, []); }
        for (const r of a) points.get(key).push({ x: x + (random() * 2 - 1) * jit, y: r.wrong, done: r.done, p: r.participant_id, L });
        const v = a.map(r => r.wrong);
        means.get(key).push({ x, mean: mean(v), sd: std(v), n: v.length, L });
        if (!strip) return;
        for (const r of a) (choices[r.first_choice] || choices.none).push({ x, y: rowOf.get(r.participant_id), p: r.participant_id, L });
        const ch = a.filter(r => r.first_choice === 'corner' || r.first_choice === 'face');
        if (ch.length) {                                        // share choosing a corner
          const pc = ch.filter(r => r.first_choice === 'corner').length / ch.length, bw = Math.min(w * 0.62, 0.16);
          shapes.push({ type: 'rect', xref: 'x', yref: 'y2', x0: x - bw / 2, x1: x + bw / 2, y0: meanY - 0.5, y1: meanY - 0.5 + pc, fillcolor: CHOICE_COL.corner, line: { width: 0 } },
            { type: 'rect', xref: 'x', yref: 'y2', x0: x - bw / 2, x1: x + bw / 2, y0: meanY - 0.5 + pc, y1: meanY + 0.5, fillcolor: CHOICE_COL.face, line: { width: 0 } });
        }
      });
    }
    for (const [key, pts] of points) {
      const [typ, cong] = key.split('|'), color = TYPE_COL[typ], mk = CONG_MK[cong];
      for (const done of [true, false]) {
        const a = pts.filter(p => p.done === done);
        if (o.individual !== false && a.length) traces.push({ type: 'scatter', mode: 'markers', showlegend: false, x: a.map(p => p.x), y: a.map(p => p.y),
          opacity: done ? 0.3 : 0.6, marker: { size: 5, symbol: mk, color: done ? color : c.SURF, line: { color, width: done ? 0 : 1 } },
          text: a.map(p => `${escaped(p.p)} · level ${p.L}`), hovertemplate: '%{text}<br>%{y} mistakes<extra></extra>' });
      }
      const m = means.get(key);
      traces.push({ type: 'scatter', mode: 'markers', showlegend: false, x: m.map(r => r.x), y: m.map(r => r.mean),
        marker: { size: CONG_MS[cong], symbol: mk, color, line: { color: c.SURF, width: 1 } },
        error_y: { type: 'data', array: m.map(r => r.sd), visible: true, color, thickness: 1.2, width: 2.5 },
        customdata: m.map(r => [r.L, r.n]),
        hovertemplate: `${typ === '1' ? 'Corner' : 'Face'} object · ${cong === 'True' ? 'congruent' : cong === 'False' ? 'incongruent' : 'no colour rule'}<br>Level %{customdata[0]}: %{y:.2f} (n = %{customdata[1]})<extra></extra>` });
    }
    const tot = levels.map(L => { const v = [...groupBy(O.filter(r => r.level_nr === L), r => r.participant_id).values()].map(a => a.reduce((s, r) => s + r.wrong, 0)); return { L, mean: mean(v), sd: std(v), n: v.length }; });
    traces.push({ type: 'scatter', mode: 'lines+markers', name: 'Level total, mean ± SD', x: tot.map(t => t.L + totX), y: tot.map(t => t.mean),
      line: { color: c.INK, width: 1.8 }, marker: { symbol: 'square', size: 7, color: c.INK, line: { color: c.SURF, width: 1 } },
      error_y: { type: 'data', array: tot.map(t => t.sd), visible: true, color: c.INK, thickness: 1.2, width: 4 },
      customdata: tot.map(t => t.n), hovertemplate: 'Level total: %{y:.2f}<br>n = %{customdata}<extra></extra>' });
    traces.push(legendOnly(`Corner object ${TY[1]}`, { size: 9, color: TYPE_COL[1], line: { color: c.SURF, width: 1 } }),
      legendOnly(`Face object ${TY[2]}`, { size: 9, color: TYPE_COL[2], line: { color: c.SURF, width: 1 } }),
      legendOnly('No colour rule (L0–4)', { size: 8, symbol: 'diamond', color: c.MUTED }),
      legendOnly('Congruent (L5–8)', { size: 9, color: c.MUTED }),
      legendOnly('Incongruent (L5–8)', { size: 10, symbol: 'triangle-up', color: c.MUTED }),
      legendOnly('Participant', { size: 6, color: c.MUTED, opacity: 0.4 }),
      legendOnly('Participant, level not finished', { size: 6, color: c.SURF, line: { color: c.MUTED, width: 1 } }));
    for (const L of levels) if (L % 2 === 0) shapes.push({ type: 'rect', xref: 'x', yref: 'paper', x0: L - 0.5, x1: L + 0.5, y0: 0, y1: 1, fillcolor: c.BAND, line: { width: 0 }, layer: 'below' });
    const layout = { legend: { orientation: 'v', x: 1.02, xanchor: 'left', y: 1, yanchor: 'top', font: { size: 11 } } };
    let height = 440;
    const yaxis = { ...yaxisStyle(c), title: { text: AXES.mistakes_object.y }, range: [0, top(traces)] };
    if (strip) {
      const g = { legend: 'legend2' };
      for (const [kind, name, color, symbol] of [['corner', 'Checked a corner first', CHOICE_COL.corner, 'triangle-up'], ['face', 'Checked a face first', CHOICE_COL.face, 'square'], ['none', 'No check', c.MUTED, 'x-thin-open']]) {
        const a = choices[kind];
        traces.push({ type: 'scatter', mode: 'markers', name, ...g, yaxis: 'y2', x: a.map(p => p.x), y: a.map(p => p.y),
          marker: { symbol, size: kind === 'none' ? 6 : 5.5, color, line: { color, width: kind === 'none' ? 1 : 0 } },
          text: a.map(p => `${escaped(p.p)} · level ${p.L}`), hovertemplate: `%{text}<extra>${name}</extra>` });
      }
      traces.push(legendOnly('Share corner (bar)', { symbol: 'square', size: 10, color: CHOICE_COL.corner }, g));
      shapes.push({ type: 'line', xref: 'paper', yref: 'y2', x0: 0, x1: 1, y0: npmax - 0.45, y1: npmax - 0.45, line: { color: c.GRID, width: 1 } },
        { type: 'rect', xref: 'paper', yref: 'y2', x0: 0, x1: 1, y0: -0.6, y1: npmax - 1 - order.length + 0.5, fillcolor: c.SURF, line: { width: 0 }, layer: 'below' });
      const stripPx = (npmax + 2) * 13, topPx = 330, gap = 70, total = topPx + stripPx + gap;
      height = total + 90;
      yaxis.domain = [(stripPx + gap) / total, 1];
      layout.legend2 = { orientation: 'v', x: 1.02, xanchor: 'left', y: stripPx / total, yanchor: 'top', font: { size: 11 },
        title: { text: 'Same colour as the object<br>= right kind of position', font: { size: 10, color: c.MUTED } } };
      layout.yaxis2 = { domain: [0, stripPx / total], range: [-0.6, meanY + 0.6], tickmode: 'array', tickvals: [meanY, ...order.map(p => rowOf.get(p))],
        ticktext: [`<span style="color:${c.MUTED}">corner share</span>`, ...order.map(p => escaped(label(p)))], tickfont: { size: 10 },
        showgrid: false, zeroline: false, showline: false, ticks: '', title: { text: 'First check on<br>first sighting', font: { size: 11 } } };
    }
    return { traces, rows: O.map(({ slot, slotKey, ...r }) => r), x: '', y: '', noSwap: true, height, shapes, yaxis, layout,
      axis: { ...xaxisStyle(c), ...nTicks(levels, L => tot.find(t => t.L === L).n, c), ticks: '', range: [Math.min(0, ...levels) - 0.5, Math.max(8, ...levels) + 0.5], title: { text: '<b>Level</b>' } },
      margin: { t: 20, l: strip ? Math.max(95, 45 + 6.2 * Math.max(0, ...order.map(p => label(p).length))) : 95, r: 250, b: 50 },
      note: 'Top: mistakes per object, mean ± SD across participants. Bottom: first check on the first presentation of each object in the level, one row per participant (ordered by progress, top = furthest); bar (top row) = share who checked a corner first. n = participants who played the level.' };
  }

  // ── 3. object timeline ──
  function timelineLayout(T) {
    const slots = [], x = new Map(), bounds = [];
    let pos = 0;
    for (let L = 0; L <= Math.max(8, ...T.map(t => t.level_nr)); L++) {
      const objs = [...new Map(T.filter(t => t.level_nr === L).map(t => [t.slotKey, t.slot])).values()].sort(cmpTuple);
      if (!objs.length) continue;
      const width = Math.max(objs.length, 1.6), start = pos + (width - objs.length) / 2 + 0.5;
      objs.forEach((s, i) => { x.set(L + '/' + s.join('|'), start + i); slots.push([L, s]); });
      bounds.push([L, pos, pos + width]); pos += width + 0.3;
    }
    return { slots, x, bounds, xmax: pos - 0.3 };
  }
  function roundedRect(x0, y0, x1, y1, r) {
    return `M${x0 + r},${y0} L${x1 - r},${y0} Q${x1},${y0} ${x1},${y0 + r} L${x1},${y1 - r} Q${x1},${y1} ${x1 - r},${y1} L${x0 + r},${y1} Q${x0},${y1} ${x0},${y1 - r} L${x0},${y0 + r} Q${x0},${y0} ${x0 + r},${y0} Z`;
  }
  /** Participant key (timeline, turning cells): each participant's fixed spot inside a cell, left edge at kx0, top at ky1.
   * Long names are shortened (start…end, full id on hover). Returns the key's width. */
  function participantKey({ order, spot, stopped, g, kx0, ky1, c, traces, shapes, annotations, title = 'Participant<br>position in a cell' }) {
    // The key's columns and rows grow to fit the names (pilot names on two lines).
    const keyLines = new Map(order.map(p => [p, short(p).startsWith('pilot ') ? short(p).split(' ') : [label(p)]]));
    const longest = Math.max(3, ...[...keyLines.values()].flat().map(l => l.length)), lines = Math.max(1, ...[...keyLines.values()].map(l => l.length));
    const cw = Math.max(1, Math.min(3.4, 0.18 * longest)), kh = Math.max(1, 0.3 * (lines + (stopped.size ? 1 : 0)));
    const ky0 = ky1 - g * kh;
    shapes.push({ type: 'path', path: roundedRect(kx0, ky0, kx0 + g * cw, ky1, 0.25), fillcolor: c.TILE, line: { width: 0 }, layer: 'below' });
    const keyX = p => kx0 + (spot.get(p)[0] + 0.5) * cw, keyY = p => ky0 + (spot.get(p)[1] + 0.5) * kh;
    const keyText = p => keyLines.get(p).map(escaped).join('<br>') + (stopped.has(p) ? `<br><span style="font-size:8px">stop L${stopped.get(p).level_nr}</span>` : '');
    const done = order.filter(p => !stopped.has(p)), stop = order.filter(p => stopped.has(p));
    const keySize = lines > 2 || longest > 12 ? 9 : 10;
    if (done.length) traces.push({ type: 'scatter', mode: 'text', showlegend: false, hoverinfo: 'skip', x: done.map(keyX), y: done.map(keyY),
      text: done.map(p => `<b>${keyText(p)}</b>`), textfont: { size: keySize, color: c.INK } });
    if (stop.length) traces.push({ type: 'scatter', mode: 'text', showlegend: false, hoverinfo: 'skip', x: stop.map(keyX), y: stop.map(keyY),
      text: stop.map(keyText), textfont: { size: keySize, color: c.MUTED } });
    annotations.push({ x: kx0 + g * cw / 2, y: ky1 + 0.1, yanchor: 'bottom', showarrow: false, text: title, font: { size: 10, color: c.INK } },
      { x: kx0 + g * cw / 2, y: ky0 - 0.15, yanchor: 'top', showarrow: false, text: 'furthest progress bottom left<br>bold = finished all levels<br>grey = stopped by time limit', font: { size: 9, color: c.MUTED } });
    return g * cw;
  }
  function timeline(d, o) {
    const c = palette(o.theme), T = d.T, traces = [], shapes = [], annotations = [];
    const { slots, x, bounds, xmax } = timelineLayout(T);
    const order = progressOrder(T), g = Math.max(1, Math.ceil(Math.sqrt(order.length)));
    const spot = new Map(order.map((p, k) => [p, [k % g, Math.floor(k / g)]])), step = CELL / g;   // furthest first: bottom left, rows upwards
    const last = new Map(); for (const t of T) { const l = last.get(t.participant_id); if (!l || t.trial_nr_global > l.trial_nr_global) last.set(t.participant_id, t); }
    const stopped = new Map([...last].filter(([, r]) => !r.level_done));   // time limit
    const px = t => x.get(t.level_nr + '/' + t.slotKey) - CELL / 2 + step * (spot.get(t.participant_id)[0] + 0.5);
    const py = t => t.y - CELL / 2 + step * (spot.get(t.participant_id)[1] + 0.5);
    bounds.forEach(([, a, b], k) => { if (k % 2 === 0) shapes.push({ type: 'rect', x0: a - 0.15, x1: b + 0.15, y0: 0.5, y1: CAP + 1.5, fillcolor: c.BAND, line: { width: 0 }, layer: 'below' }); });
    const shown = T.filter(t => t.y <= CAP);
    for (const key of new Set(shown.map(t => t.level_nr + '/' + t.slotKey + '/' + t.y))) {             // occupied cells
      const [L, s, y] = key.split('/'), cx = x.get(L + '/' + s);
      shapes.push({ type: 'path', path: roundedRect(cx - CELL / 2, +y - CELL / 2, cx + CELL / 2, +y + CELL / 2, 0.06), fillcolor: c.TILE, line: { width: 0 }, layer: 'below' });
    }
    const hover = t => `${escaped(t.participant_id)}<br>Level ${t.level_nr}, ${t.object_type} object, round ${t.y} (#${t.pos_in_round} in the round)<br>${t.wrong} mistakes`;
    const kinds = [['First check correct', t => !t.noguess && t.ok, GREEN, 'white'], ['Wrong position, right kind (corner/face)', t => !t.noguess && !t.ok && t.type_ok, YELLOW, 'ink'],
      ['Wrong kind (corner vs. face)', t => !t.noguess && !t.ok && !t.type_ok, RED, 'white'], ['No check', t => t.noguess, c.SURF, 'muted']];
    const numbers = { white: [], ink: [], muted: [] };
    for (const [name, test, color, num] of kinds) {
      const a = shown.filter(test);
      traces.push({ type: 'scatter', mode: 'markers', name, meta: 'dot', x: a.length ? a.map(px) : [null], y: a.length ? a.map(py) : [null], text: a.map(hover),
        marker: { size: 7, color, line: { color: num === 'muted' ? c.MUTED : color, width: num === 'muted' ? 0.8 : 0 } }, hovertemplate: '%{text}<extra></extra>' });
      for (const t of a) numbers[num].push(t);
    }
    const rings = shown.filter(t => t.completes);
    traces.push({ type: 'scatter', mode: 'markers', name: 'Chain completed', meta: 'ring', x: rings.length ? rings.map(px) : [null], y: rings.length ? rings.map(py) : [null], hoverinfo: 'skip',
      marker: { symbol: 'circle-open', size: 11, color: c.INK, line: { color: c.INK, width: 0.9 } } });
    const stops = [...stopped.values()], stopY = t => t.y <= CAP ? py(t) : CAP + 0.68;   // stops after round CAP sit in the >CAP row
    traces.push({ type: 'scatter', mode: 'markers', name: 'Last trial (time limit)', meta: 'ring', x: stops.length ? stops.map(px) : [null], y: stops.length ? stops.map(stopY) : [null],
      marker: { symbol: 'square-open', size: 11, color: c.INK, line: { color: c.INK, width: 1.6 } },
      text: stops.map(t => `${escaped(t.participant_id)} stopped by the time limit<br>level ${t.level_nr}, round ${t.y}`), hovertemplate: '%{text}<extra></extra>' });
    const late = stops.filter(t => t.y > CAP);
    if (late.length) traces.push({ type: 'scatter', mode: 'text', meta: 'num', showlegend: false, hoverinfo: 'skip', x: late.map(px), y: late.map(() => CAP + 0.68),
      text: late.map(t => String(t.y)), textfont: { color: c.INK, size: 7 } });
    for (const [kind, color, bold] of [['white', '#ffffff', true], ['ink', '#222222', true], ['muted', c.MUTED, false]]) {
      const a = numbers[kind];
      if (a.length) traces.push({ type: 'scatter', mode: 'text', meta: 'num', showlegend: false, hoverinfo: 'skip', x: a.map(px), y: a.map(py),
        text: a.map(t => bold ? `<b>${t.pos_in_round}</b>` : String(t.pos_in_round)), textfont: { color, size: 7 } });
    }
    traces.push({ type: 'scatter', mode: 'text', name: '2 = order within the round', x: [null], y: [null], text: ['2'], textfont: { color: c.INK }, hoverinfo: 'skip' });
    for (const a of groupBy(T.filter(t => t.y > CAP), t => t.level_nr + '/' + t.slotKey).values()) {      // overflow row
      annotations.push({ x: x.get(a[0].level_nr + '/' + a[0].slotKey), y: CAP + 1.72, yanchor: 'top', showarrow: false, font: { size: 8, color: c.MUTED },   // one short line each: neighbours never collide
        text: `${new Set(a.map(t => t.participant_id)).size} p.<br>${a.length} tr.<br>${Math.round(100 * a.filter(t => t.ok).length / a.length)}%` });
    }
    shapes.push({ type: 'line', x0: -0.2, x1: xmax + 0.2, y0: CAP + 0.5, y1: CAP + 0.5, line: { color: c.FAINT, width: 1, dash: '3px,3px' } });
    for (const [L, a, b] of bounds) {
      shapes.push({ type: 'line', x0: a + 0.05, x1: b - 0.05, y0: CAP + 1.75, y1: CAP + 1.75, line: { color: c.MUTED, width: 1 } });
      annotations.push({ x: (a + b) / 2, y: CAP + 1.85, yanchor: 'bottom', showarrow: false, text: `<b>Level ${L}</b>`, font: { size: 12, color: c.INK } });
    }
    // Key: position of each participant inside a cell, to the right of the levels.
    const kx0 = xmax + 0.9, kw = participantKey({ order, spot, stopped, g, kx0, ky1: CAP + 1.5, c, traces, shapes, annotations });
    const xr = [-0.2, kx0 + kw + 0.3], yr = [0.45, CAP + 2.6];
    return { traces, rows: T.map(({ slot, slotKey, id, ...t }) => t), x: '', y: 'Round', noSwap: true, shapes,
      layout: { annotations, legend: { orientation: 'h', x: 0.5, xanchor: 'center', y: 1.0, yanchor: 'bottom' } },
      fit: { step }, aspect: (yr[1] - yr[0]) / (xr[1] - xr[0]), margin: { t: 40, l: 55, r: 10, b: 75 },
      axis: { range: xr, tickmode: 'array', tickvals: slots.map(([L, s]) => x.get(L + '/' + s.join('|'))), showgrid: false, zeroline: false, showline: false, ticks: '', tickfont: { size: 10 },
        ticktext: slots.map(([L, s]) => (s[0] === 1 ? 'corner<br>' + TY[1] : 'face<br>' + TY[2]) + (L >= 5 ? `<br><span style="color:${c.MUTED}">${s[1] === 0 ? 'cong.' : 'incong.'}</span>` : '')) },
      yaxis: { range: yr, scaleanchor: 'x', scaleratio: 1, tickmode: 'array', tickvals: Array.from({ length: CAP + 1 }, (_, i) => i + 1),
        ticktext: [...Array.from({ length: CAP }, (_, i) => String(i + 1)), `>${CAP}`], showgrid: false, zeroline: false, showline: true, linecolor: c.MUTED, ticks: 'outside', tickcolor: c.MUTED, tickfont: { size: 10 } },
      note: `Each cell is one object in one round; each participant has a fixed position inside the cell (key on the right). The number in a dot is when the participant saw this object within the round (each object once per round). Row >${CAP}: participants (p.), trials (tr.) and % correct first checks after round ${CAP}; a square there marks a stop after round ${CAP} (number = round).` };
  }

  // ── Rotation Strategy: make_MAIN_sampling_figures.py, drawn as matplotlib draws it ──
  // The server sends each trial's trajectory (the script's trajectory(), a point every 3° of
  // turning, every check). Each figure has the script's size in inches and every size is the
  // script's, in points (1 pt = plot width / (inches · 72) px). The axes are placed as matplotlib
  // places them (tight_layout or subplots_adjust, then the equal aspect centred in the box), and
  // text is laid out with Arial's metrics. Lines and markers drawn in data coordinates keep their
  // size relative to the data when zooming (fitDots in admin_explorer.js).
  const LEFT_C = '#4d7fb3', RIGHT_C = '#cf8a55', KIND_COL = [GREEN, YELLOW, RED];   // check kinds: ok, type_ok, wrong
  const MPL = {
    light: { TEXT: '#000000', INK: '#222222', MUTED: '#6f6f6f', BG: '#ffffff', TILE: '#f1f1ee', CELL: '#f3f3f0', BLOCK: '#fafaf8',
      BLOCK_EDGE: '#e4e4e0', SPOKE: '#d3d3ce', CORNER: '#c4c4bf', SEP_P: '#e2e2de', SEP_C: '#e6e6e2', NUM: '#9a9a96', AXIS: '#bdbdb8',
      LIGHT: '#a3a39e', STOP: '#fbefc4', START: '#555555', LEG_DOT: '#a9a9a4', LEG_STAR: '#8f8f8f' },
    dark: { TEXT: '#e6e6e6', INK: '#e6e6e6', MUTED: '#9a9a9a', BG: '#000000', TILE: '#1b1b1b', CELL: '#151515', BLOCK: '#0b0b0b',
      BLOCK_EDGE: '#2a2a2a', SPOKE: '#3c3c3c', CORNER: '#555555', SEP_P: '#2a2a2a', SEP_C: '#2a2a2a', NUM: '#777777', AXIS: '#555555',
      LIGHT: '#777777', STOP: '#4a3f12', START: '#aaaaaa', LEG_DOT: '#777777', LEG_STAR: '#999999' },
  };
  // Arial advance widths (1/1000 em) for ' '…'~'; bold is about 7 % wider. Line metrics of "lp" (matplotlib) and of the
  // browser's text box (Plotly anchors annotations on it).
  const ARIAL_W = [278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556, 556, 556, 556, 556, 556, 556, 556,
    278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667,
    944, 667, 667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556, 556, 556, 333, 500,
    278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584];
  const OTHER_W = { '→': 1000, '✓': 750, '×': 584, '–': 556, '…': 1000 };
  const ASC = 0.716, DESC = 0.21, BOX_DESC = 0.212, LINE = ASC + DESC;
  const FONT = 'Arial, Helvetica, "Liberation Sans", sans-serif';
  const SERIF_SPAN = n => `<span style="font-family:'DejaVu Serif','Bitstream Vera Serif',Georgia,serif">${n}</span>`;
  const TYP = { 1: `(ty. ${SERIF_SPAN('I')})`, 2: `(ty. ${SERIF_SPAN('II')})` };   // M.TY: serif roman numerals
  const PAD = 1.08 * 8;                                                          // tight_layout pad: 1.08 × font size (8 pt)
  function textW(s, pt, bold = false) {
    let w = 0;
    for (const ch of String(s).replace(/<[^>]*>/g, '').replace(/&[a-z#0-9]+;/g, 'x')) {
      const c = ch.codePointAt(0); w += c >= 32 && c <= 126 ? ARIAL_W[c - 32] : OTHER_W[ch] || 556;
    }
    return w / 1000 * pt * (bold ? 1.07 : 1);
  }
  const blockH = (n, size, ls) => LINE * size + (n - 1) * (ASC * ls + DESC) * size;   // height of n lines of text

  /** A figure of wIn × hIn inches drawn `width` px wide. Positions are in points from the bottom left (paper). */
  function mplFigure(wIn, hIn, width, theme) {
    const W = wIn * 72, H = hIn * 72;
    return { W, H, k: (width || 1200) / W, width: width || 1200, C: MPL[theme === 'dark' ? 'dark' : 'light'], annotations: [], shapes: [], layers: new Map() };
  }
  // ax.set_aspect('equal', adjustable='box'): the largest box with the data's aspect, centred (anchor 'C').
  function aspectFit([x0, y0, x1, y1], xl, yl) {
    const bw = x1 - x0, bh = y1 - y0, r = (yl[1] - yl[0]) / (xl[1] - xl[0]);
    if (bh / bw > r) { const h = bw * r; return [x0, y0 + (bh - h) / 2, x1, y0 + (bh + h) / 2]; }
    const w = bh / r; return [x0 + (bw - w) / 2, y0, x0 + (bw + w) / 2, y1];
  }
  // fig.tight_layout(rect) for one axes, as matplotlib does it (one pass): the decorations' extent around the axes drawn in
  // the default subplot box (0.125, 0.11, 0.9, 0.88) sets the margins, then the equal aspect is applied in the new box.
  function tightLayout(f, rect, xl, yl, decorations) {
    const orig = [0.125 * f.W, 0.11 * f.H, 0.9 * f.W, 0.88 * f.H], act = aspectFit(orig, xl, yl), e = decorations(act);
    const ml = Math.max(orig[0] - (act[0] - e.l), 0) + PAD, mr = Math.max(act[2] + e.r - orig[2], 0) + PAD;
    const mb = Math.max(orig[1] - (act[1] - e.b), 0) + PAD, mt = Math.max(act[3] + e.t - orig[3], 0) + PAD;
    return aspectFit([rect[0] * f.W + ml, rect[1] * f.H + mb, rect[2] * f.W - mr, rect[3] * f.H - mt], xl, yl);
  }
  // Text as matplotlib lays it out: one annotation per line, so the line pitch is matplotlib's (ascent · linespacing +
  // descent) and va / ha are matplotlib's. x, y in points (paper) or data units (xref / yref 'x' / 'y').
  function mtext(f, x, y, s, { size, color, bold = false, ha = 'left', va = 'baseline', ls = 1.2, xref = 'paper', yref = 'paper', angle = 0 }) {
    const lines = String(s).split('\n'), n = lines.length, step = (ASC * ls + DESC) * size, height = blockH(n, size, ls);
    const first = va === 'top' ? -ASC * size : va === 'bottom' ? DESC * size + (n - 1) * step
      : va === 'center' ? height / 2 - ASC * size : va === 'center_baseline' ? -ASC * size / 2 : 0;
    lines.forEach((line, i) => f.annotations.push({
      x: xref === 'paper' ? x / f.W : x, y: yref === 'paper' ? y / f.H : y, xref, yref, showarrow: false, borderpad: 0, textangle: angle,
      xanchor: ha, yanchor: angle ? 'middle' : 'bottom', yshift: angle ? 0 : (first - i * step - BOX_DESC * size) * f.k,
      text: bold ? `<b>${line}</b>` : line, font: { family: FONT, size: size * f.k, color: color || f.C.TEXT }, captureevents: false }));
  }
  // matplotlib marker paths (unit size; scaled by 0.5 · ms), and the script's SHAPE_MK for the face shapes.
  const ring = (n, a0 = Math.PI / 2, sx = 1, sy = 1, r = 1) => Array.from({ length: n }, (_, k) => [sx * r * Math.cos(a0 + 2 * Math.PI * k / n), sy * r * Math.sin(a0 + 2 * Math.PI * k / n)]);
  const MARKERS = {
    o: ring(32), s: [[-1, -1], [1, -1], [1, 1], [-1, 1]], '^': [[0, 1], [-1, -1], [1, -1]], p: ring(5), D: [[0, Math.SQRT2], [Math.SQRT2, 0], [0, -Math.SQRT2], [-Math.SQRT2, 0]],
    '*': Array.from({ length: 10 }, (_, k) => { const r = k % 2 ? 0.381966 : 1, a = Math.PI / 2 + Math.PI * k / 5; return [r * Math.cos(a), r * Math.sin(a)]; }),
    rect: [[-1, -0.45], [1, -0.45], [1, 0.45], [-1, 0.45]], oval: ring(40, 0, 1, 0.6), kite: [[0, 1], [0.6, 0.25], [0, -1], [-0.6, 0.25]],
    trap: [[-1, -0.6], [1, -0.6], [0.5, 0.6], [-0.5, 0.6]], semi: Array.from({ length: 24 }, (_, k) => [Math.cos(Math.PI * k / 23), Math.sin(Math.PI * k / 23) - 0.4]),
  };
  const SHAPE_MK = { 0: ['o', 1.0], 1: ['s', 0.9], 2: ['*', 1.35], 3: ['^', 1.0], 4: ['rect', 1.05], 5: ['oval', 1.05], 6: ['p', 1.05], 7: ['kite', 1.1],
    8: ['D', 0.85], 9: ['trap', 1.05], 10: ['semi', 1.05] };
  const markerPoly = (code, cx, cy, ms) => MARKERS[code].map(([a, b]) => [cx + a * ms / 2, cy + b * ms / 2]);
  // build_dataset.face_at_position(): the prism face at an odd position, for any reward door (face k is centred on door 2k - 1).
  const FACE_ON_DOOR = { 5: 0, 1: 1, 3: 2 };
  const faceAtPosition = (p, door) => FACE_ON_DOOR[(((door - (p - (door % 2 ? 3 : 0))) % 6) + 6) % 6];
  // FancyBboxPatch "round,pad=0,rounding_size=r" and Rectangle, as polygons.
  function roundBox(x0, y0, x1, y1, r) {
    const pts = [], arc = (cx, cy, a0) => { for (let i = 0; i <= 6; i++) { const a = a0 + i * Math.PI / 12; pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a)]); } };
    arc(x1 - r, y0 + r, -Math.PI / 2); arc(x1 - r, y1 - r, 0); arc(x0 + r, y1 - r, Math.PI / 2); arc(x0 + r, y0 + r, Math.PI);
    return pts;
  }
  const box = (x0, y0, x1, y1) => [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
  // Drawing layers in data coordinates, drawn in zorder (then creation order): fills, lines (lw in pt), markers (ms, mew in pt).
  function layer(f, key, spec) { if (!f.layers.has(key)) f.layers.set(key, { ...spec, x: [], y: [] }); return f.layers.get(key); }
  function poly(l, pts, close = false) {
    for (const [a, b] of pts) { l.x.push(a); l.y.push(b); }
    if (close) { l.x.push(pts[0][0]); l.y.push(pts[0][1]); }
    l.x.push(null); l.y.push(null);
  }
  const point = (l, a, b) => { l.x.push(a); l.y.push(b); };
  let roundCaps = 0;
  function layerTraces(f, dpp) {
    return [...f.layers.values()].map((l, i) => [l, i]).sort((a, b) => a[0].z - b[0].z || a[1] - b[1]).map(([l]) => l.kind === 'fill'
      ? { type: 'scatter', mode: 'lines', x: l.x, y: l.y, fill: 'toself', fillcolor: l.color, line: { width: 0 }, opacity: l.alpha ?? 1, hoverinfo: 'skip', showlegend: false }
      : l.kind === 'line'
        ? { type: 'scatter', mode: 'lines', x: l.x, y: l.y, line: { color: l.color, width: l.lw * f.k }, opacity: l.alpha ?? 1, meta: 'mline', fitw: l.lw * dpp, hoverinfo: 'skip', showlegend: false,
          ...(l.cap === 'round' ? { uid: 'mplround' + (++roundCaps) } : {}) }   // capstyle="round": admin_explorer.css
        : { type: 'scatter', mode: 'markers', x: l.x, y: l.y, marker: { color: l.color, size: l.ms * f.k, line: { color: l.mec || l.color, width: (l.mew || 0) * f.k } },
          opacity: l.alpha ?? 1, meta: 'mmark', fitms: l.ms * dpp, fitmew: (l.mew || 0) * dpp, hoverinfo: 'skip', showlegend: false });
  }
  // One trial: spiral stretches by turning direction, check dots; returns the start (inner end). The script draws the
  // segments in time order (later ones cover earlier ones): stretch s of every trial goes in layer s, and trials never
  // overlap, so drawing the layers in order of s reproduces it.
  function spiral(f, sp, tr, cx, cy, rIn, rOut) {
    const P = tr.th.map((th, i) => { const r = rIn + (rOut - rIn) * tr.f[i], a = Math.PI / 2 - th; return [cx + r * Math.cos(a), cy + r * Math.sin(a)]; });
    for (let i = 0, s = 0; i < P.length - 1; s++) {         // one polyline per stretch of the same direction
      let j = i; while (j + 1 < P.length - 1 && tr.dir[j + 1] === tr.dir[i]) j++;
      const d = tr.dir[i];
      poly(layer(f, `spiral${s}${d}`, { kind: 'line', color: d === 'l' ? LEFT_C : RIGHT_C, lw: sp.lw, cap: sp.cap, z: sp.z + s * 1e-4 + (d === 'r' ? 5e-5 : 0) }), P.slice(i, j + 2));
      i = j + 1;
    }
    for (const [th, fr, kind] of tr.chk) { const r = rIn + (rOut - rIn) * fr, a = Math.PI / 2 - th; point(f.layers.get(sp.dots[kind]), cx + r * Math.cos(a), cy + r * Math.sin(a)); }
    return P[0] || [cx, cy];
  }
  // Legends (frameon=False), matplotlib's box model: borderaxespad 0.5, borderpad 0.4, handlelength 2 (em); columns as
  // np.array_split; each row is the text's height (ascent + descent of "lp"), rows labelspacing apart; handles at 0.35 em.
  function legendColumns(entries, ncol) {
    const cols = [], base = Math.floor(entries.length / ncol), extra = entries.length % ncol;
    for (let c = 0, i = 0; c < ncol; c++) { const n = base + (c < extra ? 1 : 0); if (n) cols.push(entries.slice(i, i + n)); i += n; }
    return cols;
  }
  function legendSize(entries, { size, ncol = 1, handletextpad = 0.8, columnspacing = 2, labelspacing = 0.5 }) {
    const cols = legendColumns(entries, ncol), em = size, rows = Math.max(...cols.map(c => c.length));
    const widths = cols.map(c => Math.max(...c.map(e => (2 + handletextpad) * em + textW(e.label, size))));
    return { w: 0.8 * em + widths.reduce((a, b) => a + b, 0) + (cols.length - 1) * columnspacing * em, h: 0.8 * em + rows * LINE * em + (rows - 1) * labelspacing * em, widths };
  }
  function mplLegend(f, entries, x, y, opts) {
    const { size, ncol = 1, handletextpad = 0.8, columnspacing = 2, labelspacing = 0.5 } = opts, em = size, { widths } = legendSize(entries, opts);
    let cx = x + 0.9 * em;
    legendColumns(entries, ncol).forEach((col, c) => {
      col.forEach((e, i) => {
        const base = y - 0.9 * em - ASC * em - i * (LINE + labelspacing) * em;
        legendHandle(f, e, cx, base + 0.35 * em, em);
        mtext(f, cx + (2 + handletextpad) * em, base, e.label, { size });
      });
      cx += widths[c] + columnspacing * em;
    });
  }
  const pshape = (f, s) => f.shapes.push({ xref: 'paper', yref: 'paper', layer: 'above', ...s });
  function legendHandle(f, h, x, y, em) {                     // HandlerLine2D: the line over the handle, the marker in its middle
    if (h.lw) pshape(f, { type: 'line', x0: (x - h.lw / 2) / f.W, x1: (x + 2 * em + h.lw / 2) / f.W, y0: y / f.H, y1: y / f.H, line: { color: h.color, width: h.lw * f.k } });
    if (!h.marker) return;
    const mx = x + em;
    if (h.marker === '2') {                                   // marker "$2$": the glyph's larger side is ms, edge = face colour
      f.annotations.push({ x: mx / f.W, y: y / f.H, xref: 'paper', yref: 'paper', showarrow: false, borderpad: 0, xanchor: 'center', yanchor: 'middle',
        text: '<b>2</b>', font: { family: FONT, size: h.ms / 0.716 * f.k, color: h.color }, captureevents: false });
      return;
    }
    const fill = h.mfc === 'none' ? 'rgba(0,0,0,0)' : h.mfc, edge = h.mec === 'none' ? 0 : (h.mew ?? 1);
    const path = markerPoly(h.marker, mx, y, h.ms).map(([a, b], i) => `${i ? 'L' : 'M'}${a / f.W},${b / f.H}`).join(' ') + ' Z';
    pshape(f, { type: 'path', path, fillcolor: fill, opacity: h.alpha ?? 1, line: { color: edge ? h.mec : 'rgba(0,0,0,0)', width: edge * f.k } });
  }
  // The script's legend_core(lw, ms): Line2D handles take mec = colour and mew = 1 unless set.
  const legendCore = (C, lw = 1.2, ms = 3) => [
    { label: 'Turning left', lw, color: LEFT_C }, { label: 'Turning right', lw, color: RIGHT_C }, { label: 'Rewarded direction (up)', lw: 1.0, color: GREEN },
    ...['Check correct', 'Wrong position, right kind', 'Wrong kind'].map((label, i) => ({ label, marker: 'o', ms, mfc: KIND_COL[i], mec: KIND_COL[i] })),
    { label: 'Chain completed', marker: 's', ms: 6, mfc: 'none', mec: C.INK, mew: 0.35 },
    { label: 'Last trial (time limit)', marker: 's', ms: 6, mfc: C.STOP, mec: C.STOP }];
  const needTurns = d => { if (!d.turns && d.T.length) throw new Error('The turning data is not loaded yet. Press Plot again.'); return d.turns || {}; };
  function lastStops(T) {                                    // the script's load(): last trial of a participant stopped by the time limit
    const last = new Map(); for (const t of T) { const l = last.get(t.participant_id); if (!l || t.trial_nr_global > l.trial_nr_global) last.set(t.participant_id, t); }
    return new Map([...last].filter(([, r]) => !r.level_done));
  }
  const tileHover = t => `${escaped(t.participant_id)}<br>Level ${t.level_nr}, ${t.object_type} object, round ${t.y} (#${t.pos_in_round} in the round)<br>${t.wrong} mistakes · ${(+t.clock__trial_duration || 0).toFixed(1)} s`;
  const hoverTrace = (pts, size, dpp, f) => ({ type: 'scatter', mode: 'markers', x: pts.map(p => p[0]), y: pts.map(p => p[1]), text: pts.map(p => p[2]),
    marker: { size: size / dpp * f.k, opacity: 0, symbol: 'square' }, meta: 'mmark', fitms: size, fitmew: 0, hovertemplate: '%{text}<extra></extra>', showlegend: false });
  const ytickW = (n, size) => Math.max(0, ...Array.from({ length: n }, (_, i) => textW(String(i + 1), size)));
  const mplPanel = (f, ax, xl, yl, traces, rows, axes = {}) => ({
    traces, rows, title: '', x: '', y: '', noSwap: true, mplWidth: f.width, aspect: f.H / f.W, margin: { t: 0, l: 0, r: 0, b: 0 }, fit: { step: 1 }, shapes: f.shapes,
    layout: { annotations: f.annotations, showlegend: false, paper_bgcolor: f.C.BG, plot_bgcolor: f.C.BG, hovermode: 'closest' },
    axis: { domain: [ax[0] / f.W, ax[2] / f.W], range: xl, showgrid: false, zeroline: false, showline: false, ticks: '', showticklabels: false, automargin: false, ...axes.x },
    yaxis: { domain: [ax[1] / f.H, ax[3] / f.H], range: yl, showgrid: false, zeroline: false, showline: false, ticks: '', showticklabels: false, automargin: false, ...axes.y } });
  const turnRows = T => T.map(({ slot, slotKey, id, ...t }) => t);

  // 1. participant_figure(): one participant, every trial as a spiral, with spokes, faces, outlines and order numbers.
  function turnParticipant(d, o) {
    const turns = needTurns(d);
    const pid = d.participants.includes(o.participant) ? o.participant : progressOrder(d.T)[0];
    const T = d.T.filter(t => t.participant_id === pid).sort((a, b) => a.trial_nr_global - b.trial_nr_global);
    if (!T.length) return { traces: [], rows: [], noSwap: true, title: 'No trials for this selection' };
    const R_IN = 0.13, R_OUT = 0.335, R_POS = 0.40, HALF = 0.46, FACE_MS = 2.6, FAINT = 0.28;
    const { slots, x, bounds, xmax } = timelineLayout(T), ymax = Math.max(...T.map(t => t.y));
    const f = mplFigure(17, 1.9 + 0.5 * ymax, o.width, o.theme), C = f.C;
    const xl = [-0.2, xmax + 0.2], yl = [0.45, ymax + 0.75], tickW = ytickW(ymax, 7), hasCong = slots.some(([L]) => L >= 5);
    const legend = [
      { label: 'Turning left (key)', lw: 1.3, color: LEFT_C }, { label: 'Turning right (key)', lw: 1.3, color: RIGHT_C },
      { label: 'Start (inner end); outer end = last check', marker: 'o', ms: 3, mfc: C.START, mec: C.BG, mew: 0.3 },
      { label: 'Rewarded position (always at the top)', lw: 1.0, color: GREEN },
      { label: 'Other positions (dot = corner)', lw: 0.8, color: C.SPOKE, marker: 'o', ms: 1.8, mfc: C.LEG_DOT, mec: 'none' },
      { label: 'Face: its shape in its colour (faint)', marker: '*', ms: 6.5, mfc: C.LEG_STAR, mec: 'none', alpha: FAINT + 0.15 },
      ...['Check: correct', 'Check: wrong position, right kind', 'Check: wrong kind (corner vs. face)'].map((label, i) => ({ label, marker: 'o', ms: 5, mfc: KIND_COL[i], mec: C.BG })),
      ...['Outline: first check correct', 'Outline: first check wrong position, right kind', 'Outline: first check wrong kind'].map((label, i) => ({ label, marker: 's', ms: 9, mfc: C.TILE, mec: KIND_COL[i], mew: 0.8 })),
      { label: 'Outer black line: chain completed', marker: 's', ms: 10, mfc: 'none', mec: C.INK, mew: 0.85 },
      { label: 'Last trial (time limit)', marker: 's', ms: 9, mfc: C.STOP, mec: 'none' },
      { label: 'Order within the round', marker: '2', ms: 5, color: C.MUTED }];
    const lopts = { size: 7.5, labelspacing: 0.9 }, lsize = legendSize(legend, lopts);
    const ax = tightLayout(f, [0, 0.03, 1, 1], xl, yl, a => {
      const w = a[2] - a[0], h = a[3] - a[1], ppd = w / (xl[1] - xl[0]);
      return { l: 3 + 3.5 + tickW + 4 + LINE * 8, r: 0.005 * w + 0.5 * 7.5 + lsize.w,
        b: Math.max(3 + blockH(2, 7, 1.25), hasCong ? 26 + LINE * 6.5 : 0, 0.5 * 7.5 + lsize.h - h),
        t: Math.max(34 + ASC * 11, 0.03 * ppd + LINE * 8.5) };
    });
    const dpp = (xl[1] - xl[0]) / (ax[2] - ax[0]), mk = (key, spec) => layer(f, key, spec);
    const sep = mk('sep', { kind: 'line', color: C.SEP_P, lw: 0.6, z: 0 });
    for (const [, a] of bounds.slice(1)) poly(sep, [[a - 0.15, 0.5], [a - 0.15, ymax + 0.5]]);
    const sp = { z: 3, lw: 1.15, cap: 'round', dots: [0, 1, 2].map(i => 'dot' + i) };
    sp.dots.forEach((key, i) => mk(key, { kind: 'mark', color: KIND_COL[i], ms: 4, mec: C.BG, mew: 0.5, z: 9 }));
    const stop = lastStops(T).get(pid), hover = [];
    for (const t of T) {
      const cx = x.get(t.level_nr + '/' + t.slotKey), cy = t.y, tr = turns[t.id];
      const outline = t.noguess ? C.MUTED : t.ok ? GREEN : t.type_ok ? YELLOW : RED, tile = roundBox(cx - HALF, cy - HALF, cx + HALF, cy + HALF, 0.06);
      poly(mk(stop && stop.id === t.id ? 'stop' : 'tile', { kind: 'fill', color: stop && stop.id === t.id ? C.STOP : C.TILE, z: 1 }), tile);
      poly(mk('edge' + outline, { kind: 'line', color: outline, lw: 0.75, z: 1 }), tile, true);
      if (t.completes) poly(mk('frame', { kind: 'line', color: C.INK, lw: 0.85, z: 1 }), roundBox(cx - HALF - 0.03, cy - HALF - 0.03, cx + HALF + 0.03, cy + HALF + 0.03, 0.08), true);
      if (tr) {
        const offset = tr.door % 2 ? 3 : 0;                   // theta = k·60° is position k (+3 for face objects)
        for (let k = 0; k < 6; k++) {                          // spokes, faces (faint) and corners
          const a = Math.PI / 2 - k * Math.PI / 3;
          poly(mk(k ? 'spoke' : 'spoke0', k ? { kind: 'line', color: C.SPOKE, lw: 0.35, z: 2 } : { kind: 'line', color: GREEN, lw: 0.6, alpha: 0.7, z: 2 }),
            [[cx, cy], [cx + R_OUT * Math.cos(a), cy + R_OUT * Math.sin(a)]]);
          const p = (k + offset) % 6, mx = cx + R_POS * Math.cos(a), my = cy + R_POS * Math.sin(a);
          if (p % 2 === 0) point(mk(k ? 'corner' : 'corner0', { kind: 'mark', color: k ? C.CORNER : GREEN, ms: 1.2, z: 2 }), mx, my);
          else {
            const face = faceAtPosition(p, tr.door), [code, sc] = SHAPE_MK[tr.shapes[face]] || SHAPE_MK[0], color = tr.faces[face] || C.MUTED;
            poly(mk('face' + color, { kind: 'fill', color, alpha: FAINT, z: 2 }), markerPoly(code, mx, my, FACE_MS * sc * dpp));
          }
        }
        const [sx, sy] = spiral(f, sp, tr, cx, cy, R_IN, R_OUT);
        point(mk('start', { kind: 'mark', color: C.START, ms: 2.4, mec: C.BG, mew: 0.3, z: 4 }), sx, sy);
      }
      mtext(f, cx - 0.425, cy + 0.425, String(t.pos_in_round), { size: 5, color: C.NUM, va: 'top', xref: 'x', yref: 'y' });
      hover.push([cx, cy, tileHover(t)]);
    }
    // axes: left spine and y ticks (MUTED), labels; x labels; congruency; level headers; title; legend; note
    for (let v = 1; v <= ymax; v++) mtext(f, ax[0] - 6.5, v, String(v), { size: 7, color: C.INK, ha: 'right', va: 'center_baseline', yref: 'y' });
    mtext(f, ax[0] - 6.5 - tickW - 4, (ax[1] + ax[3]) / 2, 'Round (= times the object was played in the level)', { size: 8, ha: 'right', angle: -90 });
    for (const [L, s] of slots) {
      const sx = x.get(L + '/' + s.join('|'));
      mtext(f, sx, ax[1] - 3, (s[0] === 1 ? 'corner\n' : 'face\n') + TYP[s[0]], { size: 7, ha: 'center', va: 'top', ls: 1.25, xref: 'x' });
      if (L >= 5) mtext(f, sx, ax[1] - 26, s[1] === 0 ? 'cong.' : 'incong.', { size: 6.5, color: C.MUTED, ha: 'center', va: 'top', xref: 'x' });
    }
    const done = new Map(T.map(t => [t.level_nr, t.level_done])), hdr = mk('hdr', { kind: 'line', color: C.MUTED, lw: 0.6, z: 5 });
    for (const [L, a, b] of bounds) {
      poly(hdr, [[a + 0.05, ymax + 0.62], [b - 0.05, ymax + 0.62]]);
      mtext(f, (a + b) / 2, ax[1] + (ymax + 0.78 - yl[0]) / dpp, `Level ${L}` + (done.get(L) ? '' : ' (not finished)'), { size: 8.5, bold: true, ha: 'center', va: 'bottom', xref: 'x' });
    }
    mtext(f, ax[0], ax[3] + 34, `How the objects were turned, ${escaped(o.name || 'Selection')} participant ${escaped(short(pid))}`, { size: 11, bold: true });
    mplLegend(f, legend, ax[2] + 0.005 * (ax[2] - ax[0]), ax[3], lopts);
    mtext(f, 0.01 * f.W, 0.01 * f.H, 'Each cell is one trial, turned so that the rewarded position points up; the six spokes are the six positions (faces show their shape in their colour); left and right are as on the screen when the reward faces the participant (turning left runs clockwise). Radius = time within the trial, scaled per trial from start (inner end) to the last check (outer end).', { size: 6.5, color: C.MUTED });
    const k = f.k;
    return mplPanel(f, ax, xl, yl, [...layerTraces(f, dpp), hoverTrace(hover, 2 * HALF, dpp, f)],
      T.map(t => ({ participant_id: t.participant_id, level_nr: t.level_nr, object_type: t.object_type, round: t.y, pos_in_round: t.pos_in_round, wrong: t.wrong,
        ...(turns[t.id] ? { checks: turns[t.id].chk.length, duration_to_last_check: turns[t.id].dur } : {}) })),
      { y: { showline: true, linecolor: C.MUTED, linewidth: 0.6 * k, ticks: 'outside', ticklen: 3 * k, tickwidth: 0.6 * k, tickcolor: C.MUTED, tickmode: 'array', tickvals: Array.from({ length: ymax }, (_, i) => i + 1) } });
  }

  // 2. group_cells_figure(): object × round cells, each participant at a fixed spot in the cell.
  function turnCells(d, o) {
    const turns = needTurns(d), TT = d.T, T = TT.filter(t => t.y <= CAP);
    if (!T.length) return { traces: [], rows: [], noSwap: true };
    const CELL2 = 0.94, { slots, x, bounds, xmax } = timelineLayout(TT);
    const order = progressOrder(TT), g = Math.ceil(Math.sqrt(order.length)), step = CELL2 / g;
    const spot = new Map(order.map((p, k) => [p, [k % g, Math.floor(k / g)]])), stopped = lastStops(TT);   // furthest first: bottom left, rows upwards
    const rOut = 0.43 * step, rIn = 0.15 * step, ymax = Math.min(CAP, Math.max(...T.map(t => t.y)));
    const top = ymax + (TT.some(t => t.y > CAP) ? 1.1 : 0.6), xl = [-0.2, xmax + 0.2], yl = [0.45, top];
    const f = mplFigure(22, 1.6 + 22 * (ymax + 1.0) / (xmax + 2.2), o.width, o.theme), C = f.C;
    const tickW = ytickW(ymax, 6.5), hasCong = slots.some(([L]) => L >= 5);
    const ax = tightLayout(f, [0, 0.015, 1, 0.955], xl, yl, a => ({
      l: 3 + 3.5 + tickW + 4 + LINE * 8, r: 0, b: Math.max(2 + blockH(2, 6.5, 1.2), hasCong ? 22 + LINE * 6 : 0),
      t: 0.05 * (a[2] - a[0]) / (xl[1] - xl[0]) + LINE * 8 }));
    const dpp = (xl[1] - xl[0]) / (ax[2] - ax[0]), mk = (key, spec) => layer(f, key, spec);
    const sep = mk('sep', { kind: 'line', color: C.SEP_C, lw: 0.6, z: 0 });
    for (const [, a] of bounds.slice(1)) poly(sep, [[a - 0.15, 0.5], [a - 0.15, ymax + 0.5]]);
    const keyBg = mk('keybg', { kind: 'fill', color: C.CELL, z: 0.5 }), cellBg = mk('cell', { kind: 'fill', color: C.CELL, z: 1 });
    for (const key of new Set(T.map(t => t.level_nr + '/' + t.slotKey + '/' + t.y))) {
      const [L, s, y] = key.split('/'), cx = x.get(L + '/' + s);
      poly(cellBg, roundBox(cx - CELL2 / 2, +y - CELL2 / 2, cx + CELL2 / 2, +y + CELL2 / 2, 0.04));
    }
    const spots = mk('spot', { kind: 'fill', color: C.BG, z: 2 }), stops = mk('stops', { kind: 'fill', color: C.STOP, z: 2 });
    const ticks = mk('tick', { kind: 'line', color: GREEN, lw: 0.5, z: 3 });
    const frames = mk('frame', { kind: 'line', color: C.INK, lw: 0.22, z: 5 });
    const sp = { z: 4, lw: 0.6, dots: [0, 1, 2].map(i => 'dot' + i) };
    sp.dots.forEach((key, i) => mk(key, { kind: 'mark', color: KIND_COL[i], ms: Math.sqrt(1.4), z: 6 }));   // scatter(s=1.4, lw=0)
    const hover = [];
    for (const t of T) {
      const [i, j] = spot.get(t.participant_id), stop = stopped.get(t.participant_id);
      const sx = x.get(t.level_nr + '/' + t.slotKey) - CELL2 / 2 + step * (i + 0.5), sy = t.y - CELL2 / 2 + step * (j + 0.5), h = step * 0.47;
      poly(stop && stop.id === t.id ? stops : spots, box(sx - h, sy - h, sx + h, sy + h));
      if (t.completes) poly(frames, box(sx - h, sy - h, sx + h, sy + h), true);
      poly(ticks, [[sx, sy + rOut * 0.9], [sx, sy + rOut * 1.12]]);
      if (turns[t.id]) spiral(f, sp, turns[t.id], sx, sy, rIn, rOut);
      hover.push([sx, sy, tileHover(t)]);
    }
    for (const a of groupBy(TT.filter(t => t.y > CAP), t => t.level_nr + '/' + t.slotKey).values())
      mtext(f, x.get(a[0].level_nr + '/' + a[0].slotKey), CAP + 0.8, `+${a.length} trials\nafter round ${CAP}`, { size: 4.5, color: C.MUTED, ha: 'center', va: 'center', ls: 1.1, xref: 'x', yref: 'y' });
    for (let v = 1; v <= ymax; v++) mtext(f, ax[0] - 6.5, v, String(v), { size: 6.5, color: C.MUTED, ha: 'right', va: 'center_baseline', yref: 'y' });
    mtext(f, ax[0] - 6.5 - tickW - 4, (ax[1] + ax[3]) / 2, 'Round', { size: 8, ha: 'right', angle: -90 });
    for (const [L, s] of slots) {
      const sx = x.get(L + '/' + s.join('|'));
      mtext(f, sx, ax[1] - 2, (s[0] === 1 ? 'corner\n' : 'face\n') + TYP[s[0]], { size: 6.5, color: C.MUTED, ha: 'center', va: 'top', ls: 1.2, xref: 'x' });
      if (L >= 5) mtext(f, sx, ax[1] - 22, s[1] === 0 ? 'cong.' : 'incong.', { size: 6, color: C.LIGHT, ha: 'center', va: 'top', xref: 'x' });
    }
    for (const [L, a, b] of bounds) mtext(f, (a + b) / 2, ax[1] + (top + 0.05 - yl[0]) / dpp, `Level ${L}`, { size: 8, bold: true, ha: 'center', va: 'bottom', xref: 'x' });
    // key: participant spots, in the empty top of the last level
    const ks = 0.42, Lk = bounds[bounds.length - 1], kx0 = (Lk[1] + Lk[2]) / 2 - g * ks / 2, ky0 = ymax - g * ks - 0.3;
    poly(keyBg, roundBox(kx0, ky0, kx0 + g * ks, ky0 + g * ks, 0.08));
    for (const [p, [i, j]] of spot) mtext(f, kx0 + ks * (i + 0.5), ky0 + ks * (j + 0.5), escaped(short(p).replace('pilot ', 'p')),
      { size: 6, color: stopped.has(p) ? C.MUTED : C.INK, bold: !stopped.has(p), ha: 'center', va: 'center', xref: 'x', yref: 'y' });
    mtext(f, kx0 + g * ks / 2, ky0 + g * ks + 0.08, 'Participant in each cell', { size: 6.5, ha: 'center', va: 'bottom', xref: 'x', yref: 'y' });
    mtext(f, kx0 + g * ks / 2, ky0 - 0.08, 'bold = finished all levels', { size: 5.5, color: C.MUTED, ha: 'center', va: 'top', xref: 'x', yref: 'y' });
    mtext(f, 0.01 * f.W, 0.98 * f.H, `How the objects were turned, ${escaped(o.name || 'Selection')} (n = ${order.length})`, { size: 10, bold: true, va: 'top' });
    mplLegend(f, legendCore(C, 1.0), 0.01 * f.W, 0.975 * f.H, { size: 7, ncol: 8, columnspacing: 1.8, handletextpad: 0.5 });
    mtext(f, 0.01 * f.W, 0.006 * f.H, 'Each cell is one object in one round; each participant has a fixed spot in it (key in the last level) showing how they turned the object in that trial, rotated so the rewarded position points up (left/right as on screen; turning left runs clockwise). Inner end = start, outer end = last check.', { size: 6, color: C.MUTED });
    const k = f.k;
    return mplPanel(f, ax, xl, yl, [...layerTraces(f, dpp), hoverTrace(hover, step * 0.94, dpp, f)], turnRows(T),
      { y: { showline: true, linecolor: C.AXIS, linewidth: 0.6 * k, ticks: 'outside', ticklen: 3 * k, tickwidth: 0.6 * k, tickcolor: C.AXIS, tickmode: 'array', tickvals: Array.from({ length: ymax }, (_, i) => i + 1) } });
  }

  // 3. group_matrix_figure(): participants (rows, by progress) × levels (columns); rounds left to right, objects stacked.
  function turnMatrix(d, o) {
    const turns = needTurns(d), TT = d.T, T = TT.filter(t => t.y <= CAP);
    if (!T.length) return { traces: [], rows: [], noSwap: true };
    const BOX = 0.95, R_OUT = 0.42, R_IN = 0.15, GAP_L = 0.6, GAP_P = 0.35, NOBJ = 4;
    const order = progressOrder(TT), stopped = lastStops(TT), levels = [...new Set(TT.map(t => t.level_nr))].sort((a, b) => a - b);
    const slotsOf = new Map(levels.map(L => [L, [...new Map(TT.filter(t => t.level_nr === L).map(t => [t.slotKey, t.slot])).values()].sort(cmpTuple).map(s => s.join('|'))]));
    const width = new Map(levels.map(L => [L, Math.min(CAP, Math.max(...TT.filter(t => t.level_nr === L).map(t => t.y)))]));
    const lvlOff = new Map(); let pos = 0;
    for (const L of levels) { lvlOff.set(L, pos); pos += width.get(L) + GAP_L; }
    const lvlTotal = pos - GAP_L, pOff = new Map(order.map((p, i) => [p, i * (NOBJ + GAP_P)])), pTotal = order.length * (NOBJ + GAP_P) - GAP_P;
    const cell = (L, s, y, pid) => { const ks = slotsOf.get(L), obj = (NOBJ - ks.length) / 2 + ks.indexOf(s); return [lvlOff.get(L) + (y - 1) + 0.5, pTotal - (pOff.get(pid) + obj + 0.5)]; };
    const f = mplFigure(0.2 * lvlTotal + 2.2, 0.2 * pTotal + 1.6, o.width, o.theme), C = f.C;
    const xl = [-5.0, lvlTotal + 0.8], yl = [-0.3, pTotal + 2.0];
    const ax = aspectFit([0.005 * f.W, 0.25 * 72, 0.995 * f.W, f.H - 0.75 * 72], xl, yl);   // subplots_adjust(...)
    const dpp = (xl[1] - xl[0]) / (ax[2] - ax[0]), mk = (key, spec) => layer(f, key, spec);
    const blocks = mk('block', { kind: 'fill', color: C.BLOCK, z: 0 }), blockEdges = mk('blockEdge', { kind: 'line', color: C.BLOCK_EDGE, lw: 0.3, z: 0 });
    const tiles = mk('tile', { kind: 'fill', color: C.TILE, z: 1 }), stops = mk('stops', { kind: 'fill', color: C.STOP, z: 1 });
    const ticks = mk('tick', { kind: 'line', color: GREEN, lw: 0.45, z: 2 });
    const frames = mk('frame', { kind: 'line', color: C.INK, lw: 0.25, z: 4 });
    const sp = { z: 3, lw: 0.55, dots: [0, 1, 2].map(i => 'dot' + i) };
    sp.dots.forEach((key, i) => mk(key, { kind: 'mark', color: KIND_COL[i], ms: Math.sqrt(1.3), z: 5 }));   // scatter(s=1.3, lw=0)
    const hover = [];
    for (const t of T) {
      const [cx, cy] = cell(t.level_nr, t.slotKey, t.y, t.participant_id), h = BOX / 2, stop = stopped.get(t.participant_id);
      poly(stop && stop.id === t.id ? stops : tiles, box(cx - h, cy - h, cx + h, cy + h));
      if (t.completes) poly(frames, box(cx - h, cy - h, cx + h, cy + h), true);
      poly(ticks, [[cx, cy + R_OUT * 0.92], [cx, cy + R_OUT * 1.12]]);
      if (turns[t.id]) spiral(f, sp, turns[t.id], cx, cy, R_IN, R_OUT);
      hover.push([cx, cy, tileHover(t)]);
    }
    for (const L of levels) for (const pid of order) {
      const sel = TT.filter(t => t.participant_id === pid && t.level_nr === L);
      if (!sel.length) continue;
      const bx = lvlOff.get(L), by = pTotal - pOff.get(pid) - NOBJ, b = box(bx - 0.05, by - 0.05, bx + width.get(L) + 0.05, by + NOBJ + 0.05);
      poly(blocks, b); poly(blockEdges, b, true);
      const over = sel.filter(t => t.y > CAP).length;
      if (over) mtext(f, bx + width.get(L) + 0.05, by + NOBJ / 2, `+${over}`, { size: 3.8, color: C.MUTED, va: 'center', xref: 'x', yref: 'y' });
    }
    for (const L of levels) {
      mtext(f, lvlOff.get(L) + width.get(L) / 2, pTotal + 0.75, `Level ${L}`, { size: 7.5, bold: true, ha: 'center', va: 'bottom', xref: 'x', yref: 'y' });
      mtext(f, lvlOff.get(L), pTotal + 0.15, 'round →', { size: 4.5, color: C.LIGHT, va: 'bottom', xref: 'x', yref: 'y' });
    }
    for (const p of order) {
      const yc = pTotal - pOff.get(p) - NOBJ / 2;
      mtext(f, -0.6, yc + 0.35, escaped(short(p)), { size: 7, bold: true, ha: 'right', va: 'center', xref: 'x', yref: 'y' });
      mtext(f, -0.6, yc - 0.6, stopped.has(p) ? `time limit L${stopped.get(p).level_nr}` : '✓ all levels', { size: 4.8, color: C.MUTED, ha: 'right', va: 'center', xref: 'x', yref: 'y' });
    }
    mtext(f, 0.01 * f.W, f.H - 0.1 * 72, `How the objects were turned, ${escaped(o.name || 'Selection')} (n = ${order.length}): participants × levels`, { size: 10, bold: true, va: 'top' });
    mplLegend(f, legendCore(C), 0.005 * f.W, f.H - 0.38 * 72, { size: 7, ncol: 8, columnspacing: 1.6, handletextpad: 0.5 });
    mtext(f, 0.01 * f.W, 0.004 * f.H, `Each tile is one trial, rotated so the rewarded position points up; inner end = start, outer end = last check; left/right as on screen. Participants ordered by progress. Rounds 1–${CAP} shown (+n = later trials); objects stacked inside each band, top to bottom: corner ${TYP[1]} congruent, corner incongruent, face ${TYP[2]} congruent, face incongruent (Levels 0–4: no congruency).`, { size: 5.8, color: C.MUTED, va: 'bottom' });
    return mplPanel(f, ax, xl, yl, [...layerTraces(f, dpp), hoverTrace(hover, BOX, dpp, f)], turnRows(T));
  }

  // ── other views on the same table ──
  const LABELS = { y: 'Round (object presentation)', level_nr: 'Level', trial_nr_global: 'Trial number in session', chain__step: 'Chain step',
    clock__trial_duration: 'Trial duration (s)', wrong: 'Mistakes (wrong checks)', nr_attempts: 'Checks', object_rep_nr: 'Object repetition',
    pos_in_round: 'Order within the round', clock__trial_start_time: 'Session time (s)', hr: 'Mean heart rate (bpm)', trial: 'Trial number in session',
    participant_id: 'Participant', object_type: 'Object type', features_congruent: 'Congruency', first: 'First-check outcome' };
  const PLOTS = { mistakes_level: 'Mistakes per level', mistakes_object: 'Mistakes per object + first checks', timeline: 'Object timeline',
    trials: 'Trials per level', first_choice: 'First check on first sighting, per level', duration: 'Trial duration', chain: 'Chain progression',
    scatter: 'Custom scatter / line', histogram: 'Distribution / histogram', hr_summary: 'Mean heart rate',
    turn_participant: 'Rotation Strategy: one participant', turn_cells: 'Rotation Strategy: group (object × round cells)', turn_matrix: 'Rotation Strategy: participants × levels' };
  // The axes of each plot that the explorer's X / Y selects cannot change (shown there by name).
  const OBJECT_AXIS = 'Object in each level (corner / face)';
  const AXES = {
    mistakes_level: { x: 'Level', y: 'Mistakes per level (wrong checks)' }, trials: { x: 'Level', y: 'Trials per level' },
    mistakes_object: { x: 'Level', y: 'Mistakes (wrong checks)' }, timeline: { x: OBJECT_AXIS, y: 'Round' },
    first_choice: { x: 'Level', y: 'First checks on first sighting (%)' }, duration: { y: LABELS.clock__trial_duration }, chain: { y: LABELS.chain__step },
    histogram: { density: 'Probability density' }, hr_summary: { y: 'Mean heart rate (bpm)' },
    // make_MAIN_sampling_figures.py (drawn on the server)
    turn_participant: { x: OBJECT_AXIS, y: 'Round (= times the object was played in the level)' }, turn_cells: { x: OBJECT_AXIS, y: 'Round' },
    turn_matrix: { x: 'Round, within each level', y: 'Participant (ordered by progress)' },
  };
  function colourOf(d, o) {
    const by = o.color || 'participant_id';
    if (by === 'participant_id') { const i = new Map(d.participants.map((p, k) => [p, k])); return t => ({ key: t.participant_id, name: label(t.participant_id), color: COLORS[i.get(t.participant_id) % 8], dash: i.get(t.participant_id) < 8 ? 'solid' : DASH, symbol: i.get(t.participant_id) < 8 ? 'circle' : 'diamond' }); }
    if (by === 'object_type') return t => ({ key: t.object_type, name: `${t.object_type} object ${TY[t.object_type_nr]}`, color: TYPE_COL[t.object_type_nr] });
    if (by === 'features_congruent') return t => ({ key: t.features_congruent, name: { True: 'congruent', False: 'incongruent', '': 'no colour rule' }[t.features_congruent], color: { True: COLORS[0], False: COLORS[7], '': '#6f6f6f' }[t.features_congruent] });
    if (by === 'first') return t => { const k = t.noguess ? 'none' : t.ok ? 'correct' : t.type_ok ? 'kind' : 'wrong'; return { key: k, name: { correct: 'First check correct', kind: 'Wrong position, right kind', wrong: 'Wrong kind', none: 'No check' }[k], color: { correct: GREEN, kind: YELLOW, wrong: RED, none: '#999999' }[k] }; };
    const levels = [...new Set(d.T.map(t => t.level_nr))].sort((a, b) => a - b);
    return t => ({ key: t.level_nr, name: 'Level ' + t.level_nr, color: COLORS[levels.indexOf(t.level_nr) % 8] });
  }
  function scatter(d, o) {
    const c = palette(o.theme), traces = [], xk = o.x || 'y', yk = o.y || 'clock__trial_duration', col = colourOf(d, o), seen = new Set();
    for (const a of groupBy(d.T, t => K(col(t).key, t.participant_id)).values()) {
      const s = col(a[0]), sorted = a.slice().sort((p, q) => p.trial_nr_global - q.trial_nr_global);
      traces.push({ type: 'scatter', mode: o.style === 'line' ? 'lines+markers' : 'markers', name: escaped(s.name), legendgroup: String(s.key), showlegend: !seen.has(s.key),
        x: sorted.map(t => t[xk]), y: sorted.map(t => t[yk]), line: { color: s.color, width: 1.2, dash: s.dash || 'solid' }, marker: { color: s.color, size: 5, opacity: 0.8, symbol: s.symbol || 'circle' },
        text: sorted.map(t => `${escaped(t.participant_id)} · level ${t.level_nr}`), hovertemplate: '%{text}<br>%{x}, %{y}<extra></extra>' });
      seen.add(s.key);
    }
    return { traces, rows: d.T.map(t => ({ participant_id: t.participant_id, level_nr: t.level_nr, [xk]: t[xk], [yk]: t[yk] })), x: LABELS[xk], y: LABELS[yk],
      axis: { ...xaxisStyle(c), ...intTicks(d.T.map(t => t[xk])) }, yaxis: { ...yaxisStyle(c), rangemode: 'normal', ...intTicks(d.T.map(t => t[yk])) } };
  }
  function intTicks(values) {
    const v = values.filter(finite);
    return v.length && v.every(Number.isInteger) && Math.max(...v) - Math.min(...v) <= 12 ? { dtick: 1, tick0: 0 } : {};
  }
  function firstChoice(d, o) {
    const c = palette(o.theme), O = objects(d.T), traces = [], rows = [];
    const levels = [...new Set(O.map(r => r.level_nr))].sort((a, b) => a - b), byPosition = o.y === 'position';
    const first = new Map(); for (const t of d.T) { const k = K(t.participant_id, t.level_nr, t.object_id); if (!first.has(k)) first.set(k, t); }
    const choice = r => byPosition ? (first.get(K(r.participant_id, r.level_nr, r.object_id)).first_attempt_position) : r.first_choice;
    const cats = byPosition ? [0, 1, 2, 3, 4, 5, ''] : ['corner', 'face', ''];
    cats.forEach((k, i) => {
      const y = levels.map(L => {
        const props = [...groupBy(O.filter(r => r.level_nr === L), r => r.participant_id).values()].map(a => a.filter(r => String(choice(r) ?? '') === String(k)).length / a.length);
        const v = 100 * mean(props); rows.push({ level_nr: L, choice: k === '' ? 'no check' : k, percent: v, n: props.length }); return v;
      });
      traces.push({ type: 'bar', name: k === '' ? 'No check' : byPosition ? `Position ${k} (${k % 2 ? 'face' : 'corner'})` : `Checked a ${k} first`, x: levels, y,
        marker: { color: k === '' ? c.MUTED : byPosition ? COLORS[i % 8] : CHOICE_COL[k] } });
    });
    return { traces, rows, x: AXES.first_choice.x, y: AXES.first_choice.y, layout: { barmode: 'stack' }, axis: xaxisStyle(c), yaxis: yaxisStyle(c),
      note: 'First check on the first presentation of each object in the level; shares are computed per participant, then averaged.' };
  }
  function distribution(d, o) {
    // The measured value is on x in a histogram, on y in a box / violin plot (one per colour group).
    const boxed = ['box', 'violin'].includes(o.style), c = palette(o.theme), field = (boxed ? o.y : o.x) || 'clock__trial_duration', col = colourOf(d, o), traces = [];
    const all = d.T.map(t => t[field]).filter(finite), lo = Math.min(...all), hi = Math.max(...all);
    const size = all.every(Number.isInteger) ? Math.max(1, Math.ceil((hi - lo + 1) / 30)) : (hi - lo) / 30 || 1;
    const xbins = { start: all.every(Number.isInteger) ? lo - 0.5 : lo, end: hi + size, size };
    for (const a of groupBy(d.T, t => col(t).key).values()) {
      const s = col(a[0]), v = a.map(t => t[field]).filter(finite);
      traces.push(boxed
        ? { type: o.style, name: escaped(s.name), y: v, line: { color: s.color }, [o.style === 'box' ? 'boxpoints' : 'points']: o.individual !== false ? 'all' : false, jitter: 0.5, pointpos: -1.6, marker: { color: s.color, size: 4, opacity: 0.6 }, ...(o.style === 'violin' ? { box: { visible: true } } : {}) }
        : { type: 'histogram', name: escaped(s.name), x: v, histnorm: 'probability density', opacity: 0.6, marker: { color: s.color }, xbins, autobinx: false });
    }
    return { traces, rows: d.T.map(t => ({ participant_id: t.participant_id, level_nr: t.level_nr, [field]: t[field] })), x: boxed ? LABELS[o.color || 'participant_id'] : LABELS[field], y: boxed ? LABELS[field] : AXES.histogram.density,
      layout: boxed ? {} : { barmode: 'overlay' }, axis: { ...xaxisStyle(c), ...(boxed ? { type: 'category' } : {}) }, yaxis: yaxisStyle(c) };
  }
  function hrSummary(d, o) {
    const c = palette(o.theme), xkey = o.x === 'trial' ? 'trial' : 'level_nr', traces = [];
    const eligible = groupBy(d.packets.filter(p => p.valid && (p.phase === 'trial' || p.phase === 'file bucket (timing unknown)')), p => p.trial);
    const obs = [...groupBy(d.T, t => xkey === 'trial' ? t.id : K(t.participant_id, t.level_nr)).values()].map(a => {
      const hr = a.flatMap(t => eligible.get(t.id) || []).map(p => p.hr);
      return { participant_id: a[0].participant_id, level_nr: a[0].level_nr, trial: a[0].trial_nr_global, value: mean(hr), n_readings: hr.length };
    });
    const index = new Map(d.participants.map((p, k) => [p, k]));
    if (o.individual !== false) for (const a of groupBy(obs, r => r.participant_id).values()) {
      if (!a.some(r => finite(r.value))) continue;
      const i = index.get(a[0].participant_id), sorted = a.slice().sort((p, q) => p[xkey] - q[xkey]), color = COLORS[i % 8];
      traces.push({ type: 'scatter', mode: 'lines+markers', name: escaped(label(a[0].participant_id)), x: sorted.map(r => r[xkey]), y: sorted.map(r => r.value), opacity: 0.8,
        line: { color, width: 1.4, dash: i < 8 ? 'solid' : DASH }, marker: { color, size: 5 }, connectgaps: false,
        customdata: sorted.map(r => [r.level_nr, r.n_readings]), hovertemplate: '%{x}: %{y:.1f} bpm<br>level %{customdata[0]} · %{customdata[1]} readings<extra>%{fullData.name}</extra>' });
    }
    const xs = [...new Set(obs.filter(r => finite(r.value)).map(r => r[xkey]))].sort((a, b) => a - b);
    const st = xs.map(x => { const v = [...groupBy(obs.filter(r => r[xkey] === x && finite(r.value)), r => r.participant_id).values()].map(a => mean(a.map(r => r.value))); return { mean: mean(v), sd: std(v), n: v.length }; });
    traces.push({ type: 'scatter', mode: 'lines+markers', name: 'Mean ± SD', x: xs.map(x => xkey === 'level_nr' ? x + 0.2 : x), y: st.map(s => s.mean), line: { color: c.INK, width: 2.4 },
      marker: { size: 8, color: c.INK, line: { color: c.SURF, width: 1.2 } }, error_y: { type: 'data', array: st.map(s => s.sd), visible: true, color: c.INK, thickness: 1.4, width: 4 },
      customdata: st.map(s => s.n), hovertemplate: '%{y:.1f} bpm<br>n = %{customdata}<extra>Mean ± SD</extra>' });
    return { traces, rows: obs, x: LABELS[xkey], y: AXES.hr_summary.y, axis: xaxisStyle(c), yaxis: { ...yaxisStyle(c), rangemode: 'normal' },
      note: 'Each point is the mean of valid heart-rate readings within the trial or level (recorded trial boundaries; older files: file membership). Missing readings stay blank. Mean ± SD across participants.' };
  }

  // ── figure ──
  // Compare mode, one participant: the name of the group holding that participant's sessions.
  function groupOf(d, o) {
    if (!o.compare || !o.participant) return '';
    const own = new Set(d.T.filter(t => t.participant_id === o.participant).map(t => t.session));
    return ['A', 'B'].map(g => (o['group' + g] || []).some(s => own.has(s)) ? o['name' + g] || 'Group ' + g : '').find(Boolean) || '';
  }
  function figure(dataset, options) {
    const d = filter(dataset, options);
    const groups = options.compare && options.plot !== 'turn_participant'
      ? ['A', 'B'].map(letter => ({ letter, name: options['name' + letter] || 'Group ' + letter, data: filter(d, { sessions: new Set(options['group' + letter] || []) }) }))
      : [{ letter: '', name: options.name || groupOf(d, options) || 'Selection', data: d }];
    // Same strip row spacing in both panels, as make_MAIN_figures.py.
    const o = { ...options, npmax: Math.max(1, ...groups.map(g => g.data.participants.length)) };
    const make = (g, name) => {
      const o2 = { ...o, name };
      switch (o.plot) {
        case 'mistakes_level': case 'trials': return mistakesPerLevel(g, o);
        case 'mistakes_object': return mistakesPerObject(g, o);
        case 'timeline': return timeline(g, o);
        case 'first_choice': return firstChoice(g, o);
        case 'duration': return scatter(g, { ...o, y: 'clock__trial_duration', x: o.x || 'y' });
        case 'chain': return scatter(g, { ...o, y: 'chain__step', x: o.x || 'trial_nr_global', style: o.style || 'line' });
        case 'scatter': return scatter(g, o);
        case 'histogram': return distribution(g, o);
        case 'hr_summary': return hrSummary(g, o);
        case 'turn_participant': return turnParticipant(g, o2);   // the group name is in the script's title
        case 'turn_cells': return turnCells(g, o2);
        case 'turn_matrix': return turnMatrix(g, o2);
        default: throw new Error('Unknown plot');
      }
    };
    const panels = groups.map(g => { const made = make(g.data, g.name); return { ...made, letter: g.letter, title: made.title ?? `${escaped(g.name)}  (n = ${g.data.participants.length})`,
      names: Object.fromEntries(g.data.participants.map(p => [label(p), p])) }; });   // shown name → full id, for hover
    if (o.compare && o.plot === 'mistakes_object') {                     // one legend, right of panel B
      panels[0].layout = { ...panels[0].layout, showlegend: false };
      panels[0].margin = { ...panels[0].margin, r: 20 };
    }
    if (o.compare && ['timeline', 'turn_cells', 'turn_matrix'].includes(o.plot)) for (const p of panels) p.stacked = true;   // stacked, like the script's alternative
    if (o.compare && ['mistakes_level', 'trials', 'mistakes_object', 'hr_summary'].includes(o.plot)) {      // sharey=True
      const ends = sign => panels.flatMap(p => p.traces.filter(t => !t.yaxis || t.yaxis === 'y')
        .flatMap(t => (t.y || []).map((y, i) => finite(y) ? y + sign * (t.error_y?.array?.[i] || 0) : null))).filter(finite);
      const hi = ends(1), lo = ends(-1);
      if (hi.length) {
        const pad = (Math.max(...hi) - Math.min(...lo)) * 0.05 || 1;
        for (const p of panels) p.yaxis = { ...p.yaxis, range: [o.plot === 'hr_summary' ? Math.min(...lo) - pad : 0, Math.max(...hi) + pad] };
      }
    }
    const warnings = [...d.warnings];
    if (o.plot === 'hr_summary' && d.packets.some(p => p.timing !== 'synchronized beat')) warnings.push('Some heart-rate recordings use packet arrival time; assignment to trial boundaries is approximate.');
    return { panels, warnings, count: d.T.length, sessions: new Set(d.T.map(t => t.session)).size };
  }
  function meta(dataset) {
    return { levels: [...new Set(dataset.T.map(t => t.level_nr))].sort((a, b) => a - b), participants: dataset.participants };
  }
  const api = { load, figure, meta, palette, objects, progressOrder, timelineLayout, short, label, mean, std, PLOTS, LABELS, AXES, COLORS };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.AdminAnalysis = api;
})(typeof self !== 'undefined' ? self : globalThis);
