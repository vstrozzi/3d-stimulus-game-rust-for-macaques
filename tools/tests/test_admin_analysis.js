// node tools/tests/test_admin_analysis.js
// The browser only groups the server's script table per figure, as make_MAIN_figures.py does.
'use strict';
const assert = require('node:assert/strict');
const A = require('../../deploy_frontend/admin_analysis.js');

const columns = ['participant_id', 'session', 'trial_run_counter', 'level_nr', 'object_id', 'object_type', 'object_type_nr',
  'features_congruent', 'cong_key', 'object_id_order_by_type', 'trial_nr_global', 'trial_nr_in_level', 'round', 'pos_in_round', 'y',
  'completes', 'wrong', 'ok', 'type_ok', 'noguess', 'level_done', 'first_attempt_type', 'first_attempt_position', 'nr_attempts',
  'outcome__success', 'outcome__chain_action', 'chain__step', 'clock__trial_duration', 'clock__trial_start_time', 'object_rep_nr',
  'object_rep_nr_x_level', 'starting_position', 'nr_position_visits', 'loaded_feature', 'train_test'];
let counter = 0;
function row(pid, level, object, { wrong = 0, round = 0, first = 'corner', done = true, cong = '', ok = true, type_ok = true, completes = false } = {}) {
  const typ = object % 2 === 0 ? 1 : 2;
  const r = { participant_id: pid, session: pid + '/s', trial_run_counter: counter, level_nr: level, object_id: level * 10 + object,
    object_type: typ === 1 ? 'corner' : 'face', object_type_nr: typ, features_congruent: cong, cong_key: cong === 'False' ? 1 : 0,
    object_id_order_by_type: object + 1, trial_nr_global: counter++, trial_nr_in_level: 0, round, pos_in_round: object + 1, y: round + 1,
    completes, wrong, ok, type_ok, noguess: first === '', level_done: done, first_attempt_type: first, first_attempt_position: first === 'corner' ? '0' : first === 'face' ? '1' : '',   // text, as the server sends it (pandas reads the blanks as text)
    nr_attempts: wrong + 1, outcome__success: true, outcome__chain_action: 'advance', chain__step: 0, clock__trial_duration: 2 + wrong,
    clock__trial_start_time: counter, object_rep_nr: round, object_rep_nr_x_level: round, starting_position: 2, nr_position_visits: 3,
    loaded_feature: 'shape', train_test: 'train' };
  return columns.map(c => r[c]);
}
const rows = [
  // Alice: two levels, both finished.
  row('Alice', 0, 0, { wrong: 2, first: 'face', ok: false, type_ok: false }), row('Alice', 0, 0, { wrong: 1, round: 1, completes: true }),
  row('Alice', 5, 0, { wrong: 1, cong: 'True' }), row('Alice', 5, 1, { wrong: 0, cong: 'False', first: 'face' }),
  // Bob: level 0 only, time limit in level 5.
  row('Bob', 0, 0, { wrong: 5, first: 'corner', ok: false }), row('Bob', 5, 0, { wrong: 3, cong: 'True', first: '', done: false }),
  // kjpp-style names are shortened in labels.
  row('kjpp_vrsafe_bl_pilot_001', 0, 0, { wrong: 0 }),
];
const heart = [
  { session: 'Alice/s', trial_run_counter: 0, start: 0, end: 4, device: 'Polar H10', readings: [[1, 1.5, 60, true], [2, 2.5, 80, true], [3, 3.5, 200, false], [5, 6, 90, true]] },
  { session: 'Alice/s', trial_run_counter: 1, start: 4.5, end: 9, device: 'Polar H10', readings: [] },
  { session: 'Bob/s', trial_run_counter: 4, start: null, end: null, device: 'Polar Sense 1', readings: [[null, 1, 70, false]] },
];
const payload = { columns, rows, participants: ['Alice', 'Bob', 'kjpp_vrsafe_bl_pilot_001'], heart, warnings: [] };
const d = A.load(payload);
const base = { theme: 'light', levels: [0, 5], individual: true };

assert.equal(d.T.length, 7);
assert.deepEqual(d.T[2].slot, [1, 0, 1], 'slot = (type, congruency, object order)');
assert.equal(A.short('kjpp_vrsafe_bl_pilot_001'), 'pilot 001');
assert.equal(A.short('kjpp_vrsafe_003'), '003');
assert.equal(A.label('kjpp_vrsafe_003'), '003');
assert.equal(A.label('test_polar_verity_full_short #1'), 'test_pola…short #1', 'long names: start…end');
assert.equal(A.label('test_disconnet_hr #3').length, 18, 'every shortened name has the same length');

// Heart rate: assigned by recorded boundaries; strap-off readings (no RR) dropped; optical sensors need no RR.
assert.deepEqual(d.packets.filter(p => p.session === 'Alice/s').map(p => [p.trial, p.phase, p.valid]),
  [['Alice/s|0', 'trial', true], ['Alice/s|0', 'trial', true], ['Alice/s|0', 'trial', false], ['Alice/s|1', 'trial', true]]);
assert.equal(d.packets.find(p => p.session === 'Bob/s').valid, true);
let f = A.figure(d, { ...base, plot: 'hr_summary', x: 'trial' });
assert.deepEqual(f.panels[0].rows.filter(r => r.participant_id === 'Alice').map(r => r.value), [70, 90, null, null], 'trials without readings stay blank');

// O and progress order (make_MAIN_figures.load()).
const O = A.objects(d.T);
assert.equal(O.find(r => r.participant_id === 'Alice' && r.level_nr === 0).wrong, 3, 'mistakes summed over presentations');
assert.equal(O.find(r => r.participant_id === 'Alice' && r.level_nr === 0).first_choice, 'face', 'first sighting = first row');
assert.deepEqual(A.progressOrder(d.T), ['Alice', 'Bob', 'kjpp_vrsafe_bl_pilot_001'], 'levels completed, furthest level, then fewer trials');

// 1. Mistakes per level: one value per participant x level; mean ± SD (ddof 1) shifted 0.2 right.
f = A.figure(d, { ...base, plot: 'mistakes_level' });
const mean = f.panels[0].traces.find(t => t.name === 'Mean ± SD');
assert.deepEqual(mean.x, [0.2, 5.2]);
assert.deepEqual(mean.y, [8 / 3, 2]);
assert.ok(Math.abs(mean.error_y.array[0] - A.std([3, 5, 0])) < 1e-12);
assert.equal(f.panels[0].yaxis.range[0], 0, 'y axis starts at 0');
assert.deepEqual(f.panels[0].traces.find(t => t.name === 'Bob').marker.color, [A.COLORS[1], '#ffffff'], 'open marker = level not finished');
assert.equal(f.panels[0].traces.find(t => t.name === 'pilot 001').line.dash, 'solid');

// 2. Mistakes per object: level total at +0.37, corner share among corner/face choices only.
f = A.figure(d, { ...base, plot: 'mistakes_object' });
const total = f.panels[0].traces.find(t => t.name === 'Level total, mean ± SD');
assert.deepEqual(total.x, [0.37, 5.37]);
assert.deepEqual(total.y, [8 / 3, 2]);
const bars = f.panels[0].shapes.filter(s => s.yref === 'y2' && s.type === 'rect' && s.xref === 'x');
assert.ok(Math.abs(bars[0].y1 - bars[0].y0 - 2 / 3) < 1e-12, 'level 0: 2 of 3 first checks on a corner');
assert.equal(f.panels[0].layout.yaxis2.ticktext.length, 4, 'corner share + one row per participant');

// 3. Timeline: script layout (width = max(objects, 1.6), gap 0.3), stops by time limit.
const lay = A.timelineLayout(d.T);
assert.deepEqual(lay.bounds.map(b => b.map(v => +v.toFixed(2))), [[0, 0, 1.6], [5, 1.9, 3.9]]);
f = A.figure(d, { ...base, plot: 'timeline' });
assert.deepEqual(f.panels[0].traces.find(t => t.name === 'Last trial (time limit)').text.length, 1, 'Bob stopped by the time limit');
assert.equal(f.panels[0].traces.find(t => t.name === 'No check').x.length, 1);

// First check by position: positions arrive as text.
f = A.figure(d, { ...base, plot: 'first_choice', y: 'position' });
assert.ok(f.panels[0].traces.find(t => t.name === 'Position 0 (corner)').y.some(v => v > 0), 'position shares are not empty');
// Whole-number columns get integer ticks; participants without heart rate get no legend entry.
assert.equal(A.figure(d, { ...base, plot: 'chain' }).panels[0].yaxis.dtick, 1);
assert.deepEqual(A.figure(d, { ...base, plot: 'hr_summary' }).panels[0].traces.map(t => t.name), ['Alice', 'Bob', 'Mean ± SD']);
assert.equal(A.figure(d, { ...base, plot: 'timeline' }).panels[0].names['pilot 001'], 'kjpp_vrsafe_bl_pilot_001', 'shown name → full id for hover');

// Filters map to the script's columns.
assert.equal(A.figure(d, { ...base, plot: 'timeline', congruency: 'incongruent' }).count, 1);
assert.equal(A.figure(d, { ...base, plot: 'timeline', congruency: 'no rule' }).count, 4);
assert.equal(A.figure(d, { ...base, plot: 'timeline', completion: 'incomplete' }).count, 1);

// Comparison: named panels, shared y axis, same strip spacing.
f = A.figure(d, { ...base, plot: 'mistakes_object', compare: true, groupA: ['Alice/s'], groupB: ['Bob/s', 'kjpp_vrsafe_bl_pilot_001/s'], nameA: 'pilot_Gymer', nameB: 'kjpp' });
assert.deepEqual(f.panels.map(p => [p.letter, p.title]), [['A', 'pilot_Gymer  (n = 1)'], ['B', 'kjpp  (n = 2)']]);
assert.deepEqual(f.panels[0].yaxis.range, f.panels[1].yaxis.range);
assert.equal(f.panels[0].layout.yaxis2.range[1], f.panels[1].layout.yaxis2.range[1]);
assert.equal(f.panels[0].layout.showlegend, false, 'one legend, right of panel B');

// How the objects were turned: drawn on the server by make_MAIN_sampling_figures.py, not here.
assert.throws(() => A.figure(d, { ...base, plot: 'turn_cells' }), /Unknown plot/);

// Distribution: the value is on x in a histogram, on y in a box plot (one box per colour group).
f = A.figure(d, { ...base, plot: 'histogram', style: 'histogram', x: 'wrong' });
assert.deepEqual([f.panels[0].x, f.panels[0].y], [A.LABELS.wrong, 'Probability density']);
f = A.figure(d, { ...base, plot: 'histogram', style: 'box', y: 'wrong', color: 'level_nr' });
assert.deepEqual([f.panels[0].x, f.panels[0].y], ['Level', A.LABELS.wrong]);
// The axis names the explorer shows for fixed axes are the figures' own.
for (const plot of ['mistakes_level', 'trials', 'first_choice', 'hr_summary', 'duration', 'chain']) {
  const fixed = A.AXES[plot], panel = A.figure(d, { ...base, plot }).panels[0];
  if (fixed.x) assert.equal(panel.x, fixed.x, plot + ': x');
  if (fixed.y) assert.equal(panel.y, fixed.y, plot + ': y');
}
assert.match(A.PLOTS.turn_cells, /^Rotation Strategy/);

// Every plot draws, exports rows and survives an empty selection.
for (const plot of Object.keys(A.PLOTS).filter(p => !p.startsWith('turn_'))) {
  for (const o of [{ ...base, plot }, { ...base, plot, theme: 'dark', compare: true, groupA: ['Alice/s'], groupB: ['Bob/s'] }]) {
    const fig = A.figure(d, o);
    for (const panel of fig.panels) {
      assert(Array.isArray(panel.rows), plot);
      for (const t of panel.traces) if (t.x && t.y) assert.equal(t.x.length, t.y.length, plot + ': coordinate lengths');
    }
  }
  assert.doesNotThrow(() => A.figure(A.load({ columns, rows: [], participants: [], heart: [] }), { ...base, plot }), plot + ': empty selection');
}
console.log('admin analysis: all tests passed');
