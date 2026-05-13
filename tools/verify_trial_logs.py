#!/usr/bin/env python3
"""
verify_trial_logs.py — Cross-platform trial log analyzer.

Parses trial logs from native (Python controller) and WASM (JS controller),
generates per-trial plots and an aggregate summary comparing timing,
frame delivery, and logging consistency.

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


@dataclass
class TrialInfo:
    label: str               # e.g. "trial_000_run_0001" or "wasm_trial_3"
    platform: str            # "native" or "wasm"
    level_index: int = 0
    active_chain: int = 0
    trial_index: int = 0
    outcome: str = ""
    nr_attempts: int = 0
    elapsed_time: float = 0.0   # wall-clock from controller
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
    game_elapsed_final: float = 0.0
    wall_elapsed: float = 0.0
    timing_drift: float = 0.0
    freeze_count: int = 0


# ──────────────────────────────────────────────────────────────────────────────
# Loading helpers
# ──────────────────────────────────────────────────────────────────────────────
def _parse_trial_dict(d: dict, label: str, platform: str) -> TrialInfo:
    """Parse a single trial dict (same schema for native/WASM)."""
    trial = TrialInfo(
        label=label,
        platform=platform,
        level_index=d.get("level_index", 0),
        active_chain=d.get("active_chain", 0),
        trial_index=d.get("trial_index_in_chain", 0),
        outcome=d.get("outcome", ""),
        nr_attempts=d.get("nr_attempts", 0),
        elapsed_time=d.get("elapsed_time", 0.0),
        cosine_threshold=d.get("trial_config", {}).get("cosine_alignment_threshold", 0.0),
    )

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
        else:
            sr = {}

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
        ))

    # Sort by frame_number for consistent analysis
    trial.frames.sort(key=lambda f: f.frame_number)
    return trial


def load_path(path: str, platform: str) -> list[TrialInfo]:
    """Load trials from a file, directory, or zip archive."""
    p = Path(path)
    trials = []

    if p.is_file() and p.suffix.lower() == ".zip":
        # Web download bundle: zip of per-level folders with per-trial JSON
        # files matching controller.py's on-disk schema.
        with zipfile.ZipFile(p) as zf, tempfile.TemporaryDirectory() as tmp:
            zf.extractall(tmp)
            for jf in sorted(jf for jf in Path(tmp).rglob("*.json")
                             if "_summary_" not in jf.name):
                trials.extend(load_path(str(jf), platform))
    elif p.is_file():
        with open(p) as f:
            data = json.load(f)

        if isinstance(data, list):
            # WASM legacy format: array of trial dicts in one big JSON
            for i, d in enumerate(data):
                label = f"{p.stem}_trial_{i:03d}"
                trials.append(_parse_trial_dict(d, label, platform))
        elif isinstance(data, dict):
            # Native / current WASM zip format: one trial per file
            label = p.stem
            trials.append(_parse_trial_dict(data, label, platform))
        else:
            print(f"Warning: unexpected JSON root type in {p}, skipping")

    elif p.is_dir():
        # Skip level-summary files; they live alongside per-trial files but
        # describe the whole level run, not a single trial.
        json_files = sorted(jf for jf in p.rglob("*.json") if "_summary_" not in jf.name)
        if not json_files:
            print(f"Warning: no .json files found under {p}")
        for jf in json_files:
            trials.extend(load_path(str(jf), platform))
    else:
        print(f"Warning: path {p} does not exist, skipping")

    return trials


# ──────────────────────────────────────────────────────────────────────────────
# Analysis
# ──────────────────────────────────────────────────────────────────────────────
def _active_drift_series(frames) -> list[float]:
    """`(present - present[0]) - active_tick_count / 60`, animation pauses excluded."""
    if not frames:
        return []
    t0 = frames[0].present_elapsed_secs
    active = 0
    out: list[float] = []
    for f in frames:
        if not f.is_animating:
            active += 1
        out.append((f.present_elapsed_secs - t0) - active / 60.0)
    return out


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

    stats.game_elapsed_final = times[-1] if times else 0.0
    stats.wall_elapsed = trial.elapsed_time
    drift_series = _active_drift_series(frames)
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


# ──────────────────────────────────────────────────────────────────────────────
# Plotting
# ──────────────────────────────────────────────────────────────────────────────
def plot_trial(trial: TrialInfo, stats: TrialStats, out_dir: Path):
    """Generate six plots for a single trial and save as a single PNG."""
    if len(trial.frames) < 2:
        return

    fig, axes = plt.subplots(2, 3, figsize=(20, 9))
    fig.suptitle(
        f"{trial.label}  [{trial.platform}]  outcome={trial.outcome}  "
        f"attempts={trial.nr_attempts}  frames={stats.n_frames}",
        fontsize=11, fontweight="bold",
    )

    times = [f.present_elapsed_secs for f in trial.frames]
    angles = [f.current_angle for f in trial.frames]

    # ── Plot 1: Camera angle vs target over time ─────────────────────────────
    ax1 = axes[0, 0]
    ax1.plot(times, angles, linewidth=0.8, color="#2196F3", label="current_angle (rad)")
    if trial.cosine_threshold > 0:
        # Convert cosine threshold to angle for visualization
        thresh_angle = math.acos(min(trial.cosine_threshold, 1.0))
        ax1.axhline(thresh_angle, color="#4CAF50", linestyle="--", linewidth=1,
                     label=f"cos_threshold → {thresh_angle:.3f} rad")
        ax1.axhline(-thresh_angle, color="#4CAF50", linestyle="--", linewidth=1)
    ax1.set_xlabel("present_elapsed_secs (s)")
    ax1.set_ylabel("current_angle (rad)")
    ax1.set_title("Camera Angle Trajectory")
    ax1.legend(fontsize=8)
    ax1.grid(True, alpha=0.3)

    # ── Plot 2: Frame delta histogram ────────────────────────────────────────
    ax2 = axes[0, 1]
    deltas = [times[i] - times[i - 1] for i in range(1, len(times)) if times[i] - times[i - 1] > 0]
    if deltas:
        deltas_ms = [d * 1000 for d in deltas]
        n_bins = min(80, max(20, len(deltas_ms) // 5))
        ax2.hist(deltas_ms, bins=n_bins, color="#FF9800", edgecolor="#E65100", alpha=0.8)
        ax2.axvline(EXPECTED_DT * 1000, color="#F44336", linestyle="--", linewidth=1.5,
                     label=f"ideal {EXPECTED_DT * 1000:.2f} ms")
        ax2.axvline(stats.dt_mean * 1000, color="#2196F3", linestyle="-", linewidth=1.5,
                     label=f"mean {stats.dt_mean * 1000:.2f} ms")
        ax2.set_xlabel("Δt (ms)")
        ax2.set_ylabel("count")
        ax2.set_title(f"Frame Delta Distribution  (σ={stats.dt_std * 1000:.2f} ms, outliers={stats.outlier_pct:.1f}%)")
        ax2.legend(fontsize=8)
    ax2.grid(True, alpha=0.3)

    # ── Plot 3: FPS timeline ─────────────────────────────────────────────────
    ax3 = axes[1, 0]
    if deltas:
        fps_vals = [1.0 / d for d in deltas]
        fps_times = times[1:len(deltas) + 1]
        ax3.plot(fps_times, fps_vals, linewidth=0.5, color="#9C27B0", alpha=0.7)
        ax3.axhline(60, color="#4CAF50", linestyle="--", linewidth=1, label="60 FPS target")
        ax3.set_ylim(0, max(120, max(fps_vals) * 1.1) if fps_vals else 120)
        ax3.set_xlabel("present_elapsed_secs (s)")
        ax3.set_ylabel("FPS")
        ax3.set_title(f"Instantaneous FPS (avg={stats.avg_fps:.1f})")
        ax3.legend(fontsize=8)
    ax3.grid(True, alpha=0.3)

    # ── Plot 4: Active-time drift (ignores animation pauses) ─────────────────
    # `elapsed_secs` is frozen during door animations by design, so naive
    # drift = elapsed - frame/60 gets a −1.5 s step per attempt. Here we
    # count only frames where `is_animating == False` to expose only real
    # stalls (Bevy's FixedUpdate falling behind its 60 Hz cadence).
    ax4 = axes[1, 1]
    drift = _active_drift_series(trial.frames)
    if drift:
        ax4.plot(times, drift, linewidth=0.8, color="#F44336")
        ax4.axhline(0, color="#888", linestyle=":", linewidth=0.8)
        ax4.set_xlabel("present_elapsed_secs (s)")
        ax4.set_ylabel("(present − t0) − active_ticks/60 [s]")
        ax4.set_title("Active-time drift (animation pauses excluded)")
        ax4.yaxis.set_major_formatter(mticker.FormatStrFormatter("%.3f"))
    ax4.grid(True, alpha=0.3)

    # ── Plot 5: Presentation Δt distribution (real flip-time intervals) ──────
    # `present_elapsed_secs` is wall-clock at the next frame's `First`, which
    # under Fifo equals the vsync at which this frame was latched. Deltas
    # between consecutive rows reconstruct the actual frame interval as seen
    # by the display. Compare to plot 2 which uses `elapsed_secs` (fixed-tick
    # game clock) — divergence indicates fixed-loop catch-up / dropped renders.
    ax5 = axes[0, 2]
    present = [f.present_elapsed_secs for f in trial.frames]
    rfn = [f.render_frame_number for f in trial.frames]
    p_deltas = []
    for i in range(1, len(present)):
        d = present[i] - present[i - 1]
        # rAF can produce a tiny negative skew at trial boundaries; drop those
        if d > 0:
            p_deltas.append(d)
    if any(present):
        if p_deltas:
            p_ms = [d * 1000 for d in p_deltas]
            n_bins = min(80, max(20, len(p_ms) // 5))
            ax5.hist(p_ms, bins=n_bins, color="#00ACC1", edgecolor="#006064", alpha=0.85)
            ax5.axvline(EXPECTED_DT * 1000, color="#F44336", linestyle="--", linewidth=1.5,
                        label=f"ideal {EXPECTED_DT * 1000:.2f} ms")
            mean_p = sum(p_deltas) / len(p_deltas)
            std_p = (sum((d - mean_p) ** 2 for d in p_deltas) / len(p_deltas)) ** 0.5
            ax5.axvline(mean_p * 1000, color="#2196F3", linestyle="-", linewidth=1.5,
                        label=f"mean {mean_p * 1000:.2f} ms")
            ax5.set_xlabel("present Δt (ms)")
            ax5.set_ylabel("count")
            ax5.set_title(f"Presentation Δt  (σ={std_p * 1000:.2f} ms, n={len(p_deltas)})")
            ax5.legend(fontsize=8)
    else:
        ax5.text(0.5, 0.5, "no present_elapsed_secs in log",
                 ha="center", va="center", transform=ax5.transAxes, fontsize=10, color="#888")
        ax5.set_title("Presentation Δt")
    ax5.grid(True, alpha=0.3)

    # ── Plot 6: Present Δt timeline + render gap markers ─────────────────────
    ax6 = axes[1, 2]
    if any(present) and len(trial.frames) > 1:
        idx = list(range(1, len(trial.frames)))
        p_dt = [(present[i] - present[i - 1]) * 1000 for i in idx]
        ax6.plot(idx, p_dt, linewidth=0.6, color="#00ACC1", alpha=0.8, label="present Δt")
        ax6.axhline(EXPECTED_DT * 1000, color="#888", linestyle=":", linewidth=0.8)
        gap_idx = [i for i in idx if rfn[i] - rfn[i - 1] > 1]
        if gap_idx:
            ax6.scatter(gap_idx, [p_dt[i - 1] for i in gap_idx],
                        s=18, color="#F44336", zorder=5,
                        label=f"render gap (n={len(gap_idx)})")
        ax6.set_xlabel("log row index")
        ax6.set_ylabel("Δt (ms)")
        ax6.set_title("Present Δt timeline  (gaps = dropped renders)")
        ax6.legend(fontsize=8, loc="upper right")
        # Clamp y to keep huge stop_rendering jumps from squashing the plot
        if p_dt:
            ax6.set_ylim(0, min(max(p_dt) * 1.1, 200))
    else:
        ax6.text(0.5, 0.5, "no present_elapsed_secs in log",
                 ha="center", va="center", transform=ax6.transAxes, fontsize=10, color="#888")
        ax6.set_title("Present Δt timeline")
    ax6.grid(True, alpha=0.3)

    plt.tight_layout(rect=[0, 0, 1, 0.95])
    out_path = out_dir / f"{trial.label}.png"
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
        f"{'Out%':>5s}  {'FPS':>5s}  {'Frz':>3s}  {'Drift':>7s}"
    )
    lines.append(hdr)
    lines.append("-" * len(hdr))

    for s in all_stats:
        row = (
            f"{s.label:<35s}  {s.platform:<6s}  {s.n_frames:>6d}  {s.gaps:>4d}  {s.duplicates:>4d}  "
            f"{s.dt_mean * 1000:>7.2f}ms  {s.dt_std * 1000:>6.2f}ms  {s.dt_min * 1000:>6.2f}ms  {s.dt_max * 1000:>6.2f}ms  "
            f"{s.outlier_pct:>4.1f}%  {s.avg_fps:>5.1f}  {s.freeze_count:>3d}  {s.timing_drift:>6.3f}s"
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

    # Summary
    summary = format_summary(all_stats)
    print("\n" + summary)

    summary_path = out_dir / "summary.txt"
    with open(summary_path, "w") as f:
        f.write(summary)
    print(f"\nSummary saved to {summary_path}")
    print(f"Plots saved to   {out_dir}/")


if __name__ == "__main__":
    main()
