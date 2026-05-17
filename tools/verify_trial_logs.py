#!/usr/bin/env python3
"""
verify_trial_logs.py — Cross-platform trial log analyzer.

Parses trial logs from native (Python controller) and WASM (JS controller),
generates per-trial plots, a session overview plot, and an aggregate summary
comparing timing, frame delivery, and logging consistency.

Usage:
    python tools/verify_trial_logs.py out/trial_logs/                         # all native logs
    python tools/verify_trial_logs.py monkeyA_2026-05-12_14-32-08.zip         # web download bundle
    python tools/verify_trial_logs.py wasm_logs/trial_logs_2026-04-08.json    # legacy single-file WASM dump
    python tools/verify_trial_logs.py out/trial_logs/ wasm_logs.zip           # cross-platform comparison

Requirements: matplotlib (pip install matplotlib)
"""

import argparse
import json
import math
import sys
import tempfile
import zipfile
from dataclasses import dataclass, field
from pathlib import Path

try:
    import matplotlib
    matplotlib.use("Agg")  # non-interactive backend
    import matplotlib.pyplot as plt
    import matplotlib.ticker as mticker
except ImportError:
    print("Error: matplotlib is required.  Install with:  pip install matplotlib")
    sys.exit(1)


# ──────────────────────────────────────────────────────────────────────────────
# Constants
# ──────────────────────────────────────────────────────────────────────────────
EXPECTED_DT = 1.0 / 60.0          # 16.67 ms at 60 FPS
OUTLIER_LO = EXPECTED_DT * 0.5    # < 8.33 ms
OUTLIER_HI = EXPECTED_DT * 2.0    # > 33.33 ms
OUTPUT_DIR = "out/analysis"

OUTCOME_COLORS = {
    "advance": "#55cc88",
    "stay": "#e8a735",
    "retroceed": "#e05555",
}


# ──────────────────────────────────────────────────────────────────────────────
# Data structures
# ──────────────────────────────────────────────────────────────────────────────
@dataclass
class FrameRecord:
    frame_number: int
    present_elapsed_secs: float = 0.0
    camera_radius: float = 0.0
    camera_x: float = 0.0
    camera_y: float = 0.0
    camera_z: float = 0.0
    attempts: int = 0
    current_alignment: float = 0.0
    current_angle: float = 0.0
    is_animating: bool = False
    win_elapsed_secs: float = 0.0
    render_frame_number: int = 0
    # Commands_sent flags surfaced for the actions-over-time raster.
    # toggle_blank and toggle_stop_rendering are merged into a single channel
    # because the FSM always fires them together at trial boundaries.
    cmd_rotate_left: bool = False
    cmd_rotate_right: bool = False
    cmd_check: bool = False
    cmd_reset: bool = False
    cmd_toggle_blank_or_stop: bool = False
    cmd_animation_door: bool = False


@dataclass
class TrialInfo:
    label: str               # e.g. "trial_000_run_0001" or "wasm_trial_3"
    platform: str            # "native" or "wasm"
    # Path of this trial relative to the input root, no extension. Used to
    # mirror the input directory tree into out/analysis/<platform>/...
    source_rel_path: str = ""
    level_index: int = 0
    active_chain: int = 0
    trial_index: int = 0
    outcome: str = ""
    nr_attempts: int = 0
    elapsed_time: float = 0.0   # wall-clock from controller (no_anim + anim)
    elapsed_time_no_anim: float = 0.0
    elapsed_time_anim: float = 0.0
    trial_run_counter: int = 0
    cosine_threshold: float = 0.0
    frames: list = field(default_factory=list)  # sorted list[FrameRecord]


@dataclass
class TrialStats:
    label: str
    platform: str
    n_frames: int = 0
    gaps: int = 0
    duplicates: int = 0
    dt_mean: float = 0.0
    dt_std: float = 0.0
    dt_min: float = 0.0
    dt_max: float = 0.0
    outlier_pct: float = 0.0
    avg_fps: float = 0.0
    # Frame-pacing tail metrics (GN/HUB convention: average of worst N% Δt).
    dt_1pct_low: float = 0.0
    dt_01pct_low: float = 0.0
    fps_1pct_low: float = 0.0
    fps_01pct_low: float = 0.0
    game_elapsed_final: float = 0.0
    wall_elapsed: float = 0.0
    timing_drift: float = 0.0
    freeze_count: int = 0


# ──────────────────────────────────────────────────────────────────────────────
# Loading helpers
# ──────────────────────────────────────────────────────────────────────────────
def _parse_trial_dict(d: dict, label: str, platform: str) -> TrialInfo:
    """Parse a single trial dict (same schema for native/WASM)."""
    no_anim = float(d.get("elapsed_time_no_anim", 0.0) or 0.0)
    anim_t = float(d.get("elapsed_time_anim", 0.0) or 0.0)
    total = no_anim + anim_t if (no_anim or anim_t) else float(d.get("elapsed_time", 0.0) or 0.0)
    trial = TrialInfo(
        label=label,
        platform=platform,
        level_index=d.get("level_index", 0),
        active_chain=d.get("active_chain", 0),
        trial_index=d.get("trial_index_in_chain", 0),
        outcome=d.get("outcome", ""),
        nr_attempts=d.get("nr_attempts", 0),
        elapsed_time=total,
        cosine_threshold=d.get("trial_config", {}).get("cosine_alignment_threshold", 0.0),
    )
    trial.elapsed_time_no_anim = no_anim if (no_anim or anim_t) else total
    trial.elapsed_time_anim = anim_t
    trial.trial_run_counter = int(d.get("trial_run_counter", 0) or 0)

    raw_frames = d.get("frames", {})

    # Current controllers write `frames` as a dict keyed by frame_number,
    # where each value is `{state_read, commands_sent}`. Keep backward
    # compatibility with older dumps that used a list or direct state rows.
    if isinstance(raw_frames, dict):
        frame_items = raw_frames.items()
    elif isinstance(raw_frames, list):
        frame_items = enumerate(raw_frames)
    else:
        frame_items = []

    for fnum_key, entry in frame_items:
        if isinstance(entry, dict):
            if "state_read" in entry and isinstance(entry.get("state_read"), dict):
                sr = entry.get("state_read", {})
            else:
                sr = entry
            cs = entry.get("commands_sent", {}) if isinstance(entry.get("commands_sent"), dict) else {}
        else:
            sr = {}
            cs = {}

        fnum_default = fnum_key if isinstance(fnum_key, int) else 0
        # Legacy logs only had `elapsed_secs`; fall back so old sessions still load.
        present = float(sr.get("present_elapsed_secs", sr.get("elapsed_secs", 0.0)))
        # SHM-direct names are canonical; the `or`-chained fallbacks accept
        # logs from before the schema unification (camera_position array,
        # nr_attempts, cosine_alignment).
        cam_pos = sr.get("camera_position") or [0.0, 0.0, 0.0]
        trial.frames.append(FrameRecord(
            frame_number=int(sr.get("frame_number", fnum_default)),
            present_elapsed_secs=present,
            camera_radius=float(sr.get("camera_radius", 0.0)),
            camera_x=float(sr.get("camera_x", cam_pos[0])),
            camera_y=float(sr.get("camera_y", cam_pos[1])),
            camera_z=float(sr.get("camera_z", cam_pos[2])),
            attempts=int(sr.get("attempts", sr.get("nr_attempts", 0))),
            current_alignment=float(sr.get("current_alignment", sr.get("cosine_alignment", 0.0))),
            current_angle=float(sr.get("current_angle", 0.0)),
            is_animating=bool(sr.get("is_animating", False)),
            win_elapsed_secs=float(sr.get("win_elapsed_secs", 0.0)),
            render_frame_number=int(sr.get("render_frame_number", 0)),
            cmd_rotate_left=bool(cs.get("rotate_left", False)),
            cmd_rotate_right=bool(cs.get("rotate_right", False)),
            cmd_check=bool(cs.get("check", False)),
            cmd_reset=bool(cs.get("reset", False)),
            cmd_toggle_blank_or_stop=bool(cs.get("toggle_blank", False)) or bool(cs.get("toggle_stop_rendering", False)),
            cmd_animation_door=bool(cs.get("animation_door", False)),
        ))

    # Sort by frame_number for consistent analysis
    trial.frames.sort(key=lambda f: f.frame_number)
    return trial


def _load_json_file(jf: Path, source_rel: str, platform: str) -> list[TrialInfo]:
    """Parse one JSON file into one (dict root) or many (list root) trials."""
    with open(jf) as f:
        data = json.load(f)

    trials = []
    if isinstance(data, list):
        # WASM legacy format: array of trial dicts in one big JSON.
        for i, d in enumerate(data):
            label = f"{jf.stem}_trial_{i:03d}"
            t = _parse_trial_dict(d, label, platform)
            t.source_rel_path = f"{source_rel}_trial_{i:03d}"
            trials.append(t)
    elif isinstance(data, dict):
        t = _parse_trial_dict(data, jf.stem, platform)
        t.source_rel_path = source_rel
        trials.append(t)
    else:
        print(f"Warning: unexpected JSON root type in {jf}, skipping")
    return trials


def _load_dir_recursive(d: Path, root: Path, root_label: str, platform: str) -> list[TrialInfo]:
    """Walk `d` for JSON trial files; preserve paths relative to `root`."""
    trials = []
    # Skip level-summary files; they describe the whole level, not a trial.
    json_files = sorted(jf for jf in d.rglob("*.json") if "_summary_" not in jf.name)
    if not json_files:
        print(f"Warning: no .json files found under {d}")
        return trials
    for jf in json_files:
        rel = jf.relative_to(root).with_suffix("")
        source_rel = f"{root_label}/{rel}" if str(rel) != "." else root_label
        trials.extend(_load_json_file(jf, source_rel, platform))
    return trials


def load_path(path: str, platform: str) -> list[TrialInfo]:
    """Load trials from a file, directory, or zip archive.

    Each loaded trial records its position relative to the input root in
    `source_rel_path`, so the analysis output can mirror the input layout.
    """
    p = Path(path)
    if not p.exists():
        print(f"Warning: path {p} does not exist, skipping")
        return []

    if p.is_file() and p.suffix.lower() == ".zip":
        with zipfile.ZipFile(p) as zf, tempfile.TemporaryDirectory() as tmp:
            zf.extractall(tmp)
            root = Path(tmp)
            # Many bundles wrap everything in a single top-level dir named
            # like the zip itself; descend into it so the zip-stem doesn't
            # appear twice in source_rel_path.
            children = [c for c in root.iterdir() if not c.name.startswith(".")]
            if len(children) == 1 and children[0].is_dir():
                root = children[0]
            return _load_dir_recursive(root, root, p.stem, platform)

    if p.is_file():
        return _load_json_file(p, p.stem, platform)

    return _load_dir_recursive(p, p, p.name, platform)


# ──────────────────────────────────────────────────────────────────────────────
# Analysis
# ──────────────────────────────────────────────────────────────────────────────
def _drift_series(frames) -> list[float]:
    """`(present - present[0]) - row_count / 60`, animation pauses included.

    Naive drift matching what the controller writes in its level summary —
    every logged frame counts as one 60 Hz tick, including the rows captured
    during door animations. This intentionally absorbs the animation pauses
    into the reported drift rather than excluding them.
    """
    if not frames:
        return []
    t0 = frames[0].present_elapsed_secs
    return [(f.present_elapsed_secs - t0) - i / 60.0 for i, f in enumerate(frames)]


def analyse_trial(trial: TrialInfo) -> TrialStats:
    """Compute per-trial statistics."""
    stats = TrialStats(label=trial.label, platform=trial.platform)
    frames = trial.frames

    if len(frames) < 2:
        stats.n_frames = len(frames)
        return stats

    stats.n_frames = len(frames)

    # Frame continuity
    nums = [f.frame_number for f in frames]
    for i in range(1, len(nums)):
        diff = nums[i] - nums[i - 1]
        if diff > 1:
            stats.gaps += diff - 1
        elif diff == 0:
            stats.duplicates += 1

    times = [f.present_elapsed_secs for f in frames]
    deltas = [times[i] - times[i - 1] for i in range(1, len(times))]

    # Filter out non-positive deltas (can happen at animation boundaries)
    pos_deltas = [d for d in deltas if d > 0]

    if pos_deltas:
        stats.dt_mean = sum(pos_deltas) / len(pos_deltas)
        stats.dt_std = math.sqrt(sum((d - stats.dt_mean) ** 2 for d in pos_deltas) / len(pos_deltas))
        stats.dt_min = min(pos_deltas)
        stats.dt_max = max(pos_deltas)
        outliers = sum(1 for d in pos_deltas if d < OUTLIER_LO or d > OUTLIER_HI)
        stats.outlier_pct = 100.0 * outliers / len(pos_deltas)
        stats.avg_fps = 1.0 / stats.dt_mean if stats.dt_mean > 0 else 0.0

        # 1%/0.1% lows: mean Δt of the worst 1% / 0.1% of frames, reported as
        # FPS. Captures stutter tail that mean/σ hide. Floor of 1 frame so
        # short trials still produce a value.
        sorted_desc = sorted(pos_deltas, reverse=True)
        n = len(sorted_desc)
        n_1pct = max(1, n // 100)
        n_01pct = max(1, n // 1000)
        stats.dt_1pct_low = sum(sorted_desc[:n_1pct]) / n_1pct
        stats.dt_01pct_low = sum(sorted_desc[:n_01pct]) / n_01pct
        stats.fps_1pct_low = 1.0 / stats.dt_1pct_low if stats.dt_1pct_low > 0 else 0.0
        stats.fps_01pct_low = 1.0 / stats.dt_01pct_low if stats.dt_01pct_low > 0 else 0.0

    stats.game_elapsed_final = times[-1] if times else 0.0
    stats.wall_elapsed = trial.elapsed_time
    drift_series = _drift_series(frames)
    stats.timing_drift = max(abs(d) for d in drift_series) if drift_series else 0.0

    # 3+ consecutive frozen frame_numbers outside an animation = a stall.
    freeze_run = 0
    for i in range(1, len(frames)):
        if frames[i].frame_number == frames[i - 1].frame_number and not frames[i].is_animating:
            freeze_run += 1
        else:
            if freeze_run >= 3:  # 3+ consecutive frozen frames = a freeze event
                stats.freeze_count += 1
            freeze_run = 0
    if freeze_run >= 3:
        stats.freeze_count += 1

    return stats


def _strip_platform_prefix(path: str, platform: str) -> str:
    """Drop a leading `{platform}/` segment so out paths don't double-nest."""
    if not path:
        return path
    if path == platform:
        return ""
    prefix = platform + "/"
    return path[len(prefix):] if path.startswith(prefix) else path


# ──────────────────────────────────────────────────────────────────────────────
# Plotting
# ──────────────────────────────────────────────────────────────────────────────
def plot_trial(trial: TrialInfo, stats: TrialStats, out_dir: Path):
    """Generate six plots for a single trial and save as a single PNG.

    Layout (2 rows × 3 cols):
        col 0 (sharex):  Camera angle  / Commands raster
        col 1 (sharex):  FPS timeline  / Drift
        col 2:           Presentation Δt histogram / Present Δt timeline
    Cols 0–1 share the x-axis (`present_elapsed_secs`) between top and bottom,
    so events in the raster line up vertically with the angle / fps / drift
    series above and beside them.
    """
    if len(trial.frames) < 2:
        return

    fig, axes = plt.subplots(2, 3, figsize=(20, 10))
    # Share x within each of col 0 and col 1 — top/bottom of each column are
    # both indexed on present_elapsed_secs. Col 2 has its own x for each plot.
    axes[0, 0].sharex(axes[1, 0])
    axes[0, 1].sharex(axes[1, 1])
    short = (
        f"level_{trial.level_index:03d}_trial_{trial.trial_index:03d}"
        f"_run_{trial.trial_run_counter:04d}_object_{trial.active_chain:03d}"
    )
    fig.suptitle(
        f"{short}  [{trial.platform}]  outcome={trial.outcome}  "
        f"attempts={trial.nr_attempts}  frames={stats.n_frames}  "
        f"(no_anim={trial.elapsed_time_no_anim:.2f}s, "
        f"anim={trial.elapsed_time_anim:.2f}s, "
        f"total={trial.elapsed_time:.2f}s)",
        fontsize=11, fontweight="bold",
    )

    times = [f.present_elapsed_secs for f in trial.frames]
    angles = [f.current_angle for f in trial.frames]
    t0 = times[0] if times else 0.0
    t_end = times[-1] if times else 1.0

    # Find animation intervals and attempt indices for annotations
    anim_intervals = []
    anim_start = None
    for f in trial.frames:
        if f.is_animating and anim_start is None:
            anim_start = f.present_elapsed_secs
        elif not f.is_animating and anim_start is not None:
            anim_intervals.append((anim_start, f.present_elapsed_secs))
            anim_start = None
    if anim_start is not None and trial.frames:
        anim_intervals.append((anim_start, trial.frames[-1].present_elapsed_secs))

    attempt_indices = []
    for i in range(1, len(trial.frames)):
        if trial.frames[i].attempts > trial.frames[i - 1].attempts:
            attempt_indices.append(i)

    # ── [0,0] Camera angle trajectory ─────────────────────────────────────────
    ax_cam = axes[0, 0]
    ax_cam.plot(times, angles, linewidth=0.8, color="#2196F3", label="current_angle (rad)")
    if trial.cosine_threshold > 0:
        thresh_angle = math.acos(min(trial.cosine_threshold, 1.0))
        ax_cam.axhline(thresh_angle, color="#4CAF50", linestyle="--", linewidth=1,
                       label=f"cos_threshold → {thresh_angle:.3f} rad")
        ax_cam.axhline(-thresh_angle, color="#4CAF50", linestyle="--", linewidth=1)
    for i, (start, end) in enumerate(anim_intervals):
        ax_cam.axvspan(start, end, color="green", alpha=0.15,
                       label="animation" if i == 0 else "")
    if attempt_indices:
        ax_cam.scatter([times[i] for i in attempt_indices],
                       [angles[i] for i in attempt_indices],
                       color="red", marker="o", s=30, zorder=5,
                       label="checked attempt")
    ax_cam.set_ylabel("current_angle (rad)")
    ax_cam.set_title("Camera Angle Trajectory")
    ax_cam.legend(fontsize=8)
    ax_cam.grid(True, alpha=0.3)

    # ── [0,1] Instantaneous FPS timeline ─────────────────────────────────────
    ax_fps = axes[0, 1]
    deltas = [times[i] - times[i - 1] for i in range(1, len(times)) if times[i] - times[i - 1] > 0]
    if deltas:
        fps_vals = [1.0 / d for d in deltas]
        fps_times = times[1:len(deltas) + 1]
        ax_fps.plot(fps_times, fps_vals, linewidth=0.5, color="#9C27B0", alpha=0.7)
        ax_fps.axhline(60, color="#4CAF50", linestyle="--", linewidth=1, label="60 FPS target")
        ax_fps.set_ylim(0, max(120, max(fps_vals) * 1.1) if fps_vals else 120)
        for i, (start, end) in enumerate(anim_intervals):
            ax_fps.axvspan(start, end, color="green", alpha=0.15,
                           label="animation" if i == 0 else "")
        if attempt_indices:
            a_t, a_y = [], []
            for i in attempt_indices:
                if i >= 1 and (i - 1) < len(fps_vals):
                    a_t.append(times[i])
                    a_y.append(fps_vals[i - 1])
            if a_t:
                ax_fps.scatter(a_t, a_y, color="red", marker="o", s=30, zorder=5,
                               label="checked attempt")
        ax_fps.set_ylabel("FPS")
        ax_fps.set_title(f"Instantaneous FPS (avg={stats.avg_fps:.1f})")
        ax_fps.legend(fontsize=8)
    ax_fps.grid(True, alpha=0.3)

    # ── [0,2] Presentation Δt distribution ───────────────────────────────────
    # Real flip-time intervals reconstructed from `present_elapsed_secs`.
    ax_hist = axes[0, 2]
    present = [f.present_elapsed_secs for f in trial.frames]
    rfn = [f.render_frame_number for f in trial.frames]
    p_deltas = [present[i] - present[i - 1] for i in range(1, len(present))
                if present[i] - present[i - 1] > 0]
    if any(present) and p_deltas:
        p_ms = [d * 1000 for d in p_deltas]
        n_bins = min(80, max(20, len(p_ms) // 5))
        ax_hist.hist(p_ms, bins=n_bins, color="#00ACC1", edgecolor="#006064", alpha=0.85)
        ax_hist.axvline(EXPECTED_DT * 1000, color="#F44336", linestyle="--", linewidth=1.5,
                        label=f"ideal {EXPECTED_DT * 1000:.2f} ms")
        mean_p = sum(p_deltas) / len(p_deltas)
        std_p = (sum((d - mean_p) ** 2 for d in p_deltas) / len(p_deltas)) ** 0.5
        ax_hist.axvline(mean_p * 1000, color="#2196F3", linestyle="-", linewidth=1.5,
                        label=f"mean {mean_p * 1000:.2f} ms")
        out_count = sum(1 for d in p_deltas if d < OUTLIER_LO or d > OUTLIER_HI)
        out_pct = 100.0 * out_count / len(p_deltas)
        sorted_desc = sorted(p_deltas, reverse=True)
        n = len(sorted_desc)
        dt_1 = sum(sorted_desc[:max(1, n // 100)]) / max(1, n // 100)
        dt_01 = sum(sorted_desc[:max(1, n // 1000)]) / max(1, n // 1000)
        ax_hist.axvline(dt_1 * 1000, color="#FF9800", linestyle="--", linewidth=1.2,
                        label=f"1% low {dt_1 * 1000:.2f} ms ({1.0 / dt_1:.1f} fps)")
        ax_hist.axvline(dt_01 * 1000, color="#D32F2F", linestyle="--", linewidth=1.2,
                        label=f"0.1% low {dt_01 * 1000:.2f} ms ({1.0 / dt_01:.1f} fps)")
        ax_hist.set_xlabel("present Δt (ms)")
        ax_hist.set_ylabel("count")
        ax_hist.set_title(
            f"Presentation Δt  (σ={std_p * 1000:.2f} ms, outliers={out_pct:.1f}%, n={len(p_deltas)})"
        )
        ax_hist.legend(fontsize=8)
    else:
        ax_hist.text(0.5, 0.5, "no present_elapsed_secs in log",
                     ha="center", va="center", transform=ax_hist.transAxes,
                     fontsize=10, color="#888")
        ax_hist.set_title("Presentation Δt")
    ax_hist.grid(True, alpha=0.3)

    # ── [1,0] Commands raster (60 Hz-snapped event ticks) ───────────────────
    # One row per command channel; each frame where the controller emitted the
    # command produces a tick snapped to the 60 Hz grid (`round((t-t0)/dt)*dt`)
    # so the spacing reads as discrete fixed-tick slots even when the logged
    # present_elapsed_secs jitters. toggle_blank / toggle_stop_rendering share
    # a row since the FSM always fires them as a pair at trial boundaries.
    ax_cmds = axes[1, 0]
    cmd_channels = [
        ("rotate_left",        "cmd_rotate_left",          "#1976D2"),
        ("rotate_right",       "cmd_rotate_right",         "#0D47A1"),
        ("check",              "cmd_check",                "#7B1FA2"),
        ("reset",              "cmd_reset",                "#C62828"),
        ("toggle_blank/stop",  "cmd_toggle_blank_or_stop", "#455A64"),
        ("animation_door",     "cmd_animation_door",       "#2E7D32"),
    ]
    positions = list(range(len(cmd_channels)))

    def _snap(t: float) -> float:
        return round((t - t0) / EXPECTED_DT) * EXPECTED_DT + t0

    event_times = [
        [_snap(f.present_elapsed_secs) for f in trial.frames if getattr(f, attr)]
        for _, attr, _ in cmd_channels
    ]
    colors = [c for _, _, c in cmd_channels]
    if any(event_times):
        ax_cmds.eventplot(event_times, lineoffsets=positions,
                          linelengths=0.7, colors=colors)
    else:
        ax_cmds.text(0.5, 0.5, "no commands_sent in log",
                     ha="center", va="center", transform=ax_cmds.transAxes,
                     fontsize=10, color="#888")
    for start, end in anim_intervals:
        ax_cmds.axvspan(start, end, color="green", alpha=0.08)
    ax_cmds.set_yticks(positions)
    ax_cmds.set_yticklabels([name for name, _, _ in cmd_channels], fontsize=9)
    ax_cmds.set_ylim(-0.6, len(cmd_channels) - 0.4)
    ax_cmds.set_xlabel("present_elapsed_secs (s)")
    ax_cmds.set_title("Commands sent (60 Hz-snapped)")
    ax_cmds.grid(True, axis="x", alpha=0.3)

    # ── [1,1] Drift (naive: animation pauses included) ──────────────────────
    # `(present − t0) − row_count/60`. Animation rows are counted just like
    # play rows, so intentional pauses show up as drift — matching what the
    # controller writes into its level summary.
    ax_drift = axes[1, 1]
    drift = _drift_series(trial.frames)
    if drift:
        ax_drift.plot(times, drift, linewidth=0.8, color="#F44336", label="drift")
        ax_drift.axhline(0, color="#888", linestyle=":", linewidth=0.8)
        for i, (start, end) in enumerate(anim_intervals):
            ax_drift.axvspan(start, end, color="green", alpha=0.15,
                             label="animation" if i == 0 else "")
        if attempt_indices:
            ax_drift.scatter([times[i] for i in attempt_indices],
                             [drift[i] for i in attempt_indices],
                             color="red", marker="o", s=30, zorder=5,
                             label="checked attempt")
        if anim_intervals or attempt_indices:
            ax_drift.legend(fontsize=8)
        ax_drift.set_xlabel("present_elapsed_secs (s)")
        ax_drift.set_ylabel("(present − t0) − ticks/60 [s]")
        ax_drift.set_title("Drift (animation included)")
        ax_drift.yaxis.set_major_formatter(mticker.FormatStrFormatter("%.3f"))
    ax_drift.grid(True, alpha=0.3)

    # ── [1,2] Present Δt timeline (x = present_elapsed_secs) ────────────────
    ax_dtt = axes[1, 2]
    if any(present) and len(trial.frames) > 1:
        idx = list(range(1, len(trial.frames)))
        p_dt_ms = [(present[i] - present[i - 1]) * 1000 for i in idx]
        x_times = [present[i] for i in idx]
        ax_dtt.plot(x_times, p_dt_ms, linewidth=0.6, color="#00ACC1", alpha=0.8,
                    label="present Δt")
        ax_dtt.axhline(EXPECTED_DT * 1000, color="#888", linestyle=":", linewidth=0.8)
        gap_pos = [(present[i], p_dt_ms[k]) for k, i in enumerate(idx) if rfn[i] - rfn[i - 1] > 1]
        if gap_pos:
            ax_dtt.scatter([x for x, _ in gap_pos], [y for _, y in gap_pos],
                           s=18, color="#F44336", zorder=5,
                           label=f"render gap (n={len(gap_pos)})")
        ax_dtt.set_xlabel("present_elapsed_secs (s)")
        ax_dtt.set_ylabel("Δt (ms)")
        ax_dtt.set_title(f"Present Δt timeline (gaps = dropped renders) — duration {t_end - t0:.6f}s")
        ax_dtt.legend(fontsize=8, loc="upper right")
        if p_dt_ms:
            ax_dtt.set_ylim(0, min(max(p_dt_ms) * 1.1, 200))
    else:
        ax_dtt.text(0.5, 0.5, "no present_elapsed_secs in log",
                    ha="center", va="center", transform=ax_dtt.transAxes,
                    fontsize=10, color="#888")
        ax_dtt.set_title("Present Δt timeline")
    ax_dtt.grid(True, alpha=0.3)

    # Clamp the shared-x columns so axvspan/eventplot extent stays bounded.
    for ax in (ax_cam, ax_cmds, ax_fps, ax_drift, ax_dtt):
        ax.set_xlim(t0, t_end)

    plt.tight_layout(rect=[0, 0, 1, 0.95])
    rel = _strip_platform_prefix(trial.source_rel_path or trial.label, trial.platform)
    out_path = out_dir / trial.platform / f"{rel}.png"
    out_path.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  → {out_path}")


def plot_session(trials: list, out_path: Path, title: str = "level",
                 mark_level_breaks: bool = False) -> None:
    """Session overview. X axis = cumulative elapsed time (s) across trials."""
    if not trials:
        return

    trials = sorted(trials, key=lambda t: t.label)

    fig, axes = plt.subplots(5, 1, figsize=(16, 14), sharex=True)
    total_no_anim = sum(t.elapsed_time_no_anim or 0.0 for t in trials)
    total_anim = sum(t.elapsed_time_anim or 0.0 for t in trials)
    fig.suptitle(
        f"{title} — {len(trials)} trial{'s' if len(trials) != 1 else ''}  "
        f"(no_anim={total_no_anim:.2f}s, anim={total_anim:.2f}s, "
        f"total={total_no_anim + total_anim:.2f}s)",
        fontsize=12, fontweight="bold",
    )

    widths = [max(0.05, float(t.elapsed_time)) for t in trials]
    lefts = []
    cum = 0.0
    for w in widths:
        lefts.append(cum)
        cum += w
    total_span = cum if cum > 0 else 1.0

    level_break_xs = []
    if mark_level_breaks:
        for i in range(1, len(trials)):
            if trials[i].level_index != trials[i - 1].level_index:
                level_break_xs.append(lefts[i])

    def _draw_level_breaks(ax):
        for x in level_break_xs:
            ax.axvline(x, color="#111", linewidth=1.8, linestyle="-", alpha=0.9, zorder=10)

    def _draw_trial_breaks(ax):
        for x in lefts[1:]:
            ax.axvline(x, color="#666", linewidth=1.0, linestyle=(0, (3, 2)),
                       alpha=0.85, zorder=9)

    def _trial_x(left, w, t, p):
        """Map a trial-internal present_elapsed_secs to session-cumulative x."""
        if len(t.frames) < 2:
            return left
        t0 = t.frames[0].present_elapsed_secs
        tN = t.frames[-1].present_elapsed_secs
        span = tN - t0 if tN > t0 else 1.0
        return left + (p - t0) / span * w

    # ── Panel 1: outcome strip ────────────────────────────────────────────
    ax = axes[0]
    for left, w, t in zip(lefts, widths, trials):
        color = OUTCOME_COLORS.get(t.outcome, "#888")
        ax.barh(0, w, left=left, height=0.8, color=color, edgecolor="#222", linewidth=0.4)
    ax.set_yticks([])
    ax.set_xlim(0, total_span)
    ax.set_title("Outcome")
    handles = [plt.Rectangle((0, 0), 1, 1, color=c, label=name)
               for name, c in OUTCOME_COLORS.items()]
    ax.legend(handles=handles, loc="upper right", fontsize=8, ncol=3)
    _draw_trial_breaks(ax)
    _draw_level_breaks(ax)

    # ── Panel 2: attempts CDF (step plot of cumulative attempts) ──────────
    ax = axes[1]
    cdf_x, cdf_y = [0.0], [0]
    cum_attempts = 0
    for left, w, t in zip(lefts, widths, trials):
        for i in range(1, len(t.frames)):
            if t.frames[i].attempts > t.frames[i - 1].attempts:
                cum_attempts += 1
                cdf_x.append(_trial_x(left, w, t, t.frames[i].present_elapsed_secs))
                cdf_y.append(cum_attempts)
    cdf_x.append(total_span)
    cdf_y.append(cum_attempts)
    ax.step(cdf_x, cdf_y, where="post", color="#7c6ff7", linewidth=1.2)
    ax.fill_between(cdf_x, cdf_y, step="post", color="#7c6ff7", alpha=0.18)
    ax.set_ylabel("attempts (cumulative)")
    ax.yaxis.set_major_locator(mticker.MaxNLocator(integer=True))
    ax.set_title("Attempts CDF")
    ax.grid(True, alpha=0.3, axis="y")
    _draw_trial_breaks(ax)
    _draw_level_breaks(ax)

    # ── Panel 3: activity strip (blue active / green animating) ───────────
    ax = axes[2]
    for left, w, t in zip(lefts, widths, trials):
        if len(t.frames) < 2:
            ax.axvspan(left, left + w, color="#4a8fd4", alpha=0.6)
            continue
        seg_start = t.frames[0].present_elapsed_secs
        cur_anim = t.frames[0].is_animating
        for f in t.frames[1:]:
            if f.is_animating != cur_anim:
                color = "#9fbf86" if cur_anim else "#4a8fd4"
                ax.axvspan(_trial_x(left, w, t, seg_start),
                           _trial_x(left, w, t, f.present_elapsed_secs),
                           color=color, alpha=0.85)
                seg_start = f.present_elapsed_secs
                cur_anim = f.is_animating
        color = "#9fbf86" if cur_anim else "#4a8fd4"
        ax.axvspan(_trial_x(left, w, t, seg_start),
                   _trial_x(left, w, t, t.frames[-1].present_elapsed_secs),
                   color=color, alpha=0.85)
    ax.set_yticks([])
    ax.set_title("Activity (blue = active, green = animation)")
    handles = [
        plt.Rectangle((0, 0), 1, 1, color="#4a8fd4", label="active"),
        plt.Rectangle((0, 0), 1, 1, color="#9fbf86", label="animation"),
    ]
    ax.legend(handles=handles, loc="upper right", fontsize=8, ncol=2)
    _draw_trial_breaks(ax)
    _draw_level_breaks(ax)

    # ── Panel 4: alignment trajectory + attempt dots + animation shading ──
    ax = axes[3]
    for left, w, t in zip(lefts, widths, trials):
        if len(t.frames) < 2:
            continue
        xs_t = [_trial_x(left, w, t, f.present_elapsed_secs) for f in t.frames]
        ys_t = [f.current_alignment for f in t.frames]
        ax.plot(xs_t, ys_t, linewidth=0.6, alpha=0.85,
                color=OUTCOME_COLORS.get(t.outcome, "#888"))

        a_start = None
        for f in t.frames:
            if f.is_animating and a_start is None:
                a_start = f.present_elapsed_secs
            elif (not f.is_animating) and a_start is not None:
                ax.axvspan(_trial_x(left, w, t, a_start),
                           _trial_x(left, w, t, f.present_elapsed_secs),
                           color="green", alpha=0.10)
                a_start = None
        if a_start is not None:
            ax.axvspan(_trial_x(left, w, t, a_start),
                       _trial_x(left, w, t, t.frames[-1].present_elapsed_secs),
                       color="green", alpha=0.10)

        for i in range(1, len(t.frames)):
            if t.frames[i].attempts > t.frames[i - 1].attempts:
                ax.scatter([_trial_x(left, w, t, t.frames[i].present_elapsed_secs)],
                           [t.frames[i].current_alignment],
                           color="red", marker="o", s=14, zorder=5)
    if trials and trials[0].cosine_threshold > 0:
        ax.axhline(trials[0].cosine_threshold, color="#222", linestyle="--",
                   linewidth=1, label=f"threshold {trials[0].cosine_threshold:.2f}")
        ax.legend(fontsize=8, loc="lower right")
    ax.set_ylabel("cosine_alignment")
    ax.set_title("Alignment trajectory")
    ax.grid(True, alpha=0.3)
    _draw_trial_breaks(ax)
    _draw_level_breaks(ax)

    # ── Panel 5: per-frame Δt timeline across the session ─────────────────
    ax = axes[4]
    dt_x, dt_y = [], []
    for left, w, t in zip(lefts, widths, trials):
        for i in range(1, len(t.frames)):
            dt = t.frames[i].present_elapsed_secs - t.frames[i - 1].present_elapsed_secs
            if dt <= 0:
                continue
            dt_x.append(_trial_x(left, w, t, t.frames[i].present_elapsed_secs))
            dt_y.append(dt * 1000)
    if dt_x:
        ax.plot(dt_x, dt_y, linewidth=0.5, color="#00ACC1", alpha=0.85)
        ax.axhline(EXPECTED_DT * 1000, color="#4CAF50", linestyle="--",
                   linewidth=1, label=f"ideal {EXPECTED_DT * 1000:.2f} ms")
        ax.legend(fontsize=8, loc="upper right")
        ax.set_ylim(0, min(max(dt_y) * 1.1, 200))
    else:
        ax.text(0.5, 0.5, "no present_elapsed_secs in logs",
                ha="center", va="center", transform=ax.transAxes, fontsize=10, color="#888")
    ax.set_ylabel("present Δt (ms)")
    ax.set_title("Per-frame Δt timeline")
    ax.grid(True, alpha=0.3, axis="y")
    _draw_trial_breaks(ax)
    _draw_level_breaks(ax)

    axes[-1].set_xlim(0, total_span)

    centers = [l + w / 2 for l, w in zip(lefts, widths)]
    short_labels = [
        f"level_{t.level_index:03d}_trial_{t.trial_index:03d}"
        f"_run_{t.trial_run_counter:04d}_object_{t.active_chain:03d}"
        for t in trials
    ]
    step = max(1, len(trials) // 30)
    axes[-1].set_xticks(centers[::step])
    axes[-1].set_xticklabels(short_labels[::step],
                             rotation=45, ha="right", fontsize=6)
    axes[-1].set_xlabel("trial")

    plt.tight_layout(rect=[0, 0, 1, 0.96])
    fig.savefig(out_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"  → {out_path}")


# ──────────────────────────────────────────────────────────────────────────────
# Summary
# ──────────────────────────────────────────────────────────────────────────────
def format_summary(all_stats: list[TrialStats]) -> str:
    """Build a text summary of all trials."""
    lines = []
    sep = "=" * 100
    lines.append(sep)
    lines.append("TRIAL LOG VERIFICATION SUMMARY")
    lines.append(sep)
    lines.append("")

    # Per-trial table
    hdr = (
        f"{'Trial':<35s}  {'Platfm':<6s}  {'Frames':>6s}  {'Gaps':>4s}  {'Dups':>4s}  "
        f"{'Δt mean':>8s}  {'Δt σ':>7s}  {'Δt min':>7s}  {'Δt max':>7s}  "
        f"{'Out%':>5s}  {'FPS':>5s}  {'1%low':>6s}  {'.1%low':>6s}  {'Frz':>3s}  {'Drift':>7s}"
    )
    lines.append(hdr)
    lines.append("-" * len(hdr))

    for s in all_stats:
        row = (
            f"{s.label:<35s}  {s.platform:<6s}  {s.n_frames:>6d}  {s.gaps:>4d}  {s.duplicates:>4d}  "
            f"{s.dt_mean * 1000:>7.2f}ms  {s.dt_std * 1000:>6.2f}ms  {s.dt_min * 1000:>6.2f}ms  {s.dt_max * 1000:>6.2f}ms  "
            f"{s.outlier_pct:>4.1f}%  {s.avg_fps:>5.1f}  {s.fps_1pct_low:>6.1f}  {s.fps_01pct_low:>6.1f}  "
            f"{s.freeze_count:>3d}  {s.timing_drift:>6.3f}s"
        )
        lines.append(row)

    lines.append("")

    # Aggregate stats per platform
    by_platform: dict[str, list[TrialStats]] = {}
    for s in all_stats:
        by_platform.setdefault(s.platform, []).append(s)

    for plat, stats_list in sorted(by_platform.items()):
        lines.append(f"── {plat.upper()} aggregate ({len(stats_list)} trials) ──")
        total_frames = sum(s.n_frames for s in stats_list)
        total_gaps = sum(s.gaps for s in stats_list)
        total_dups = sum(s.duplicates for s in stats_list)
        total_freezes = sum(s.freeze_count for s in stats_list)
        dt_means = [s.dt_mean for s in stats_list if s.dt_mean > 0]
        fps_vals = [s.avg_fps for s in stats_list if s.avg_fps > 0]
        outlier_pcts = [s.outlier_pct for s in stats_list]
        drifts = [s.timing_drift for s in stats_list]

        lines.append(f"  Total frames:      {total_frames}")
        lines.append(f"  Total gaps:        {total_gaps}")
        lines.append(f"  Total duplicates:  {total_dups}")
        lines.append(f"  Total freezes:     {total_freezes}")
        if dt_means:
            avg_dt = sum(dt_means) / len(dt_means)
            lines.append(f"  Mean Δt (across trials):  {avg_dt * 1000:.2f} ms  (target: {EXPECTED_DT * 1000:.2f} ms)")
        if fps_vals:
            avg_fps = sum(fps_vals) / len(fps_vals)
            lines.append(f"  Mean FPS:          {avg_fps:.1f}  (target: 60.0)")
        lows_1 = [s.fps_1pct_low for s in stats_list if s.fps_1pct_low > 0]
        lows_01 = [s.fps_01pct_low for s in stats_list if s.fps_01pct_low > 0]
        if lows_1:
            lines.append(f"  Mean 1% low FPS:   {sum(lows_1) / len(lows_1):.1f}  (min: {min(lows_1):.1f})")
        if lows_01:
            lines.append(f"  Mean 0.1% low FPS: {sum(lows_01) / len(lows_01):.1f}  (min: {min(lows_01):.1f})")
        if outlier_pcts:
            avg_out = sum(outlier_pcts) / len(outlier_pcts)
            lines.append(f"  Mean outlier %:    {avg_out:.1f}%")
        if drifts:
            avg_drift = sum(drifts) / len(drifts)
            max_drift = max(drifts)
            lines.append(f"  Mean drift:        {avg_drift:.3f}s  (max: {max_drift:.3f}s)")
        lines.append("")

    # Cross-platform comparison (if multiple platforms)
    if len(by_platform) > 1:
        lines.append(sep)
        lines.append("CROSS-PLATFORM COMPARISON")
        lines.append(sep)
        header = f"{'Metric':<30s}" + "".join(f"  {p.upper():>12s}" for p in sorted(by_platform))
        lines.append(header)
        lines.append("-" * len(header))

        metrics: list[tuple[str, callable]] = [
            ("Mean Δt (ms)", lambda sl: f"{(sum(s.dt_mean for s in sl) / len(sl)) * 1000:.2f}" if sl else "N/A"),
            ("Mean FPS", lambda sl: f"{sum(s.avg_fps for s in sl if s.avg_fps > 0) / max(1, sum(1 for s in sl if s.avg_fps > 0)):.1f}" if sl else "N/A"),
            ("1% low FPS (mean)", lambda sl: f"{sum(s.fps_1pct_low for s in sl if s.fps_1pct_low > 0) / max(1, sum(1 for s in sl if s.fps_1pct_low > 0)):.1f}" if sl else "N/A"),
            ("0.1% low FPS (mean)", lambda sl: f"{sum(s.fps_01pct_low for s in sl if s.fps_01pct_low > 0) / max(1, sum(1 for s in sl if s.fps_01pct_low > 0)):.1f}" if sl else "N/A"),
            ("Mean outlier %", lambda sl: f"{sum(s.outlier_pct for s in sl) / len(sl):.1f}%" if sl else "N/A"),
            ("Total gaps", lambda sl: str(sum(s.gaps for s in sl))),
            ("Total duplicates", lambda sl: str(sum(s.duplicates for s in sl))),
            ("Total freezes", lambda sl: str(sum(s.freeze_count for s in sl))),
            ("Mean drift (s)", lambda sl: f"{sum(s.timing_drift for s in sl) / len(sl):.3f}" if sl else "N/A"),
            ("Max drift (s)", lambda sl: f"{max(s.timing_drift for s in sl):.3f}" if sl else "N/A"),
        ]

        for name, fn in metrics:
            row = f"{name:<30s}"
            for plat in sorted(by_platform):
                val = fn(by_platform[plat])
                row += f"  {val:>12s}"
            lines.append(row)

        lines.append("")

    # Verdict
    lines.append(sep)
    issues = []
    for s in all_stats:
        if s.gaps > 0:
            issues.append(f"  ⚠ {s.label}: {s.gaps} frame gaps")
        if s.duplicates > 0:
            issues.append(f"  ⚠ {s.label}: {s.duplicates} duplicate frames")
        if s.outlier_pct > 10:
            issues.append(f"  ⚠ {s.label}: {s.outlier_pct:.1f}% outlier frame deltas")
        if s.freeze_count > 0:
            issues.append(f"  ⚠ {s.label}: {s.freeze_count} freeze events detected")
        if s.timing_drift > 1.0:
            issues.append(f"  ⚠ {s.label}: {s.timing_drift:.3f}s timing drift")

    if issues:
        lines.append("ISSUES FOUND:")
        lines.extend(issues)
    else:
        lines.append("✓ No significant issues detected across all trials.")
    lines.append(sep)

    return "\n".join(lines)


# ──────────────────────────────────────────────────────────────────────────────
# Main
# ──────────────────────────────────────────────────────────────────────────────
def main():
    parser = argparse.ArgumentParser(
        description="Verify trial logs from native and/or WASM builds.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  python tools/verify_trial_logs.py out/trial_logs/
  python tools/verify_trial_logs.py wasm_logs/trial_logs_*.json
  python tools/verify_trial_logs.py out/trial_logs/ wasm_logs/
        """,
    )
    parser.add_argument(
        "paths", nargs="+",
        help="Paths to trial log files or directories. "
             "If two paths are given, the first is treated as 'native' and the second as 'wasm'.",
    )
    parser.add_argument(
        "--output", "-o", default=OUTPUT_DIR,
        help=f"Output directory for plots and summary (default: {OUTPUT_DIR})",
    )
    parser.add_argument(
        "--no-plots", action="store_true",
        help="Skip generating per-trial plots (summary only).",
    )
    args = parser.parse_args()

    # Determine platforms
    paths = args.paths
    if len(paths) == 1:
        p = Path(paths[0])
        # Zip downloads come exclusively from the web controller.
        if p.is_file() and p.suffix.lower() == ".zip":
            platform = "wasm"
        # A directory with `level_*` subfolders is the unpacked web bundle.
        elif p.is_dir() and any(f.is_dir() and f.name.startswith("level_") for f in p.iterdir()):
            platform = "wasm"
        # Flat directory with `trial_*.json` files = native controller.py output.
        elif p.is_dir() and ("out" in str(p) or any("trial_" in f.name and f.suffix == ".json" for f in p.iterdir())):
            platform = "native"
        else:
            platform = "wasm"
        all_trials = load_path(paths[0], platform)
    else:
        # Two+ paths: first = native, second = wasm
        all_trials = []
        all_trials.extend(load_path(paths[0], "native"))
        for extra in paths[1:]:
            all_trials.extend(load_path(extra, "wasm"))

    if not all_trials:
        print("Error: no trials loaded. Check your paths.")
        sys.exit(1)

    print(f"\nLoaded {len(all_trials)} trial(s) from {len(paths)} path(s).")

    # Output directory
    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)

    # Analyse
    all_stats = []
    for trial in all_trials:
        stats = analyse_trial(trial)
        all_stats.append(stats)

        if not args.no_plots:
            plot_trial(trial, stats, out_dir)

    if not args.no_plots:
        def _level_run_key(t):
            parts = t.source_rel_path.split("/") if t.source_rel_path else []
            if len(parts) >= 3 and parts[-2] == "trials":
                return "/".join(parts[:-2])
            return None

        level_groups: dict[tuple[str, str], list] = {}
        for t in all_trials:
            key = _level_run_key(t)
            if key is not None:
                level_groups.setdefault((t.platform, key), []).append(t)
        for (plat, key), group_trials in sorted(level_groups.items()):
            sess_path = out_dir / plat / _strip_platform_prefix(key, plat) / "session_overview.png"
            sess_path.parent.mkdir(parents=True, exist_ok=True)
            lvl_name = next((p for p in key.split("/") if p.startswith("level_")), "level")
            plot_session(group_trials, sess_path, title=lvl_name)

        root_groups: dict[tuple[str, str], list] = {}
        for t in all_trials:
            root_label = t.source_rel_path.split("/", 1)[0] if t.source_rel_path else t.label
            root_groups.setdefault((t.platform, root_label), []).append(t)
        for (plat, root_label), group_trials in sorted(root_groups.items()):
            sess_path = out_dir / plat / _strip_platform_prefix(root_label, plat) / "session_overview.png"
            sess_path.parent.mkdir(parents=True, exist_ok=True)
            plot_session(group_trials, sess_path,
                         title="all levels",
                         mark_level_breaks=True)

    # Summary — one per platform, written under that platform's output dir
    # alongside the per-trial PNGs and session overviews.
    summary = format_summary(all_stats)
    print("\n" + summary)

    # Write summary.txt next to the cross-level session_overview.png, i.e.
    # under <out_dir>/<platform>/<root_label>/.
    by_group: dict[tuple[str, str], list[TrialStats]] = {}
    for t, s in zip(all_trials, all_stats):
        root_label = t.source_rel_path.split("/", 1)[0] if t.source_rel_path else t.label
        by_group.setdefault((t.platform, root_label), []).append(s)
    for (plat, root_label), grp_stats in by_group.items():
        grp_dir = out_dir / plat / _strip_platform_prefix(root_label, plat)
        grp_dir.mkdir(parents=True, exist_ok=True)
        grp_summary_path = grp_dir / "summary.txt"
        with open(grp_summary_path, "w") as f:
            f.write(format_summary(grp_stats))
        print(f"Summary saved to {grp_summary_path}")
    print(f"Plots saved to   {out_dir}/")


if __name__ == "__main__":
    main()