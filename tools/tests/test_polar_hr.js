// node tools/tests/test_polar_hr.js
// Checks deploy_frontend/polar_hr.js: the heart rate packet parser against
// packets recorded from a Polar H9 (and hand-built ones for the layouts the H9
// does not send), the per-trial log block, and the plot series.
const assert = require("assert");
const P = require("../../deploy_frontend/polar_hr.js");

function dv(hex) {
  return new DataView(Uint8Array.from(hex.split(" ").map((b) => parseInt(b, 16))).buffer);
}

// Recorded from the H9: one RR, two RRs, no RR.
assert.deepStrictEqual(P.parseHeartRate(dv("10 44 66 03")), { hr: 68, rr_ms: [849.609375] });
assert.deepStrictEqual(P.parseHeartRate(dv("10 46 1f 03 67 03")), { hr: 70, rr_ms: [780.2734375, 850.5859375] });
assert.deepStrictEqual(P.parseHeartRate(dv("00 40")), { hr: 64, rr_ms: [] });

// 1024 raw units are exactly one second.
assert.deepStrictEqual(P.parseHeartRate(dv("10 3c 00 04")).rr_ms, [1000]);

// 16-bit heart rate (flag bit 0) and an energy-expended field (flag bit 3) shift the RR values.
assert.deepStrictEqual(P.parseHeartRate(dv("11 2c 01 00 04")), { hr: 300, rr_ms: [1000] });
assert.deepStrictEqual(P.parseHeartRate(dv("18 48 aa bb 00 02")), { hr: 72, rr_ms: [500] });

// The bytes shown to the user are the bytes received.
assert.strictEqual(P.toHex(dv("10 3e f0 03")), "10 3e f0 03");
assert.strictEqual(
  P.describeHeartRate(dv("10 46 1f 03 67 03")),
  "flags=0x10 (hr u8, rr present) | hr=70 bpm | rr=799/1024 s=780.3 ms | rr=871/1024 s=850.6 ms"
);
assert.strictEqual(P.describeHeartRate(dv("00 40")), "flags=0x00 (hr u8, rr absent) | hr=64 bpm");

// ── Trial-log block ─────────────────────────────────────────────────────
// Controller clock zero at 2000 ms on the page timer; times are 32-bit floats
// like the game's present_elapsed_secs, and pre-start packets are negative.
{
  assert.strictEqual(P.elapsedSecs(53348.2, 2000), Math.fround(51.3482));
  assert.strictEqual(P.elapsedSecs(1500, 2000), -0.5);

  const packets = [
    { t_ms: 1500, hr: 60, rr_ms: [], raw: "00 3c" },
    { t_ms: 53348.2, hr: 62, rr_ms: [984.375], raw: "10 3e f0 03" },
  ];
  const events = [{ t_ms: 1000, event: "connected" }];
  const block = P.buildTrialLog("Polar H9 20FCA238", packets, events, 2000, 52000, 58000);
  assert.deepStrictEqual(block.hr_info, {
    device: "Polar H9 20FCA238",
    trial_start_elapsed_secs: 50,
    trial_end_elapsed_secs: 56,
    units: P.HR_UNITS,
    connection_events: [{ elapsed_secs: -1, event: "connected" }],
  });
  assert.deepStrictEqual(block.hr_meas, [
    { arrival_elapsed_secs: -0.5, hr: 60, rr_ms: [], raw: "00 3c" },
    { arrival_elapsed_secs: Math.fround(51.3482), hr: 62, rr_ms: [984.375], raw: "10 3e f0 03" },
  ]);
  // A trial that ends before it ever started playing has no start marker.
  assert.strictEqual(P.buildTrialLog(null, [], [], 0, null, 10).hr_info.trial_start_elapsed_secs, null);
}

// Consecutive trial logs hand out every packet and event exactly once.
{
  const s = new P.PolarSession();
  s.device = { name: "Polar H9 20FCA238" };
  const push = (t_ms) => {
    const row = { t_ms, hr: 60, rr_ms: [1000], raw: "10 3c 00 04" };
    s.hr.push(row);
    s.pending.push(row);
  };
  s._event("connected");
  push(1000);
  push(2000);
  const first = s.takeTrialLog(0, 500, 2500);
  push(3000);
  const second = s.takeTrialLog(0, 2600, 3500);
  assert.deepStrictEqual(first.hr_meas.map((p) => p.arrival_elapsed_secs), [1, 2]);
  assert.strictEqual(first.hr_info.connection_events.length, 1);
  assert.deepStrictEqual(second.hr_meas.map((p) => p.arrival_elapsed_secs), [3]);
  assert.strictEqual(second.hr_info.connection_events.length, 0);
  assert.strictEqual(s.takeTrialLog(0, 3600, 4000).hr_meas.length, 0);
  assert.strictEqual(s.hr.length, 3); // the display copy is untouched
  assert.strictEqual(s.isSupportedDevice, true);
  s.device = { name: "Polar H10 0A1B2C3D" };
  assert.strictEqual(s.isSupportedDevice, false);
}

// ── Plot series ─────────────────────────────────────────────────────────
{
  const hr = [
    { t_ms: 1000, hr: 60, rr_ms: [900, 1000] },
    { t_ms: 2000, hr: 61, rr_ms: [] },
    { t_ms: 9000, hr: 62, rr_ms: [950] },
  ];
  const { bpm, rr } = P.beatSeries(hr);
  assert.deepStrictEqual(bpm.map((q) => q.v), [60, 61, 62]);
  // Two beats in one packet: the earlier one is placed one RR before arrival.
  assert.deepStrictEqual(rr, [{ t: 0, v: 900 }, { t: 1000, v: 1000 }, { t: 9000, v: 950 }]);
  assert.deepStrictEqual(P.packetGaps(hr, []), [1000, 7000]);
  assert.deepStrictEqual(P.packetGaps(hr, [{ t_ms: 5000, event: "disconnected" }]), [1000]);
  const stats = P.trailingStats([{ t: 0, v: 10 }, { t: 5000, v: 20 }, { t: 12000, v: 40 }], 10000);
  assert.deepStrictEqual(stats.map((q) => q.mean), [10, 15, 30]);
}

assert.strictEqual(P.median([3, 1, 2]), 2);
assert.strictEqual(P.median([4, 1, 2, 3]), 2.5);

console.log("all polar_hr tests passed");
