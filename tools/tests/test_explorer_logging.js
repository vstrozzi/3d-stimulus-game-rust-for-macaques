// Exercise the actual controller logging functions without booting WASM.
'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require.resolve('../../controller_main.js'), 'utf8');
const start = source.indexOf('function saveTrialLog(');
const end = source.indexOf('\n/** Sanitize a session name', start);
assert(start > 0 && end > start);
for (const partial of [false, true]) {
  const files = [];
  const context = {
    Date, console: { log() {} }, performance: { now: () => 5000 },
    _startLevelRunIfNeeded() {}, _trialFilename: () => 'trial.json', _trialIdx: () => 2,
    currentLevelIndex: 0, activeChain: 0, trialRunCounter: 7, trialStartTime: 1000,
    frameLog: [
      { state_read: { present_elapsed_secs: 1, current_alignment: -.5, is_animating: false }, commands_sent: { check: true }, check_input_event_elapsed_secs: 1 },
      { state_read: { present_elapsed_secs: 2, current_alignment: 1, is_animating: false }, commands_sent: { check: true }, check_input_event_elapsed_secs: 2 },
    ], frameLogLen: 2, trialProceeding: 1, PROCEEDING: { ADVANCE: 1 }, currentLevel: () => ({ trials: [{}, {}, {}] }),
    currentLevelSummary: { _dir: 'session/level_000/120000', trials_runs: [] },
    nrAttempts: 2, trialStartOrient: 0, levelRandomSeed: 9, flatTrial: () => ({ target_door: 2, cosine_alignment_threshold: .95 }),
    _sessionInfo: () => ({}), winEvent: null, enqueueLog: (path, data) => files.push(JSON.parse(data)), _enqueueSummary() {},
  };
  vm.createContext(context);
  vm.runInContext(source.slice(start, end), context);
  context.saveTrialLog(partial ? 'timeout' : 'advance', partial);
  const data = files[0];
  assert.equal(data.partial_trial, partial);
  assert.equal(data.trial_chain_completed, !partial);
  assert.equal(data.check_events.length, 2);
  assert.equal(Object.keys(data.frames).length, 2);
  assert.equal(data.elapsed_time_no_anim, 1);
  assert.equal(context.currentLevelSummary.trials_runs.length, 1);
  assert.equal(context.currentLevelSummary.trials_runs[0].partial_trial, partial);
}
const timeoutStart = source.indexOf('  if (sessionStartMs !== null && Date.now() - sessionStartMs >= MAX_SESSION_DURATION_MS)');
const timeoutEnd = source.indexOf('  // Dispatch FSM', timeoutStart);
for (const state of ['PLAYING', 'WAITING_ANIMATION_START', 'WAITING_ANIMATION_END', 'TRIAL_COMPLETE', 'INIT', 'WAITING_FOR_START']) {
  const calls = [];
  const context = {
    sessionStartMs: 1, MAX_SESSION_DURATION_MS: 1, MAX_SESSION_DURATION_MIN: 20,
    console: { log() {} }, fsmState: state, FSM: Object.fromEntries(['PLAYING', 'WAITING_ANIMATION_START', 'WAITING_ANIMATION_END', 'TRIAL_COMPLETE'].map(s => [s, s])),
    frameLogLen: 2, state: {}, _running: true,
    handleTrialComplete: () => calls.push('complete'), saveTrialLog: (outcome, partial) => calls.push([outcome, partial]),
    _finalizeLevelRun: status => calls.push(status), showEndPopup: () => calls.push('popup'),
  };
  vm.createContext(context);
  vm.runInContext(`function finish() { ${source.slice(timeoutStart, timeoutEnd)} } finish();`, context);
  if (state === 'TRIAL_COMPLETE') assert.deepEqual(calls, ['complete', 'timeout', 'popup']);
  else if (['INIT', 'WAITING_FOR_START'].includes(state)) assert.deepEqual(calls, ['timeout', 'popup'], 'Do not resave the preceding trial while waiting');
  else assert.deepEqual(calls, [['timeout', true], 'timeout', 'popup']);
  assert.equal(context._running, false);
}
console.log('explorer logging: partial trials, check events and timeout states passed');
