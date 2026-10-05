// Polar H9 heart rate band over Web Bluetooth: packet parsing, a session
// recorder, the per-trial log block, and the live plots.
//
// Only the standard Heart Rate service is used. The band's other services are
// deliberately never touched: on an unpaired link, accessing them makes the
// operating system start a pairing that drops the connection 30 s later if
// nothing confirms it.
//
// Loaded as a plain <script> by deploy_frontend/index.html (exports on
// `window.Polar`) and as a CommonJS module by tools/tests/test_polar_hr.js.
// Nothing above `class PolarSession` needs a browser.
(function (root) {
  "use strict";

  const HR_SERVICE = "heart_rate";
  const HR_MEASUREMENT = "heart_rate_measurement";
  const OPEN_ATTEMPTS = 4;
  const SUPPORTED_NAME = /^Polar H9\b/;

  // Written into every trial log next to the measurements.
  const HR_UNITS = {
    arrival_elapsed_secs: "packet arrival time, seconds on the controller clock (zero = session_info.app_start_unix_ns)",
    hr: "bpm",
    rr_ms: "ms",
    raw: "packet bytes, hex",
    trial_start_elapsed_secs: "controller clock when the trial switched to playing",
    trial_end_elapsed_secs: "controller clock when the trial log was written",
    connection_events: "elapsed_secs on the controller clock",
  };

  // ───────────────────────────── parsing ─────────────────────────────

  /** Heart Rate Measurement (0x2A37). RR intervals are sent in 1/1024 s. */
  function parseHeartRate(dv) {
    const flags = dv.getUint8(0);
    let i = 1;
    const hr = flags & 0x01 ? dv.getUint16(i, true) : dv.getUint8(i);
    i += flags & 0x01 ? 2 : 1;
    if (flags & 0x08) i += 2; // energy expended
    const rr_ms = [];
    if (flags & 0x10) {
      for (; i + 1 < dv.byteLength; i += 2) rr_ms.push((dv.getUint16(i, true) * 1000) / 1024);
    }
    return { hr, rr_ms };
  }

  /** Field-by-field reading of a Heart Rate Measurement packet, for display. */
  function describeHeartRate(dv) {
    const flags = dv.getUint8(0);
    const wide = flags & 0x01;
    const parts = [
      `flags=0x${flags.toString(16).padStart(2, "0")} (hr ${wide ? "u16" : "u8"}, rr ${flags & 0x10 ? "present" : "absent"})`,
      `hr=${wide ? dv.getUint16(1, true) : dv.getUint8(1)} bpm`,
    ];
    let i = wide ? 3 : 2;
    if (flags & 0x08) i += 2;
    if (flags & 0x10) {
      for (; i + 1 < dv.byteLength; i += 2) {
        const raw = dv.getUint16(i, true);
        parts.push(`rr=${raw}/1024 s=${((raw * 1000) / 1024).toFixed(1)} ms`);
      }
    }
    return parts.join(" | ");
  }

  function toHex(dv) {
    const out = [];
    for (let i = 0; i < dv.byteLength; i++) out.push(dv.getUint8(i).toString(16).padStart(2, "0"));
    return out.join(" ");
  }

  function median(a) {
    if (!a.length) return NaN;
    const s = [...a].sort((x, y) => x - y);
    const m = s.length >> 1;
    return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
  }

  // ───────────────────────────── trial log ─────────────────────────────

  /**
   * performance.now() milliseconds → seconds on the controller clock, kept as
   * a 32-bit float like the game's `present_elapsed_secs`. Packets that
   * arrived before the controller started come out negative.
   */
  function elapsedSecs(t_ms, appStartMs) {
    return Math.fround((t_ms - appStartMs) / 1000);
  }

  /**
   * The `hr_info` / `hr_meas` pair of one trial log, from packets and events
   * given in performance.now() milliseconds.
   */
  function buildTrialLog(device, packets, events, appStartMs, trialStartMs, trialEndMs) {
    return {
      hr_info: {
        device,
        trial_start_elapsed_secs: trialStartMs === null ? null : elapsedSecs(trialStartMs, appStartMs),
        trial_end_elapsed_secs: elapsedSecs(trialEndMs, appStartMs),
        units: HR_UNITS,
        connection_events: events.map((e) => ({ elapsed_secs: elapsedSecs(e.t_ms, appStartMs), event: e.event })),
      },
      hr_meas: packets.map((p) => ({
        arrival_elapsed_secs: elapsedSecs(p.t_ms, appStartMs),
        hr: p.hr,
        rr_ms: p.rr_ms,
        raw: p.raw,
      })),
    };
  }

  // ───────────────────────────── series ─────────────────────────────

  /** Plot points: one per packet for bpm, one per beat for RR. */
  function beatSeries(hr) {
    const bpm = hr.map((r) => ({ t: r.t_ms, v: r.hr }));
    const rr = [];
    for (const r of hr) {
      // The last RR of a packet ends near its arrival; earlier ones precede it.
      let back = 0;
      for (let i = r.rr_ms.length - 1; i >= 0; i--) {
        rr.push({ t: r.t_ms - back, v: r.rr_ms[i] });
        back += r.rr_ms[i];
      }
    }
    rr.sort((a, b) => a.t - b.t);
    return { bpm, rr };
  }

  /** Arrival gaps between consecutive packets, skipping gaps across a disconnection. */
  function packetGaps(hr, events) {
    const drops = events.filter((e) => e.event === "disconnected").map((e) => e.t_ms);
    const gaps = [];
    for (let i = 1; i < hr.length; i++) {
      const a = hr[i - 1].t_ms;
      const b = hr[i].t_ms;
      if (!drops.some((d) => d > a && d < b)) gaps.push(b - a);
    }
    return gaps;
  }

  /** Mean and SD of the values in the `window_ms` up to and including each point. */
  function trailingStats(points, window_ms) {
    const out = [];
    let start = 0;
    for (let i = 0; i < points.length; i++) {
      while (points[start].t <= points[i].t - window_ms) start++;
      const win = points.slice(start, i + 1);
      const mean = win.reduce((s, p) => s + p.v, 0) / win.length;
      const sd = Math.sqrt(win.reduce((s, p) => s + (p.v - mean) ** 2, 0) / win.length);
      out.push({ t: points[i].t, mean, sd });
    }
    return out;
  }

  // ───────────────────────────── recorder ─────────────────────────────

  /** One Web Bluetooth connection to a band; accumulates everything received. */
  class PolarSession {
    constructor(log = () => {}) {
      this.log = log;
      this.device = null;
      this.hr = []; // every packet { t_ms, hr, rr_ms, raw }; t_ms = performance.now() at arrival
      this.events = []; // { t_ms, event: connected | disconnected | silent }
      this.pending = []; // packets not yet handed to a trial log
      this.pendingEvents = [];
      this.t_connected_ms = null;
      this.onPacket = null; // ({ t_ms, hex, note }) for every packet
      this.onDisconnect = null; // the link is gone and nothing is retrying
      this._linkUp = false;
      this._opening = false;
    }

    get connected() {
      return this._linkUp && !this._opening;
    }

    /** False for anything but the band this recording was built and tested for. */
    get isSupportedDevice() {
      return !!this.device && SUPPORTED_NAME.test(this.device.name || "");
    }

    /** Must be called from a user gesture (click). Calling it again keeps the data. */
    async connect() {
      this.device = await navigator.bluetooth.requestDevice({
        // Any Polar device, or any band advertising the heart rate service:
        // a wrong choice is reported by `isSupportedDevice`, not hidden.
        filters: [{ namePrefix: "Polar" }, { services: [HR_SERVICE] }],
        optionalServices: [HR_SERVICE],
      });
      this.device.addEventListener("gattserverdisconnected", () => this._linkLost());
      await this._open();
    }

    /**
     * Give up on the link from the page's side, e.g. after a long silence that
     * the browser did not report as a disconnection. `reason` is logged as an
     * event ahead of the disconnection itself.
     */
    drop(reason) {
      if (!this.device) return;
      if (reason) this._event(reason);
      try {
        this.device.gatt.disconnect();
      } catch (_) {}
      this._linkLost(); // the browser may never fire its own event
    }

    /**
     * The trial-log block for everything received since the previous call.
     * Each packet and event is returned exactly once, so reading consecutive
     * trial logs in order gives the unbroken stream.
     */
    takeTrialLog(appStartMs, trialStartMs, trialEndMs) {
      const device = this.device ? this.device.name : null;
      return buildTrialLog(device, this.pending.splice(0), this.pendingEvents.splice(0), appStartMs, trialStartMs, trialEndMs);
    }

    _event(event) {
      const e = { t_ms: performance.now(), event };
      this.events.push(e);
      this.pendingEvents.push(e);
      return e;
    }

    _linkLost() {
      if (!this._linkUp) return; // already reported
      this._linkUp = false;
      this._event("disconnected");
      this.log("Band disconnected.");
      // While _open() is still retrying, it reports the outcome itself.
      if (!this._opening && this.onDisconnect) this.onDisconnect();
    }

    /**
     * The link often drops while services are still being discovered (band
     * not fully awake, weak signal), so setup is retried a few times.
     */
    async _open() {
      this._opening = true;
      try {
        for (let attempt = 1; ; attempt++) {
          try {
            await this._openOnce();
            return;
          } catch (err) {
            if (attempt >= OPEN_ATTEMPTS) throw err;
            this.log(`Connection attempt ${attempt} failed (${err.name}); retrying…`);
            await new Promise((resolve) => setTimeout(resolve, 1000 * attempt));
          }
        }
      } finally {
        this._opening = false;
      }
    }

    async _openOnce() {
      const gatt = await this.device.gatt.connect();
      this._linkUp = true;
      this.t_connected_ms = this._event("connected").t_ms;
      this.log(`Connected to ${this.device.name}; looking up the heart rate service…`);

      const hrChar = await (await gatt.getPrimaryService(HR_SERVICE)).getCharacteristic(HR_MEASUREMENT);
      hrChar.addEventListener("characteristicvaluechanged", (e) => {
        const row = { t_ms: performance.now(), ...parseHeartRate(e.target.value), raw: toHex(e.target.value) };
        this.hr.push(row);
        this.pending.push(row);
        if (this.onPacket) this.onPacket({ t_ms: row.t_ms, hex: row.raw, note: describeHeartRate(e.target.value) });
      });
      await hrChar.startNotifications();
      const setup_s = (performance.now() - this.t_connected_ms) / 1000;
      this.log(`Heart rate packets switched on (setup took ${setup_s.toFixed(1)} s); waiting for the first packet…`);
    }
  }

  // ───────────────────────────── plots ─────────────────────────────
  // Drawn in CSS pixels at the canvas's displayed size, so they stay sharp
  // and legible at any layout width.

  const COLORS = { text: "#aaa", title: "#ccc", note: "#eee", grid: "#2a2a2a", frame: "#666", dot: "#7bd", mean: "#f90", band: "rgba(255, 153, 0, 0.22)" };

  /** Round tick positions covering [lo, hi], about `n` of them. */
  function niceTicks(lo, hi, n) {
    const raw = (hi - lo) / n;
    const mag = 10 ** Math.floor(Math.log10(raw));
    const step = [1, 2, 5, 10].map((m) => m * mag).find((x) => x >= raw);
    const ticks = [];
    for (let v = Math.ceil(lo / step) * step; v <= hi + 1e-9; v += step) ticks.push(v);
    return ticks;
  }

  /** Size the bitmap to the element; null while the canvas is not laid out. */
  function prepare(canvas) {
    const W = canvas.clientWidth;
    const H = canvas.clientHeight;
    if (!W || !H) return null;
    const dpr = root.devicePixelRatio || 1;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
    }
    const g = canvas.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    const fs = Math.max(10, Math.round(parseFloat(getComputedStyle(canvas).fontSize) * 0.78));
    g.font = `${fs}px system-ui, sans-serif`;
    return { g, W, H, fs, m: { l: fs * 4.2, r: fs * 2.2, t: fs * 0.9, b: fs * 3.3 } };
  }

  /** Axes with ticks, grid and titles; returns the data → pixel mappers. */
  function drawAxes(p, xr, yr, xTicks, xFmt, xTitle, yTitle) {
    const { g, W, H, fs, m } = p;
    const x = (v) => m.l + ((v - xr[0]) / (xr[1] - xr[0])) * (W - m.l - m.r);
    const y = (v) => H - m.b - ((v - yr[0]) / (yr[1] - yr[0])) * (H - m.t - m.b);
    g.clearRect(0, 0, W, H);
    g.lineWidth = 1;
    g.textBaseline = "middle";
    g.textAlign = "right";
    for (const v of niceTicks(yr[0], yr[1], 5)) {
      g.strokeStyle = COLORS.grid;
      g.beginPath();
      g.moveTo(m.l, y(v));
      g.lineTo(W - m.r, y(v));
      g.stroke();
      g.fillStyle = COLORS.text;
      g.fillText(String(Math.round(v * 100) / 100), m.l - fs * 0.5, y(v));
    }
    g.textAlign = "center";
    g.textBaseline = "top";
    for (const v of xTicks) {
      g.strokeStyle = COLORS.grid;
      g.beginPath();
      g.moveTo(x(v), m.t);
      g.lineTo(x(v), H - m.b);
      g.stroke();
      g.fillStyle = COLORS.text;
      g.fillText(xFmt(v), x(v), H - m.b + fs * 0.35);
    }
    g.strokeStyle = COLORS.frame;
    g.strokeRect(m.l, m.t, W - m.l - m.r, H - m.t - m.b);
    g.fillStyle = COLORS.title;
    g.fillText(xTitle, m.l + (W - m.l - m.r) / 2, H - fs * 1.4);
    g.save();
    g.translate(fs * 0.9, m.t + (H - m.t - m.b) / 2);
    g.rotate(-Math.PI / 2);
    g.textBaseline = "middle";
    g.fillText(yTitle, 0, 0);
    g.restore();
    return { x, y };
  }

  function drawNote(p, text) {
    const { g, W, fs, m } = p;
    g.fillStyle = COLORS.note;
    g.textAlign = "right";
    g.textBaseline = "top";
    g.fillText(text, W - m.r - fs * 0.4, m.t + fs * 0.35);
  }

  /**
   * Time series on the laptop's wall clock: dots for the values, a line for
   * the mean of the preceding `averageMs`, a band for ± 1 SD over the same.
   * `opts.note(last, mean, sd)` gives the corner text.
   */
  function drawSeries(canvas, points, opts) {
    const p = prepare(canvas);
    if (!p) return;
    const windowMs = opts.windowMs || 120000;
    const now = performance.now();
    const t0 = now - windowMs;
    const stats = trailingStats(points, opts.averageMs || 10000).filter((s) => s.t >= t0);
    const pts = points.filter((q) => q.t >= t0);
    let lo = 0;
    let hi = 1;
    if (pts.length) {
      lo = Math.min(...pts.map((q) => q.v), ...stats.map((s) => s.mean - s.sd));
      hi = Math.max(...pts.map((q) => q.v), ...stats.map((s) => s.mean + s.sd));
      const pad = Math.max(1, (hi - lo) * 0.1);
      lo -= pad;
      hi += pad;
    }
    // Ticks sit on round marks of the wall clock (Unix time =
    // performance.timeOrigin + performance.now(), the mapping the game logs use).
    const origin = performance.timeOrigin;
    const step = p.W < 520 ? 30000 : 20000;
    const xTicks = [];
    for (let u = Math.ceil((origin + t0) / step) * step; u <= origin + now; u += step) xTicks.push(u - origin);
    const clock = (t) => new Date(origin + t).toLocaleTimeString("en-GB");
    const { x, y } = drawAxes(p, [t0, now], [lo, hi], xTicks, clock, opts.xTitle, opts.yTitle);
    if (!pts.length) return;
    const { g } = p;

    g.fillStyle = COLORS.band;
    g.beginPath();
    stats.forEach((s, i) => (i ? g.lineTo(x(s.t), y(s.mean + s.sd)) : g.moveTo(x(s.t), y(s.mean + s.sd))));
    for (let i = stats.length - 1; i >= 0; i--) g.lineTo(x(stats[i].t), y(stats[i].mean - stats[i].sd));
    g.closePath();
    g.fill();

    g.fillStyle = COLORS.dot;
    for (const q of pts) g.fillRect(x(q.t) - 1.5, y(q.v) - 1.5, 3, 3);

    g.strokeStyle = COLORS.mean;
    g.lineWidth = 1.5;
    g.beginPath();
    stats.forEach((s, i) => (i ? g.lineTo(x(s.t), y(s.mean)) : g.moveTo(x(s.t), y(s.mean))));
    g.stroke();

    const last = stats.at(-1);
    drawNote(p, opts.note(pts.at(-1).v, last.mean, last.sd));
  }

  /** Histogram of packet intervals. `opts.note(n, median, min, max)` gives the corner text. */
  function drawGapHistogram(canvas, gaps, opts) {
    const p = prepare(canvas);
    if (!p) return;
    const bin = opts.binMs || 10;
    if (!gaps.length) {
      drawAxes(p, [900, 1100], [0, 1], niceTicks(900, 1100, 8), String, opts.xTitle, opts.yTitle);
      return;
    }
    const lo = Math.floor(Math.min(...gaps) / bin) * bin - bin;
    const hi = Math.ceil(Math.max(...gaps) / bin) * bin + bin;
    const counts = new Array(Math.round((hi - lo) / bin)).fill(0);
    for (const v of gaps) counts[Math.min(counts.length - 1, Math.floor((v - lo) / bin))]++;
    const top = Math.max(...counts);
    const { x, y } = drawAxes(p, [lo, hi], [0, top * 1.1], niceTicks(lo, hi, p.W < 520 ? 6 : 10), String, opts.xTitle, opts.yTitle);
    const { g, H, m } = p;
    g.fillStyle = COLORS.dot;
    counts.forEach((c, i) => {
      if (!c) return;
      const x0 = x(lo + i * bin);
      const x1 = x(lo + (i + 1) * bin);
      g.fillRect(x0 + 0.5, y(c), Math.max(1, x1 - x0 - 1), y(0) - y(c));
    });
    const med = median(gaps);
    g.strokeStyle = COLORS.mean;
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(x(med), m.t);
    g.lineTo(x(med), H - m.b);
    g.stroke();
    drawNote(p, opts.note(gaps.length, med, Math.min(...gaps), Math.max(...gaps)));
  }

  const api = {
    HR_UNITS,
    parseHeartRate,
    describeHeartRate,
    toHex,
    median,
    elapsedSecs,
    buildTrialLog,
    beatSeries,
    packetGaps,
    trailingStats,
    drawSeries,
    drawGapHistogram,
    PolarSession,
  };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.Polar = api;
})(typeof window !== "undefined" ? window : globalThis);
