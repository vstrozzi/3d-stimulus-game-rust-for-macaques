// node tools/tests/test_admin_analysis.js
'use strict';
const assert = require('node:assert/strict');
const A = require('../../deploy_frontend/admin_analysis.js');
const catalog = [
  { id: 'Alice/run1', participant: 'Alice', date: '2026-11-15' },
  { id: 'Alice/run2', participant: 'Alice', date: '2026-11-16' },
  { id: 'Bob/run1', participant: 'Bob', date: '2026-11-16' },
];
function check(door, correct, t = 2) {
  const theta = door * Math.PI / 3 + Math.PI / 2 + Math.PI / 6;
  return { state_read: { camera_x: Math.cos(theta), camera_z: Math.sin(theta), current_alignment: correct ? 1 : -1, present_elapsed_secs: t }, commands_sent: { check: true }, check_input_event_elapsed_secs: t };
}
function trial(session, counter, checks, extra = {}) {
  return { session, kind: 'trial', path: `2026-11-16/${session}/level_000/120000/trials/trial_${counter}.json`, data: {
    level_index: 0, active_chain: 0, trial_run_counter: counter, trial_index_in_chain: counter - 1,
    trial_config: { target_door: 2, cosine_alignment_threshold: .95, analysis_congruency: 'congruent' },
    start_orient: 0, outcome: 'stay', trial_chain_completed: counter === 2, nr_attempts: checks.length,
    timestamp_start: `2026-11-16T12:00:${String(counter).padStart(2, '0')}Z`, elapsed_time_no_anim: 3,
    check_frames: checks, hr_info: { device: { name: 'H10' }, trial_start_elapsed_secs: counter * 5, sync_info: { status: 'start+end', precision_ms: 1 } },
    hr_meas: Array.from({ length: 6 }, (_, i) => ({ arrival_elapsed_secs: counter * 5 + i + 1, beat_elapsed_secs: counter * 5 + i, hr: 70 + i, rr_ms: [800] })),
    hr_ecg: [{ first_elapsed_secs: counter * 5, dt_ms: 10, uV: [0, 1, -1] }], ...extra,
  } };
}
function summary(session, complete = true) {
  return { session, kind: 'level', path: `2026-11-16/${session}/level_000/120000/level_000_summary.json`, data: {
    level_index: 0, level_completed: complete ? 'completed' : 'timeout', level_config: { objects: [{ target_door: 2 }], trials: [{}, {}], fixed: { remove_completed_chains: true } }, trials_runs: [],
  } };
}
const documents = [
  ...catalog.map(s => summary(s.id, s.participant !== 'Bob')),
  trial('Alice/run1', 1, [check(2, true)]),
  trial('Alice/run1', 2, [check(0, false), check(1, false), check(2, true)]),
  trial('Alice/run2', 1, [check(2, true)]),
  trial('Bob/run1', 1, [check(1, false), check(1, false), check(1, false), check(2, true)]),
];
let d = A.normalize(documents, catalog);
assert.equal(d.trials.length, 4);
assert.equal(d.trials[0].mistakes, 0, 'Correct STAY is not a mistake');
assert.equal(d.trials[0].level_complete, true, 'Join summaries by directory, even when the filename begins with level_');
assert.equal(d.trials[1].mistakes, 2);
assert.equal(d.trials[1].checks[0].status, 'position');
assert.equal(d.trials[1].checks[1].status, 'kind');
assert.equal(d.trials[1].checks[0].door, 0);
assert.equal(d.trials[1].round, 2);
assert.equal(d.trials[1].first_sighting, false);
assert.equal(d.trials[3].timeout, true);
assert.equal(A.metric(d.trials.slice(0, 2), 'percent'), 50);
assert.equal(A.metric(d.trials.slice(0, 2), 'per_object'), 2);
assert.equal(A.congruency({}), 'unknown');
assert.equal(d.packets[0].t, 5, 'Timed beat uses sensor time');
assert.equal(A.classify({ state_read: { current_alignment: -.5 } }, { target_door: 2, cosine_alignment_threshold: .95 }, null).status, 'position');
for (let door = 0; door < 6; door++) {
  for (const orient of [0, .7, 5.8]) {
    const row = check(door, door === 2), theta = door * Math.PI / 3 + Math.PI / 2 + Math.PI / 6 + orient;
    row.state_read.camera_x = Math.cos(theta); row.state_read.camera_z = Math.sin(theta);
    assert.equal(A.classify(row, { target_door: 2, cosine_alignment_threshold: .95 }, orient).door, door);
  }
}
const base = { measure: 'mistakes', weight: 'participant', individual: true, levels: [0] };
let f = A.figure(d, { ...base, plot: 'mistakes_level' });
assert.equal(f.panels[0].traces.at(-1).y[0], 2, 'Alice: mean 1; Bob: 3; equal participant mean = 2');
f = A.figure(d, { ...base, plot: 'mistakes_level', weight: 'session' });
assert.equal(f.panels[0].traces.at(-1).y[0], 5 / 3);
for (const plot of Object.keys(A.PLOTS)) {
  const options = { ...base, plot };
  const fig = A.figure(d, options);
  assert(fig.panels.length, plot);
  for (const panel of fig.panels) {
    assert(Array.isArray(panel.rows), plot + ': export table');
    assert(Array.isArray(panel.traces), plot + ': traces');
    for (const trace of panel.traces) if (trace.x && trace.y) assert.equal(trace.x.length, trace.y.length, plot + ': coordinate lengths');
  }
  assert.doesNotThrow(() => A.figure(A.normalize([], []), options), plot + ': empty selection');
}
const config = d.trials[0].config;
f = A.figure(d, { ...base, plot: 'mistakes_object', labelOverrides: { [`${config}|0|0`]: 'incongruent' }, congruency: 'incongruent' });
assert.equal(f.count, 4);
assert.equal(d.trials[0].congruency, 'congruent', 'Local labeling must not mutate the raw dataset');
f = A.figure(d, { ...base, plot: 'timeline', compare: true, groupA: ['Alice/run1'], groupB: ['Bob/run1'] });
assert.equal(f.panels.length, 2);
assert.equal(f.panels[0].rows.length, 2);
assert.equal(f.panels[1].rows.length, 1);
const midnight = { ...documents[3], path: documents[3].path.replace('2026-11-16', '2026-11-17') };
assert.equal(A.normalize([...documents, midnight], catalog).trials.length, 4, 'Re-upload across midnight is not a new trial');
const missing = trial('Alice/run1', 99, [], { nr_attempts: 2 });
assert.equal(A.normalize([missing], catalog).trials[0].mistakes, null);
const packetOnly = trial('Alice/run1', 1, [], { hr_meas: [{ arrival_elapsed_secs: 8, hr: 80, rr_ms: [750, 750] }, { arrival_elapsed_secs: 9, hr: 80, rr_ms: [] }] });
const old = A.normalize([packetOnly], catalog);
assert.equal(A.figure(old, { ...base, plot: 'hr_summary' }).panels[0].rows[0].n_readings, 1, 'Stale chest-strap HR without RR is excluded');
assert.equal(A.figure(old, { ...base, plot: 'hr_summary' }).panels[0].rows[0].value, 80);
const optical = A.normalize([trial('Alice/run1', 1, [], { hr_info: { device: { name: 'Polar Sense' } }, hr_meas: [{ arrival_elapsed_secs: 8, hr: 80, rr_ms: [] }] })], catalog);
assert.equal(A.figure(optical, { ...base, plot: 'hr_summary' }).panels[0].rows[0].value, 80, 'Optical sensors can report HR without RR');
assert.equal(A.figure(d, { ...base, plot: 'first_choice', y: 'region' }).panels[0].traces.length, 8);
const early = trial('Alice/run1', 1, [], { hr_info: { trial_start_elapsed_secs: 0, trial_end_elapsed_secs: 4 }, hr_meas: [] });
const late = trial('Alice/run1', 2, [], { level_index: 1, hr_info: { trial_start_elapsed_secs: 5, trial_end_elapsed_secs: 9 },
  hr_meas: [{ arrival_elapsed_secs: 6, beat_elapsed_secs: 3, hr: 90, rr_ms: [800] }, { arrival_elapsed_secs: 7, beat_elapsed_secs: 6, hr: 95, rr_ms: [800] }] });
const context = A.normalize([early, late], catalog);
assert.deepEqual(context.packets.map(p => p.level), [0, 1], 'Delayed beats belong to their timestamped trial, not their save file');
assert.deepEqual(context.packets.map(p => p.phase), ['trial', 'trial']);
const simpleHR = A.normalize([
  trial('Alice/run1', 1, [], { hr_info: { trial_start_elapsed_secs: 0, trial_end_elapsed_secs: 4 }, hr_meas: [
    { beat_elapsed_secs: -1, hr: 200, rr_ms: [800] },
    { beat_elapsed_secs: 1, hr: 60, rr_ms: [800] },
    { beat_elapsed_secs: 2, hr: 80, rr_ms: [800] },
    { beat_elapsed_secs: 4.5, hr: 200, rr_ms: [800] },
  ] }),
  trial('Alice/run1', 2, [], { hr_info: { trial_start_elapsed_secs: 5, trial_end_elapsed_secs: 9 }, hr_meas: [{ beat_elapsed_secs: 6, hr: 100, rr_ms: [800] }] }),
  trial('Alice/run1', 3, [], { hr_info: { trial_start_elapsed_secs: 10, trial_end_elapsed_secs: 14 }, hr_meas: [] }),
], catalog);
f = A.figure(simpleHR, { ...base, plot: 'hr_summary', x: 'counter' });
assert.deepEqual(f.panels[0].rows.map(r => r.value), [70, 100, null], 'One mean per trial; gaps stay null');
assert.deepEqual(f.panels[0].rows.map(r => r.n_readings), [2, 1, 0], 'Exclude pre-game and between-trial samples');
f = A.figure(simpleHR, { ...base, plot: 'hr_summary', x: 'level' });
assert.equal(f.panels[0].rows[0].value, 80, 'Level mean includes all valid selected readings');
const weightedHR = A.normalize([
  trial('Alice/run1', 1, [], { hr_meas: [{ arrival_elapsed_secs: 1, hr: 60, rr_ms: [800] }] }),
  trial('Alice/run2', 1, [], { hr_meas: [{ arrival_elapsed_secs: 1, hr: 80, rr_ms: [800] }] }),
  trial('Bob/run1', 1, [], { hr_meas: [{ arrival_elapsed_secs: 1, hr: 100, rr_ms: [800] }] }),
], catalog);
assert.equal(A.figure(weightedHR, { ...base, plot: 'hr_summary' }).panels[0].traces.at(-1).y[0], 85);
assert.equal(A.figure(weightedHR, { ...base, plot: 'hr_summary', weight: 'session' }).panels[0].traces.at(-1).y[0], 80);
assert.deepEqual(Object.keys(A.PLOTS).filter(k => /hr|rr|poincare|waveform/.test(k)), ['hr_summary']);
assert.throws(() => A.figure(d, { ...base, plot: 'histogram', y: 'hr' }), /duration or mistakes/);
console.log('admin analysis: all tests passed');
