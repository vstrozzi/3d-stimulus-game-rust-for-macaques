// node tools/tests/test_polar_hr_sync.js
// Checks the H10 path of deploy_frontend/polar_hr.js without a browser: a
// simulated H10 (clock drifting 25 ppm, accelerometer jolts from space bar
// presses, ECG with breathing-driven RR, standard heart rate packets once a
// second) goes through a start sync, 80 trial logs and an end sync; the beat
// times written to the files are compared with the true beats, before and
// after the end sync re-times and re-sends them. Then the same with the
// sensor switched off and on mid-session (its clock restarts).
const assert = require("assert");
const P = require("../../deploy_frontend/polar_hr.js");
const D = require("../../deploy_frontend/polar_pmd.js");

let now = 0;
Object.defineProperty(globalThis, "performance", { value: { now: () => now }, configurable: true });

function rng(seed) {
  let s = seed >>> 0;
  return () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 2 ** 32;
}
const u16 = (v) => [v & 255, (v >> 8) & 255];
const i24 = (v) => {
  v = v < 0 ? v + 2 ** 24 : v;
  return [v & 255, (v >> 8) & 255, (v >> 16) & 255];
};
function u64(big) {
  const out = [];
  for (let i = 0; i < 8; i++, big >>= 8n) out.push(Number(big & 255n));
  return out;
}

const PAGE = 100000; // laptop ms at true time 0
const KEY_BIAS = 8; // keydown timestamps come this long after the impact
const DRIFT = 25e-6;
const CLOCK_ZERO = PAGE + 30000; // the game starts at true time 30 s

function run({ seed, resetAt = null }) {
  const r = rng(seed);
  const gauss = () => Math.sqrt(-2 * Math.log(r() + 1e-12)) * Math.cos(2 * Math.PI * r());
  const T_END = 400000;
  // Device clock: ns since 2000; after a reset it starts again from the 2019 default.
  const devNs = (T) => (resetAt !== null && T >= resetAt ? 599616000000000000n : 724060800000000000n) + BigInt(Math.round(T * (1 + DRIFT) * 1e6));

  const beats = [];
  for (let t = 700, k = 0; t < T_END; k++) {
    beats.push(t);
    t += 900 + 70 * Math.sin((2 * Math.PI * k) / 4.3) + 20 * gauss();
  }
  const startKeys = Array.from({ length: 10 }, (_, k) => 20000 + 1000 * k + 200 * r());
  const endKeys = Array.from({ length: 10 }, (_, k) => 370000 + 1000 * k + 200 * r());
  const impacts = [...startKeys, ...endKeys];
  const g = (d, s) => Math.exp(-0.5 * (d / s) ** 2);
  const ecg = (T) => {
    let v = 150 * Math.sin((2 * Math.PI * T) / 3700) + 12 * gauss();
    for (const b of beats) {
      const d = T - b;
      if (d > -300 && d < 600) v += 90 * g(d + 160, 20) - 120 * g(d + 25, 8) + 1300 * g(d, 7) - 300 * g(d - 25, 8) + 280 * g(d - 260, 40);
    }
    return Math.round(v);
  };
  const acc = (T, period) => {
    let e = 0;
    for (const tau of impacts) {
      if (T - tau < -period || T - tau > 200) continue;
      for (let k = 0; k < 8; k++) {
        const d = T - tau - (k * period) / 8;
        if (d >= 0) e += (1500 * (1 - Math.exp(-d / 2)) * Math.exp(-d / 15) * Math.cos((2 * Math.PI * 25 * d) / 1000)) / 8;
      }
    }
    return [20 + 0.3 * e, -30, 1000 + e].map((v) => Math.round(v + 5 * gauss()));
  };

  // Everything that happens, in laptop time.
  const ev = [];
  const stream = (mt, rateHz, perFrame, sample, pack) => {
    const period = 1000 / rateHz / (1 + DRIFT);
    let buf = [];
    for (let i = 0; i * period < T_END; i++) {
      const T = i * period;
      if (resetAt !== null && T >= resetAt - 2000 && T < resetAt + 3000) {
        buf = [];
        continue; // switched off: nothing is sent
      }
      buf.push(sample(T, period));
      if (buf.length === perFrame) {
        const bytes = [mt, ...u64(devNs(T)), mt === D.MEAS.ACC ? 0x01 : 0x00];
        for (const s of buf) bytes.push(...pack(s));
        ev.push({ t: PAGE + T + 8 + 25 * r(), frame: Uint8Array.from(bytes) });
        buf = [];
      }
    }
  };
  stream(D.MEAS.ACC, 200, 36, acc, (s) => s.flatMap(u16));
  stream(D.MEAS.ECG, 130, 73, (T) => ecg(T), i24);
  let k = 1;
  let lastArrival = 0;
  for (let s = 300; s < T_END; s += 1000) {
    const rr = [];
    for (; k < beats.length && beats[k] <= s; k++) rr.push(Math.round((beats[k] - beats[k - 1]) * 1.024));
    if (resetAt !== null && s >= resetAt - 2000 && s < resetAt + 3000) continue;
    const bytes = [rr.length ? 0x10 : 0x00, 60, ...rr.flatMap(u16)];
    // Delivered in order, as on a Bluetooth link (late ones bunch up).
    lastArrival = Math.max(lastArrival + 1, PAGE + s + 20 + 1480 * r() ** 2);
    ev.push({ t: lastArrival, packet: new DataView(Uint8Array.from(bytes).buffer) });
  }
  if (resetAt !== null) {
    ev.push({ t: PAGE + resetAt - 1900, drop: true });
    ev.push({ t: PAGE + resetAt + 3100, reconnect: true });
  }
  ev.push({ t: PAGE + startKeys[9] + 1500, sync: "start", keys: startKeys.map((x) => PAGE + x + KEY_BIAS) });
  ev.push({ t: PAGE + endKeys[9] + 1500, sync: "end", keys: endKeys.map((x) => PAGE + x + KEY_BIAS) });
  for (let n = 0; n < 80; n++) ev.push({ t: PAGE + 40000 + 4000 * (n + 1), trial: n });
  ev.sort((a, b) => a.t - b.t);
  return { ev, beats };
}

async function session(sim) {
  const s = new P.PolarSession();
  s.device = { name: "Polar H10 TEST0001" };
  s._setUpSensor();
  const rec = s.pmd.rec;
  const up = () => {
    s._linkUp = true;
    s._run++;
    s.pmd.linkUp = true;
    s.pmd.active[D.MEAS.ACC] = { sel: { SAMPLE_RATE: 200, RANGE: 8 } };
    s._event("connected");
    for (const [mt, hz] of [[D.MEAS.ACC, 200], [D.MEAS.ECG, 130]]) {
      rec.streamStarting(mt, hz);
      rec.streamStarted(mt, 1);
    }
  };
  now = sim.ev[0].t - 1000;
  up();
  const sent = new Map();
  const results = {};
  for (const e of sim.ev) {
    now = e.t;
    if (e.frame) rec.ingestFrame(e.t, e.frame);
    else if (e.packet) s._hrPacket(e.packet);
    else if (e.drop) {
      s._linkUp = true;
      s._opening = true; // no background reconnection in this test
      s._linkLost();
      s._opening = false;
    } else if (e.reconnect) up();
    else if (e.sync) {
      results[e.sync] = await s.sync(e.sync, e.keys);
      if (s._resendRun) await s._resendRun;
    } else if (e.trial !== undefined) {
      s.useLogger((relpath, json) => sent.set(relpath, json), "tester/tester_session", CLOCK_ZERO);
      const block = s.takeTrialLog(CLOCK_ZERO, e.t - 4000, e.t);
      const log = { trial: e.trial, hr_info: block.hr_info, hr_meas: block.hr_meas, hr_ecg: block.hr_ecg, frames: { 0: { x: 1 } } };
      const relpath = `tester/tester_session/level_000/120000/trials/trial_${String(e.trial).padStart(3, "0")}.json`;
      const json = JSON.stringify(log, null, 2);
      sent.set(relpath, json);
      s.keepTrialFile(relpath, json, block);
    }
  }
  return { s, sent, results };
}

/** Beat-time errors (file time − true beat time, ms) over the trial files. */
function beatErrors(sent, beats, from = 0, to = Infinity) {
  const errs = [];
  let reported = 0;
  let timed = 0;
  for (const [path, json] of sent) {
    if (!path.includes("/trials/")) continue;
    const log = JSON.parse(json);
    if (log.trial < from || log.trial >= to) continue;
    reported += log.hr_info.hr_beats_reported;
    timed += log.hr_info.hr_beats_timed;
    for (const row of log.hr_meas) {
      if (row.beat_elapsed_secs === null) continue;
      const lap = CLOCK_ZERO + row.beat_elapsed_secs * 1000;
      const near = beats.reduce((b, x) => (Math.abs(PAGE + x - lap) < Math.abs(PAGE + b - lap) ? x : b));
      errs.push(lap - (PAGE + near));
    }
  }
  const mean = errs.reduce((a, b) => a + b, 0) / errs.length;
  return { errs, mean, max: Math.max(...errs.map(Math.abs)), reported, timed };
}

(async () => {
  // ── One clock all session ──
  {
    const sim = run({ seed: 7 });
    const { sent, results } = await session(sim);
    assert.ok(results.start.accepted, `start sync accepted (${results.start.reason})`);
    assert.ok(results.end.accepted, `end sync accepted (${results.end.reason})`);
    assert.ok(results.start.sd < 6, `start sync SD ${results.start.sd}`);

    const trials = [...sent.keys()].filter((p) => p.includes("/trials/"));
    assert.strictEqual(trials.length, 80);
    const logs = trials.map((p) => JSON.parse(sent.get(p)));
    for (const log of logs) {
      assert.deepStrictEqual(Object.keys(log), ["trial", "hr_info", "hr_meas", "hr_ecg", "frames"], "key order kept by the re-send");
      assert.deepStrictEqual(Object.keys(log.hr_meas[0] || { arrival_elapsed_secs: 0, beat_elapsed_secs: 0, hr: 0, rr_ms: 0, raw: 0 }),
        ["arrival_elapsed_secs", "beat_elapsed_secs", "hr", "rr_ms", "raw"]);
      assert.strictEqual(log.hr_info.sync_info.status, "start+end");
      assert.deepStrictEqual(log.hr_info.sync_info.problems, []);
      assert.strictEqual(log.hr_info.hr_beat_source, "ecg_r_peak");
      assert.strictEqual(log.hr_info.hr_beat_signal_hz, 130);
    }
    const info = logs[40].hr_info;
    assert.ok(Math.abs(info.sync_info.drift_ppm - 25) < 3, `drift ${info.sync_info.drift_ppm} ppm (true 25)`);
    assert.ok(info.sync_info.precision_ms > 0 && info.sync_info.precision_ms < 3, `precision ${info.sync_info.precision_ms} ms`);
    assert.deepStrictEqual(Object.keys(info), [
      "device", "firmware", "hr_beat_source", "hr_beat_signal_hz", "hr_beats_reported", "hr_beats_timed", "hr_rr_check_sd_ms",
      "trial_start_elapsed_secs", "trial_end_elapsed_secs", "units", "connection_events", "sync_info",
    ]);
    assert.deepStrictEqual(Object.keys(info.sync_info.start), ["elapsed_secs", "sensor", "acc_hz", "acc_range_g", "presses_used", "sd_ms"]);
    assert.strictEqual(info.sync_info.start.sensor, "h10");
    assert.ok(info.hr_rr_check_sd_ms < 3, `RR check ${info.hr_rr_check_sd_ms} ms`);

    // Beat times: the sync's constant offset (keyboard 8 ms, minus the few ms
    // by which the impact onset is picked late) is in every time; the rest is small.
    const b = beatErrors(sent, sim.beats);
    assert.ok(b.timed >= 0.9 * b.reported, `${b.timed} of ${b.reported} beats timed`);
    assert.ok(Math.abs(b.mean - KEY_BIAS) < 5, `mean beat-time error ${b.mean.toFixed(2)} ms (keyboard ${KEY_BIAS})`);
    const sd = Math.sqrt(b.errs.reduce((t, e) => t + (e - b.mean) ** 2, 0) / b.errs.length);
    assert.ok(sd < 2 && b.errs.every((e) => Math.abs(e - b.mean) < 6), `beat-time SD ${sd.toFixed(2)} ms around that offset`);

    // ECG frames: the first sample's time on the controller clock.
    const f = logs[10].hr_ecg[3];
    assert.strictEqual(f.uV.length, 73);
    assert.ok(Math.abs(f.dt_ms - 1000 / 130) < 0.01);

    // The per-session sync file: both syncs, presses and accelerometer.
    const syncFile = JSON.parse(sent.get("tester/tester_session/hr_sync.json"));
    assert.deepStrictEqual(syncFile.syncs.map((x) => [x.kind, x.accepted]), [["start", true], ["end", true]]);
    assert.strictEqual(syncFile.syncs[0].presses.length, 10);
    assert.ok(syncFile.syncs[0].acc.x.length > 2000);
    const used = syncFile.syncs[0].presses.filter((p) => p.used);
    assert.ok(used.every((p) => Math.abs(p.residual_ms) < 15));
    console.log(`one clock: beats ${b.timed}/${b.reported} timed, error ${b.mean.toFixed(2)} ms (SD ${sd.toFixed(2)} ms; 8 ms keyboard included); ` +
      `drift ${info.sync_info.drift_ppm} ppm, precision ${info.sync_info.precision_ms} ms, RR check ${info.hr_rr_check_sd_ms} ms`);
  }

  // ── Before the end sync the files say "start" ──
  {
    const sim = run({ seed: 8 });
    sim.ev = sim.ev.filter((e) => e.sync !== "end");
    // A press 200 ms after another is not measured (here: one after each real press).
    const st = sim.ev.find((e) => e.sync === "start");
    st.keys = st.keys.flatMap((k) => [k, k + 200]);
    const { sent, results } = await session(sim);
    assert.strictEqual(results.start.keys.length, 10, "fast presses dropped");
    assert.ok(results.start.accepted && results.start.used === 10);
    const log = JSON.parse(sent.get("tester/tester_session/level_000/120000/trials/trial_070.json"));
    assert.strictEqual(log.hr_info.sync_info.status, "start");
    // One sync: no drift correction, so ECG sample spacing on the controller clock = on the sensor clock.
    // (over the whole file: the elapsed seconds are float32, ~0.01 ms steps)
    const fr = log.hr_ecg;
    let devSpan = 0;
    for (let i = 1; i < fr.length; i++) devSpan += fr[i].dt_ms + fr[i - 1].dt_ms * (fr[i - 1].uV.length - 1);
    const lapSpan = (fr[fr.length - 1].first_elapsed_secs - fr[0].first_elapsed_secs) * 1000;
    assert.ok(devSpan > 2000 && Math.abs(lapSpan / devSpan - 1) < 8e-6, `no drift with one sync (${((lapSpan / devSpan - 1) * 1e6).toFixed(1)} ppm; the clock drifts 25)`);
    assert.strictEqual(log.hr_info.sync_info.end, null);
    assert.strictEqual(log.hr_info.sync_info.precision_ms, null);
    assert.ok(log.hr_info.sync_info.problems[0].startsWith("no end sync"));
    assert.ok(log.hr_meas.some((r) => r.beat_elapsed_secs !== null), "start-only files still have beat times");
  }

  // ── No sync at all: no times, but the rows are all there ──
  {
    const sim = run({ seed: 9 });
    sim.ev = sim.ev.filter((e) => !e.sync);
    const { sent } = await session(sim);
    const log = JSON.parse(sent.get("tester/tester_session/level_000/120000/trials/trial_010.json"));
    assert.strictEqual(log.hr_info.sync_info.status, null);
    assert.ok(log.hr_meas.length >= 3 && log.hr_meas.every((r) => r.beat_elapsed_secs === null));
    assert.ok(log.hr_ecg.every((f) => f.first_elapsed_secs === null));
    assert.strictEqual(sent.has("tester/tester_session/hr_sync.json"), false);
  }

  // ── The sensor is switched off mid-session: its clock restarts ──
  {
    const sim = run({ seed: 10, resetAt: 200000 });
    const { sent, results } = await session(sim);
    assert.ok(results.start.accepted && results.end.accepted);
    const at = (n) => JSON.parse(sent.get(`tester/tester_session/level_000/120000/trials/trial_${String(n).padStart(3, "0")}.json`));
    // Before the restart only the start sync applies, after it only the end sync.
    assert.strictEqual(at(20).hr_info.sync_info.status, "start");
    assert.strictEqual(at(60).hr_info.sync_info.status, "end");
    assert.ok(at(60).hr_info.sync_info.problems[0].includes("restarted"));
    const before = beatErrors(sent, sim.beats, 0, 38);
    const after = beatErrors(sent, sim.beats, 45, 80);
    assert.ok(before.timed > 0.9 * before.reported && after.timed > 0.9 * after.reported, "beats timed on both sides");
    assert.ok(Math.abs(before.mean - KEY_BIAS) < 6 && Math.abs(after.mean - KEY_BIAS) < 6,
      `beat times on both sides of the restart (${before.mean.toFixed(1)}, ${after.mean.toFixed(1)} ms)`);
    const events = [...sent.entries()].filter(([p]) => p.includes("/trials/")).flatMap(([, j]) => JSON.parse(j).hr_info.connection_events.map((e) => e.event));
    assert.deepStrictEqual(events, ["connected", "disconnected", "connected"]);
    console.log(`clock restart: before ${before.timed}/${before.reported} beats, error ${before.mean.toFixed(1)} ms; after ${after.timed}/${after.reported}, ${after.mean.toFixed(1)} ms`);
  }

  console.log("polar_hr.js sync: all tests passed");
  process.exit(0);  // the background beat-matching timer would keep Node alive
})().catch((err) => {
  console.error(err);
  process.exit(1);
});
