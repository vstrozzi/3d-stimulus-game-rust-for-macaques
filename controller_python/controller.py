import atexit
import datetime
import json
import math
import os
import random
import signal
import sys
import tempfile
import time
from enum import Enum, auto

from pynput import keyboard

PARTICIPANT_NAME = "native"
LOG_ROOT = "./out/logs"

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build the shared library with 'cargo build --release -p shared --features python' and copy the resulting '.so' to controller_python/monkey_shared.so.")
    sys.exit(1)

# Constants imported from shared/src/constants.rs via monkey_shared.
# Single source of truth — mirrored to controller_main.js through wasm-bindgen.
REFRESH_RATE_HZ = monkey_shared.REFRESH_RATE_HZ
WIN_BLANK_DURATION_FRAMES = monkey_shared.WIN_BLANK_DURATION_FRAMES
SHM_NAME = monkey_shared.SHM_NAME
POLLING_RATE_S = monkey_shared.POLLING_RATE_S
GAME_UNRESPONSIVENESS_THRESHOLD_S = monkey_shared.GAME_UNRESPONSIVENESS_THRESHOLD_S
COLOR_SUGGESTION_COS_SIM = monkey_shared.COLOR_SUGGESTION_COS_SIM
DEFAULT_CAMERA_Y = monkey_shared.DEFAULT_CAMERA_Y
CAMERA_3D_INITIAL_RADIUS = monkey_shared.CAMERA_3D_INITIAL_RADIUS
N_START_ORIENTS = monkey_shared.N_START_ORIENTS

# Evenly-spaced start orientations (one per door of the hexagonal base)
START_ORIENTS = [k * 2.0 * math.pi / N_START_ORIENTS for k in range(N_START_ORIENTS)]

CONTROLLER_META_FIELDS = set(monkey_shared.CONTROLLER_META_FIELDS)

# Game-state schema (fields written to shared memory) — matches SharedGameState in shared/src/lib.rs
state_schema = {
    "decorations_seeds": [int],
    "base_radius": float,
    "height": float,
    "start_orient": float,
    "target_door": int,
    "colors": [[float]],
    "textures": [int],
    "decorations_count": [int],
    "decorations_size": [float],
    "decorations_thickness": [float],
    "decorations_texture": [int],
    "decorations_shape": [int],
    "decorations_rotation": [int],
    "decorations_color": [[float]],
    "cosine_alignment_threshold": float,
    "door_anim_fade_out": float,
    "door_anim_stay_open": float,
    "door_anim_fade_in": float,
    "main_spotlight_intensity": float,
    "ambient_brightness": float,
    "max_spotlight_intensity": float,
    "camera_radius": float,
    "camera_speed_rotate": float,
    "camera_rotation_sense": int,
}

# Fields present in level["fixed"] that are shared across all trials in a level
FIXED_FIELDS = {
    "base_radius",
    "height",
    "start_orient",
    "cosine_alignment_threshold",
    "door_anim_fade_out",
    "door_anim_stay_open",
    "door_anim_fade_in",
    "main_spotlight_intensity",
    "ambient_brightness",
    "max_spotlight_intensity",
    "camera_radius",
    "camera_speed_rotate",
    "camera_rotation_sense",
    "camera_y",
}


def validate_object_schema(obj):
    """Check that an object dict contains all required game-state fields (excluding fixed fields)."""
    object_fields = {k for k in state_schema if k not in FIXED_FIELDS}
    for key in object_fields:
        if key not in obj:
            print(f"Missing required object key: '{key}'")
            return False
    return True


def expand_flat_trial(obj, trial_cfg, fixed):
    """
    Merge object + trial_cfg + fixed into one flat dict suitable for write_game_state.
    start_orient comes from fixed (set once per level).
    """
    flat = {}
    # Object visual fields
    for k, v in obj.items():
        flat[k] = v
    # Fixed fields (base_radius, height, start_orient, lighting, animation, camera)
    for k, v in fixed.items():
        if k not in ("pr_switching_chain", "start_trial"):
            flat[k] = v
    # Controller meta fields
    for k, v in trial_cfg.items():
        flat[k] = v
    return flat


def _backfill_level_defaults(level):
    """Backfill fields that were added after the original schema, so older
    trials.jsonl files keep loading."""
    fixed = level.setdefault("fixed", {})
    fixed.setdefault("camera_rotation_sense", 1)
    for obj in level.get("objects", []):
        obj.setdefault("decorations_rotation", [0, 0, 0])


def load_levels(trials_path="trials_config/trials.jsonl"):
    """Load levels from new JSONL format. Each line is a level with objects/trials/fixed."""
    levels = []
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    trial_file = os.path.join(parent_dir, trials_path)

    try:
        with open(trial_file, "r", encoding="utf-8") as f:
            for line_num, line in enumerate(f, 1):
                line = line.strip()
                if not line:
                    continue
                level = json.loads(line)
                # Validate structure
                if "objects" not in level or "trials" not in level or "fixed" not in level:
                    print(f"Warning: line {line_num} missing objects/trials/fixed, skipping")
                    continue
                if len(level["objects"]) < 1:
                    print(f"Warning: line {line_num} needs at least 1 object, skipping")
                    continue
                _backfill_level_defaults(level)
                ok = all(validate_object_schema(o) for o in level["objects"])
                if not ok:
                    print(f"Warning: line {line_num} has invalid object structure, skipping")
                    continue
                levels.append(level)

        print(f"Loaded {len(levels)} levels from {trial_file}")
    except Exception as e:
        print(f"Failed to load levels: {e}")
    return levels


class ControllerState(Enum):
    INIT = auto()
    WAITING_FOR_START = auto()
    PLAYING = auto()
    WAITING_ANIMATION_START = auto()
    WAITING_ANIMATION_END = auto()
    TRIAL_COMPLETE = auto()


class TrialProceeding(Enum):
    ADVANCE = auto()
    STAY = auto()
    RETROCEED = auto()


# Catches drift if a state is renamed only on one side: each ControllerState
# name must appear in monkey_shared.FSM_STATES (which controller_main.js also reads).
assert [s.name for s in ControllerState] == list(monkey_shared.FSM_STATES), (
    f"ControllerState drifted from shared FSM_STATES: "
    f"py={[s.name for s in ControllerState]} shared={list(monkey_shared.FSM_STATES)}"
)
assert [s.name for s in TrialProceeding] == list(monkey_shared.PROCEEDING_VALUES), (
    f"TrialProceeding drifted from shared PROCEEDING_VALUES: "
    f"py={[s.name for s in TrialProceeding]} shared={list(monkey_shared.PROCEEDING_VALUES)}"
)


class MonkeyGameController:
    def __init__(self):
        self.pressed_keys = set()

        # Shared-memory handle
        try:
            self.shm_wrapper = monkey_shared.SharedMemoryWrapper(SHM_NAME)
            print("Connected to shared memory interface.")
        except Exception as exc:
            print(f"SHM Connection Error: {exc}")
            sys.exit(1)

        # Catch log-schema drift early: every name in LOGGED_STATE_FIELDS must
        # be a real key in the SHM-read dict, otherwise the verifier and
        # controller_main.js will silently disagree.
        _default_keys = set(self.shm_wrapper.read_default_game_state().keys())
        _missing = self.LOGGED_STATE_FIELDS - _default_keys
        if _missing:
            raise RuntimeError(
                f"LOGGED_STATE_FIELDS references SHM keys that read_game_state "
                f"doesn't expose: {sorted(_missing)}"
            )

        # Continuous inputs
        self.inputs = {
            "rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
        }
        # One-shot triggers (cleared after every write)
        self.triggers = {
            "check": False,
            "reset": False,
            "toggle_blank": False,
            "toggle_stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        }

        # Session metadata, written once into every trial log.
        self.session_info = {
            "app_start_unix_ns": time.time_ns(),
            "platform": "native",
            "os": sys.platform,
            "refresh_rate_hz": float(REFRESH_RATE_HZ),
            "present_mode": "fifo",
        }

        # Level configuration
        self.levels = load_levels()
        self.total_levels = len(self.levels)

        # Current level state
        self.current_level_index = 0
        self.chain_idxs = []
        self.active_chain = 0
        if self.levels:
            start = self.levels[0]["fixed"].get("start_trial", 0)
            self.chain_idxs = [start] * len(self.levels[0]["objects"])

        # Frame tracking
        self.current_frame = -1
        self.last_write_head = 0

        # FSM
        self.fsm_state = ControllerState.INIT
        self.trial_proceeding = TrialProceeding.ADVANCE

        # Special commands
        self._start = False
        self._time_win_expired = False

        # Per-trial tracking
        self.nr_attempts = 0
        self.trial_start_time = 0.0
        self.game_time_unresponsive = 0.0
        self.trial_start_state = None
        self.trial_start_orient = None
        self._frame_zero = None
        self._render_frame_zero = None
        self.frame_log = {}
        self.win_event = None
        self.trial_run_counter = 0
        self.current_state = None

        # Per-level-run tracking
        self.level_run_counter = 0
        self.current_level_summary = None
        self.current_level_summary_path = None
        self.current_level_dir = None
        self._last_summary_filename = None
        self._last_summary_path = None

        # Output root for logs
        script_dir = os.path.dirname(os.path.abspath(__file__))
        parent_dir = os.path.dirname(script_dir)
        self.log_root = os.path.join(parent_dir, LOG_ROOT.lstrip("./"))
        os.makedirs(self.log_root, exist_ok=True)

        atexit.register(self._finalize_level_run_safe, "interrupted")
        for _sig in (signal.SIGINT, signal.SIGTERM):
            try:
                signal.signal(_sig, lambda *_: (self._finalize_level_run_safe("interrupted"), sys.exit(0)))
            except (ValueError, OSError):
                pass

        self._running = True

        # Keyboard listener (own thread)
        self.listener = keyboard.Listener(
            on_press=self.on_key_press, on_release=self.on_key_release
        )
        self.listener.start()

    # Level/chain helpers
    @property
    def level(self):
        """Current level config dict."""
        return self.levels[self.current_level_index]

    @property
    def flat_trial(self):
        """Current flat trial dict (object + fixed + trial_cfg)."""
        obj = self.level["objects"][self.active_chain]
        trial_idx = min(self.chain_idxs[self.active_chain], len(self.level["trials"]) - 1)
        trial_cfg = self.level["trials"][trial_idx]
        return expand_flat_trial(obj, trial_cfg, self.level["fixed"])

    def _trial_idx(self):
        return self.chain_idxs[self.active_chain]

    def _set_trial_idx(self, val):
        self.chain_idxs[self.active_chain] = val

    def _level_start_trial(self):
        return self.level["fixed"].get("start_trial", 0)

    def _level_complete(self):
        n = len(self.level["trials"])
        return all(idx >= n for idx in self.chain_idxs)

    def _maybe_switch_chain(self):
        """With probability pr, switch to a random other non-exhausted chain."""
        n_objects = len(self.level["objects"])
        if n_objects <= 1:
            return
        pr = self.level["fixed"].get("pr_switching_chain", 1.0 / n_objects)
        if random.random() >= pr:
            return
        n = len(self.level["trials"])
        candidates = [i for i in range(n_objects) if i != self.active_chain and self.chain_idxs[i] < n]
        if candidates:
            self.active_chain = random.choice(candidates)

    def game_state_fields(self, flat):
        """Return only the game-state keys (no controller meta)."""
        return {k: v for k, v in flat.items() if k not in CONTROLLER_META_FIELDS}

    def write_config_on_state(self, flat, state):
        """Overlay flat trial game-state config onto a base state dict."""
        for key, value in flat.items():
            if key not in CONTROLLER_META_FIELDS:
                state[key] = value
        return state

    # Count only progress from the loaded start_trial so resumed levels
    # reflect the visible chain segment rather than the absolute trial index.
    def _progress_bar_cur(self):
        return sum(max(0, idx) for idx in self.chain_idxs)

    def _progress_bar_size(self):
        remaining = max(0, len(self.level["trials"]))
        return remaining * len(self.level["objects"])

    # Command helpers
    def check_has_finished(self, state):
        trial = self.flat_trial
        nr_attempts_to_retroceed = trial.get("nr_attempts_to_retroceed", 0)
        time_elapsed = state.get("elapsed_secs", 0.0)
        elapsed_time_to_retroceed = trial.get("elapsed_time_to_retroceed", 0.0)

        return (
            state.get("win_elapsed_secs", 0.0) != 0.0
            or self.nr_attempts > nr_attempts_to_retroceed
            or time_elapsed > elapsed_time_to_retroceed
        )

    def reset_commands(self):
        self.inputs = {k: False for k in self.inputs}
        self.triggers = {k: False for k in self.triggers}

    def reset_triggers(self):
        self.triggers = {k: False for k in self.triggers}

    def _has_pending_toggles(self):
        """Check if there are unacknowledged commands in SHM."""
        return self.shm_wrapper.read_command_ack() < self.shm_wrapper.command_seq()

    def write_commands(self, commands=None):
        if commands is None:
            data_to_write = {**self.inputs, **self.triggers}
        else:
            data_to_write = commands

        self.shm_wrapper.write_commands(**data_to_write)
        cmds_snapshot = dict(data_to_write)
        self.reset_triggers()
        return cmds_snapshot
    
    def write_no_commands(self):
        cmds = {
            "rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": False,
            "toggle_blank": False,
            "toggle_stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        }
        self.shm_wrapper.write_commands(**cmds)
        return cmds

    # Fields that are game→controller only (written by the game, not the controller)
    READ_ONLY_FIELDS = {"render_frame_number", "render_elapsed_secs", "present_elapsed_secs", "photodiode_white", "_type"}

    def write_game_state(self, state):
        filtered = {k: v for k, v in state.items() if k not in self.READ_ONLY_FIELDS}
        self.shm_wrapper.write_game_state(**filtered)

    # Pulled from shared/src/constants.rs so the Python and JS controllers
    # log the exact same set of SHM fields.
    LOGGED_STATE_FIELDS = set(monkey_shared.LOGGED_STATE_FIELDS)

    def log_frame(self, state_read, commands_sent):
        raw_fn = int(state_read.get("frame_number", self.current_frame))
        raw_rfn = int(state_read.get("render_frame_number", 0))

        if self._frame_zero is None:
            self._frame_zero = raw_fn
        if self._render_frame_zero is None:
            self._render_frame_zero = raw_rfn

        logged_fn = raw_fn - self._frame_zero
        logged_rfn = raw_rfn - self._render_frame_zero

        win_secs = state_read.get("win_elapsed_secs", 0.0) or 0.0
        if win_secs != 0.0 and self.win_event is None:
            self.win_event = {
                "win_elapsed_secs": float(win_secs),
                "win_frame_number": logged_fn,
                "present_elapsed_secs": float(state_read.get("present_elapsed_secs", 0.0)),
            }
        filtered_state = {k: v for k, v in state_read.items() if k in self.LOGGED_STATE_FIELDS}
        if "frame_number" in filtered_state:
            filtered_state["frame_number"] = logged_fn
        if "render_frame_number" in filtered_state:
            filtered_state["render_frame_number"] = logged_rfn
        entry = {"state_read": filtered_state, "commands_sent": commands_sent}
        key = str(logged_fn)
        self.frame_log[key] = entry

    @staticmethod
    def _stamp(dt):
        return dt.strftime("%Y%m%d-%H%M%S")

    @staticmethod
    def _time_folder(dt):
        return dt.strftime("%H%M%S")

    def _level_run_paths(self, level_index, started_at):
        date = started_at.strftime("%Y-%m-%d")
        level_name = f"level_{level_index:03d}"
        run_stamp = self._stamp(started_at)
        run_time = self._time_folder(started_at)
        level_dir = os.path.join(
            self.log_root, PARTICIPANT_NAME, date, level_name, run_time
        )
        trials_dir = os.path.join(level_dir, "trials")
        summary_filename = f"{PARTICIPANT_NAME}_{level_name}_summary_{run_stamp}.json"
        return level_dir, trials_dir, level_name, summary_filename

    def _trial_filename(self, level_index, trial_idx_in_chain, active_chain, trial_start_dt):
        level_name = f"level_{level_index:03d}"
        return (
            f"{PARTICIPANT_NAME}_{level_name}"
            f"_trial_{trial_idx_in_chain:03d}_run_{self.trial_run_counter:04d}"
            f"_object_{active_chain:03d}_{self._stamp(trial_start_dt)}.json"
        )

    def _atomic_write_json(self, path, payload):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        fd, tmp = tempfile.mkstemp(prefix=".tmp_", dir=os.path.dirname(path))
        try:
            with os.fdopen(fd, "w") as f:
                json.dump(payload, f, indent=2, default=str)
            os.replace(tmp, path)
        finally:
            if os.path.exists(tmp):
                try:
                    os.remove(tmp)
                except OSError:
                    pass

    def _start_level_run_if_needed(self):
        if self.current_level_summary is not None:
            return
        started = datetime.datetime.now()
        level_dir, trials_dir, level_name, summary_filename = self._level_run_paths(
            self.current_level_index, started
        )
        self.current_level_dir = level_dir
        self.current_level_summary_path = os.path.join(level_dir, summary_filename)
        os.makedirs(trials_dir, exist_ok=True)
        level_cfg = {k: v for k, v in self.levels[self.current_level_index].items() if k != "fixed"}
        self.current_level_summary = {
            "participant": PARTICIPANT_NAME,
            "level_index": self.current_level_index,
            "level_name": level_name,
            "level_run_counter": self.level_run_counter,
            "session_info": self.session_info,
            "level_config": level_cfg,
            "timestamp_start": started.isoformat(),
            "timestamp_end": None,
            "elapsed_time_no_anim": None,
            "elapsed_time_anim": None,
            "level_completed": None,
            "trials_runs": [],
            "timing_health": None,
            "prev_file": self._last_summary_filename,
            "next_file": None,
        }
        self._flush_level_summary()
        if self._last_summary_path:
            self._patch_prev_summary_next(summary_filename)

    def _flush_level_summary(self):
        if self.current_level_summary is None:
            return
        self._atomic_write_json(self.current_level_summary_path, self.current_level_summary)

    def _patch_prev_summary_next(self, new_filename):
        prev_path = self._last_summary_path
        if not prev_path or not os.path.exists(prev_path):
            return
        try:
            with open(prev_path, encoding="utf-8") as f:
                data = json.load(f)
            data["next_file"] = new_filename
            self._atomic_write_json(prev_path, data)
        except Exception as e:
            print(f"[LOG] Could not patch prev summary {prev_path}: {e}")

    def _compute_timing_health(self):
        present = []
        render_gaps = 0
        freezes = 0
        last_rfn = None
        for trial in self.current_level_summary["trials_runs"]:
            frames = trial.get("_frames_for_health") or []
            for f in frames:
                p = f.get("present_elapsed_secs")
                if p is not None:
                    present.append(p)
                rfn = f.get("render_frame_number")
                if last_rfn is not None and rfn is not None and rfn - last_rfn > 1:
                    render_gaps += rfn - last_rfn - 1
                if rfn is not None:
                    last_rfn = rfn
        deltas = [present[i] - present[i - 1] for i in range(1, len(present))
                  if present[i] - present[i - 1] > 0]
        if deltas:
            mean = sum(deltas) / len(deltas)
            std = math.sqrt(sum((d - mean) ** 2 for d in deltas) / len(deltas))
        else:
            mean = std = 0.0
        return {
            "present_dt_mean_ms": round(mean * 1000, 3),
            "present_dt_std_ms":  round(std  * 1000, 3),
            "render_gaps": render_gaps,
            "freeze_events": freezes,
            "drift_max_s": 0.0,
        }

    def _finalize_level_run_safe(self, status):
        try:
            self._finalize_level_run(status)
        except Exception as e:
            print(f"[LOG] finalize_level_run failed: {e}")

    def _finalize_level_run(self, status):
        if self.current_level_summary is None:
            return
        end = datetime.datetime.now()
        self.current_level_summary["timestamp_end"] = end.isoformat()
        runs = self.current_level_summary["trials_runs"]
        no_anim = sum(t.get("elapsed_time_no_anim", 0.0) or 0.0 for t in runs)
        anim = sum(t.get("elapsed_time_anim", 0.0) or 0.0 for t in runs)
        self.current_level_summary["elapsed_time_no_anim"] = no_anim
        self.current_level_summary["elapsed_time_anim"] = anim
        self.current_level_summary["level_completed"] = status
        self.current_level_summary["timing_health"] = self._compute_timing_health()
        # Drop the transient _frames_for_health field used only for health stats.
        for t in runs:
            t.pop("_frames_for_health", None)
        self._flush_level_summary()
        self._last_summary_filename = os.path.basename(self.current_level_summary_path)
        self._last_summary_path = self.current_level_summary_path
        self.current_level_summary = None
        self.current_level_summary_path = None
        self.current_level_dir = None
        self.level_run_counter += 1

    def save_trial_log(self, outcome):
        self._start_level_run_if_needed()
        trial_start_dt = datetime.datetime.fromtimestamp(self.trial_start_time)
        end_dt = datetime.datetime.now()
        trial_filename = self._trial_filename(
            self.current_level_index, self._trial_idx(), self.active_chain, trial_start_dt
        )
        trial_path = os.path.join(self.current_level_dir, "trials", trial_filename)

        # Bucket present_elapsed_secs deltas by preceding frame's is_animating.
        rows = sorted(
            (
                (int(k), fr) for k, fr in self.frame_log.items()
                if isinstance(fr, dict) and isinstance(fr.get("state_read"), dict)
            ),
            key=lambda kv: kv[0],
        )
        elapsed_time_anim = 0.0
        elapsed_time_no_anim = 0.0
        for (_, prev), (_, cur) in zip(rows, rows[1:]):
            p0 = prev["state_read"].get("present_elapsed_secs")
            p1 = cur["state_read"].get("present_elapsed_secs")
            if p0 is None or p1 is None:
                continue
            dt = float(p1) - float(p0)
            if dt <= 0:
                continue
            if prev["state_read"].get("is_animating"):
                elapsed_time_anim += dt
            else:
                elapsed_time_no_anim += dt

        log = {
            "level_index": self.current_level_index,
            "active_chain": self.active_chain,
            "trial_index_in_chain": self._trial_idx(),
            "trial_run_counter": self.trial_run_counter,
            "trial_config": {k: v for k, v in self.flat_trial.items() if k != "start_orient"},
            "start_orient": self.trial_start_orient,
            "outcome": outcome,
            "nr_attempts": self.nr_attempts,
            "elapsed_time_no_anim": elapsed_time_no_anim,
            "elapsed_time_anim": elapsed_time_anim,
            "timestamp_start": trial_start_dt.isoformat(),
            "timestamp_end": end_dt.isoformat(),
            "session_info": self.session_info,
            "win_event": self.win_event,
            "frames": self.frame_log,
        }
        try:
            self._atomic_write_json(trial_path, log)
            print(f"[LOG] Saved trial log → {trial_path}")
        except Exception as e:
            print(f"[LOG] Failed to save trial: {e}")
            return

        summary_row = {
            "trial_index_in_chain": self._trial_idx(),
            "active_chain": self.active_chain,
            "trial_run_counter": self.trial_run_counter,
            "outcome": outcome,
            "nr_attempts": self.nr_attempts,
            "elapsed_time_no_anim": elapsed_time_no_anim,
            "elapsed_time_anim": elapsed_time_anim,
            "start_orient": self.trial_start_orient,
            "win_event": self.win_event,
            "file": trial_filename,
            "_frames_for_health": [
                {"present_elapsed_secs": fr.get("state_read", {}).get("present_elapsed_secs"),
                 "render_frame_number":  fr.get("state_read", {}).get("render_frame_number")}
                for fr in self.frame_log.values()
            ],
        }
        self.current_level_summary["trials_runs"].append(summary_row)
        self._flush_level_summary()

    # Main loop
    def loop(self):
        print("[FSM] Controller loop started")
        self._resync_with_game()
        while self._running:
            new_head, states = self.shm_wrapper.read_game_state_since(self.last_write_head)
            ack_nr = self.shm_wrapper.read_command_ack()
            seq_nr = self.shm_wrapper.command_seq()
            # Game has not updated yet, sleep
            if not states or ack_nr < seq_nr:
                self.game_time_unresponsive += POLLING_RATE_S
                time.sleep(POLLING_RATE_S)
                if self.game_time_unresponsive >= GAME_UNRESPONSIVENESS_THRESHOLD_S or self.current_frame == 0:
                    print(f"[FSM] Game unresponsive for {self.game_time_unresponsive:.1f}s, resyncing...")
                    self._resync_with_game()
                continue

            self.last_write_head = new_head

            if self.current_frame == -1:
                self.current_frame = states[-1].get("frame_number", 0)
                print(f"[FSM] Starting at frame {self.current_frame}")
                continue

            # Log intermediate frames only in meaningful states
            if self.fsm_state in (
                ControllerState.PLAYING,
                ControllerState.WAITING_ANIMATION_START,
                ControllerState.WAITING_ANIMATION_END,
            ):
                for s in states[:-1]:
                    fn = s.get("frame_number", 0)
                    if str(fn) not in self.frame_log:
                        self.log_frame(s, {})

            # Use the latest state for FSM dispatch
            self.current_state = states[-1]
            
            
            self.current_frame = self.current_state.get("frame_number", 0)

            self.current_state["progress_bar_cur_size"] = self._progress_bar_cur()
            self.current_state["progress_bar_size"] = self._progress_bar_size()
            
            # Clear commands    
            self.write_no_commands()

            # This modify the current state
            if self.fsm_state == ControllerState.INIT:
                self._handle_init()
            elif self.fsm_state == ControllerState.WAITING_FOR_START:
                self._handle_waiting_for_start()
            elif self.fsm_state == ControllerState.PLAYING:
                self._handle_playing()
            elif self.fsm_state == ControllerState.WAITING_ANIMATION_START:
                self._handle_waiting_animation_start()
            elif self.fsm_state == ControllerState.WAITING_ANIMATION_END:
                self._handle_waiting_animation_end()
            elif self.fsm_state == ControllerState.TRIAL_COMPLETE:
                self._handle_trial_complete()
   

            # Write the game state
            self.write_game_state(self.current_state)

            # Update the sequence number to indicate to the game that new commands/state written
            self.shm_wrapper.increment_command_seq()

            self.game_time_unresponsive = 0.0

        self.listener.stop()
        print("[FSM] Controller stopped.")

    # FSM handlers (command logic unchanged)
    def _handle_init(self):
        print("[FSM] INIT → issuing blank_screen + stop_rendering + load trial")
        flat = self.flat_trial
        print(f"[FSM] Level {self.current_level_index} chain {self.active_chain} "
              f"trial {self._trial_idx()}: {self.game_state_fields(flat)}")

        default_state = self.shm_wrapper.read_default_game_state()
        trial_state = self.write_config_on_state(flat, default_state)
        # Sample start orientation randomly from the 6 evenly-spaced door angles.
        # Editor stores a sentinel (-1) for this field; the real value is chosen
        # here and recorded into the trial log so analyses can recover it.
        trial_state["start_orient"] = random.choice(START_ORIENTS)
        self.trial_start_orient = float(trial_state["start_orient"])
        trial_state["progress_bar_cur_size"] = self._progress_bar_cur()
        trial_state["progress_bar_size"] = self._progress_bar_size()

        # Position camera using fixed camera_y and camera_radius
        cam_y = self.level["fixed"].get("camera_y", DEFAULT_CAMERA_Y)
        cam_r = trial_state.get("camera_radius", CAMERA_3D_INITIAL_RADIUS)

        trial_state["camera_x"] = 0.0
        trial_state["camera_y"] = cam_y
        trial_state["camera_z"] = cam_r

        state_old = self.shm_wrapper.read_game_state()

        self.write_commands({
            "rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": True,
            "toggle_blank": not state_old.get("is_blank", False),
            "toggle_stop_rendering": not state_old.get("is_rendering_stopped", False),
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        })

        self.current_state.update(trial_state)
        self.trial_start_state = trial_state

        self.nr_attempts = 0
        self.trial_start_time = time.time()
        self.frame_log = {}
        self._frame_zero = None
        self._render_frame_zero = None
        self.win_event = None
        self._time_win_expired = False

        # Re-sync frame tracking with the game's fresh counter.
        # `handle_reset_command` on the game side zeros `frame_number` (and
        # `elapsed_secs`) via setup_round. Without this re-sync, the
        # controller's `self.current_frame` and `last_write_head` still point
        # at pre-reset ring-buffer slots, which causes a brief window of
        # stale states to be observed and any FSM logic gated on
        # `current_frame` continuity to misbehave. Drop them.
        self.current_frame = -1
        self.last_write_head = self.shm_wrapper.frame_write_head()

        self.fsm_state = ControllerState.WAITING_FOR_START
        print("[FSM] → WAITING_FOR_START  (press 'r' to begin)")

    def _handle_waiting_for_start(self):
        # Waiting for textures to be loaded
        if not self.current_state.get("is_scene_ready", False):
            return
        # Game is stopped and blank
        if self._start:
            cmds = self.write_commands({
                "rotate_left": False,
                "rotate_right": False,
                "zoom_in": False,
                "zoom_out": False,
                "check": False,
                "reset": False,
                "toggle_blank": True,
                "toggle_stop_rendering": self.current_state.get("is_rendering_stopped", False), # resume rendering
                "animation_door": False,
                "animation_all_door": False,
                "animation_colored": False,
            })
            self.fsm_state = ControllerState.PLAYING
            self.log_frame(self.current_state, cmds)
            print(f"[FSM] R pressed → PLAYING (level {self.current_level_index} chain {self.active_chain} trial {self._trial_idx()})")
            return

    def _handle_playing(self):
        # Check at which state are we
        flat = self.flat_trial
        time_elapsed = self.current_state.get("elapsed_secs", 0.0)
        has_won = self.current_state.get("win_elapsed_secs", 0.0) != 0.0

        # Budget flags drive the in-play animation selection below
        in_win_budget = (
            time_elapsed <= flat.get("elapsed_time_to_win", 0.0)
            and self.nr_attempts <= flat.get("nr_attempts_to_win", 0)
        )
        in_stay_budget = (
            not in_win_budget
            and time_elapsed <= flat.get("elapsed_time_to_retroceed", 0.0)
            and self.nr_attempts <= flat.get("nr_attempts_to_retroceed", 0)
        )

        # Trial outcome: game's win signal is ground truth; budget decides ADVANCE vs STAY
        if has_won and in_win_budget:
            self.trial_proceeding = TrialProceeding.ADVANCE
        elif has_won:
            self.trial_proceeding = TrialProceeding.STAY
        else:
            self.trial_proceeding = TrialProceeding.RETROCEED

        if self.check_has_finished(self.current_state):
            print(f"[FSM] Check finished with outcome {self.trial_proceeding.name} → TRIAL_COMPLETE")
            self.log_frame(self.current_state, {**self.inputs, **self.triggers})
            self.fsm_state = ControllerState.TRIAL_COMPLETE
            return

        # Input based triggers
        if self.triggers["check"]:
            suggestion_threshold = flat.get("nr_attempts_suggestion", 0)
            retroceeds_threshold = flat.get("nr_attempts_to_retroceed", 0)
            cosine_current = self.current_state.get("current_alignment", 0.0)
            cosine_threshold = flat.get("cosine_alignment_threshold", 0.0)

            # Since animating stop rendering
            cmds =  {
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "toggle_blank": False,
                    "toggle_stop_rendering": not self.current_state.get("is_rendering_stopped", False),
                    "animation_door": True,
                    "animation_all_door": False, "animation_colored": False,
                }
            if (self.nr_attempts) == retroceeds_threshold and cosine_current < cosine_threshold:
                print(f"[PLAY] Attempt {self.nr_attempts} == {retroceeds_threshold} → retroceed")
                cmds = self.write_commands(cmds)
                self.fsm_state = ControllerState.WAITING_ANIMATION_START
                self.nr_attempts += 1
                print("[FSM] → WAITING_ANIMATION_START")
                self.log_frame(self.current_state, cmds)
                return
            # Suggestion available and can play: animate depending on cosine alignment
            if (self.nr_attempts < suggestion_threshold and cosine_current < cosine_threshold) or \
                 (in_stay_budget and cosine_current > cosine_threshold):
                colored_light = cosine_current > COLOR_SUGGESTION_COS_SIM
                cmds["animation_colored"] = colored_light
                cmds = self.write_commands(cmds)
            # Won: animate green light
            elif in_win_budget and cosine_current > cosine_threshold and self.nr_attempts < suggestion_threshold:
                cmds["animation_colored"] = True
                cmds = self.write_commands(cmds)
            # No suggestions available but can still play: animate all lights with red
            else:
                cmds["animation_all_door"] = True
                cmds = self.write_commands(cmds)

            self.nr_attempts += 1
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.log_frame(self.current_state, cmds)
            return

        cmds = self.write_commands()
        self.log_frame(self.current_state, cmds)

    def _handle_waiting_animation_start(self):
        if self.current_state.get("is_animating", False):
            print("[FSM] Animation started → WAITING_ANIMATION_END")
            self.fsm_state = ControllerState.WAITING_ANIMATION_END

        self.log_frame(self.current_state, self.shm_wrapper.read_commands())

    def _handle_waiting_animation_end(self):
        if not self.current_state.get("is_animating", True):
            print("[FSM] Animation finished → issuing toggle_stop_rendering (resume)")
            self.reset_commands()
            self.write_commands({
                "rotate_left": False, "rotate_right": False,
                "zoom_in": False, "zoom_out": False,
                "check": False, "reset": False, "toggle_blank": False,
                "toggle_stop_rendering": self.current_state.get("is_rendering_stopped", False), "animation_door": False,
                "animation_all_door": False, "animation_colored": False,
            })
            self.log_frame(self.current_state, self.shm_wrapper.read_commands())
            self.fsm_state = ControllerState.PLAYING
            print("[FSM] → PLAYING")
            return

        # Resume rendering
        cmds = self.write_commands({
            "rotate_left": False, "rotate_right": False,
            "zoom_in": False, "zoom_out": False,
            "check": False, "reset": False, "toggle_blank": False,
            "toggle_stop_rendering": False, "animation_door": False,
            "animation_all_door": False, "animation_colored": False,
        })

        self.log_frame(self.current_state, cmds)

    def _handle_trial_complete(self):
        flat = self.flat_trial
        elapsed = self.current_state.get("elapsed_secs", 0.0)
        nr_to_win = flat.get("nr_attempts_to_win", 999)
        nr_to_retro = flat.get("nr_attempts_to_retroceed", 999)
        time_to_win = flat.get("elapsed_time_to_win", 9999.0)
        time_to_retro = flat.get("elapsed_time_to_retroceed", 9999.0)

        print(f"[EVAL] attempts={self.nr_attempts} elapsed={elapsed:.1f}s | "
              f"win<={nr_to_win}/{time_to_win}s  retro>={nr_to_retro}/{time_to_retro}s")

        outcome = {
            TrialProceeding.ADVANCE: "advance",
            TrialProceeding.STAY: "stay",
            TrialProceeding.RETROCEED: "retroceed",
        }[self.trial_proceeding]
        print(f"[EVAL] {outcome.upper()} → level {self.current_level_index} chain {self.active_chain} trial {self._trial_idx()}")

        self.save_trial_log(outcome)
        self._handle_trial_index_update()
        self._resync_with_game()

    def _handle_trial_index_update(self):
        """Advance/stay/retroceed within the active chain, then maybe switch chain."""
        n = len(self.level["trials"])
        idx = self._trial_idx()

        if self.trial_proceeding == TrialProceeding.ADVANCE:
            new_idx = idx + 1
        elif self.trial_proceeding == TrialProceeding.RETROCEED:
            new_idx = max(0, idx - 1)
        else:  # STAY
            new_idx = idx

        # Update trial counter
        self.trial_run_counter += 1

        self._set_trial_idx(new_idx)

        # Advance to next level if all chains exhausted
        if self._level_complete():
            self._finalize_level_run("completed")
            self.current_level_index = (self.current_level_index + 1) % self.total_levels
            start = self.level["fixed"].get("start_trial", 0)
            self.chain_idxs = [start] * len(self.level["objects"])
            self.active_chain = 0
            print(f"[LEVEL] Level complete → level {self.current_level_index}")
            return

        # If current chain is done, force switch to a non-exhausted one
        if self._trial_idx() >= n:
            candidates = [i for i in range(len(self.level["objects"])) if self.chain_idxs[i] < n]
            if candidates:
                self.active_chain = candidates[0]
            print(f"[CHAIN] Chain exhausted, switching to chain {self.active_chain}")
        else:
            self._maybe_switch_chain()

    def _resync_with_game(self):
        self.current_frame = -1
        self.last_write_head = self.shm_wrapper.frame_write_head()
        self.shm_wrapper.resync_seq()  # Reset seq counter to current ack (handles game restart)
        self.game_time_unresponsive = 0.0
        self.fsm_state = ControllerState.INIT

    def on_key_press(self, key):
        if key == keyboard.Key.left:
            self.inputs["rotate_left"] = True
        if key == keyboard.Key.right:
            self.inputs["rotate_right"] = True
        if key == keyboard.Key.up:
            self.inputs["zoom_in"] = True
        if key == keyboard.Key.down:
            self.inputs["zoom_out"] = True

        if key == keyboard.Key.space and key not in self.pressed_keys:
            self.triggers["check"] = True

        if hasattr(key, 'char') and key.char == "r" and key not in self.pressed_keys:
            self._start = True

        if hasattr(key, 'char') and key.char == "q" and key not in self.pressed_keys:
            self._running = False

        self.pressed_keys.add(key)

    def on_key_release(self, key):
        self.pressed_keys.discard(key)

        if key == keyboard.Key.left:
            self.inputs["rotate_left"] = False
        if key == keyboard.Key.right:
            self.inputs["rotate_right"] = False
        if key == keyboard.Key.up:
            self.inputs["zoom_in"] = False
        if key == keyboard.Key.down:
            self.inputs["zoom_out"] = False
        if key == keyboard.Key.space:
            self.triggers["check"] = False
        if hasattr(key, 'char') and key.char == "r":
            self._start = False


if __name__ == "__main__":
    app = MonkeyGameController()
    app.loop()
