// Polar heart rate bands over Web Bluetooth: packet parsing, a session
// recorder, the per-trial log block, and the live plots.
//
// Every band is recorded through the standard Heart Rate service. On a Polar
// H9 nothing else is touched: on an unpaired link, accessing other services
// makes the operating system start a pairing that drops the connection 30 s
// later if nothing confirms it. A Polar H10 or Verity Sense also streams its
// accelerometer and its ECG (H10) or optical signal (Verity) over the Polar
// Measurement Data service (polar_pmd.js). Ten space bar presses felt by the
// accelerometer sync the sensor's clock to the controller clock, at the start
// and at the end of the session; the ECG then times each beat on that clock.
//
// Loaded as a plain <script> after polar_pmd.js by deploy_frontend/index.html
// (exports on `window.Polar`) and as a CommonJS module by
// tools/tests/test_polar_hr.js. Nothing above `class PolarSession` needs a browser.
(function (root) {
  "use strict";

  const PMD = typeof module !== "undefined" && module.exports ? require("./polar_pmd.js") : root.PolarPmd;
  const MEAS = PMD ? PMD.MEAS : { ECG: 0, PPG: 1, ACC: 2 };

  const HR_SERVICE = "heart_rate";
  const HR_MEASUREMENT = "heart_rate_measurement";
  const OPEN_ATTEMPTS = 4;
  const SUPPORTED_NAME = /^Polar (H9|H10|Sense)\b/;

  /** h9 | h10 | verity from the advertised name (a Verity Sense calls itself "Polar Sense …"); null for others. */
  function sensorKind(name) {
    const m = /^Polar (H9|H10|Sense)\b/.exec(name || "");
    return m ? { H9: "h9", H10: "h10", Sense: "verity" }[m[1]] : null;
  }

  // Bands with the measurement service: the signal streamed besides the
  // accelerometer, its trial-log key, and what the beat times come from
  // (the Verity's pulses are not timed here: its optical signal is saved).
  const WAVE = {
    h10: { mt: MEAS.ECG, key: "hr_ecg", source: "ecg_r_peak" },
    verity: { mt: MEAS.PPG, key: "hr_ppg", source: null },
  };

  const SYNC_MAX_SD_MS = 15;   // a sync whose press offsets spread more than this (SD) is not used
  const SYNC_SETTLE_MS = 4000; // longest wait for the accelerometer data of the last press
  const BEAT_CONTEXT = 30;     // neighbouring beats used to match a beat to its R-peak
  // A beat is matched to its R-peak in the background once it arrived this
  // long ago (its ECG and its neighbours are in by then), a few per second, so
  // saving a trial log only looks the matches up and never stalls the game.
  const BEAT_SETTLE_MS = 12000;
  const BEAT_JOB_MS = 1000;
  const BEAT_JOB_MAX = 10;

  // Written into every trial log next to the measurements.
  const HR_UNITS = {
    arrival_elapsed_secs: "packet arrival time, seconds on the controller clock (zero = session_info.app_start_unix_ns, the game start), comparable with present_elapsed_secs",
    hr: "bpm",
    rr_ms: "ms",
    raw: "packet bytes, hex",
    trial_start_elapsed_secs: "controller clock when the trial switched to playing",
    trial_end_elapsed_secs: "controller clock when the trial log was written",
    connection_events: "elapsed_secs on the controller clock",
  };

  // Added for an H10 / Verity Sense (an H9's trial logs are unchanged).
  const PMD_UNITS = {
    beat_elapsed_secs: "time of the heartbeat that ends the packet's last RR interval (its R-peak in the ECG), seconds on the controller clock via the sync; null when the packet has no RR or the beat could not be timed. An earlier beat of the same packet = this minus the later RR values",
    firmware: "sensor firmware (standard Device Information service)",
    hr_beat_source: "what beat_elapsed_secs comes from: ecg_r_peak (H10); null when beats are not timed (Verity Sense)",
    hr_beat_signal_hz: "sample rate of that signal",
    hr_beats_reported: "RR values in this file",
    hr_beats_timed: "of those, how many were matched to their R-peak and have a time on the controller clock",
    hr_rr_check_sd_ms: "SD over the matched beats of (interval between their R-peaks in the ECG − the band's own RR), ms: how well the beat detection agrees with the band's",
    sync_info: "clock sync in effect for this file. status: start | end | start+end | null. start / end: first press on the controller clock, sensor pressed, accelerometer rate (Hz) and range (g), presses used, SD of their offsets (ms). drift_ppm: how much faster the sensor's clock runs than the controller clock (µs per s), from the two syncs. precision_ms: one standard error of the clock mapping at the middle of this trial (sync error only). problems: why the times may be missing or less precise",
  };
  const WAVE_UNITS = {
    hr_ecg: "ECG frames received since the previous trial log: arrival_elapsed_secs, first_elapsed_secs (first sample on the controller clock via the sync; null when unsynced), dt_ms (sample spacing on the sensor clock), uV (µV)",
    hr_ppg: "optical frames received since the previous trial log: arrival_elapsed_secs, first_elapsed_secs (first sample on the controller clock via the sync; null when unsynced), dt_ms (sample spacing on the sensor clock), ppg0, ppg1, ppg2, ambient (raw units)",
  };
  // The per-session file with every sync attempt (hr_sync.json).
  const SYNC_FILE_UNITS = {
    kind: "start: before the game; end: after it",
    accepted: "used for the clock mapping: at least half of the presses (and 3) felt, their offsets' SD at most 15 ms",
    reason: "why not: nodata (no accelerometer data), few (too few presses felt), irregular (offsets spread too much)",
    key_elapsed_secs: "keydown time (event.timeStamp) on the controller clock",
    onset_elapsed_secs: "impact onset in the accelerometer on the controller clock (via the syncs of its clock segment; via packet arrival times if it has none)",
    residual_ms: "keydown minus impact onset on that clock (ms)",
    acc: "accelerometer from 3 s before the first press to 1 s after the last: t_elapsed_secs (as onset_elapsed_secs), x, y, z (mG)",
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
   * performance.now() milliseconds → seconds since `clockZeroMs` (the
   * controller clock's zero, the game start, so the result compares with
   * `present_elapsed_secs`), kept as a 32-bit float like it. Packets that arrived before that come out negative.
   */
  function elapsedSecs(t_ms, clockZeroMs) {
    return Math.fround((t_ms - clockZeroMs) / 1000);
  }

  /**
   * The `hr_info` / `hr_meas` pair of one trial log, from packets and events
   * given in performance.now() milliseconds.
   */
  function buildTrialLog(device, packets, events, clockZeroMs, trialStartMs, trialEndMs) {
    return {
      hr_info: {
        device,
        trial_start_elapsed_secs: trialStartMs === null ? null : elapsedSecs(trialStartMs, clockZeroMs),
        trial_end_elapsed_secs: elapsedSecs(trialEndMs, clockZeroMs),
        units: HR_UNITS,
        connection_events: events.map((e) => ({ elapsed_secs: elapsedSecs(e.t_ms, clockZeroMs), event: e.event })),
      },
      hr_meas: packets.map((p) => ({
        arrival_elapsed_secs: elapsedSecs(p.t_ms, clockZeroMs),
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

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const round = (v, k) => (v === null || v === undefined || !Number.isFinite(v) ? null : Math.round(v * k) / k);

  function sampleSd(a) {
    if (a.length < 2) return null;
    const m = a.reduce((s, v) => s + v, 0) / a.length;
    return Math.sqrt(a.reduce((s, v) => s + (v - m) ** 2, 0) / (a.length - 1));
  }

  /**
   * One Web Bluetooth connection to a band; accumulates everything received.
   * A lost link is reconnected in the background (no click needed for a band
   * the page already has). On an H10 / Verity Sense the measurement streams
   * are restarted with it; the sensor's clock keeps running, so its syncs
   * stay valid unless the sensor was switched off (a clock restart).
   */
  class PolarSession {
    constructor(log = () => {}) {
      this.log = log;
      this.device = null;
      this.kind = null; // h9 | h10 | verity | null (another band)
      this.hr = []; // every packet { t_ms, hr, rr_ms, raw }; t_ms = performance.now() at arrival
      this.events = []; // { t_ms, event: connected | disconnected | silent }
      this.pending = []; // packets not yet handed to a trial log
      this.pendingEvents = [];
      this.t_connected_ms = null;
      this.onPacket = null; // ({ t_ms, hex, note }) for every packet
      this.onDisconnect = null; // the link is gone; a reconnection runs in the background
      this.onReconnect = null; // that reconnection succeeded
      this._linkUp = false;
      this._opening = false;
      this._token = 0; // bumped by connect() to stop a background reconnection
      this._reconnecting = false;
      this._onHrValue = (e) => this._hrPacket(e.target.value);
      this._onGattDisconnected = () => this._linkLost();
      // H10 / Verity Sense only:
      this.pmd = null; // PolarPmd.PmdDevice on the same link
      this.pmdError = null; // why the measurement service is not usable, while it is not
      this.firmware = null;
      this.syncs = []; // every sync attempt (see sync())
      this._beats = []; // every RR value received, in order: { rr, arrival, run }
      this._run = 0; // counts connections: beats are matched to R-peaks within one
      this._waveIdx = 0; // recorder frames before this index are in earlier trial logs
      this._kept = []; // trial files written so far: { relpath, json, take }
      this._logger = null; // { enqueue, sessionDir, clockZeroMs } from the controller
      this._streamsStopped = false;
      this._cache = new Map(); // per finished clock segment: envelope and wave samples
      this._resendWanted = false;
      this._resendRun = null;
      this._peakOf = []; // per beat, once settled: { seg, t (dev ms of its R-peak), diff } or null
      this._settledUpTo = 0; // beats before this index are settled
      this._beatTimer = null;
    }

    get connected() {
      return this._linkUp && !this._opening;
    }

    /** False for anything but the bands this recording was built and tested for. */
    get isSupportedDevice() {
      return !!this.device && SUPPORTED_NAME.test(this.device.name || "");
    }

    get reconnecting() {
      return this._reconnecting;
    }

    /** performance.now() of the last data of any kind (heart rate packet or measurement frame). */
    get lastDataMs() {
      const hr = this.hr.length ? this.hr[this.hr.length - 1].t_ms : 0;
      return Math.max(hr, (this.pmd && this.pmd.lastFrameAt) || 0);
    }

    /** The accelerometer is streaming, so the space bar presses of a sync can be felt now. */
    get canSync() {
      return !!(this.pmd && this.connected && this.pmd.active[MEAS.ACC]);
    }

    /** Trial files are being re-timed and sent again after a sync. */
    get resending() {
      return !!this._resendRun;
    }

    /** The latest sync attempt of `kind` (start | end), or null. */
    lastSync(kind) {
      for (let i = this.syncs.length - 1; i >= 0; i--) if (this.syncs[i].kind === kind) return this.syncs[i];
      return null;
    }

    /** Must be called from a user gesture (click). Calling it again keeps the data. */
    async connect() {
      const device = await navigator.bluetooth.requestDevice({
        // Any Polar device, or any band advertising the heart rate service:
        // a wrong choice is reported by `isSupportedDevice`, not hidden. The
        // other services are only opened on an H10 or Verity Sense.
        filters: [{ namePrefix: "Polar" }, { services: [HR_SERVICE] }],
        optionalServices: PMD ? [HR_SERVICE, PMD.PMD_SERVICE, "device_information"] : [HR_SERVICE],
      });
      this._token++; // a background reconnection stops at its next step
      while (this._opening) await sleep(100);
      if (device !== this.device) {
        if (this.device) {
          this.device.removeEventListener("gattserverdisconnected", this._onGattDisconnected);
          this._linkLost();
          try {
            this.device.gatt.disconnect();
          } catch (_) {}
        }
        this.device = device;
        device.addEventListener("gattserverdisconnected", this._onGattDisconnected);
        this._setUpSensor();
      }
      if (this._linkUp) return; // the background reconnection got there first
      await this._open();
    }

    /** A new band was chosen: what it can stream, and a fresh sync state. */
    _setUpSensor() {
      this.kind = sensorKind(this.device.name);
      this.pmd = null;
      this.pmdError = null;
      this.firmware = null;
      this.syncs = [];
      this._waveIdx = 0;
      this._streamsStopped = false;
      this._cache.clear();
      // Beats of an earlier band are not this sensor's.
      this._peakOf = new Array(this._beats.length).fill(null);
      this._settledUpTo = this._beats.length;
      if (!PMD || !WAVE[this.kind]) return;
      const pmd = new PMD.PmdDevice((msg) => this.log(msg));
      pmd.device = this.device;
      pmd.name = this.device.name || "?";
      pmd.autoReconnect = false; // this session reconnects the link
      pmd.useHeartRate = false; // and reads the heart rate itself
      this.pmd = pmd;
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

    /** Stop the sensor's measurement streams (end of the session, page closing). */
    stopStreams() {
      this._streamsStopped = true;
      if (this.pmd && this.pmd.linkUp) this.pmd.stopAll().catch(() => {});
    }

    /**
     * The trial-log block for everything received since the previous call.
     * Each packet, frame and event is returned exactly once, so reading
     * consecutive trial logs in order gives the unbroken stream.
     */
    takeTrialLog(clockZeroMs, trialStartMs, trialEndMs) {
      const device = this.device ? this.device.name : null;
      const rows = this.pending.splice(0);
      const events = this.pendingEvents.splice(0);
      if (!this.pmd) return buildTrialLog(device, rows, events, clockZeroMs, trialStartMs, trialEndMs);
      const frames = this.pmd.rec.frames;
      const wave = [];
      for (; this._waveIdx < frames.length; this._waveIdx++) {
        if (frames[this._waveIdx].mt === WAVE[this.kind].mt) wave.push(frames[this._waveIdx]);
      }
      return this._pmdTrialLog({ device, rows, events, wave, clockZeroMs, trialStartMs, trialEndMs });
    }

    /**
     * How the controller sends files (its retrying queue) and where this
     * session's folder is; called before each trial log is taken.
     */
    useLogger(enqueue, sessionDir, clockZeroMs) {
      if (!this.pmd) return;
      const first = !this._logger;
      this._logger = { enqueue, sessionDir, clockZeroMs };
      if (first) this._writeSyncFile(); // the start sync was done before the game
    }

    /** A trial file as sent: kept so that a later sync can re-time it and send it again. */
    keepTrialFile(relpath, json, block) {
      if (block && block._take) this._kept.push({ relpath, json, take: block._take });
    }

    /**
     * A sync from space bar presses (`keys`: keydown event.timeStamp, ms) at
     * the start or the end of the session (`kind`). Waits for the
     * accelerometer data of the last press, finds each press's impact, and
     * accepts the sync when at least half of the presses (and 3) were felt
     * and their offsets spread at most 15 ms (SD). An accepted sync re-times
     * every trial file written so far and sends it again.
     */
    async sync(kind, keys) {
      const pmd = this.pmd;
      const sel = pmd && pmd.active[MEAS.ACC] ? pmd.active[MEAS.ACC].sel : {};
      const res = {
        kind, keys: keys.slice(), sensor: this.kind, acc_hz: sel.SAMPLE_RATE ?? null, acc_range_g: sel.RANGE ?? null,
        accepted: false, reason: "nodata", seg: null, o: null, x: null, sd: null, se: null, used: 0, found: 0, presses: [], acc: null,
      };
      if (this.canSync && keys.length) {
        const rec = pmd.rec;
        const last = keys[keys.length - 1];
        const locate = () => this._mapping(rec.segment) || rec.envelope(true);
        for (let waited = 0; waited < SYNC_SETTLE_MS; waited += 200) {
          const buf = rec.streams[MEAS.ACC];
          const loc = locate();
          if (buf && buf.length && loc && PMD.toLaptop(loc, buf.t[buf.length - 1]) > last + 600) break;
          await sleep(200);
        }
        const buf = rec.streams[MEAS.ACC];
        const loc = locate();
        if (buf && buf.length > 20 && loc) {
          const r = PMD.analyzePresses(keys, buf, loc);
          const s = r.summary;
          const need = Math.max(3, Math.ceil(keys.length / 2));
          Object.assign(res, { seg: rec.segment, o: r.o, x: r.x, sd: s.sd ?? null, se: s.se ?? null, used: s.used, found: r.detected, presses: r.presses });
          res.accepted = r.o !== null && s.used >= need && s.sd <= SYNC_MAX_SD_MS;
          res.reason = res.accepted ? null : s.used < need ? "few" : "irregular";
          const i0 = PMD.lowerBound(buf.t, PMD.toDevice(loc, keys[0]) - 3000);
          const i1 = PMD.lowerBound(buf.t, PMD.toDevice(loc, last) + 1000);
          res.acc = { t: buf.t.slice(i0, i1), ch: buf.ch.map((c) => c.slice(i0, i1)) };
        }
      }
      this.syncs.push(res);
      this.log(`Sync (${kind}): ${res.accepted ? "accepted" : `not accepted (${res.reason})`}, ${res.used} of ${keys.length} presses used, SD ${res.sd === null ? "–" : res.sd.toFixed(1)} ms`);
      this._writeSyncFile();
      if (res.accepted) this._resendAll();
      return res;
    }

    _event(event) {
      const e = { t_ms: performance.now(), event };
      this.events.push(e);
      this.pendingEvents.push(e);
      return e;
    }

    _hrPacket(dv) {
      const row = { t_ms: performance.now(), ...parseHeartRate(dv), raw: toHex(dv) };
      if (this.pmd) {
        row.beats = row.rr_ms.map((rr) => this._beats.push({ rr, arrival: row.t_ms, run: this._run }) - 1);
        this._scheduleBeatJob();
      }
      this.hr.push(row);
      this.pending.push(row);
      if (this.onPacket) this.onPacket({ t_ms: row.t_ms, hex: row.raw, note: describeHeartRate(dv) });
    }

    _linkLost() {
      if (!this._linkUp) return; // already reported
      this._linkUp = false;
      this._event("disconnected");
      if (this.pmd) this.pmd._linkLost("browser");
      this.log("Band disconnected.");
      // While _open() is still retrying, it reports the outcome itself.
      if (this._opening) return;
      if (this.onDisconnect) this.onDisconnect();
      this._reconnect();
    }

    /** Background reconnection after a lost link, until it is back or connect() is called. */
    async _reconnect() {
      if (this._reconnecting) return;
      this._reconnecting = true;
      const token = this._token;
      let delay = 1000;
      try {
        while (token === this._token && !this._linkUp) {
          await sleep(delay);
          if (token !== this._token || this._linkUp) break;
          try {
            await this._open();
            if (token === this._token) {
              this.log("Band reconnected.");
              if (this.onReconnect) this.onReconnect();
            }
          } catch (err) {
            this.log(`Reconnection failed (${err.name}); next try in ${Math.round(delay / 1000)} s.`);
            delay = Math.min(2 * delay, 10000);
          }
        }
      } finally {
        this._reconnecting = false;
      }
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
            if (attempt >= OPEN_ATTEMPTS) {
              this._linkLost(); // a half-open link counts as lost
              try {
                this.device.gatt.disconnect();
              } catch (_) {}
              throw err;
            }
            this.log(`Connection attempt ${attempt} failed (${err.name}); retrying…`);
            await sleep(1000 * attempt);
          }
        }
      } finally {
        this._opening = false;
      }
    }

    async _openOnce() {
      const gatt = await this.device.gatt.connect();
      this._linkUp = true;
      this._run++;
      this.t_connected_ms = this._event("connected").t_ms;
      this.log(`Connected to ${this.device.name}; looking up the heart rate service…`);

      const hrChar = await (await gatt.getPrimaryService(HR_SERVICE)).getCharacteristic(HR_MEASUREMENT);
      hrChar.addEventListener("characteristicvaluechanged", this._onHrValue); // the same listener is added only once
      await hrChar.startNotifications();
      const setup_s = (performance.now() - this.t_connected_ms) / 1000;
      this.log(`Heart rate packets switched on (setup took ${setup_s.toFixed(1)} s); waiting for the first packet…`);
      if (this.pmd) await this._openPmd(gatt);
    }

    /**
     * The measurement streams on an H10 / Verity Sense: the accelerometer
     * (for the syncs) and the ECG or optical signal, at their highest rates.
     * After a reconnection the same streams are started again (a stream left
     * running on the sensor is restarted). A failure leaves the heart rate
     * recording as it is; it is retried at the next connection.
     */
    async _openPmd(gatt) {
      const pmd = this.pmd;
      try {
        await pmd._openOnce();
        if (pmd.sdkMode) await pmd.setSdkMode(false); // SDK mode switches off the sensor's own heart rate
        const wanted = Object.entries(pmd.wanted).map(([mt, sel]) => [Number(mt), sel]);
        if (!wanted.length && !this._streamsStopped) {
          for (const mt of [MEAS.ACC, WAVE[this.kind].mt]) wanted.push([mt, PMD.pickSettings(mt, await pmd.getSettings(mt))]);
        }
        for (const [mt, sel] of wanted) await pmd.startStream(mt, sel);
        this.pmdError = null;
      } catch (err) {
        this.pmdError = err.message;
        this.log(`Measurement service not usable (${err.message}): the heart rate is recorded, but the sensor cannot be synchronized.`);
      }
      if (!this.firmware) {
        try {
          const ch = await (await gatt.getPrimaryService("device_information")).getCharacteristic("firmware_revision_string");
          this.firmware = new TextDecoder().decode(await ch.readValue()).replace(/\0+$/, "").trim() || null;
        } catch (err) {
          this.log(`Firmware version not readable (${err.name}).`);
        }
      }
    }

    // ── clock mapping (H10 / Verity Sense) ──

    /** Envelope mapping of clock segment `seg` (from packet arrival times). */
    _env(seg) {
      const rec = this.pmd.rec;
      if (seg === rec.segment) return rec.envelope();
      const key = `env ${seg}`;
      if (!this._cache.has(key)) {
        const to = seg + 1 < rec.segments.length ? rec.segments[seg + 1].from : rec.pairs.a.length;
        this._cache.set(key, PMD.envelopeFit(rec.pairs.a, rec.pairs.d, rec.segments[seg].from, to));
      }
      return this._cache.get(key);
    }

    /** ECG / optical samples of clock segment `seg`. */
    _waveBuffer(seg) {
      const rec = this.pmd.rec;
      const mt = WAVE[this.kind].mt;
      if (seg === rec.segment) return rec.streams[mt] || null;
      const key = `wave ${seg}`;
      if (!this._cache.has(key)) this._cache.set(key, PMD.bufferFromFrames(rec.frames, seg, mt));
      return this._cache.get(key);
    }

    /** Clock segment of laptop time `t`: a new one starts when the sensor's clock restarted. */
    _segAt(t) {
      const segs = this.pmd.rec.segments;
      let s = 0;
      for (let i = 1; i < segs.length; i++) if (segs[i].t <= t) s = i;
      return s;
    }

    /** The latest accepted sync of `kind` in clock segment `seg`. */
    _syncIn(seg, kind) {
      for (let i = this.syncs.length - 1; i >= 0; i--) {
        const s = this.syncs[i];
        if (s.accepted && s.seg === seg && s.kind === kind) return s;
      }
      return null;
    }

    /**
     * Sensor ms → controller ms in clock segment `seg`, from its syncs: a
     * line through the start and end syncs, or one sync with the drift from
     * packet arrival times. Null without a sync.
     */
    _mapping(seg) {
      const start = this._syncIn(seg, "start");
      const end = this._syncIn(seg, "end");
      if (!start && !end) return null;
      const m = PMD.chooseMapping([start, end].filter(Boolean).map((s) => ({ o: s.o, x: s.x })), this._env(seg));
      return m && { ...m, seg, start, end };
    }

    /**
     * R-peaks of the beats `ids` (indices into this._beats): Map id → { seg,
     * t (dev ms), diff (R–R − RR, ms) } or null. Neighbouring beats are used
     * for the matching; beats are only matched to R-peaks of their own clock
     * segment.
     */
    _matchBeats(ids) {
      const out = new Map();
      if (!ids.length || !WAVE[this.kind].source) {
        for (const i of ids) out.set(i, null);
        return out;
      }
      const lo = Math.max(0, Math.min(...ids) - BEAT_CONTEXT);
      const hi = Math.min(this._beats.length, Math.max(...ids) + BEAT_CONTEXT + 1);
      const want = new Set(ids);
      const bySeg = new Map();
      for (let i = lo; i < hi; i++) {
        const s = this._segAt(this._beats[i].arrival);
        if (!bySeg.has(s)) bySeg.set(s, []);
        bySeg.get(s).push(i);
      }
      const mt = WAVE[this.kind].mt;
      const rate = (this.pmd.rec.ctx[mt] && this.pmd.rec.ctx[mt].rateHz) || 130;
      for (const [seg, all] of bySeg) {
        const only = new Set();
        all.forEach((i, k) => want.has(i) && only.add(k));
        if (!only.size) continue;
        const loc = this._mapping(seg) || this._env(seg);
        const buf = this._waveBuffer(seg);
        if (!loc || !buf || !buf.length) continue;
        const beats = all.map((i) => this._beats[i]);
        const from = PMD.toDevice(loc, beats[0].arrival - 4000);
        const to = PMD.toDevice(loc, beats[beats.length - 1].arrival + 500);
        const peaks = PMD.detectRPeaks(buf, from, to, rate);
        PMD.matchBeats(beats, peaks, (d) => PMD.toLaptop(loc, d), { only }).forEach((r, k) => {
          if (only.has(k)) out.set(all[k], r ? { seg, t: peaks[r.j].t, diff: r.diff } : null);
        });
      }
      for (const i of ids) if (!out.has(i)) out.set(i, null);
      return out;
    }

    /** Settled beats are matched once, in the background, a few at a time. */
    _scheduleBeatJob() {
      if (this._beatTimer || !WAVE[this.kind] || !WAVE[this.kind].source) return;
      this._beatTimer = setTimeout(() => {
        const run = () => {
          this._beatTimer = null;
          const n = this._settleBeats(BEAT_JOB_MAX);
          if (n || this._settledUpTo < this._beats.length) this._scheduleBeatJob();
        };
        // Between the game's frames when the browser offers the time.
        if (root.requestIdleCallback) root.requestIdleCallback(run, { timeout: BEAT_JOB_MS });
        else run();
      }, BEAT_JOB_MS);
    }

    /** Match up to `max` settled beats; returns how many were done. */
    _settleBeats(max) {
      if (!this.pmd) return 0;
      const cutoff = performance.now() - BEAT_SETTLE_MS;
      const ids = [];
      for (let i = this._settledUpTo; i < this._beats.length && ids.length < max && this._beats[i].arrival < cutoff; i++) ids.push(i);
      if (!ids.length) return 0;
      for (const [i, r] of this._matchBeats(ids)) this._peakOf[i] = r;
      this._settledUpTo = ids[ids.length - 1] + 1;
      return ids.length;
    }

    /**
     * Times of the beats `idx`: Map idx → { lap (controller ms of its R-peak,
     * null when unsynced), diff }. Settled beats come from the background
     * matches; the few recent ones are matched now.
     */
    _timeBeats(idx) {
      const out = new Map();
      if (!idx.length || !WAVE[this.kind].source) return out;
      const fresh = this._matchBeats(idx.filter((i) => this._peakOf[i] === undefined));
      for (const i of idx) {
        const r = this._peakOf[i] !== undefined ? this._peakOf[i] : fresh.get(i);
        if (!r) continue;
        const m = this._mapping(r.seg);
        out.set(i, { lap: m ? PMD.toLaptop(m, r.t) : null, diff: r.diff });
      }
      return out;
    }

    /** The trial-log block of an H10 / Verity Sense, from what one takeTrialLog() handed over. */
    _pmdTrialLog(take) {
      const { device, rows, events, wave, clockZeroMs, trialStartMs, trialEndMs } = take;
      const E = (ms) => elapsedSecs(ms, clockZeroMs);
      const w = WAVE[this.kind];
      const idx = [];
      for (const r of rows) for (const i of r.beats || []) idx.push(i);
      const timed = this._timeBeats(idx);
      const meas = rows.map((p) => {
        const last = p.beats && p.beats.length ? timed.get(p.beats[p.beats.length - 1]) : null;
        return {
          arrival_elapsed_secs: E(p.t_ms),
          beat_elapsed_secs: last && last.lap !== null ? E(last.lap) : null,
          hr: p.hr,
          rr_ms: p.rr_ms,
          raw: p.raw,
        };
      });
      const names = PMD.STREAM_INFO[w.mt].channels;
      const frames = [];
      const segs = new Set();
      let undecoded = 0;
      for (const f of wave) {
        if (f.err || !f.ch || f.first === null) {
          undecoded += f.err ? 1 : 0;
          continue;
        }
        segs.add(f.seg);
        const m = this._mapping(f.seg);
        const o = { arrival_elapsed_secs: E(f.t), first_elapsed_secs: m ? E(PMD.toLaptop(m, f.first)) : null, dt_ms: round(f.dt, 1e6) };
        names.forEach((name, c) => (o[name] = f.ch[c]));
        frames.push(o);
      }
      const seg = wave.length ? wave[wave.length - 1].seg : this._segAt(trialEndMs);
      const mid = ((trialStartMs === null ? trialEndMs : trialStartMs) + trialEndMs) / 2;
      const rate = (this.pmd.rec.ctx[w.mt] && this.pmd.rec.ctx[w.mt].rateHz) || null;
      const hr_info = {
        device,
        firmware: this.firmware,
        hr_beat_source: w.source,
        hr_beat_signal_hz: w.source ? rate : null,
        hr_beats_reported: idx.length,
        hr_beats_timed: idx.filter((i) => timed.has(i) && timed.get(i).lap !== null).length,
        hr_rr_check_sd_ms: round(sampleSd(idx.filter((i) => timed.has(i)).map((i) => timed.get(i).diff)), 100),
        trial_start_elapsed_secs: trialStartMs === null ? null : E(trialStartMs),
        trial_end_elapsed_secs: E(trialEndMs),
        units: { ...HR_UNITS, ...PMD_UNITS, [w.key]: WAVE_UNITS[w.key] },
        connection_events: events.map((e) => ({ elapsed_secs: E(e.t_ms), event: e.event })),
        sync_info: this._syncInfo(seg, mid, E, segs.size > 1, undecoded),
      };
      return { hr_info, hr_meas: meas, [w.key]: frames, _take: take };
    }

    _syncInfo(seg, midMs, E, restarted, undecoded) {
      const problems = [];
      if (this.pmdError) problems.push(`measurement service not usable (${this.pmdError}): no sync and no ${WAVE[this.kind].key === "hr_ecg" ? "ECG" : "optical signal"}`);
      const m = this.pmd.rec.segments.length ? this._mapping(seg) : null;
      const start = m ? m.start : null;
      const end = m ? m.end : null;
      const status = start && end ? "start+end" : start ? "start" : end ? "end" : null;
      let drift_ppm = null;
      let precision_ms = null;
      if (!status) problems.push("not synchronized: no successful sync covers this part of the recording, so nothing has a controller-clock time");
      else if (status === "start") problems.push("no end sync (yet): the clock drift since the start sync is not corrected and the precision is unknown");
      else if (status === "end") {
        problems.push(this.syncs.some((s) => s.accepted && s.kind === "start")
          ? "the sensor's clock restarted after the start sync (the sensor was switched off): only the end sync applies, without drift correction"
          : "no start sync: only the end sync applies, without drift correction");
      } else if (m.drift_from === "blocks") {
        drift_ppm = round((1 / (1 + m.s) - 1) * 1e6, 100); // > 0: the sensor's clock runs fast
        const a = PMD.toLaptop(m, start.x);
        const b = PMD.toLaptop(m, end.x);
        const f = (midMs - a) / (b - a);
        precision_ms = round(Math.sqrt(((1 - f) * start.se) ** 2 + (f * end.se) ** 2), 100);
        if (Math.abs(drift_ppm) > 100) problems.push(`a drift of ${drift_ppm} ppm is implausible for the sensor's clock (expect within ±50): one of the syncs is off`);
      } else problems.push("the two syncs are less than a minute apart: the drift comes from packet arrival times and the precision is unknown");
      if (restarted) problems.push("the sensor's clock restarted during this trial");
      if (undecoded) problems.push(`${undecoded} frame(s) could not be decoded`);
      const one = (s) => s && {
        elapsed_secs: E(s.keys[0]), sensor: s.sensor, acc_hz: s.acc_hz, acc_range_g: s.acc_range_g, presses_used: s.used, sd_ms: round(s.sd, 100),
      };
      return { status, start: one(start), end: one(end), drift_ppm, precision_ms, problems };
    }

    // ── files ──

    /** Every sync attempt with its presses and accelerometer, in the session folder. */
    _writeSyncFile() {
      if (!this._logger || !this.syncs.length) return;
      const E = (ms) => elapsedSecs(ms, this._logger.clockZeroMs);
      const content = JSON.stringify({
        device: this.device ? this.device.name : null,
        firmware: this.firmware,
        units: SYNC_FILE_UNITS,
        syncs: this.syncs.map((s) => {
          const loc = s.seg === null ? null : this._mapping(s.seg) || this._env(s.seg);
          const L = (d) => (loc ? E(PMD.toLaptop(loc, d)) : null);
          return {
            kind: s.kind, accepted: s.accepted, reason: s.reason, sensor: s.sensor, acc_hz: s.acc_hz, acc_range_g: s.acc_range_g,
            presses_used: s.used, sd_ms: round(s.sd, 100), se_ms: round(s.se, 100),
            presses: s.keys.map((k, i) => {
              const p = s.presses[i] || {};
              const felt = p.onset !== undefined;
              return {
                key_elapsed_secs: E(k),
                onset_elapsed_secs: felt ? L(p.onset) : null,
                residual_ms: felt && loc ? round(k - PMD.toLaptop(loc, p.onset), 1000) : null,
                used: felt && !p.outlier,
                why_not: felt ? (p.outlier ? "outlier" : null) : p.reason || "no data",
              };
            }),
            acc: s.acc && { t_elapsed_secs: s.acc.t.map(L), x: s.acc.ch[0], y: s.acc.ch[1], z: s.acc.ch[2] },
          };
        }),
      });
      this._logger.enqueue(`${this._logger.sessionDir}/hr_sync.json`, content);
    }

    /** Re-time every kept trial file with the syncs as they are now and send it again. */
    _resendAll() {
      this._resendWanted = true;
      if (!this._resendRun) this._resendRun = this._resendLoop().finally(() => (this._resendRun = null));
      return this._resendRun;
    }

    async _resendLoop() {
      while (this._resendWanted && this._logger) {
        this._resendWanted = false;
        for (const f of this._kept) {
          const obj = JSON.parse(f.json);
          const block = this._pmdTrialLog(f.take);
          for (const key of Object.keys(block)) if (key !== "_take") obj[key] = block[key];
          f.json = JSON.stringify(obj, null, 2);
          this._logger.enqueue(f.relpath, f.json);
          await sleep(0); // keep the page responsive
        }
      }
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
    PMD_UNITS,
    sensorKind,
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
