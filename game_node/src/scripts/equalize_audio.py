#!/usr/bin/env python3
"""Peak-equalize a set of audio files so they all share the same maximum level.

Every input file is scanned for its peak amplitude and then gained so its peak
lands on a single common target (default -1.0 dBFS). After this the game's
per-track volume constants (BACKGROUND_MUSIC_VOLUME / SOUND_EFFECTS_VOLUME in
shared/src/constants.rs) balance the tracks against each other instead of
fighting per-file peak differences.

Requires ffmpeg on PATH (peak detection via the `volumedetect` filter).

Usage:
    python game_node/src/scripts/equalize_audio.py game_node/assets/sounds
    python game_node/src/scripts/equalize_audio.py a.ogg b.ogg --target -1.0
    python game_node/src/scripts/equalize_audio.py sounds/ --out-dir normalized/
"""
import argparse
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

AUDIO_EXTS = {".ogg", ".mp3", ".wav", ".flac", ".m4a", ".aac", ".opus"}
_MAX_VOLUME_RE = re.compile(r"max_volume:\s*(-?\d+(?:\.\d+)?)\s*dB")


def detect_peak_db(path: Path) -> float:
    """Return the file's peak amplitude in dBFS (0.0 = full scale)."""
    proc = subprocess.run(
        ["ffmpeg", "-hide_banner", "-i", str(path), "-af", "volumedetect", "-f", "null", "-"],
        capture_output=True, text=True,
    )
    match = _MAX_VOLUME_RE.search(proc.stderr)
    if not match:
        raise RuntimeError(f"could not read max_volume for {path}\n{proc.stderr}")
    return float(match.group(1))


def apply_gain(src: Path, dst: Path, gain_db: float) -> None:
    """Re-encode src into dst (same container, inferred from extension) with gain."""
    subprocess.run(
        ["ffmpeg", "-hide_banner", "-loglevel", "error", "-y",
         "-i", str(src), "-af", f"volume={gain_db:.4f}dB", str(dst)],
        check=True,
    )


def collect_files(inputs: list[str]) -> list[Path]:
    files: list[Path] = []
    for raw in inputs:
        p = Path(raw)
        if p.is_dir():
            files += sorted(f for f in p.iterdir() if f.suffix.lower() in AUDIO_EXTS)
        elif p.is_file():
            files.append(p)
        else:
            print(f"  ! skipping (not found): {p}", file=sys.stderr)
    return files


def main() -> int:
    ap = argparse.ArgumentParser(description="Peak-equalize audio files to a common max level.")
    ap.add_argument("inputs", nargs="+", help="audio files and/or directories")
    ap.add_argument("--target", type=float, default=-1.0,
                    help="common peak target in dBFS (default: -1.0)")
    ap.add_argument("--out-dir", type=Path, default=None,
                    help="write normalized files here (default: edit in place)")
    args = ap.parse_args()

    if shutil.which("ffmpeg") is None:
        print("error: ffmpeg not found on PATH (install it, e.g. `sudo apt install ffmpeg`)",
              file=sys.stderr)
        return 1

    files = collect_files(args.inputs)
    if not files:
        print("error: no audio files to process", file=sys.stderr)
        return 1

    if args.out_dir is not None:
        args.out_dir.mkdir(parents=True, exist_ok=True)

    for f in files:
        peak = detect_peak_db(f)
        gain = args.target - peak
        print(f"{f.name}: peak {peak:+.2f} dB -> {args.target:+.2f} dB (gain {gain:+.2f} dB)")
        if args.out_dir is not None:
            apply_gain(f, args.out_dir / f.name, gain)
        else:
            # ffmpeg can't read and write the same path; stage in a temp file.
            with tempfile.NamedTemporaryFile(suffix=f.suffix, delete=False) as tmp:
                tmp_path = Path(tmp.name)
            apply_gain(f, tmp_path, gain)
            shutil.move(str(tmp_path), str(f))

    print(f"\nDone — {len(files)} file(s) equalized to {args.target:+.2f} dBFS peak.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
