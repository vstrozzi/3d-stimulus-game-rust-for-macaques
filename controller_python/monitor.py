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

import sys
import time
import argparse

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build with: maturin develop (in shared/)")
    sys.exit(1)


def main():
    parser = argparse.ArgumentParser(description="SHM IPC monitor")
    parser.add_argument("--hz", type=float, default=50, help="Sampling rate (default 50)")
    args = parser.parse_args()
    interval = 1.0 / args.hz

    try:
        shm = monkey_shared.SharedMemoryWrapper("monkey_game")
    except Exception as e:
        print(f"Cannot attach to SHM: {e}")
        sys.exit(1)

    print(f"Monitoring at {args.hz:.0f} Hz  (Ctrl-C to stop)")
    print(f"{'time_s':>8}  {'seq':>6}  {'ack':>6}  {'pend':>4}  {'head':>8}  {'Δhead':>5}  {'dt_ms':>6}  {'rtt_ms':>7}")
    print("-" * 72)

    prev_seq = shm.read_command_seq()
    prev_ack = shm.read_command_ack()
    prev_head = shm.frame_write_head()
    prev_time = time.perf_counter()

    # Track when seq last changed (for round-trip estimate)
    seq_change_time = None
    rtt_display = ""

    t0 = time.perf_counter()

    try:
        while True:
            time.sleep(interval)
            now = time.perf_counter()
            dt = now - prev_time

            seq = shm.read_command_seq()
            ack = shm.read_command_ack()
            head = shm.frame_write_head()
            pending = seq - ack
            d_head = head - prev_head

            # Detect seq change → start RTT clock
            if seq != prev_seq:
                seq_change_time = now

            # Detect ack catching up to seq → measure RTT
            if seq_change_time is not None and ack >= seq:
                rtt_ms = (now - seq_change_time) * 1000
                rtt_display = f"{rtt_ms:7.2f}"
                seq_change_time = None
            elif seq_change_time is not None:
                rtt_display = "   ..."
            else:
                rtt_display = "      -"

            elapsed = now - t0
            dt_ms = dt * 1000

            print(f"{elapsed:8.2f}  {seq:6d}  {ack:6d}  {pending:4d}  {head:8d}  {d_head:5d}  {dt_ms:6.2f}  {rtt_display}")

            prev_seq = seq
            prev_ack = ack
            prev_head = head
            prev_time = now

    except KeyboardInterrupt:
        print("\nStopped.")


if __name__ == "__main__":
    main()
