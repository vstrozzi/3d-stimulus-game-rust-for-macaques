// Polar H10 / Verity Sense over Web Bluetooth: the Polar Measurement Data
// (PMD) protocol, a recorder that keeps every frame with its arrival time and
// its device timestamp, and the clock-sync estimates used by
// deploy_frontend/polar_sync_test.html.
//
// The protocol follows the official Polar BLE SDK (Android sources
// BlePMDClient.kt, PmdSetting.kt, PmdControlPointResponse.kt, model/*Data.kt,
// PmdTimeStampUtils.kt); tools/tests/test_polar_pmd.js replays the SDK's own
// test vectors against this file.
//
// Clocks. Every PMD frame carries the device's own timestamp: nanoseconds
// since 2000-01-01, the time of the frame's LAST sample. Here device times are
// kept as "dev ms", milliseconds since the device's first frame, and laptop
// times as performance.now() milliseconds ("laptop ms", the clock of
// KeyboardEvent.timeStamp). A mapping between the two is
//     laptop = dev + c + s * dev
// with c the offset and s the relative drift (s * 1e6 = ppm).
//   - Arrival times give a mapping for free: a frame arrives after its last
//     sample, never before, so the lowest points of (arrival - dev) lie on a
//     line whose slope is the drift ("envelope"). Its offset is late by the
//     smallest Bluetooth delay, which is unknown. Its drift is wrong when
//     every frame waits the same time for its radio slot (envelopeFit
//     reports this as `locked`); two press blocks then give the drift.
//   - Spacebar presses give the offset: each press is a keydown time on the
//     laptop and an impact in the accelerometer ("press block").
//   - Knocks seen by two sensors at once give the offset between them
//     ("knock block").
//
// Loaded as a plain <script> (exports on `window.PolarPmd`) and as a CommonJS
// module by the tests. Nothing above `class PmdDevice` needs a browser.
(function (root) {
  "use strict";

  // ───────────────────────────── protocol constants ─────────────────────────────

  const PMD_SERVICE = "fb005c80-02e7-f387-1cad-8acd2d8df0c8";
  const PMD_CP = "fb005c81-02e7-f387-1cad-8acd2d8df0c8";
  const PMD_DATA = "fb005c82-02e7-f387-1cad-8acd2d8df0c8";

  /** Measurement types: first byte of a data frame (low 6 bits). */
  const MEAS = {
    ECG: 0, PPG: 1, ACC: 2, PPI: 3, GYRO: 5, MAG: 6, SKIN_TEMP: 7, SDK_MODE: 9,
    LOCATION: 10, PRESSURE: 11, TEMPERATURE: 12, OFFLINE_RECORDING: 13, OFFLINE_HR: 14,
  };
  const MEAS_NAME = {};
  for (const [k, v] of Object.entries(MEAS)) MEAS_NAME[v] = k;

  /** Control point op codes, client → device. */
  const OP = { GET_SETTINGS: 1, START: 2, STOP: 3, GET_SDK_MODE_SETTINGS: 4, GET_STATUS: 5, GET_SDK_MODE_STATUS: 6 };

  /** Setting types in a settings list, and the size of one value in bytes. */
  const SETTING = {
    SAMPLE_RATE: 0, RESOLUTION: 1, RANGE: 2, RANGE_MILLIUNIT: 3, CHANNELS: 4, FACTOR: 5, SECURITY: 6,
    DERIVED_MEASUREMENT_METHOD: 7, SOURCE_MEASUREMENT_TYPE: 8, SOURCE_MEASUREMENT_SAMPLE_RATE: 9,
    SOURCE_MEASUREMENT_RANGE: 10, DERIVED_MEASUREMENT_TIME_WINDOW: 11, DERIVED_MEASUREMENT_SETTINGS_GROUP_ID: 12,
  };
  const SETTING_NAME = {};
  for (const [k, v] of Object.entries(SETTING)) SETTING_NAME[v] = k;
  const SETTING_SIZE = [2, 2, 2, 4, 1, 4, 16, 1, 1, 2, 4, 4, 1];

  const STATUS_NAME = [
    "SUCCESS", "INVALID_OP_CODE", "INVALID_MEASUREMENT_TYPE", "NOT_SUPPORTED", "INVALID_LENGTH",
    "INVALID_PARAMETER", "ALREADY_IN_STATE", "INVALID_RESOLUTION", "INVALID_SAMPLE_RATE", "INVALID_RANGE",
    "INVALID_MTU", "INVALID_NUMBER_OF_CHANNELS", "INVALID_STATE", "DEVICE_IN_CHARGER", "DISK_FULL",
    "INVALID_SOURCE_MEASUREMENT_TYPE", "INVALID_SOURCE_MEASUREMENT_RATE",
    "INVALID_DERIVED_MEASUREMENT_SETTINGS_GROUP", "INVALID_DERIVED_MEASUREMENT_METHOD",
  ];
  const STATUS_SUCCESS = 0;
  const STATUS_ALREADY_IN_STATE = 6;

  /** Unix time of the Polar epoch (2000-01-01T00:00:00Z), ns. */
  const POLAR_EPOCH_UNIX_NS = 946684800000000000n;

  /** Feature bits of the control point read: [byte, mask, type]. */
  const FEATURE_BITS = [
    [1, 0x01, "ECG"], [1, 0x02, "PPG"], [1, 0x04, "ACC"], [1, 0x08, "PPI"], [1, 0x20, "GYRO"],
    [1, 0x40, "MAG"], [1, 0x80, "SKIN_TEMP"], [2, 0x02, "SDK_MODE"], [2, 0x04, "LOCATION"],
    [2, 0x08, "PRESSURE"], [2, 0x10, "TEMPERATURE"], [2, 0x20, "OFFLINE_RECORDING"], [2, 0x40, "OFFLINE_HR"],
  ];

  // ───────────────────────────── bytes ─────────────────────────────

  function toHex(b) {
    let s = "";
    for (let i = 0; i < b.length; i++) s += (i ? " " : "") + b[i].toString(16).padStart(2, "0");
    return s;
  }

  function fromHex(s) {
    const t = s.replace(/[^0-9a-fA-F]/g, "");
    const out = new Uint8Array(t.length / 2);
    for (let i = 0; i < out.length; i++) out[i] = parseInt(t.substr(2 * i, 2), 16);
    return out;
  }

  /** Copy of a characteristic value (a DataView) as bytes. */
  function bytesOf(dv) {
    return new Uint8Array(dv.buffer.slice(dv.byteOffset, dv.byteOffset + dv.byteLength));
  }

  /** Little-endian unsigned integer of n ≤ 6 bytes. */
  function readUint(b, off, n) {
    let v = 0;
    for (let i = n - 1; i >= 0; i--) v = v * 256 + b[off + i];
    return v;
  }

  /** Two's complement of the low `bits` bits of a non-negative integer. */
  function signExtend(v, bits) {
    const full = 2 ** bits;
    v %= full;
    return v >= full / 2 ? v - full : v;
  }

  function readInt(b, off, n) {
    return signExtend(readUint(b, off, n), 8 * n);
  }

  function readUint64(b, off) {
    let v = 0n;
    for (let i = 7; i >= 0; i--) v = (v << 8n) | BigInt(b[off + i]);
    return v;
  }

  function readFloat32(b, off) {
    return new DataView(b.buffer, b.byteOffset + off, 4).getFloat32(0, true);
  }

  // ───────────────────────────── control point ─────────────────────────────

  /** Measurement types the device offers, from a read of the control point. */
  function parseFeatures(b) {
    return FEATURE_BITS.filter(([i, mask]) => b.length > i && b[i] & mask).map((f) => f[2]);
  }

  /** Settings list → { SAMPLE_RATE: [52, …], …, FACTOR: [float] }. */
  function parseSettings(b) {
    const out = {};
    let i = 0;
    if (b.length <= 1) return out;
    while (i < b.length) {
      if (i + 2 > b.length) throw new Error(`settings list truncated: ${toHex(b)}`);
      const type = b[i++];
      const count = b[i++];
      const size = SETTING_SIZE[type];
      if (size === undefined) throw new Error(`unknown setting type ${type} in ${toHex(b)}`);
      const vals = [];
      for (let k = 0; k < count; k++, i += size) {
        if (i + size > b.length) throw new Error(`settings list truncated: ${toHex(b)}`);
        if (type === SETTING.FACTOR) vals.push(readFloat32(b, i));
        else if (size <= 4) vals.push(readInt(b, i, size));
        else vals.push(toHex(b.subarray(i, i + size)));
      }
      out[SETTING_NAME[type]] = vals;
    }
    return out;
  }

  /** Chosen settings ({ SAMPLE_RATE: 200, … }) → bytes of a start request. FACTOR is response-only. */
  function serializeSettings(selected) {
    const out = [];
    const names = Object.keys(selected || {}).filter((k) => k in SETTING && k !== "FACTOR" && k !== "SECURITY");
    names.sort((a, b) => SETTING[a] - SETTING[b]);
    for (const name of names) {
      const type = SETTING[name];
      const size = SETTING_SIZE[type];
      let v = selected[name];
      if (v < 0) v += 2 ** (8 * size);
      out.push(type, 1);
      for (let k = 0; k < size; k++) out.push(Math.floor(v / 256 ** k) & 0xff);
    }
    return out;
  }

  function startCommand(measType, selected) {
    return [OP.START, measType, ...serializeSettings(selected)];
  }

  /**
   * A control point indication: a response to a command (first byte 0xF0),
   * or the device stopping measurements by itself (first byte 0x01).
   */
  function parseCpResponse(b) {
    if (b[0] === 0xf0 && b.length >= 4) {
      const status = b[3];
      const ok = status === STATUS_SUCCESS;
      return {
        kind: "response",
        op: b[1],
        measType: b[2],
        status,
        statusName: STATUS_NAME[status] || `ERROR_${status}`,
        more: ok && b.length > 4 && b[4] !== 0,
        params: ok && b.length > 5 ? b.slice(5) : new Uint8Array(0),
      };
    }
    if (b[0] === 0x01) return { kind: "stopped", types: Array.from(b.slice(1), (x) => MEAS_NAME[x & 0x3f] || String(x)) };
    return { kind: "unknown", hex: toHex(b) };
  }

  // ───────────────────────────── data frames ─────────────────────────────

  function parseFrameHeader(b) {
    if (b.length < 10) throw new Error(`PMD frame too short (${b.length} bytes)`);
    return {
      measType: b[0] & 0x3f,
      ts: readUint64(b, 1),
      frameType: b[9] & 0x7f,
      compressed: (b[9] & 0x80) !== 0,
      content: b.subarray(10),
    };
  }

  /**
   * Delta-compressed samples: a reference sample (`channels` values of
   * `resolution` bits, each in whole bytes), then blocks of
   * [delta bit width, sample count, packed deltas (LSB first)].
   * Returns one array of channel values per sample, reference included.
   */
  function parseDeltaFrames(content, channels, resolution) {
    const refBytes = Math.ceil(resolution / 8);
    let off = channels * refBytes;
    if (content.length < off) throw new Error("delta frame shorter than its reference sample");
    let prev = [];
    for (let c = 0; c < channels; c++) prev.push(signExtend(readUint(content, c * refBytes, refBytes), resolution));
    const rows = [prev];
    while (off < content.length) {
      if (off + 2 > content.length) throw new Error("delta frame truncated");
      const width = content[off++];
      const count = content[off++];
      const len = Math.ceil((width * count * channels) / 8);
      if (off + len > content.length) throw new Error("delta frame truncated");
      let bit = off * 8;
      for (let s = 0; s < count && width > 0; s++) {
        const next = new Array(channels);
        for (let c = 0; c < channels; c++) {
          let v = 0;
          for (let k = 0; k < width; k++, bit++) if ((content[bit >> 3] >> (bit & 7)) & 1) v += 2 ** k;
          next[c] = prev[c] + signExtend(v, width);
        }
        rows.push(next);
        prev = next;
      }
      off += len;
    }
    return rows;
  }

  /** Uncompressed samples of `channels` signed values of `size` bytes each. */
  function rawChannels(content, channels, size) {
    const step = channels * size;
    if (!content.length || content.length % step) throw new Error(`frame of ${content.length} bytes is not a multiple of ${step}`);
    const ch = Array.from({ length: channels }, () => []);
    for (let i = 0; i < content.length; i += step) {
      for (let c = 0; c < channels; c++) ch[c].push(readInt(content, i + c * size, size));
    }
    return ch;
  }

  /** Rows → channels, each value passed through `f`. */
  function columns(rows, channels, f) {
    const ch = Array.from({ length: channels }, () => new Array(rows.length));
    for (let i = 0; i < rows.length; i++) for (let c = 0; c < channels; c++) ch[c][i] = f(rows[i][c]);
    return ch;
  }

  const F = Math.fround;
  const STREAM_INFO = {
    [MEAS.ACC]: { channels: ["x", "y", "z"], unit: "mG" },
    [MEAS.ECG]: { channels: ["uV"], unit: "µV" },
    [MEAS.PPG]: { channels: ["ppg0", "ppg1", "ppg2", "ambient"], unit: "raw" },
    [MEAS.PPI]: { channels: ["hr", "pp_ms", "pp_error_ms", "blocker", "skin_contact", "skin_contact_supported"], unit: "" },
    [MEAS.GYRO]: { channels: ["x", "y", "z"], unit: "deg/s" },
  };

  /**
   * Values of one frame as channels ({ ch: [[…], …], n }), scaled as the SDK
   * does (Kotlin Float arithmetic, truncation to Int). Throws on frame types
   * this page does not decode; the raw bytes are kept either way.
   */
  function decodeFrame(h, factor) {
    const f = factor === undefined || factor === null ? 1 : factor;
    const c = h.content;
    let ch = null;
    if (h.measType === MEAS.ACC) {
      if (!h.compressed && h.frameType <= 2) ch = rawChannels(c, 3, h.frameType + 1);
      else if (h.compressed && h.frameType === 0) {
        const k = F(F(f) * 1000); // type 0 arrives in G
        ch = columns(parseDeltaFrames(c, 3, 16), 3, (v) => Math.trunc(F(v * k)));
      } else if (h.compressed && h.frameType === 1) {
        ch = columns(parseDeltaFrames(c, 3, 16), 3, f === 1 ? (v) => v : (v) => Math.trunc(F(v * F(f))));
      }
    } else if (h.measType === MEAS.ECG) {
      if (!h.compressed && h.frameType === 0) ch = rawChannels(c, 1, 3);
    } else if (h.measType === MEAS.PPG) {
      if (!h.compressed && h.frameType === 0) ch = rawChannels(c, 4, 3);
      else if (h.compressed && h.frameType === 0) ch = columns(parseDeltaFrames(c, 4, 24), 4, (v) => Math.trunc(F(v * F(f))));
    } else if (h.measType === MEAS.GYRO) {
      if (h.compressed && h.frameType === 0) ch = columns(parseDeltaFrames(c, 3, 16), 3, f === 1 ? (v) => v : (v) => F(v * F(f)));
    } else if (h.measType === MEAS.PPI) {
      if (!h.compressed && h.frameType === 0) {
        if (!c.length || c.length % 6) throw new Error(`PPI frame of ${c.length} bytes is not a multiple of 6`);
        ch = Array.from({ length: 6 }, () => []);
        for (let i = 0; i < c.length; i += 6) {
          ch[0].push(c[i]);
          ch[1].push(readUint(c, i + 1, 2));
          ch[2].push(readUint(c, i + 3, 2));
          ch[3].push(c[i + 5] & 0x01);
          ch[4].push((c[i + 5] & 0x02) >> 1);
          ch[5].push((c[i + 5] & 0x04) >> 2);
        }
      }
    }
    if (!ch) {
      const name = MEAS_NAME[h.measType] || `type ${h.measType}`;
      throw new Error(`${name} frame type ${h.frameType}${h.compressed ? " (compressed)" : ""} is not decoded by this page`);
    }
    return { ch, n: ch[0].length };
  }

  /**
   * Device times of the samples of one frame whose last sample is at
   * `lastMs`: spread evenly since the previous frame's last sample (as the
   * SDK does), or at the nominal rate for the first frame and whenever that
   * spacing is implausible (a lost frame, a restart) — reported as a gap.
   */
  function frameSampleTimes(prevMs, lastMs, n, rateHz) {
    const nominal = rateHz > 0 ? 1000 / rateHz : null;
    let dt = null;
    let gap = false;
    if (prevMs !== null && prevMs !== undefined) {
      if (lastMs > prevMs) {
        dt = (lastMs - prevMs) / n;
        if (nominal !== null && (dt > 1.5 * nominal || dt < 0.5 * nominal)) {
          dt = null;
          gap = true;
        }
      } else gap = true;
    }
    if (dt === null) dt = nominal === null ? 0 : nominal;
    return { first: lastMs - dt * (n - 1), dt, gap };
  }

  // ───────────────────────────── statistics ─────────────────────────────

  function median(a) {
    if (!a.length) return NaN;
    const s = Float64Array.from(a).sort();
    const m = s.length >> 1;
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  }

  /** 1.4826 × median absolute deviation: the SD for normal data, robust to outliers. */
  function robustSd(a) {
    if (a.length < 2) return 0;
    const m = median(a);
    return 1.4826 * median(a.map((v) => Math.abs(v - m)));
  }

  /**
   * Summary of repeated estimates of one quantity: values further than
   * max(minLimit, 5 robust SD) from the median are set aside as outliers.
   * `se` is the standard error of the median (≈ 1.25 SD / √n).
   */
  function summarize(values, minLimit = 15) {
    const n = values.length;
    if (!n) return { n: 0, used: 0, keep: [] };
    const m0 = median(values);
    const limit = Math.max(minLimit, 5 * robustSd(values));
    const keep = values.map((v) => Math.abs(v - m0) <= limit);
    const kept = values.filter((_, i) => keep[i]);
    const mean = kept.reduce((s, v) => s + v, 0) / kept.length;
    const sd = kept.length > 1 ? Math.sqrt(kept.reduce((s, v) => s + (v - mean) ** 2, 0) / (kept.length - 1)) : NaN;
    return {
      n,
      used: kept.length,
      keep,
      median: median(kept),
      mean,
      sd,
      robust_sd: robustSd(kept),
      se: (1.2533 * sd) / Math.sqrt(kept.length),
      min: Math.min(...kept),
      max: Math.max(...kept),
    };
  }

  /** Least squares y = a + b x (x centred for precision). */
  function linearFit(x, y) {
    const n = x.length;
    let mx = 0;
    let my = 0;
    for (let i = 0; i < n; i++) {
      mx += x[i];
      my += y[i];
    }
    mx /= n;
    my /= n;
    let sxx = 0;
    let sxy = 0;
    for (let i = 0; i < n; i++) {
      sxx += (x[i] - mx) ** 2;
      sxy += (x[i] - mx) * (y[i] - my);
    }
    const b = sxx > 0 ? sxy / sxx : 0;
    return { a: my - b * mx, b };
  }

  // ───────────────────────────── clock mapping ─────────────────────────────

  function toLaptop(m, dev) {
    return dev + m.c + m.s * dev;
  }

  function toDevice(m, laptop) {
    return (laptop - m.c) / (1 + m.s);
  }

  // Below this median spread of the delays within a window, the frames all
  // wait the same time for their radio slot (see envelopeFit).
  const LOCKED_SPREAD_MS = 2;

  /**
   * Lower envelope of (arrival − dev) against dev for the pairs [from, to):
   * the smallest value in each `windowMs` of device time, then a straight
   * line through those minima, refitted after dropping minima far ABOVE the
   * line (windows where no frame got through quickly). Returns the mapping
   * { c, s } (laptop = dev + c + s·dev) or null; with fewer than 3 windows the
   * drift is taken as 0.
   *
   * The minima reach the true delay floor only if frames meet the radio's
   * connection events at varied moments. When the frame period is a multiple
   * of the connection interval, every frame waits the same time, that wait
   * absorbs the drift until it wraps, and the slope comes out wrong
   * (typically near 0). `spread_ms` (median within-window spread of the
   * delays) shows it: `locked` = true means drift must come from press blocks.
   */
  function envelopeFit(arrival, dev, from = 0, to = arrival.length, windowMs = 5000) {
    if (to - from < 1) return null;
    const xs = [];
    const ys = [];
    const spreads = [];
    let start = null;
    let bx = 0;
    let by = Infinity;
    let win = [];
    const flush = () => {
      xs.push(bx);
      ys.push(by);
      if (win.length >= 3) spreads.push(median(win) - by);
    };
    for (let i = from; i < to; i++) {
      if (start === null || dev[i] >= start + windowMs || dev[i] < start) {
        if (start !== null) flush();
        start = dev[i];
        by = Infinity;
        win = [];
      }
      const y = arrival[i] - dev[i];
      win.push(y);
      if (y < by) {
        by = y;
        bx = dev[i];
      }
    }
    flush();
    const spread = spreads.length ? median(spreads) : null;
    const locked = spread !== null && spread < LOCKED_SPREAD_MS;
    if (xs.length < 3) {
      let k = 0;
      for (let i = 1; i < ys.length; i++) if (ys[i] < ys[k]) k = i;
      return {
        c: ys[k], s: 0, windows: xs.length, used: xs.length, rms_ms: 0,
        x_from: xs[0], x_to: xs[xs.length - 1], spread_ms: spread, locked,
      };
    }
    let keep = xs.map(() => true);
    let fit = null;
    for (let it = 0; it < 4; it++) {
      const kx = xs.filter((_, i) => keep[i]);
      const ky = ys.filter((_, i) => keep[i]);
      fit = linearFit(kx, ky);
      const res = ys.map((y, i) => y - (fit.a + fit.b * xs[i]));
      const scale = Math.max(0.5, robustSd(res.filter((_, i) => keep[i])));
      const next = res.map((r) => r <= 3 * scale);
      if (next.filter(Boolean).length < 3 || next.every((v, i) => v === keep[i])) break;
      keep = next;
    }
    const used = keep.filter(Boolean).length;
    const res = ys.map((y, i) => y - (fit.a + fit.b * xs[i])).filter((_, i) => keep[i]);
    return {
      c: fit.a,
      s: fit.b,
      windows: xs.length,
      used,
      rms_ms: Math.sqrt(res.reduce((t, r) => t + r * r, 0) / res.length),
      x_from: xs[0],
      x_to: xs[xs.length - 1],
      spread_ms: spread,
      locked,
    };
  }

  /** Below this span between calibrations, drift is taken from the arrival times instead. */
  const MIN_DRIFT_SPAN_MS = 60000;

  /**
   * The device's mapping from its calibrations ({ o, x }: offset o at dev
   * time x, from press or knock blocks of the current clock segment) and the
   * arrival envelope. Drift from the calibrations when they span at least a
   * minute (a line through them), otherwise from the envelope; the offset
   * from the latest calibration, otherwise from the envelope (late by the
   * Bluetooth delay floor).
   */
  function chooseMapping(cals, env, opts = {}) {
    if (cals.length >= 2) {
      const xs = cals.map((k) => k.x);
      if (Math.max(...xs) - Math.min(...xs) >= MIN_DRIFT_SPAN_MS) {
        const fit = linearFit(xs, cals.map((k) => k.o));
        return { c: fit.a, s: fit.b, offset_from: "blocks", drift_from: "blocks" };
      }
    }
    // `arrivalDrift: false`: without a drift from the blocks the clocks are
    // taken to run at the same rate (the offset the mean of the blocks'),
    // since arrival times can give a wildly wrong drift (jittery links).
    if (opts.arrivalDrift === false && cals.length) {
      return { c: cals.reduce((t, k) => t + k.o, 0) / cals.length, s: 0, offset_from: "blocks", drift_from: "none" };
    }
    if (!env) return null;
    if (cals.length) {
      const k = cals[cals.length - 1];
      return { c: k.o - env.s * k.x, s: env.s, offset_from: "blocks", drift_from: "arrivals" };
    }
    return { c: env.c, s: env.s, offset_from: "arrivals", drift_from: "arrivals" };
  }

  // ───────────────────────────── recorder ─────────────────────────────

  /** Samples of one stream on the device clock: t (dev ms) and one array per channel. */
  class StreamBuffer {
    constructor(channels) {
      this.t = [];
      this.ch = Array.from({ length: channels }, () => []);
    }

    get length() {
      return this.t.length;
    }

    append(first, dt, ch) {
      const n = ch[0].length;
      const last = this.t.length ? this.t[this.t.length - 1] : -Infinity;
      for (let i = 0; i < n; i++) {
        const t = first + dt * i;
        if (t <= last) continue; // overlap after a clock change: keep the older samples
        this.t.push(t);
        for (let k = 0; k < this.ch.length; k++) this.ch[k].push(ch[k][i]);
      }
    }
  }

  // A frame further off the envelope than this means the device clock jumped
  // (e.g. the H10 reset to its default time after a shutdown).
  const JUMP_BACK_MS = 500;
  const JUMP_AHEAD_MS = 2000;
  const JUMP_FRAMES = 8;

  /**
   * Everything received from one device: raw frames with arrival and device
   * times, decoded streams on the device clock, the (arrival, dev) pairs of
   * the envelope, events, and clock segments (a new segment starts when the
   * device clock jumps; mappings never cross segments).
   */
  class PmdRecorder {
    constructor() {
      this.t0 = null; // BigInt: device ns (since 2000) of the first timed frame = zero of dev ms
      this.ctx = {}; // measType -> { rateHz, factor, ready, prev: { frameType: lastMs }, queue }
      this.streams = {}; // measType -> StreamBuffer (current segment)
      this.frames = []; // { t, ts, mt, ft, bytes, n, ch, first, dt, err, seg }
      this.pairs = { a: [], d: [] }; // arrival laptop ms, dev ms of the frame's last sample
      this.segments = [{ from: 0, t: null }];
      this.events = []; // { t, event, detail }
      this.gaps = {}; // measType -> count
      this._recent = [];
      this._env = null;
    }

    get segment() {
      return this.segments.length - 1;
    }

    devMs(ts) {
      if (this.t0 === null) this.t0 = ts;
      return Number(ts - this.t0) / 1e6;
    }

    event(t, event, detail) {
      const e = { t, event, detail: detail || null };
      this.events.push(e);
      return e;
    }

    /** Before the start command: frames of this type wait until its factor is known. */
    streamStarting(mt, rateHz) {
      this.ctx[mt] = { rateHz: rateHz || 0, factor: 1, ready: false, prev: {}, queue: [] };
    }

    streamStarted(mt, factor) {
      const c = this.ctx[mt] || (this.ctx[mt] = { rateHz: 0, factor: 1, ready: false, prev: {}, queue: [] });
      c.factor = factor === undefined || factor === null ? 1 : factor;
      c.ready = true;
      for (const [t, b] of c.queue.splice(0)) this.ingestFrame(t, b);
    }

    /** The link is gone: the next frames start their streams afresh. */
    linkLost() {
      for (const c of Object.values(this.ctx)) {
        c.prev = {};
        c.ready = true;
        c.queue = [];
      }
      this._recent = [];
    }

    /** One notification of the PMD data characteristic, received at laptop ms `t`. */
    ingestFrame(t, bytes) {
      const h = parseFrameHeader(bytes);
      const c = this.ctx[h.measType] || (this.ctx[h.measType] = { rateHz: 0, factor: 1, ready: true, prev: {}, queue: [] });
      if (!c.ready) {
        c.queue.push([t, bytes]);
        return null;
      }
      const rec = {
        t, ts: h.ts, mt: h.measType, ft: h.frameType | (h.compressed ? 0x80 : 0),
        bytes, n: 0, ch: null, first: null, dt: null, err: null, seg: this.segment,
      };
      this.frames.push(rec);
      let dec;
      try {
        dec = decodeFrame(h, c.factor);
      } catch (err) {
        rec.err = err.message;
        return rec;
      }
      rec.n = dec.n;
      rec.ch = dec.ch;
      // PPI has no sample clock (its timestamp is 0 or the frame's); keep it untimed.
      if (h.measType === MEAS.PPI || h.ts === 0n) return rec;
      const lastMs = this.devMs(h.ts);
      if (this._checkJump(t, lastMs)) {
        for (const k of Object.values(this.ctx)) k.prev = {};
        rec.seg = this.segment;
      }
      const prev = c.prev[rec.ft];
      const st = frameSampleTimes(prev === undefined ? null : prev, lastMs, dec.n, c.rateHz);
      c.prev[rec.ft] = lastMs;
      if (st.gap) this.gaps[h.measType] = (this.gaps[h.measType] || 0) + 1;
      rec.first = st.first;
      rec.dt = st.dt;
      this.pairs.a.push(t);
      this.pairs.d.push(lastMs);
      const buf = this.streams[h.measType] || (this.streams[h.measType] = new StreamBuffer(dec.ch.length));
      buf.append(st.first, st.dt, dec.ch);
      return rec;
    }

    /** Envelope mapping of the current segment (recomputed every 50 frames, or on `fresh`). */
    envelope(fresh) {
      const n = this.pairs.a.length;
      const seg = this.segment;
      if (fresh || !this._env || this._env.seg !== seg || n - this._env.at >= 50) {
        this._env = { seg, at: n, fit: envelopeFit(this.pairs.a, this.pairs.d, this.segments[seg].from, n) };
      }
      return this._env.fit;
    }

    _checkJump(t, lastMs) {
      const env = this.envelope();
      if (!env || this.pairs.a.length - this.segments[this.segment].from < 20) return false;
      const r = t - lastMs - (env.c + env.s * lastMs);
      this._recent.push(r);
      if (this._recent.length > JUMP_FRAMES) this._recent.shift();
      const back = r < -JUMP_BACK_MS;
      const ahead = this._recent.length === JUMP_FRAMES && Math.min(...this._recent) > JUMP_AHEAD_MS;
      if (!back && !ahead) return false;
      const from = this.pairs.a.length - (ahead ? JUMP_FRAMES - 1 : 0);
      this.event(t, "clock_jump", { by_ms: back ? r : Math.min(...this._recent) });
      this.segments.push({ from: Math.max(from, this.segments[this.segment].from), t });
      this.streams = {};
      this._recent = [];
      this._env = null;
      return true;
    }
  }

  // ───────────────────────────── impacts ─────────────────────────────

  /** First index with t[i] >= x (t ascending). */
  function lowerBound(t, x) {
    let lo = 0;
    let hi = t.length;
    while (lo < hi) {
      const m = (lo + hi) >> 1;
      if (t[m] < x) lo = m + 1;
      else hi = m;
    }
    return lo;
  }

  /** Size of the change from sample i−1 to sample i (vector norm over channels). */
  function jerk(ch, i) {
    let s = 0;
    for (const c of ch) s += (c[i] - c[i - 1]) ** 2;
    return Math.sqrt(s);
  }

  /** Typical sample-to-sample change over [from, to) dev ms: median and robust SD. */
  function jerkNoise(buf, from, to) {
    const i0 = Math.max(1, lowerBound(buf.t, from));
    const i1 = lowerBound(buf.t, to);
    const j = [];
    for (let i = i0; i < i1; i++) j.push(jerk(buf.ch, i));
    if (j.length < 5) return null;
    return { med: median(j), sd: Math.max(1, robustSd(j)), n: j.length };
  }

  // Impact timing (dev ms around the largest change): the level before the
  // impact is the median of [−150, −30] ms; the deviation peak is looked for
  // in [−30, +40] ms; the onset is looked for back to −80 ms.
  const IMPACT = { baseFrom: 150, baseTo: 30, peakBefore: 30, peakAfter: 40, onsetBack: 80 };

  function baselineAt(buf, tp, p) {
    const { t, ch } = buf;
    let b0 = lowerBound(t, tp - IMPACT.baseFrom);
    let b1 = lowerBound(t, tp - IMPACT.baseTo);
    if (b1 - b0 < 2) {
      // very low rates: the few samples just before the change
      b1 = Math.max(0, p - 1);
      b0 = Math.max(0, b1 - 3);
    }
    if (b1 <= b0) return null;
    const level = ch.map((c) => median(c.slice(b0, b1)));
    const dev = (i) => {
      let s = 0;
      for (let k = 0; k < ch.length; k++) s += (ch[k][i] - level[k]) ** 2;
      return Math.sqrt(s);
    };
    const floorVals = [];
    for (let i = b0; i < b1; i++) floorVals.push(dev(i));
    return { b0, b1, level, dev, floor: median(floorVals) };
  }

  /**
   * Onset of the impact whose largest sample-to-sample change ends at index
   * p: where the deviation from the pre-impact level first reaches half of
   * its peak, interpolated between samples. Null without a baseline.
   */
  function impactOnset(buf, p) {
    const { t } = buf;
    const tp = t[p];
    const base = baselineAt(buf, tp, p);
    if (!base) return null;
    const { dev, floor } = base;
    const q0 = Math.max(base.b1, lowerBound(t, tp - IMPACT.peakBefore));
    const q1 = Math.min(t.length, lowerBound(t, tp + IMPACT.peakAfter));
    let q = p;
    let dq = dev(p);
    for (let i = q0; i < q1; i++) {
      const v = dev(i);
      if (v > dq) {
        dq = v;
        q = i;
      }
    }
    const half = floor + (dq - floor) / 2;
    for (let i = q - 1; i >= 0 && t[i] >= tp - IMPACT.onsetBack; i--) {
      const v = dev(i);
      if (v < half) {
        const v1 = dev(i + 1);
        const frac = v1 > v ? (half - v) / (v1 - v) : 0;
        return { onset: t[i] + frac * (t[i + 1] - t[i]), peakT: t[q], peak: dq, floor };
      }
    }
    return { onset: t[q], peakT: t[q], peak: dq, floor, unsure: true };
  }

  function threshold(noise, minMg) {
    return Math.max(minMg, noise ? noise.med + 8 * noise.sd : minMg);
  }

  /** The impact within ±halfWin of `center` (dev ms): the largest change there, if large enough. */
  function findImpactNear(buf, center, halfWin, noise, minMg) {
    const i0 = Math.max(1, lowerBound(buf.t, center - halfWin));
    const i1 = lowerBound(buf.t, center + halfWin);
    let p = -1;
    let jp = -Infinity;
    for (let i = i0; i < i1; i++) {
      const v = jerk(buf.ch, i);
      if (v > jp) {
        jp = v;
        p = i;
      }
    }
    if (p < 0) return { ok: false, reason: "no samples near the press" };
    const thr = threshold(noise, minMg);
    if (jp < thr) return { ok: false, reason: `largest change ${jp.toFixed(0)} < ${thr.toFixed(0)}`, jerk: jp };
    const o = impactOnset(buf, p);
    if (!o) return { ok: false, reason: "no samples before the impact", jerk: jp };
    return { ok: true, jerk: jp, ...o };
  }

  /** All impacts in [from, to) dev ms, at least `refractoryMs` apart (largest first). */
  function findImpactsIn(buf, from, to, noise, minMg, refractoryMs = 350) {
    const i0 = Math.max(1, lowerBound(buf.t, from));
    const i1 = lowerBound(buf.t, to);
    const thr = threshold(noise, minMg);
    const cand = [];
    for (let i = i0; i < i1; i++) {
      const v = jerk(buf.ch, i);
      if (v >= thr) cand.push([i, v]);
    }
    cand.sort((x, y) => y[1] - x[1]);
    const picked = [];
    for (const [i, v] of cand) {
      if (!picked.some(([k]) => Math.abs(buf.t[k] - buf.t[i]) < refractoryMs)) picked.push([i, v]);
    }
    picked.sort((x, y) => x[0] - y[0]);
    const out = [];
    for (const [i, v] of picked) {
      const o = impactOnset(buf, i);
      if (o) out.push({ jerk: v, ...o });
    }
    return out;
  }

  /** Deviation from the pre-impact level around `centerDev`, scaled to a peak of 1 (for plots). */
  function deviationTrace(buf, centerDev, before = 150, after = 200) {
    const p = Math.min(buf.t.length - 1, lowerBound(buf.t, centerDev));
    if (p < 1) return null;
    const base = baselineAt(buf, centerDev, p);
    if (!base) return null;
    const i0 = lowerBound(buf.t, centerDev - before);
    const i1 = lowerBound(buf.t, centerDev + after);
    const t = [];
    const v = [];
    let peak = 0;
    for (let i = i0; i < i1; i++) {
      const d = base.dev(i);
      t.push(buf.t[i]);
      v.push(d);
      peak = Math.max(peak, d);
    }
    return { t, v: v.map((x) => (peak > 0 ? x / peak : 0)) };
  }

  /** Nearest one-to-one pairs within `tol` between two ascending lists. */
  function pairNearest(a, b, tol) {
    const used = new Set();
    const pairs = [];
    for (let i = 0; i < a.length; i++) {
      let best = -1;
      let bd = tol;
      for (let j = 0; j < b.length; j++) {
        if (used.has(j)) continue;
        const d = Math.abs(a[i] - b[j]);
        if (d <= bd) {
          bd = d;
          best = j;
        }
      }
      if (best >= 0) {
        used.add(best);
        pairs.push([i, best]);
      }
    }
    return pairs;
  }

  /**
   * Press block: `keys` are keydown times (laptop ms); `buf` the sensor's
   * accelerometer; `mapping` its current mapping (only used to look in the
   * right place). Each detected press gives offset = key − impact onset, so
   * laptop = dev + offset. Returns the presses, a summary of the offsets, and
   * the median offset `o` at the median impact time `x` (dev ms).
   */
  function analyzePresses(keys, buf, mapping, opts = {}) {
    const minMg = opts.minMg === undefined ? 80 : opts.minMg;
    let spacing = Infinity;
    for (let i = 1; i < keys.length; i++) spacing = Math.min(spacing, keys[i] - keys[i - 1]);
    const halfWin = Math.min(opts.halfWinMs || 250, 0.45 * spacing);
    const devKeys = keys.map((k) => toDevice(mapping, k));
    const noise = keys.length ? jerkNoise(buf, devKeys[0] - 3000, devKeys[devKeys.length - 1] + 1000) : null;
    const presses = keys.map((key, i) => {
      const r = findImpactNear(buf, devKeys[i], halfWin, noise, minMg);
      if (!r.ok) return { key, reason: r.reason };
      return { key, onset: r.onset, offset: key - r.onset, jerk: r.jerk, peak: r.peak, unsure: !!r.unsure };
    });
    const found = presses.filter((p) => p.offset !== undefined);
    const summary = summarize(found.map((p) => p.offset));
    found.forEach((p, k) => (p.outlier = !summary.keep[k]));
    const used = found.filter((p) => !p.outlier);
    return {
      presses,
      detected: found.length,
      summary,
      o: summary.used ? summary.median : null,
      x: used.length ? median(used.map((p) => p.onset)) : null,
      noise,
    };
  }

  /**
   * Knock block between two sensors: impacts found in each during
   * [fromLap, toLap] are paired by laptop time. `ref` = { buf, mapping } is
   * the sensor whose mapping is trusted; the result is the offset `o` of
   * `other` (laptop = dev + o) at its dev time `x`.
   */
  function analyzeKnocks(ref, other, fromLap, toLap, opts = {}) {
    const minMg = opts.minMg === undefined ? 80 : opts.minMg;
    const tol = opts.pairTolMs || 150;
    const find = (s) => {
      const a = toDevice(s.mapping, fromLap);
      const b = toDevice(s.mapping, toLap);
      return findImpactsIn(s.buf, a, b, jerkNoise(s.buf, a, b), minMg);
    };
    const ir = find(ref);
    const io = find(other);
    const lr = ir.map((r) => toLaptop(ref.mapping, r.onset));
    const lo = io.map((r) => toLaptop(other.mapping, r.onset));
    const pairs = pairNearest(lr, lo, tol).map(([i, j]) => ({
      ref_onset: ir[i].onset,
      other_onset: io[j].onset,
      laptop: lr[i],
      offset: lr[i] - io[j].onset,
    }));
    const summary = summarize(pairs.map((p) => p.offset));
    pairs.forEach((p, k) => (p.outlier = !summary.keep[k]));
    const used = pairs.filter((p) => !p.outlier);
    return {
      ref_impacts: ir.length,
      other_impacts: io.length,
      pairs,
      summary,
      o: summary.used ? summary.median : null,
      x: used.length ? median(used.map((p) => p.other_onset)) : null,
    };
  }

  /**
   * Motion artifact in another stream of the same device (ECG, PPG) after an
   * accelerometer impact at `onset` (dev ms): the first sample-to-sample
   * change within [−50, +300] ms larger than 1.5× the largest change of the
   * preceding 3 s (heartbeats included, so beats are not mistaken for it).
   * Rough: only firm impacts give an artifact clear of the beats.
   */
  function artifactDelay(sig, chIdx, onset) {
    const { t } = sig;
    const c = sig.ch[chIdx];
    const r0 = Math.max(1, lowerBound(t, onset - 3000));
    const r1 = lowerBound(t, onset - 150);
    if (r1 - r0 < 10) return null;
    let ref = 0;
    for (let i = r0; i < r1; i++) ref = Math.max(ref, Math.abs(c[i] - c[i - 1]));
    if (ref <= 0) return null;
    const s0 = Math.max(1, lowerBound(t, onset - 50));
    const s1 = lowerBound(t, onset + 300);
    for (let i = s0; i < s1; i++) {
      const v = Math.abs(c[i] - c[i - 1]);
      if (v > 1.5 * ref) {
        const at = (t[i - 1] + t[i]) / 2;
        return { at, delay: at - onset, ratio: v / ref };
      }
    }
    return null;
  }

  /** Sample rate on the device clock over the last `spanMs`, gaps (disconnections) left out. */
  function measuredRate(buf, spanMs = 60000) {
    const n = buf.t.length;
    if (n < 3) return null;
    const i0 = Math.max(1, lowerBound(buf.t, buf.t[n - 1] - spanMs));
    const d = [];
    for (let i = i0; i < n; i++) d.push(buf.t[i] - buf.t[i - 1]);
    const limit = 1.5 * median(d);
    let sum = 0;
    let count = 0;
    for (const v of d) {
      if (v <= limit) {
        sum += v;
        count++;
      }
    }
    return sum > 0 ? (count * 1000) / sum : null;
  }

  // ───────────────────────────── stream settings ─────────────────────────────

  /**
   * Start settings from the device's offer: the highest value of each
   * setting, except an accelerometer range of 8 g when offered (presses on a
   * keyboard stay well inside it, and the steps stay fine).
   */
  function pickSettings(mt, offered) {
    const sel = {};
    for (const [name, vals] of Object.entries(offered || {})) {
      if (name === "FACTOR" || name === "SECURITY" || !vals.length || typeof vals[0] !== "number") continue;
      sel[name] = name === "RANGE" && mt === MEAS.ACC && vals.includes(8) ? 8 : Math.max(...vals);
    }
    return sel;
  }

  /** The samples of stream `mt` in clock segment `seg`, rebuilt from the frames. */
  function bufferFromFrames(frames, seg, mt) {
    let buf = null;
    for (const f of frames) {
      if (f.seg !== seg || f.mt !== mt || !f.ch || f.first === null) continue;
      if (!buf) buf = new StreamBuffer(f.ch.length);
      buf.append(f.first, f.dt, f.ch);
    }
    return buf;
  }

  // ───────────────────────────── heartbeats ─────────────────────────────

  // R-peak search: the level is the mean of ±100 ms; QRS complexes are the
  // largest bursts of slope energy, ≥ 250 ms apart, kept when above 15 % of
  // the local typical beat (the 4th largest burst within ±3 s: at least 4
  // beats fit in 6 s even at 40 bpm, so a few motion spikes do not set it);
  // the R-peak is the extremum of the dominant polarity within ±60 ms.
  const RPEAK = { baseMs: 100, refractoryMs: 250, searchMs: 60, rel: 0.15, localMs: 3000, beatEveryMs: 1500 };

  function rPeaksInRun(t, x, a, b, dtMs) {
    const n = b - a;
    const hw = Math.max(2, Math.round(RPEAK.baseMs / dtMs));
    if (n < 2 * hw + 5) return [];
    const pre = new Float64Array(n + 1);
    for (let i = 0; i < n; i++) pre[i + 1] = pre[i] + x[a + i];
    const y = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      const lo = Math.max(0, i - hw);
      const hi = Math.min(n, i + hw + 1);
      y[i] = x[a + i] - (pre[hi] - pre[lo]) / (hi - lo);
    }
    const d2 = new Float64Array(n);
    for (let i = 1; i < n - 1; i++) d2[i] = (y[i + 1] - y[i - 1]) ** 2;
    const e = new Float64Array(n);
    for (let i = 1; i < n - 1; i++) e[i] = d2[i - 1] + d2[i] + d2[i + 1];
    const cand = [];
    for (let i = 1; i < n - 1; i++) if (e[i] > 0 && e[i] >= e[i - 1] && e[i] > e[i + 1]) cand.push(i);
    if (!cand.length) return [];
    const bursts = [];
    for (const i of cand.sort((p, q) => e[q] - e[p])) {
      if (!bursts.some((j) => Math.abs(t[a + j] - t[a + i]) < RPEAK.refractoryMs)) bursts.push(i);
    }
    bursts.sort((p, q) => p - q);
    const picked = bursts.filter((i) => {
      const ti = t[a + i];
      const local = bursts.filter((j) => Math.abs(t[a + j] - ti) <= RPEAK.localMs).map((j) => e[j]).sort((p, q) => q - p);
      // Near the ends of the stretch the window holds fewer beats.
      const span = Math.min(t[b - 1], ti + RPEAK.localMs) - Math.max(t[a], ti - RPEAK.localMs);
      const k = Math.max(1, Math.floor(span / RPEAK.beatEveryMs));
      return e[i] >= RPEAK.rel * local[Math.min(k, local.length) - 1];
    });
    const w = Math.max(1, Math.round(RPEAK.searchMs / dtMs));
    let vote = 0;
    for (const i of picked) {
      let best = i;
      for (let j = Math.max(0, i - w); j <= Math.min(n - 1, i + w); j++) if (Math.abs(y[j]) > Math.abs(y[best])) best = j;
      vote += Math.sign(y[best]);
    }
    const pol = vote >= 0 ? 1 : -1;
    const peaks = [];
    for (const i of picked) {
      let m = Math.max(1, Math.min(n - 2, i));
      for (let j = Math.max(1, i - w); j <= Math.min(n - 2, i + w); j++) if (pol * y[j] > pol * y[m]) m = j;
      const zm = pol * y[m - 1];
      const z0 = pol * y[m];
      const zp = pol * y[m + 1];
      const den = zm - 2 * z0 + zp;
      let delta = den < 0 ? (0.5 * (zm - zp)) / den : 0;
      delta = Math.max(-0.5, Math.min(0.5, delta));
      const step = delta >= 0 ? t[a + m + 1] - t[a + m] : t[a + m] - t[a + m - 1];
      const tp = t[a + m] + delta * step;
      if (peaks.length && tp - peaks[peaks.length - 1].t < RPEAK.refractoryMs / 2) continue;
      peaks.push({ t: tp, amp: z0, first: !peaks.length });
    }
    return peaks;
  }

  /**
   * R-peaks of an ECG StreamBuffer between dev ms `from` and `to`, in order:
   * { t (dev ms, refined between samples by a parabola), amp (µV above the
   * local level), first (no peak before it in the same stretch of samples) }.
   * Each stretch without missing samples is searched separately.
   */
  function detectRPeaks(buf, from, to, rateHz) {
    const { t } = buf;
    const x = buf.ch[0];
    const i0 = lowerBound(t, from);
    const i1 = lowerBound(t, to);
    const dtMs = rateHz > 0 ? 1000 / rateHz : 1000 / 130;
    const out = [];
    let s = i0;
    for (let i = i0 + 1; i <= i1; i++) {
      if (i === i1 || t[i] - t[i - 1] > 3 * dtMs) {
        for (const p of rPeaksInRun(t, x, s, i, dtMs)) out.push(p);
        s = i;
      }
    }
    return out;
  }

  /**
   * Which R-peak ends each RR interval reported by the band itself.
   * `beats`, in order: { rr (ms), arrival (laptop ms of its packet), run
   * (changes at every reconnection) }; `peaks`: R-peaks in order { t (dev
   * ms), amp } as from detectRPeaks; `toLap`: dev ms → laptop ms.
   *
   * Within a run the band reports every beat, so its RR values added up give
   * the beats' times up to one unknown anchor; R-peaks that fall on those
   * times confirm an anchor. A peak is a candidate for a beat when another
   * peak lies one RR before it (within `tolMs`) and it lies between
   * `windowMs` before and `lateMs` after the packet's arrival; each candidate
   * of the beat and of its neighbours (±`span` beats) proposes an anchor.
   * An anchor's support is the number of those beats with a peak where it
   * puts them; extra peaks (motion, T-waves) or missed ones do not break
   * this. Anchors that put a beat more than `negMs` after its packet arrived
   * are out; of those supported nearly as well as the best (`nearTop`: a
   * peak lost at the edge of the neighbourhood must not decide), those with
   * weak peaks (below half the strongest: T-waves) are out; of the rest, the
   * one with the shortest beat-to-arrival delay over ±`delaySpan` beats wins
   * (on a very regular heart anchors a beat apart are all supported, but the
   * one a beat later than the truth puts some beat after its packet as soon
   * as one of those packets came quickly). The beat is matched when that anchor has a candidate peak
   * at it and is supported by at least `minSupport` beats and `minShare` of
   * the neighbourhood, so a beat whose own R-peak is lost (under a motion
   * spike) stays unmatched rather than taking another beat's. Returns per
   * beat { j, diff (interval from the peak one RR before − RR, ms) } or null;
   * with `opts.only` (a Set of indices into `beats`) only those beats are
   * worked out, the others being context (undefined).
   */
  function matchBeats(beats, peaks, toLap, opts = {}) {
    const tol = opts.tolMs === undefined ? 10 : opts.tolMs;
    const windowMs = opts.windowMs === undefined ? 3500 : opts.windowMs;
    const lateMs = opts.lateMs === undefined ? 150 : opts.lateMs;
    const span = opts.span === undefined ? 8 : opts.span;
    const negMs = opts.negMs === undefined ? 100 : opts.negMs;
    const minSupport = opts.minSupport === undefined ? 3 : opts.minSupport;
    const minShare = opts.minShare === undefined ? 0.6 : opts.minShare;
    const nearTop = opts.nearTop === undefined ? 0.8 : opts.nearTop;
    const delaySpan = opts.delaySpan === undefined ? 30 : opts.delaySpan;
    const only = opts.only || null;
    const T = peaks.map((p) => p.t);
    const lap = T.map(toLap);
    const nearest = (x, within) => {
      let best = -1;
      for (let j = lowerBound(T, x - within); j < T.length && T[j] <= x + within; j++) {
        if (best < 0 || Math.abs(T[j] - x) < Math.abs(T[best] - x)) best = j;
      }
      return best;
    };
    // C[i]: the beat's time after its run's first beat, from the RR values.
    const C = beats.map(() => 0);
    for (let i = 1; i < beats.length; i++) C[i] = beats[i].run === beats[i - 1].run ? C[i - 1] + beats[i].rr : 0;
    // Candidate peaks of each beat: { j, diff }.
    const cand = beats.map((b) => {
      const out = [];
      for (let j = lowerBound(lap, b.arrival - windowMs); j < T.length && lap[j] <= b.arrival + lateMs; j++) {
        const before = nearest(T[j] - b.rr, tol);
        if (before >= 0 && before !== j) out.push({ j, diff: T[j] - T[before] - b.rr });
      }
      return out;
    });
    return beats.map((b, i) => {
      if (only && !only.has(i)) return undefined;
      const hood = [];
      const far = [];
      for (let k = Math.max(0, i - delaySpan); k <= Math.min(beats.length - 1, i + delaySpan); k++) {
        if (beats[k].run !== b.run) continue;
        far.push(k);
        if (Math.abs(k - i) <= span) hood.push(k);
      }
      const anchors = [];
      for (const k of hood) {
        for (const c of cand[k]) {
          const a = T[c.j] - C[k];
          if (!anchors.some((x) => Math.abs(x - a) < tol)) anchors.push(a);
        }
      }
      const scored = [];
      for (const a of anchors) {
        let support = 0;
        let minDelay = Infinity;
        const amps = [];
        for (const k of far) {
          const q = nearest(a + C[k], tol + 0.5 * Math.abs(k - i));
          if (q < 0) continue;
          minDelay = Math.min(minDelay, beats[k].arrival - lap[q]);
          if (Math.abs(k - i) > span) continue;
          support++;
          amps.push(peaks[q].amp);
        }
        if (support && minDelay >= -negMs) scored.push({ a, support, minDelay, amp: median(amps) });
      }
      if (!scored.length) return null;
      const top = Math.max(...scored.map((s) => s.support));
      const best = scored.filter((s) => s.support >= nearTop * top);
      const strongest = Math.max(...best.map((s) => s.amp));
      const pick = best.filter((s) => s.amp >= 0.5 * strongest).sort((p, q) => p.minDelay - q.minDelay)[0];
      if (pick.support < Math.max(minSupport, minShare * hood.length)) return null;
      const own = cand[i].filter((c) => Math.abs(T[c.j] - (pick.a + C[i])) <= tol);
      if (!own.length) return null;
      own.sort((p, q) => Math.abs(T[p.j] - pick.a - C[i]) - Math.abs(T[q.j] - pick.a - C[i]));
      return { j: own[0].j, diff: own[0].diff };
    });
  }

  // ───────────────────────────── export ─────────────────────────────

  const r3 = (v) => (v === null || v === undefined || !Number.isFinite(v) ? null : Math.round(v * 1000) / 1000);

  /** One JSONL record of a frame: raw bytes, both clocks, and the decoded values. */
  function frameRecord(devId, f, mapping) {
    return {
      type: "frame",
      dev: devId,
      t: r3(f.t),
      ts: f.ts.toString(),
      mt: MEAS_NAME[f.mt] || f.mt,
      ft: f.ft,
      seg: f.seg,
      hex: toHex(f.bytes),
      n: f.n,
      first_dev_ms: r3(f.first),
      dt_ms: f.dt === null ? null : Math.round(f.dt * 1e6) / 1e6,
      first_laptop_ms: mapping && f.first !== null && f.seg === mapping.seg ? r3(toLaptop(mapping, f.first)) : null,
      v: f.ch,
      err: f.err,
    };
  }

  // ───────────────────────────── browser: one device ─────────────────────────────

  const OPEN_ATTEMPTS = 4;
  const CP_TIMEOUT_MS = 6000;
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

  /**
   * One Polar device on the PMD service. Keeps its recorder across
   * disconnections and reconnects by itself (no click needed for a device the
   * page already has), restarting the streams it was running.
   */
  class PmdDevice {
    constructor(log = () => {}) {
      this.log = log;
      this.device = null;
      this.name = null;
      this.rec = new PmdRecorder();
      this.hr = []; // standard heart rate service: { t, bytes }
      this.features = [];
      this.sdkMode = null;
      this.wanted = {}; // measType -> settings, restarted after a reconnection
      this.active = {}; // measType -> { sel, factor, t }
      this.linkUp = false;
      this.userClosed = false;
      this.autoReconnect = true;
      this.useHeartRate = true; // false when another object already listens to the heart rate service
      this.connectedAt = null;
      this.lastFrameAt = null;
      this.drops = 0;
      this.onChange = null; // state changed (link, streams)
      this._cp = null;
      this._data = null;
      this._queue = Promise.resolve();
      this._waiter = null;
      this._reconnecting = false;
      this._onCp = (e) => this._cpIndication(bytesOf(e.target.value));
      this._onData = (e) => {
        const t = performance.now();
        this.lastFrameAt = t;
        try {
          this.rec.ingestFrame(t, bytesOf(e.target.value));
        } catch (err) {
          this.log(`Bad frame: ${err.message}`);
        }
      };
      this._onHr = (e) => this.hr.push({ t: performance.now(), bytes: bytesOf(e.target.value) });
    }

    get streaming() {
      return Object.keys(this.active).length > 0;
    }

    _event(event, detail) {
      this.rec.event(performance.now(), event, detail);
      if (this.onChange) this.onChange();
    }

    /** Must be called from a click. Any Polar device can be chosen; non-PMD ones fail on open. */
    async connect() {
      this.device = await navigator.bluetooth.requestDevice({
        filters: [{ namePrefix: "Polar" }],
        optionalServices: [PMD_SERVICE, "heart_rate"],
      });
      this.name = this.device.name || "?";
      this.device.addEventListener("gattserverdisconnected", () => this._linkLost("browser"));
      this.userClosed = false;
      this._connecting = true; // a failed setup here is retried below, not by _reconnect
      try {
        for (let attempt = 1; ; attempt++) {
          try {
            await this._openOnce();
            return;
          } catch (err) {
            this.linkUp = false;
            this._closeQuietly();
            if (err.name === "NotFoundError" && /service/i.test(err.message)) {
              throw new Error(`${this.name} has no Polar Measurement Data service (an H9, or not a Polar sensor?)`);
            }
            if (attempt >= OPEN_ATTEMPTS) throw err;
            this.log(`Connection attempt ${attempt} failed (${err.name}: ${err.message}); retrying…`);
            await sleep(1000 * attempt);
          }
        }
      } catch (err) {
        this.userClosed = true;
        throw err;
      } finally {
        this._connecting = false;
      }
    }

    /**
     * One step of the service setup, retried a few times (desktop Chrome
     * reports transient failures as "GATT Error Unknown"); a final failure
     * names the step.
     */
    async _step(label, fn, tries = 3) {
      for (let i = 1; ; i++) {
        try {
          return await fn();
        } catch (err) {
          if (i >= tries || !this.linkUp || err.name === "NotFoundError" || err.name === "SecurityError") {
            const e = new Error(`${label}: ${err.message}`);
            e.name = err.name;
            throw e;
          }
          this.log(`${label} failed (${err.name}: ${err.message}); retrying…`);
          await sleep(400 * i);
        }
      }
    }

    async _openOnce() {
      const server = await this.device.gatt.connect();
      this.linkUp = true;
      this.connectedAt = performance.now();
      this._event("connected", { name: this.name });
      this.log(`Connected to ${this.name}; opening the measurement service…`);
      const svc = await this._step("measurement service", () => server.getPrimaryService(PMD_SERVICE));
      this._cp = await this._step("control point", () => svc.getCharacteristic(PMD_CP));
      this._data = await this._step("data characteristic", () => svc.getCharacteristic(PMD_DATA));
      this._cp.addEventListener("characteristicvaluechanged", this._onCp);
      this._data.addEventListener("characteristicvaluechanged", this._onData);
      await this._step("control point indications", () => this._cp.startNotifications());
      await this._step("data notifications", () => this._data.startNotifications());
      try {
        this.features = parseFeatures(bytesOf(await this._step("feature read", () => this._cp.readValue())));
      } catch (err) {
        this.log(`${err.message}; asking for each measurement's settings instead.`);
        this.features = await this._probeFeatures();
      }
      this.log(`Measurement service ready ${((performance.now() - this.connectedAt) / 1000).toFixed(1)} s after connecting; ${this.name} offers: ${this.features.join(", ") || "nothing"}`);
      if (this.features.includes("SDK_MODE")) {
        try {
          this.sdkMode = await this.getSdkMode();
        } catch (err) {
          this.log(`SDK mode status unknown (${err.message})`);
        }
      }
      if (this.useHeartRate) try {
        const hrSvc = await server.getPrimaryService("heart_rate");
        const hrChar = await hrSvc.getCharacteristic("heart_rate_measurement");
        hrChar.addEventListener("characteristicvaluechanged", this._onHr);
        await hrChar.startNotifications();
      } catch (err) {
        this.log(`Heart rate service not used (${err.name})`);
      }
    }

    /** Measurement types whose settings the device returns (when the feature read fails). */
    async _probeFeatures() {
      const found = [];
      for (const name of ["ECG", "PPG", "ACC", "PPI", "GYRO"]) {
        try {
          const r = await this.command([OP.GET_SETTINGS, MEAS[name]], `settings of ${name}`);
          if (r.status === STATUS_SUCCESS) found.push(name);
        } catch (err) {
          this.log(`settings of ${name}: ${err.message}`);
        }
      }
      return found;
    }

    _closeQuietly() {
      try {
        if (this.device && this.device.gatt.connected) this.device.gatt.disconnect();
      } catch (_) {}
    }

    _cpIndication(b) {
      const r = parseCpResponse(b);
      if (r.kind === "response") {
        if (this._waiter) this._waiter.take(r);
      } else if (r.kind === "stopped") {
        this.log(`${this.name} stopped by itself: ${r.types.join(", ")}`);
        for (const name of r.types) delete this.active[MEAS[name]];
        this._event("device_stopped", { types: r.types });
      } else this.log(`Unexpected control point data: ${r.hex}`);
    }

    /** One control point command; commands run one at a time. Resolves to the (merged) response. */
    command(bytes, label) {
      const run = async () => {
        if (!this._cp || !this.linkUp) throw new Error(`${label}: not connected`);
        const op = bytes[0];
        const parts = [];
        const done = new Promise((resolve, reject) => {
          const timer = setTimeout(() => {
            this._waiter = null;
            reject(new Error(`${label}: no response`));
          }, CP_TIMEOUT_MS);
          this._waiter = {
            take: (r) => {
              if (r.op !== op) return;
              parts.push(r);
              if (!r.more) {
                clearTimeout(timer);
                this._waiter = null;
                resolve();
              }
            },
            fail: (err) => {
              clearTimeout(timer);
              this._waiter = null;
              reject(err);
            },
          };
        });
        const data = Uint8Array.from(bytes);
        await (this._cp.writeValueWithResponse ? this._cp.writeValueWithResponse(data) : this._cp.writeValue(data));
        await done;
        const r = parts[0];
        if (parts.length > 1) {
          const all = [];
          for (const p of parts) all.push(...p.params);
          r.params = Uint8Array.from(all);
        }
        return r;
      };
      const p = this._queue.then(run, run);
      this._queue = p.catch(() => {});
      return p;
    }

    async getSettings(mt) {
      const r = await this.command([OP.GET_SETTINGS, mt], `settings of ${MEAS_NAME[mt]}`);
      if (r.status !== STATUS_SUCCESS) throw new Error(`settings of ${MEAS_NAME[mt]}: ${r.statusName}`);
      return parseSettings(r.params);
    }

    async getSdkMode() {
      const r = await this.command([OP.GET_SDK_MODE_STATUS], "SDK mode status");
      if (r.status !== STATUS_SUCCESS) throw new Error(r.statusName);
      return r.params.length ? r.params[0] !== 0 : null;
    }

    /** Switching mode stops every stream first (the device refuses otherwise). */
    async setSdkMode(on) {
      await this.stopAll();
      const r = await this.command(on ? [OP.START, MEAS.SDK_MODE] : [OP.STOP, MEAS.SDK_MODE], on ? "SDK mode on" : "SDK mode off");
      if (r.status !== STATUS_SUCCESS && r.status !== STATUS_ALREADY_IN_STATE) throw new Error(`SDK mode: ${r.statusName}`);
      this.sdkMode = on;
      this._event("sdk_mode", { on });
    }

    async startStream(mt, sel) {
      const name = MEAS_NAME[mt];
      const send = async () => {
        this.rec.streamStarting(mt, sel.SAMPLE_RATE || 0);
        return this.command(startCommand(mt, sel), `start ${name}`);
      };
      let r = await send();
      if (r.status === STATUS_ALREADY_IN_STATE) {
        this.log(`${name} was still running on the device: restarting it.`);
        await this.command([OP.STOP, mt], `stop ${name}`);
        r = await send();
      }
      if (r.status !== STATUS_SUCCESS) {
        this.rec.streamStarted(mt, 1);
        const hint = r.statusName === "INVALID_MTU" ? " (the browser's Bluetooth packet size is too small for this rate)" : "";
        throw new Error(`${name} refused: ${r.statusName}${hint}`);
      }
      let factor = 1;
      try {
        const resp = parseSettings(r.params);
        if (resp.FACTOR) factor = resp.FACTOR[0];
      } catch (err) {
        this.log(`${name}: unreadable start response (${err.message}); factor 1 assumed`);
      }
      this.rec.streamStarted(mt, factor);
      this.wanted[mt] = sel;
      this.active[mt] = { sel, factor, t: performance.now() };
      this._event("stream_started", { type: name, settings: sel, factor });
    }

    async stopStream(mt) {
      delete this.wanted[mt];
      delete this.active[mt];
      const r = await this.command([OP.STOP, mt], `stop ${MEAS_NAME[mt]}`);
      this._event("stream_stopped", { type: MEAS_NAME[mt], status: r.statusName });
    }

    async stopAll() {
      for (const mt of Object.keys(this.active).map(Number)) {
        try {
          await this.stopStream(mt);
        } catch (err) {
          this.log(`Stop ${MEAS_NAME[mt]} failed: ${err.message}`);
        }
      }
      this.wanted = {};
    }

    /** Give up on the link from the page's side (e.g. silent for too long); reconnection follows. */
    drop(reason) {
      this._event(reason);
      this._closeQuietly();
      this._linkLost(reason);
    }

    async disconnect() {
      this.userClosed = true;
      if (this.linkUp) await this.stopAll();
      this._closeQuietly();
      this._linkLost("user");
    }

    _linkLost(reason) {
      if (!this.linkUp) return;
      const t = performance.now();
      this.linkUp = false;
      this.drops += reason === "user" ? 0 : 1;
      const after = this.connectedAt === null ? null : (t - this.connectedAt) / 1000;
      if (this._waiter) this._waiter.fail(new Error("link lost"));
      this._cp = this._data = null;
      this.active = {};
      this.rec.linkLost();
      this._event("disconnected", { reason, after_s: after });
      this.log(`${this.name} disconnected (${reason}) after ${after === null ? "?" : after.toFixed(1)} s.`);
      if (reason !== "user" && after !== null && after >= 25 && after <= 40) {
        this.log("The link dropped 25–40 s after connecting: this is what an unconfirmed OS pairing does (seen with the H9).");
        this._event("suspect_pairing_timeout", { after_s: after });
      }
      if (!this.userClosed && this.autoReconnect && !this._connecting) this._reconnect();
    }

    async _reconnect() {
      if (this._reconnecting) return;
      this._reconnecting = true;
      let delay = 1000;
      try {
        while (!this.userClosed && !this.linkUp) {
          await sleep(delay);
          if (this.userClosed) break;
          try {
            await this._openOnce();
            if (this.userClosed) {
              // Disconnect was clicked while this attempt was under way.
              this.linkUp = false;
              this._closeQuietly();
              break;
            }
            const wanted = Object.entries(this.wanted);
            for (const [mt, sel] of wanted) await this.startStream(Number(mt), sel);
            this._event("reconnected", { streams: wanted.map(([mt]) => MEAS_NAME[mt]) });
            this.log(`${this.name} reconnected; streams restarted.`);
          } catch (err) {
            this.log(`Reconnect failed (${err.name}: ${err.message}); next try in ${(delay / 1000).toFixed(0)} s.`);
            this.linkUp = false;
            this._closeQuietly();
            delay = Math.min(2 * delay, 10000);
          }
        }
      } finally {
        this._reconnecting = false;
      }
    }
  }

  const api = {
    PMD_SERVICE, PMD_CP, PMD_DATA, MEAS, MEAS_NAME, OP, SETTING, STATUS_NAME, POLAR_EPOCH_UNIX_NS, STREAM_INFO,
    toHex, fromHex, bytesOf, readUint, readInt, signExtend,
    parseFeatures, parseSettings, serializeSettings, startCommand, parseCpResponse,
    parseFrameHeader, parseDeltaFrames, decodeFrame, frameSampleTimes,
    median, robustSd, summarize, linearFit,
    toLaptop, toDevice, envelopeFit, chooseMapping,
    StreamBuffer, PmdRecorder,
    lowerBound, jerk, jerkNoise, impactOnset, findImpactNear, findImpactsIn, deviationTrace, pairNearest,
    analyzePresses, analyzeKnocks, artifactDelay, measuredRate,
    pickSettings, bufferFromFrames, detectRPeaks, matchBeats,
    frameRecord,
    PmdDevice,
  };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.PolarPmd = api;
})(typeof window !== "undefined" ? window : globalThis);
