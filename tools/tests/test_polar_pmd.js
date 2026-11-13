// node tools/tests/test_polar_pmd.js
// Checks deploy_frontend/polar_pmd.js: the PMD protocol against the test
// vectors of the official Polar BLE SDK (Android: PmdSettingTest,
// BlePmdClientControlPointResponseTest, AccDataTest, EcgDataTest,
// PpgDataTest, PpiDataTest), and the clock-sync estimates against simulated
// sensors whose clock offset, drift and Bluetooth delays are known.
const assert = require("assert");
const P = require("../../deploy_frontend/polar_pmd.js");

const hex = (s) => P.fromHex(s);
const near = (a, b, tol, what) => assert.ok(Math.abs(a - b) <= tol, `${what}: ${a} vs ${b} (±${tol})`);

// ── Settings and commands ────────────────────────────────────────────────
// PmdSettingTest.testPmdSettingsWithRange
{
  const s = P.parseSettings(hex("00 01 34 00 01 01 10 00 02 04 F5 00 F4 01 E8 03 D0 07 04 01 03"));
  assert.deepStrictEqual(s, { SAMPLE_RATE: [52], RESOLUTION: [16], RANGE: [245, 500, 1000, 2000], CHANNELS: [3] });
}
// PmdSettingTest.testPmdSettingWithRangeMilliUnit: 4-byte values are signed.
assert.deepStrictEqual(P.parseSettings(hex("03 02 FF FF FF FF FF 00 00 00 01 01 0E 00")), { RANGE_MILLIUNIT: [-1, 255], RESOLUTION: [14] });
// A start response carrying the scale factor (float32 1.0 = 00 00 80 3F).
assert.deepStrictEqual(P.parseSettings(hex("05 01 00 00 80 3F")), { FACTOR: [1] });
assert.deepStrictEqual(P.parseSettings(hex("00")), {});

// The start requests widely used for the H10 (ACC 200 Hz ±8 g, ECG 130 Hz).
assert.strictEqual(
  P.toHex(P.startCommand(P.MEAS.ACC, { RANGE: 8, SAMPLE_RATE: 200, RESOLUTION: 16 })),
  "02 02 00 01 c8 00 01 01 10 00 02 01 08 00"
);
assert.strictEqual(P.toHex(P.startCommand(P.MEAS.ECG, { SAMPLE_RATE: 130, RESOLUTION: 14 })), "02 00 00 01 82 00 01 01 0e 00");
// FACTOR is never sent; PPI starts with no settings.
assert.strictEqual(P.toHex(P.startCommand(P.MEAS.PPG, { SAMPLE_RATE: 55, FACTOR: 1.5 })), "02 01 00 01 37 00");
assert.strictEqual(P.toHex(P.startCommand(P.MEAS.PPI, {})), "02 03");

// BlePmdClientControlPointResponseTest: success with parameters.
{
  const r = P.parseCpResponse(hex("F0 01 02 00 00 FF FF FF"));
  assert.strictEqual(r.kind, "response");
  assert.strictEqual(r.op, 1);
  assert.strictEqual(r.measType, 2);
  assert.strictEqual(r.statusName, "SUCCESS");
  assert.strictEqual(r.more, false);
  assert.strictEqual(P.toHex(r.params), "ff ff ff");
}
// An error carries no parameters; "more" announces a continuation.
assert.strictEqual(P.parseCpResponse(hex("F0 02 06 05")).statusName, "INVALID_PARAMETER");
assert.strictEqual(P.parseCpResponse(hex("F0 02 02 06 00")).statusName, "ALREADY_IN_STATE");
assert.strictEqual(P.parseCpResponse(hex("F0 01 02 00 01 00 01 34 00")).more, true);
// The device stopping measurements by itself.
assert.deepStrictEqual(P.parseCpResponse(hex("01 00 02")), { kind: "stopped", types: ["ECG", "ACC"] });

// Feature read: byte 1/2 bit masks (H10 = ECG + ACC).
assert.deepStrictEqual(P.parseFeatures(hex("0F 05 00")), ["ECG", "ACC"]);
assert.deepStrictEqual(P.parseFeatures(hex("0F 2E 02")), ["PPG", "ACC", "PPI", "GYRO", "SDK_MODE"]);

// ── Data frames ──────────────────────────────────────────────────────────
// AccDataTest: raw type 1 (16-bit), frame timestamp 2000000000 ns = last sample.
{
  const head = "02 00 94 35 77 00 00 00 00 01";
  const body = "F7 FF FF FF E7 03 F8 FF FE FF E5 03";
  const h = P.parseFrameHeader(hex(`${head} ${body}`));
  assert.strictEqual(h.measType, P.MEAS.ACC);
  assert.strictEqual(h.ts, 2000000000n);
  assert.strictEqual(h.frameType, 1);
  assert.strictEqual(h.compressed, false);
  assert.deepStrictEqual(P.decodeFrame(h, 1).ch, [[-9, -8], [-1, -2], [999, 997]]);
}
// AccDataTest: compressed type 1 — reference (−15, 20, 1008), one 6-bit delta (−5, −3, 0).
{
  const h = P.parseFrameHeader(hex("02 65 00 00 00 00 00 00 00 81 F1 FF 14 00 F0 03 06 01 7B 0F 08"));
  assert.strictEqual(h.compressed, true);
  assert.deepStrictEqual(P.decodeFrame(h, 1).ch, [[-15, -20], [20, 17], [1008, 1008]]);
}
// EcgDataTest: raw type 0, 24-bit signed µV.
assert.deepStrictEqual(P.decodeFrame(P.parseFrameHeader(hex("00 00 94 35 77 00 00 00 00 00 02 80 FF 02 80 00")), 1).ch, [[-32766, 32770]]);
// PpgDataTest: compressed type 0 — 24-bit reference of 4 channels, then 41 deltas of 10 bits.
{
  const body =
    "2C 2D 00 C2 77 00 D3 D2 FF 3D 88 FF 0A 29 B2 F0 EE 34 11 B2 EC EE 74 11 B1 E8 FE B4 11 B1 E8 FE B4 11 B1 E0 FE 34 12 B0 DC 0E 75 12 B0 D8 0E B5 12 AF D4 1E F5 12 AF D0 1E 35 13 AE CC 2E 75 13 AE C8 2E B5 13 AD C4 3E F5 13 AD BC 3E 75 14 AD BC 3E 75 14 AC B8 4E B5 14 AC B4 4E F5 14 AB B0 5E 35 15 AA AC 6E 75 15 AA A8 6E B5 15 AA A4 6E F5 15 A9 A0 7E 35 16 A9 9C 7E 75 16 A8 98 8E B5 16 A7 94 9E F5 16 A7 90 9E 35 17 A7 8C 9E 75 17 A6 88 AE B5 17 A5 88 BE B5 17 A5 80 BE 35 18 A4 7C CE 75 18 A4 78 CE B5 18 A3 78 DE B5 18 A2 70 EE 35 19 A2 6C EE 75 19 A2 6C EE 75 19 A1 68 FE B5 19 A0 60 0E 36 1A 9F 60 1E 36 1A 9F 5C 1E 76 1A 9F 58 1E B6 1A 9D 54 3E F6 1A";
  const d = P.decodeFrame(P.parseFrameHeader(hex(`01 00 94 35 77 00 00 00 00 80 ${body}`)), 1);
  assert.strictEqual(d.n, 42);
  assert.deepStrictEqual(d.ch.map((c) => c[0]), [11564, 30658, -11565, -30659]);
  assert.deepStrictEqual(d.ch.map((c) => c[1]), [11564 + 178, 30658 - 68, -11565 - 178, -30659 + 68]);
}
// PpiDataTest: 6-byte samples (hr, pp, error, flags).
assert.deepStrictEqual(P.decodeFrame(P.parseFrameHeader(hex("03 00 00 00 00 00 00 00 00 00 80 80 80 80 80 FF 00 01 00 01 00 00")), 1).ch, [
  [128, 0], [32896, 1], [32896, 1], [1, 0], [1, 0], [1, 0],
]);
// Frame types this page does not decode are reported, not misread.
assert.throws(() => P.decodeFrame(P.parseFrameHeader(hex("00 00 00 00 00 00 00 00 00 80 00")), 1), /not decoded/);
assert.throws(() => P.parseFrameHeader(hex("02 00 00")), /too short/);

// Sample times: spread since the previous frame, nominal rate for the first
// frame and across implausible spacing (a gap).
assert.deepStrictEqual(P.frameSampleTimes(null, 100, 3, 200), { first: 90, dt: 5, gap: false });
assert.deepStrictEqual(P.frameSampleTimes(100, 115.75, 3, 200), { first: 105.25, dt: 5.25, gap: false });
assert.deepStrictEqual(P.frameSampleTimes(100, 400, 3, 200), { first: 390, dt: 5, gap: true });

// Measured rate ignores a gap (a disconnection) in the buffer.
{
  const buf = new P.StreamBuffer(1);
  buf.append(0, 5, [Array(100).fill(0)]);
  buf.append(2000, 5, [Array(100).fill(0)]);
  near(P.measuredRate(buf), 200, 1e-9, "rate across a gap");
}

// ── Statistics ───────────────────────────────────────────────────────────
{
  const s = P.summarize([10, 11, 9, 10.5, 9.5, 80]);
  assert.strictEqual(s.used, 5);
  assert.deepStrictEqual(s.keep, [true, true, true, true, true, false]);
  assert.strictEqual(s.median, 10);
}

// ── Simulated sensors ────────────────────────────────────────────────────
// A sensor samples evenly on its own clock, which runs `driftPpm` fast;
// impacts are a sharp ringing jolt at known true times. Each frame arrives a
// delay floor plus up to 1 ms of jitter after its last sample, plus the wait
// for a radio slot: random in [0, ciMs), or — `locked` — until the next tick
// of a fixed connection-interval grid (frame period a multiple of it, so
// every frame waits the same time). Laptop time = true time + LAPTOP_OFFSET.
const LAPTOP_OFFSET = 123456.789;

function rng(seed) {
  let s = seed >>> 0;
  return () => (s = (Math.imul(s, 1664525) + 1013904223) >>> 0) / 2 ** 32;
}
const u16 = (v) => [v & 255, (v >> 8) & 255];
function u64(big) {
  const out = [];
  for (let i = 0; i < 8; i++, big >>= 8n) out.push(Number(big & 255n));
  return out;
}

function simulate({ rateHz, perFrame, driftPpm, t0ns, durationMs, impacts, ciMs, floorMs, seed, locked }) {
  const r = rng(seed);
  const gauss = () => Math.sqrt(-2 * Math.log(r() + 1e-12)) * Math.cos(2 * Math.PI * r());
  const devPeriod = 1000 / rateHz;
  const truePeriod = devPeriod / (1 + driftPpm * 1e-6);
  const phase = r() * ciMs;
  const frames = [];
  let buf = [];
  for (let i = 0; i * truePeriod < durationMs; i++) {
    const tt = i * truePeriod;
    // A limb jolt (a few-ms rise, ~15 ms decay, 25 Hz ringing), averaged over
    // the sample period as an accelerometer does before handing out a sample.
    let e = 0;
    for (const tau of impacts) {
      if (tt - tau < -truePeriod || tt - tau > 200) continue;
      for (let k = 0; k < 8; k++) {
        const d = tt - tau - (k * truePeriod) / 8;
        if (d >= 0) e += (1500 * (1 - Math.exp(-d / 2)) * Math.exp(-d / 15) * Math.cos((2 * Math.PI * 25 * d) / 1000)) / 8;
      }
    }
    const x = 20 + 0.3 * e;
    const y = -30;
    const z = 1000 + e;
    buf.push([x + 5 * gauss(), y + 5 * gauss(), z + 5 * gauss()].map(Math.round));
    if (buf.length === perFrame) {
      const ts = t0ns + BigInt(Math.round(i * devPeriod * 1e6));
      const arrival = locked
        ? phase + Math.ceil((tt + floorMs - phase) / ciMs) * ciMs + r()
        : tt + floorMs + r() * ciMs + r();
      const bytes = [0x02, ...u64(ts), 0x01];
      for (const s of buf) for (const v of s) bytes.push(...u16(v));
      frames.push({ t: arrival + LAPTOP_OFFSET, bytes: Uint8Array.from(bytes) });
      buf = [];
    }
  }
  return frames;
}

/** Recorder fed with a simulation, and the true mapping laptop = dev + c + s·dev. */
function record(sim) {
  const rec = new P.PmdRecorder();
  rec.streamStarting(P.MEAS.ACC, sim.rateHz);
  rec.streamStarted(P.MEAS.ACC, 1);
  for (const f of simulate(sim)) rec.ingestFrame(f.t, f.bytes);
  // dev ms counts from the first frame's last sample, (perFrame − 1) samples in.
  const d0 = ((sim.perFrame - 1) * 1000) / sim.rateHz;
  const k = 1 / (1 + sim.driftPpm * 1e-6);
  return { rec, truth: { c: d0 * k + LAPTOP_OFFSET, s: k - 1 } };
}

function keysFor(taus, biasMs, seed) {
  const r = rng(seed);
  return taus.map((tau) => tau + LAPTOP_OFFSET + biasMs + (r() - 0.5) * 2);
}

const KEY_BIAS = 8; // keyboard delay: the laptop sees each press 8 ms after the impact
const startTaus = Array.from({ length: 10 }, (_, k) => 5000 + 1000 * k + 37 * k);
const endTaus = Array.from({ length: 10 }, (_, k) => 580000 + 1000 * k + 41 * k);
const knockTaus = Array.from({ length: 5 }, (_, k) => 300000 + 1100 * k);
const common = { durationMs: 600000, impacts: [...startTaus, ...endTaus, ...knockTaus], ciMs: 30, floorMs: 3 };

const h10 = record({ ...common, rateHz: 200, perFrame: 36, driftPpm: 30, t0ns: 599616023171427290n, seed: 1 });
const vs = record({ ...common, rateHz: 52, perFrame: 24, driftPpm: -15, t0ns: 724060800000000000n, seed: 2 });

const h10Locked = record({ ...common, rateHz: 200, perFrame: 36, driftPpm: 30, t0ns: 599616023171427290n, seed: 7, locked: true });

for (const [name, sim, tol] of [["H10 200 Hz", h10, 3], ["Verity 52 Hz", vs, 10], ["H10 200 Hz, locked", h10Locked, 3]]) {
  const buf = sim.rec.streams[P.MEAS.ACC];
  const env = sim.rec.envelope(true);
  const locked = sim === h10Locked;
  assert.strictEqual(env.locked, locked, `${name}: locked arrivals ${locked ? "" : "not "}reported (spread ${env.spread_ms} ms)`);
  if (!locked) {
    // Drift from arrival times alone, over 10 minutes.
    near(env.s * 1e6, sim.truth.s * 1e6, 2, `${name} envelope drift (ppm)`);
    // The envelope offset is late by the Bluetooth delay floor (3 ms) plus at most a few ms of the random wait.
    const envLate = env.c + env.s * 300000 - (sim.truth.c + sim.truth.s * 300000);
    assert.ok(envLate > 0 && envLate < 10, `${name} envelope is late by about the delay floor: ${envLate}`);
  } else {
    // Every frame waits the same time: the arrivals hide the drift.
    assert.ok(Math.abs(env.s - sim.truth.s) * 1e6 > 10, `${name}: envelope drift is off (${env.s * 1e6} ppm)`);
  }

  // Press blocks at the start and the end: offset = truth + keyboard bias.
  const blocks = [startTaus, endTaus].map((taus, b) => P.analyzePresses(keysFor(taus, KEY_BIAS, 10 + b), buf, env, { minMg: 80 }));
  for (const res of blocks) {
    assert.strictEqual(res.detected, 10, `${name}: all presses found`);
    near(res.o, sim.truth.c + sim.truth.s * res.x + KEY_BIAS, tol, `${name} press offset`);
  }
  // With two blocks 575 s apart the mapping takes its drift from them.
  const m = P.chooseMapping(blocks.map((b) => ({ o: b.o, x: b.x })), env);
  assert.strictEqual(m.drift_from, "blocks");
  near(m.s * 1e6, sim.truth.s * 1e6, (2 * tol * 1e6) / 575000, `${name} press-block drift (ppm)`);
  // With one block: its offset, the envelope's drift.
  const m1 = P.chooseMapping([{ o: blocks[0].o, x: blocks[0].x }], env);
  assert.deepStrictEqual([m1.offset_from, m1.drift_from], ["blocks", "arrivals"]);
  near(P.toLaptop(m1, blocks[0].x), blocks[0].x + blocks[0].o, 1e-6, `${name} single-block mapping passes through the block`);
  sim.cal = m;
}
assert.deepStrictEqual(P.chooseMapping([], { c: 5, s: 1e-6 }), { c: 5, s: 1e-6, offset_from: "arrivals", drift_from: "arrivals" });

// Knock block: the Verity's offset through the H10's press calibration.
{
  const k = P.analyzeKnocks(
    { buf: h10.rec.streams[P.MEAS.ACC], mapping: h10.cal },
    { buf: vs.rec.streams[P.MEAS.ACC], mapping: vs.rec.envelope(true) },
    knockTaus[0] + LAPTOP_OFFSET - 1000,
    knockTaus[knockTaus.length - 1] + LAPTOP_OFFSET + 1000,
    { minMg: 80 }
  );
  // At 52 Hz a jolt can fall between samples and stay under the threshold.
  assert.ok(k.pairs.length >= 4, `knocks paired: ${k.pairs.length} of 5`);
  near(k.o, vs.truth.c + vs.truth.s * k.x + KEY_BIAS, 12, "Verity offset via knocks");
}

// Without impacts nothing is detected.
{
  const quiet = record({ ...common, impacts: [], rateHz: 200, perFrame: 36, driftPpm: 0, t0ns: 1000000000n, seed: 3 });
  const res = P.analyzePresses(keysFor(startTaus, 0, 4), quiet.rec.streams[P.MEAS.ACC], quiet.rec.envelope(true), { minMg: 80 });
  assert.strictEqual(res.detected, 0);
  assert.strictEqual(res.o, null);
}

// A device clock reset (H10 back to its default time) starts a new segment.
{
  const sim = { ...common, impacts: [], durationMs: 60000, rateHz: 200, perFrame: 36, driftPpm: 0, seed: 5 };
  const a = simulate({ ...sim, t0ns: 724060800000000000n });
  const b = simulate({ ...sim, t0ns: 599616000000000000n, seed: 6 }).map((f) => ({ ...f, t: f.t + 70000 }));
  const rec = new P.PmdRecorder();
  rec.streamStarted(P.MEAS.ACC, 1);
  for (const f of [...a, ...b]) rec.ingestFrame(f.t, f.bytes);
  assert.strictEqual(rec.segment, 1);
  assert.strictEqual(rec.events.filter((e) => e.event === "clock_jump").length, 1);
  const env = rec.envelope(true);
  near(env.s, 0, 5e-6, "drift after the reset");
}

// Frames of a stream whose start response has not arrived wait for its factor.
{
  const rec = new P.PmdRecorder();
  rec.streamStarting(P.MEAS.ACC, 52);
  assert.strictEqual(rec.ingestFrame(1000, hex("02 65 00 00 00 00 00 00 00 81 F1 FF 14 00 F0 03 06 01 7B 0F 08")), null);
  rec.streamStarted(P.MEAS.ACC, 2);
  assert.deepStrictEqual(rec.frames[0].ch, [[-30, -40], [40, 34], [2016, 2016]]);
  // The JSONL record has no BigInt and keeps the raw bytes.
  const line = JSON.parse(JSON.stringify(P.frameRecord("h10", rec.frames[0], null)));
  assert.strictEqual(line.ts, "101");
  assert.strictEqual(line.hex.slice(0, 2), "02");
}

// ── Heartbeats: R-peaks in a synthetic H10 ECG, matched to the band's RR ──
// Beats with breathing-driven and random RR changes (or none at all), an ECG
// with P, Q, R, S, T waves, baseline wander and noise at 130 Hz on the device
// clock, a 3 s hole (a disconnection), and standard heart rate packets sent
// once a second with the RR of the beats since the previous one (1/1024 s
// steps), arriving 20–1500 ms later, in order. Laptop = dev + 5000 ms + 30 ppm.
function heartSim({ seed, regular, durationMs = 120000, spikes = [] }) {
  const r = rng(seed);
  const gauss = () => Math.sqrt(-2 * Math.log(r() + 1e-12)) * Math.cos(2 * Math.PI * r());
  const beats = [];
  for (let t = 600, k = 0; t < durationMs - 600; k++) {
    beats.push(t);
    t += regular ? 857 : 950 + 60 * Math.sin((2 * Math.PI * k) / 4.5) + 15 * gauss();
  }
  const g = (d, s) => Math.exp(-0.5 * (d / s) ** 2);
  const wave = (tt) => {
    let v = 200 * Math.sin((2 * Math.PI * tt) / 3300) + 15 * gauss();
    // Motion spikes (a sync's presses with the sensor in the hand): far larger than the R-peaks.
    for (const sp of spikes) {
      const d = tt - sp;
      if (d >= 0 && d < 200) v += 5000 * (1 - Math.exp(-d / 2)) * Math.exp(-d / 15) * Math.cos((2 * Math.PI * 25 * d) / 1000);
    }
    for (const b of beats) {
      const d = tt - b;
      if (d < -300 || d > 600) continue;
      v += 100 * g(d + 160, 20) - 100 * g(d + 25, 8) + 1200 * g(d, 7) - 250 * g(d - 25, 8) + 300 * g(d - 250, 40);
    }
    return v;
  };
  const hole = [50000, 53000];
  const buf = new P.StreamBuffer(1);
  for (let i = 0; i * (1000 / 130) < durationMs; i++) {
    const tt = i * (1000 / 130);
    if (tt >= hole[0] && tt < hole[1]) continue;
    buf.t.push(tt);
    buf.ch[0].push(Math.round(wave(tt)));
  }
  const toLap = (d) => d + 5000 + 30e-6 * d;
  const packets = [];
  let k = 1;
  for (let s = 300; s < durationMs; s += 1000) {
    const rr = [];
    for (; k < beats.length && beats[k] <= s; k++) rr.push(Math.round((beats[k] - beats[k - 1]) * 1.024) / 1.024);
    if (s >= hole[0] && s < hole[1] + 1500) continue; // the link is down: these packets are lost
    // Delivered in order, as on a Bluetooth link (late ones bunch up).
    const arrival = Math.max(packets.length ? packets[packets.length - 1].arrival + 1 : 0, toLap(s) + 20 + 1480 * r() ** 2);
    packets.push({ arrival, rr, run: s < hole[0] ? 0 : 1, lastBeat: beats[k - 1] });
  }
  return { beats, buf, toLap, packets };
}

for (const regular of [false, true]) {
  const sim = heartSim({ seed: regular ? 21 : 22, regular });
  const peaks = P.detectRPeaks(sim.buf, 0, 120000, 130);
  // Every beat with samples around it is found once, to within 3 ms.
  const visible = sim.beats.filter((b) => b > 300 && b < 119700 && (b < 49700 || b > 53300));
  let worst = 0;
  for (const b of visible) {
    const near1 = peaks.filter((p) => Math.abs(p.t - b) < 100);
    assert.strictEqual(near1.length, 1, `one peak at the beat at ${b.toFixed(0)} ms`);
    worst = Math.max(worst, Math.abs(near1[0].t - b));
  }
  assert.ok(worst < 3, `R-peak timing within 3 ms (worst ${worst.toFixed(2)})`);
  assert.ok(peaks.length <= visible.length + 2, `no extra peaks (${peaks.length} for ${visible.length} beats)`);

  // The band's beats in order, then matched: right peak or none, never a neighbour.
  const beats = [];
  for (const p of sim.packets) {
    p.rr.forEach((rr, i) => beats.push({ rr, arrival: p.arrival, run: p.run, truth: i === p.rr.length - 1 ? p.lastBeat : null }));
  }
  // Truth for the earlier RR of a two-beat packet: the beat before the last.
  for (let i = 0; i < beats.length; i++) {
    if (beats[i].truth === null) beats[i].truth = beats[i + 1].truth - beats[i + 1].rr;
  }
  for (const [label, toLap] of [["synced", sim.toLap], ["30 ms late", (d) => sim.toLap(d) + 30]]) {
    const m = P.matchBeats(beats, peaks, toLap);
    let matched = 0;
    for (let i = 0; i < beats.length; i++) {
      if (!m[i]) continue;
      matched++;
      assert.ok(Math.abs(peaks[m[i].j].t - beats[i].truth) < 5, `${label}${regular ? " regular" : ""}: beat ${i} matched to its own R-peak`);
      assert.ok(Math.abs(m[i].diff) < 6, `RR check ${m[i].diff.toFixed(2)} ms`);
    }
    assert.ok(matched >= 0.9 * beats.length, `${label}${regular ? " regular" : ""}: ${matched} of ${beats.length} beats matched`);
  }
}

// Ten motion spikes 4× the R-peaks (a sync with the sensor in the hand) only
// cost the beats right at them, and never give a beat another one's peak.
{
  const spikes = Array.from({ length: 10 }, (_, k) => 20000 + 1000 * k + 137 * k);
  const sim = heartSim({ seed: 23, regular: false, spikes });
  const peaks = P.detectRPeaks(sim.buf, 0, 120000, 130);
  const clear = sim.beats.filter((b) => b > 300 && b < 119700 && (b < 49700 || b > 53300) && spikes.every((sp) => Math.abs(b - sp) > 600));
  const found = clear.filter((b) => peaks.some((p) => Math.abs(p.t - b) < 3));
  assert.strictEqual(found.length, clear.length, `beats away from the spikes found: ${found.length} of ${clear.length}`);
  const beats = [];
  for (const p of sim.packets) p.rr.forEach((rr, i) => beats.push({ rr, arrival: p.arrival, run: p.run, truth: i === p.rr.length - 1 ? p.lastBeat : null }));
  for (let i = beats.length - 1; i >= 0; i--) if (beats[i].truth === null) beats[i].truth = beats[i + 1].truth - beats[i + 1].rr;
  const m = P.matchBeats(beats, peaks, sim.toLap);
  m.forEach((r, i) => r && assert.ok(Math.abs(peaks[r.j].t - beats[i].truth) < 5, `beat ${i} matched to its own R-peak despite the spikes`));
  const near = beats.filter((b) => b.truth > 18000 && b.truth < 34000);
  const nearMatched = beats.filter((b, i) => m[i] && b.truth > 18000 && b.truth < 34000);
  // Beats at a spike are left untimed rather than risk another beat's peak.
  assert.ok(nearMatched.length >= 0.5 * near.length, `around the spikes ${nearMatched.length} of ${near.length} beats matched`);
}

// Settings picked from an H10 offer: fastest accelerometer, 8 g; ECG 130 Hz.
assert.deepStrictEqual(P.pickSettings(P.MEAS.ACC, { SAMPLE_RATE: [25, 50, 100, 200], RESOLUTION: [16], RANGE: [2, 4, 8] }), { SAMPLE_RATE: 200, RESOLUTION: 16, RANGE: 8 });
assert.deepStrictEqual(P.startCommand(P.MEAS.ACC, P.pickSettings(P.MEAS.ACC, { SAMPLE_RATE: [25, 50, 100, 200], RESOLUTION: [16], RANGE: [2, 4, 8] })),
  Array.from(hex("02 02 00 01 c8 00 01 01 10 00 02 01 08 00")));
assert.deepStrictEqual(P.startCommand(P.MEAS.ECG, P.pickSettings(P.MEAS.ECG, { SAMPLE_RATE: [130], RESOLUTION: [14] })), Array.from(hex("02 00 00 01 82 00 01 01 0e 00")));

console.log("polar_pmd.js: all tests passed");
