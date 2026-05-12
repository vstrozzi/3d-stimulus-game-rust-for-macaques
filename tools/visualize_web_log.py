#!/usr/bin/env python3
"""
visualize_web_log.py — Session-level dashboard for a web download bundle.

Reads a `.zip` produced by the in-game "Download Logs" button (or an
already-unpacked directory) and produces:

  1. A summary table (delegated to verify_trial_logs.analyse_trial).
  2. A session timeline plot:
        - outcome strip (advance / stay / retroceed) per trial
        - attempts per trial (bar)
        - elapsed time per trial (bar)
        - cosine_alignment trajectory overlay across trials

The intent mirrors `controller_python/monitor.py` for the native side: a
single place to see whether a session is running correctly (no dropped
frames, sane outcomes, no freezes) and to inspect behaviour over time.

Usage:
    python tools/visualize_web_log.py monkeyA_2026-05-12_14-32-08.zip
    python tools/visualize_web_log.py path/to/unpacked/bundle/

Requirements: matplotlib (pip install matplotlib)
"""

import argparse
import sys
from pathlib import Path

# Reuse loaders and per-trial stats from the existing verifier so the two
# tools never drift out of sync.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from verify_trial_logs import (  # noqa: E402
    EXPECTED_DT,
    analyse_trial,
    format_summary,
    load_path,
)

try:
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
except ImportError:
    print("Error: matplotlib is required.  Install with:  pip install matplotlib")
    sys.exit(1)


OUTCOME_COLORS = {
    "advance": "#55cc88",
    "stay": "#e8a735",
    "retroceed": "#e05555",
}


def plot_session(trials, out_path: Path) -> None:
    """Render a 4-panel session overview as a single PNG."""
    if not trials:
        print("No trials to plot.")
        return

    # Sort trials by start timestamp so the timeline is meaningful even if
    # the zip preserves a different file order.
    trials = sorted(trials, key=lambda t: t.label)

    fig, axes = plt.subplots(4, 1, figsize=(14, 11), sharex=True)
    fig.suptitle(
        f"Web session overview — {len(trials)} trial{'s' if len(trials) != 1 else ''}",
        fontsize=12, fontweight="bold",
    )

    xs = list(range(len(trials)))
    labels = [t.label for t in trials]

    # ── Panel 1: outcome strip ────────────────────────────────────────────
    ax = axes[0]
    for i, t in enumerate(trials):
        color = OUTCOME_COLORS.get(t.outcome, "#888")
        ax.barh(0, 1, left=i - 0.5, height=0.8, color=color, edgecolor="#222")
    ax.set_yticks([])
    ax.set_xlim(-0.5, len(trials) - 0.5)
    ax.set_title("Outcome")
    handles = [
        plt.Rectangle((0, 0), 1, 1, color=c, label=name)
        for name, c in OUTCOME_COLORS.items()
    ]
    ax.legend(handles=handles, loc="upper right", fontsize=8, ncol=3)

    # ── Panel 2: attempts per trial ───────────────────────────────────────
    ax = axes[1]
    attempts = [t.nr_attempts for t in trials]
    ax.bar(xs, attempts, color="#7c6ff7", edgecolor="#3a2f99")
    ax.set_ylabel("nr_attempts")
    ax.set_title("Attempts per trial")
    ax.grid(True, alpha=0.3, axis="y")

    # ── Panel 3: elapsed time per trial ───────────────────────────────────
    ax = axes[2]
    elapsed = [t.elapsed_time for t in trials]
    ax.bar(xs, elapsed, color="#4a8fd4", edgecolor="#26568a")
    ax.set_ylabel("elapsed (s)")
    ax.set_title("Wall-clock duration per trial")
    ax.grid(True, alpha=0.3, axis="y")

    # ── Panel 4: cosine alignment trajectory (concatenated) ───────────────
    ax = axes[3]
    cursor = 0
    for i, t in enumerate(trials):
        if not t.frames:
            continue
        # Map each trial's frame stream onto a normalised slot [i, i+1).
        n = len(t.frames)
        if n < 2:
            continue
        xs_t = [i + k / (n - 1) for k in range(n)]
        ys_t = [f.cosine_alignment for f in t.frames]
        ax.plot(xs_t, ys_t, linewidth=0.6, alpha=0.8,
                color=OUTCOME_COLORS.get(t.outcome, "#888"))
        cursor += 1
    if trials and trials[0].cosine_threshold > 0:
        ax.axhline(trials[0].cosine_threshold, color="#222", linestyle="--",
                   linewidth=1, label=f"threshold {trials[0].cosine_threshold:.2f}")
        ax.legend(fontsize=8, loc="lower right")
    ax.set_xlabel("trial index")
    ax.set_ylabel("cosine_alignment")
    ax.set_title("Alignment trajectory (one curve per trial, coloured by outcome)")
    ax.grid(True, alpha=0.3)

    # Compact x-tick labels — every Nth trial label so it stays readable.
    step = max(1, len(trials) // 20)
    axes[-1].set_xticks(xs[::step])
    axes[-1].set_xticklabels(labels[::step], rotation=45, ha="right", fontsize=7)

    plt.tight_layout(rect=[0, 0, 1, 0.96])
    fig.savefig(out_path, dpi=150, bbox_inches="tight")
    plt.close(fig)
    print(f"Session plot saved to {out_path}")


def main():
    parser = argparse.ArgumentParser(
        description="Visualize a web-controller log bundle (zip or unpacked).",
    )
    parser.add_argument("path", help="Path to a .zip download bundle or an unpacked directory.")
    parser.add_argument(
        "--output", "-o", default="out/analysis",
        help="Output directory for plots and summary (default: out/analysis).",
    )
    args = parser.parse_args()

    out_dir = Path(args.output)
    out_dir.mkdir(parents=True, exist_ok=True)

    trials = load_path(args.path, "wasm")
    if not trials:
        print(f"No trials loaded from {args.path}.")
        sys.exit(1)

    stats = [analyse_trial(t) for t in trials]
    summary = format_summary(stats)
    print(summary)

    summary_path = out_dir / "web_summary.txt"
    summary_path.write_text(summary)
    print(f"Summary saved to {summary_path}")

    plot_session(trials, out_dir / "web_session.png")
    # `EXPECTED_DT` is imported to keep matplotlib aware of timing context
    # used internally by `analyse_trial`; reference here just to silence
    # unused-import lints in editors that don't see the indirect use.
    _ = EXPECTED_DT


if __name__ == "__main__":
    main()
