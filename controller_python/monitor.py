"""
Shared-memory monitor — run alongside the game + controller to track IPC timing.

Reads command_seq (controller→game), command_ack (game→controller), and
frame_write_head from SHM and prints per-tick stats:
  - seq / ack values and whether the game is behind
  - frame_write_head delta (frames produced since last tick)
  - round-trip latency estimate (time between seq change and ack catch-up)

Usage:
    python monitor.py              # default 50 Hz sampling
    python monitor.py --hz 100     # 100 Hz sampling
"""

import os
import sys
import time
import argparse
from collections import deque

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build with: maturin develop (in shared/)")
    sys.exit(1)


def get_p99(window_deque):
    """Calculates the 99th percentile from a deque of samples."""
    if not window_deque:
        return None
    sorted_data = sorted(window_deque)
    idx = min(len(sorted_data) - 1, int(0.99 * len(sorted_data)))
    return sorted_data[idx]


def main():
    parser = argparse.ArgumentParser(
        description="SHM IPC monitor — real-time view of game/controller shared memory.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
Column reference:
  IPC columns (left of │):
    time_s   Wall-clock seconds since monitor start
    seq      command_seq — command sequence number (controller → game)
    ack      command_ack — acknowledgement number (game → controller)
    pend     seq − ack: commands sent but not yet acknowledged
    head     Ring-buffer write_head (monotonic frame counter in SHM)
    Δhead    Frames written to ring buffer since last monitor poll
    frame#   Game tick counter (+1 per Bevy FixedUpdate, even during stop_rendering)
    rnd#     Render frame counter (+1 per Bevy Update/render frame, matches photodiode)
    dt_ms    Monitor poll interval in ms (wall-clock between samples)
    rtt_ms   Round-trip latency: seq change → ack catch-up ('...' = waiting, '-' = idle)
    p99rt    99th percentile of rtt_ms over the last 1000 samples

  Stats columns (right of │, computed from every ring-buffer frame):
    g_dt     Game-side frame delta in ms (elapsed_secs difference between frames)
    p99dt    99th percentile of g_dt over the last 1000 frames
    FPS      Instantaneous game FPS (1 / g_dt)
    avgFPS   Running average game FPS since monitor start
    r_dt     Render-frame delta in ms (render_elapsed_secs difference between snapshots)
    pdi      Photodiode square state: W=white, B=black, -=hidden/off
    gaps     Cumulative skipped frame_numbers (frame_number jumped by >1)
    dups     Cumulative duplicate frame_numbers
    out%     Percentage of game deltas outside [8.33ms, 33.33ms] (0.5×–2× of 16.67ms)
    frz      Freeze events: 3+ consecutive frames with identical elapsed_secs
             outside of animation
""",
    )
    parser.add_argument("--hz", type=float, default=60, help="Monitor sampling rate in Hz (default: 60)")
    args = parser.parse_args()
    interval = 1.0 / args.hz

    try:
        shm = monkey_shared.SharedMemoryWrapper("monkey_game")
    except Exception as e:
        print(f"Cannot attach to SHM: {e}")
        sys.exit(1)

    HEADER_LINE = (
        f"{'time_s':>8}  {'seq':>6}  {'ack':>6}  {'pend':>4}  {'head':>8}  {'Δhead':>5}  {'frame#':>8}  {'rnd#':>8}  "
        f"{'dt_ms':>6}  {'rtt_ms':>7}  {'p99rt':>6}  │  "
        f"{'g_dt':>6}  {'p99dt':>6}  {'FPS':>5}  {'avgFPS':>6}  {'r_dt':>6}  {'pdi':>3}  {'gaps':>4}  {'dups':>4}  {'out%':>5}  {'frz':>3}"
    )
    SEP_LINE = "-" * len(HEADER_LINE)

    # Get terminal height; set up a scroll region below the fixed header
    rows = os.get_terminal_size().lines
    CSI = "\033["
    # Clear screen, print header in rows 1-3, set scroll region to rows 4+
    sys.stdout.write(f"{CSI}2J{CSI}H")  # clear & home
    sys.stdout.write(f"{CSI}1;1H Monitoring at {args.hz:.0f} Hz  (Ctrl-C to stop)\n")
    sys.stdout.write(f"{CSI}2;1H{HEADER_LINE}\n")
    sys.stdout.write(f"{CSI}3;1H{SEP_LINE}")
    sys.stdout.write(f"{CSI}4;{rows}r")  # scroll region: row 4 to bottom
    sys.stdout.write(f"{CSI}4;1H")       # cursor into scroll region
    sys.stdout.flush()

    prev_seq = shm.read_command_seq()
    prev_head = shm.frame_write_head()
    prev_time = time.perf_counter()
    last_ring_head = shm.frame_write_head()

    # Track when seq last changed (for round-trip estimate)
    seq_change_time = None
    rtt_display = ""
    
    # Sliding windows for P99 calculation
    window_size = 1000
    recent_rtt = deque(maxlen=window_size)
    recent_g_dt = deque(maxlen=window_size)

    # Running trial-log statistics
    EXPECTED_DT = 1.0 / 60.0
    OUTLIER_LO = EXPECTED_DT * 0.5
    OUTLIER_HI = EXPECTED_DT * 2.0
    prev_frame_number = None
    prev_elapsed_secs = None
    total_gaps = 0
    total_dups = 0
    game_dt_sum = 0.0
    game_dt_count = 0
    outlier_count = 0
    freeze_run = 0
    freeze_count = 0
    last_game_dt = None
    latest_frame_number = 0
    prev_render_elapsed = None
    last_render_dt = None
    latest_photodiode = None

    t0 = time.perf_counter()

    try:
        while True:
            time.sleep(interval)
            now = time.perf_counter()
            dt = now - prev_time

            seq = shm.read_command_seq()
            ack = shm.read_command_ack()
            head = shm.frame_write_head()
            snapshot = shm.read_game_state()
            render_frame = snapshot.get("render_frame_number", 0) if snapshot else 0
            render_es = snapshot.get("render_elapsed_secs", 0.0) if snapshot else 0.0
            pdi_white = snapshot.get("photodiode_white", None) if snapshot else None
            latest_photodiode = pdi_white

            # Render-frame dt
            if prev_render_elapsed is not None and render_es > 0:
                r_dt = render_es - prev_render_elapsed
                if r_dt > 0:
                    last_render_dt = r_dt
            if render_es > 0:
                prev_render_elapsed = render_es

            pending = seq - ack
            d_head = head - prev_head

            # Drain all new frames from ring buffer
            new_ring_head, states = shm.read_game_state_since(last_ring_head)
            last_ring_head = new_ring_head

            for state in states:
                fn = state.get("frame_number", 0)
                es = state.get("elapsed_secs", 0.0)
                is_anim = state.get("is_animating", False)
                latest_frame_number = fn

                # Gaps & duplicates
                if prev_frame_number is not None:
                    fdiff = fn - prev_frame_number
                    if fdiff > 1:
                        total_gaps += fdiff - 1
                    elif fdiff == 0:
                        total_dups += 1

                # Game dt & outliers
                if prev_elapsed_secs is not None:
                    g_dt = es - prev_elapsed_secs
                    if g_dt > 0:
                        last_game_dt = g_dt
                        recent_g_dt.append(g_dt) # Add to sliding window
                        
                        game_dt_sum += g_dt
                        game_dt_count += 1
                        if g_dt < OUTLIER_LO or g_dt > OUTLIER_HI:
                            outlier_count += 1
                        freeze_run = 0
                    elif g_dt == 0.0:
                        # Freeze detection
                        if not is_anim:
                            freeze_run += 1
                            if freeze_run == 3:
                                freeze_count += 1
                        else:
                            freeze_run = 0

                prev_frame_number = fn
                prev_elapsed_secs = es

            # Detect seq change → start RTT clock
            if seq != prev_seq:
                seq_change_time = now

            # Detect ack catching up to seq → measure RTT
            if seq_change_time is not None and ack >= seq:
                rtt_ms = (now - seq_change_time) * 1000
                recent_rtt.append(rtt_ms) # Add to sliding window
                
                rtt_display = f"{rtt_ms:7.2f}"
                seq_change_time = None
            elif seq_change_time is not None:
                rtt_display = "   ..."
            else:
                rtt_display = "      -"

            # Calculate percentiles
            p99_rtt_val = get_p99(recent_rtt)
            p99_dt_val = get_p99(recent_g_dt)

            # Format stat columns
            p99_rtt_display = f"{p99_rtt_val:6.2f}" if p99_rtt_val is not None else "     -"
            game_dt_display = f"{last_game_dt * 1000:6.2f}" if last_game_dt else "     -"
            p99_dt_display = f"{p99_dt_val * 1000:6.2f}" if p99_dt_val is not None else "     -"
            fps_display = f"{1.0 / last_game_dt:5.1f}" if last_game_dt else "    -"
            avg_fps_display = f"{1.0 / (game_dt_sum / game_dt_count):6.1f}" if game_dt_count > 0 else "     -"
            outlier_pct = (100.0 * outlier_count / game_dt_count) if game_dt_count > 0 else 0.0
            r_dt_display = f"{last_render_dt * 1000:6.2f}" if last_render_dt else "     -"
            pdi_display = ("  W" if latest_photodiode else "  B") if latest_photodiode is not None else "  -"

            elapsed = now - t0
            dt_ms = dt * 1000

            print(
                f"{elapsed:8.2f}  {seq:6d}  {ack:6d}  {pending:4d}  {head:8d}  {d_head:5d}  {latest_frame_number:8d}  {render_frame:8d}  "
                f"{dt_ms:6.2f}  {rtt_display}  {p99_rtt_display}  │  "
                f"{game_dt_display}  {p99_dt_display}  {fps_display}  {avg_fps_display}  {r_dt_display}  {pdi_display}  {total_gaps:4d}  {total_dups:4d}  {outlier_pct:4.1f}%  {freeze_count:3d}"
            )

            prev_seq = seq
            prev_head = head
            prev_time = now

    except KeyboardInterrupt:
        # Reset scroll region and move cursor below content
        sys.stdout.write(f"{CSI}r{CSI}{rows};1H\n")
        sys.stdout.flush()
        print("Stopped.")


if __name__ == "__main__":
    main()
