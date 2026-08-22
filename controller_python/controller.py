import atexit
import datetime
import json
import math
import os
import random
import re
import signal
import subprocess
import sys
import tempfile
import threading
import time
from enum import Enum, auto

# evdev (Wayland-safe). pynput was X11-only and broke on Wayland sessions
# because there's no Wayland API for global key grabs (security by design).
# evdev reads /dev/input/event* directly via the kernel, so it works on X11,
# Wayland, and bare TTYs identically.
#
# Requires:
#   - pip install evdev
#   - user in the 'input' group:  sudo usermod -aG input $USER  (log out/in)
# Optional:
#   - MONKEY_KEYBOARD_PATH=/dev/input/eventN to pin a specific keyboard
try:
    from evdev import InputDevice, ecodes, list_devices
except ImportError:
    print("Error: 'evdev' module not found.")
    print("Install with:  pip install evdev")
    print("Then add yourself to the 'input' group:  sudo usermod -aG input $USER  (then log out/in)")
    sys.exit(1)


def _query_display_refresh_rate_hz():
    """Ask the OS for the active display's refresh rate (Hz).

    Shells out to ``xrandr --current`` and parses the line containing the
    asterisk that marks the currently-active mode. Works on X11 and XWayland
    (the latter covers most Wayland sessions). Returns ``None`` when xrandr
    is unavailable or its output doesn't parse — the caller should treat
    that as "unknown" rather than substituting a hardcoded value.
    """
    try:
        out = subprocess.check_output(
            ["xrandr", "--current"],
            text=True,
            stderr=subprocess.DEVNULL,
            timeout=2.0,
        )
    except (FileNotFoundError, subprocess.SubprocessError, OSError):
        return None
    # Active-mode line looks like:
    #   1920x1080     60.00*+  59.94    50.00
    # The token bearing '*' is the current refresh rate in Hz.
    for line in out.splitlines():
        if "*" not in line:
            continue
        for tok in line.split():
            if "*" in tok:
                try:
                    return float(tok.rstrip("*+"))
                except ValueError:
                    continue
    return None


def _find_keyboard_device():
    """Find a /dev/input/event* device that looks like a keyboard.

    Identifies a 'keyboard' as any evdev device exposing both KEY_SPACE and
    KEY_LEFT. Returns the first match. Override via the
    ``MONKEY_KEYBOARD_PATH`` env var to pin a specific device (e.g. on a
    rig with both a laptop keyboard and an external USB one).
    """
    forced = os.environ.get("MONKEY_KEYBOARD_PATH")
    if forced:
        return InputDevice(forced)
    for path in list_devices():
        try:
            dev = InputDevice(path)
        except (PermissionError, FileNotFoundError, OSError):
            continue
        keys = dev.capabilities().get(ecodes.EV_KEY, [])
        if ecodes.KEY_SPACE in keys and ecodes.KEY_LEFT in keys:
            return dev
    return None

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
SHM_NAME = monkey_shared.SHM_NAME
POLLING_RATE_S = monkey_shared.POLLING_RATE_S
GAME_UNRESPONSIVENESS_THRESHOLD_S = monkey_shared.GAME_UNRESPONSIVENESS_THRESHOLD_S
COLOR_SUGGESTION_COS_SIM = monkey_shared.COLOR_SUGGESTION_COS_SIM
DEFAULT_CAMERA_Y = monkey_shared.DEFAULT_CAMERA_Y
CAMERA_3D_INITIAL_RADIUS = monkey_shared.CAMERA_3D_INITIAL_RADIUS
N_START_ORIENTS = monkey_shared.N_START_ORIENTS
MAX_SESSION_DURATION_MIN = monkey_shared.MAX_SESSION_DURATION_MIN
MAX_SESSION_DURATION_S = MAX_SESSION_DURATION_MIN * 60
# Per-trial frame-log buffer sized to cover the whole session at up to 120 Hz.
MAX_TRIAL_FRAMES = MAX_SESSION_DURATION_S * 120

# Integer scale for the left progress bar (SHM carries u32, the mean is
# fractional). Mirrors LEVEL_PROGRESS_SCALE in controller_main.js.
LEVEL_PROGRESS_SCALE = 1000

# Frozen iteration order for hot loops — matches LOGGED_STATE_FIELDS as a list.
_LOGGED_STATE_FIELDS_LIST = list(monkey_shared.LOGGED_STATE_FIELDS)

# Stable list of all command keys; mirrors CMD_DEFAULTS in controller_main.js.
_CMD_KEYS = (
    "rotate_left", "rotate_right", "zoom_in", "zoom_out",
    "check", "reset", "toggle_blank", "toggle_stop_rendering",
    "animation_door", "animation_all_door", "animation_colored", "shake",
)


def _empty_frame_row():
    """One preallocated row in the per-level scratch frame-log buffer."""
    return {
        "state_read": {k: None for k in _LOGGED_STATE_FIELDS_LIST},
        "commands_sent": {k: False for k in _CMD_KEYS},
    }

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
    "sound_effects_volume",
    "background_music_volume",
    "fog_enabled",
    "fog_thickness_base",
    "firefly_count",
    "firefly_size",
    "firefly_expand_secs",
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
        if k not in ("pr_switching_chain", "start_trial", "start_object", "remove_completed_chains", "random_seed", "show_progress_bar"):
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
    fixed.setdefault("start_object", -1)
    fixed.setdefault("remove_completed_chains", False)
    fixed.setdefault("random_seed", -1)
    fixed.setdefault("score_bar_max", 10)
    fixed.setdefault("shake_amplitude", 0.5)
    fixed.setdefault("shake_duration", 1.0)
    fixed.setdefault("sound_effects_volume", 1.0)
    fixed.setdefault("background_music_volume", 0.20)
    fixed.setdefault("fog_enabled", True)
    fixed.setdefault("fog_thickness_base", 25.0)
    fixed.setdefault("firefly_count", 10000)
    fixed.setdefault("firefly_size", 0.013)
    fixed.setdefault("firefly_expand_secs", 1.5)
    for obj in level.get("objects", []):
        obj.setdefault("decorations_rotation", [0, 0, 0])
    for trial in level.get("trials", []):
        trial.setdefault("show_all", False)


def load_levels(trials_path=None):
    """Load levels from new JSONL format. Each line is a level with objects/trials/fixed.

    Returns ``(levels, paradigm_basename)`` so callers can stamp the paradigm
    filename into ``session_info``.
    """
    levels = []
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    if trials_path is None:
        if len(sys.argv) > 1:
            trials_path = sys.argv[1]
        else:
            trials_path = "trials_config/trials/trials.jsonl"
    trial_file = (
        trials_path
        if os.path.isabs(trials_path)
        else os.path.join(parent_dir, trials_path)
    )
    paradigm_name = os.path.basename(trial_file)

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
    return levels, paradigm_name


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
# name must appear in monkey_shared.FSM_STAT<ES (which controller_main.js also reads).
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
            "shake": False,
        }

        # Session metadata, written once into every trial log.
        # `refresh_rate_hz` is queried from the OS at startup (xrandr) — not
        # hardcoded, not measured from observed frame deltas. The measured
        # value is still surfaced separately in `timing_health.refresh_rate_hz_measured`
        # so analyses can cross-check that the display actually delivered
        # what it claims (catches VRR / dropped-frame mismatches).
        # Level configuration
        self.levels, paradigm_name = load_levels()
        self.session_info = {
            "app_start_unix_ns": time.time_ns(),
            "platform": "native",
            "os": sys.platform,
            "refresh_rate_hz": _query_display_refresh_rate_hz(),
            "present_mode": "fifo",
            "paradigm": paradigm_name,
        }
        self.total_levels = len(self.levels)

        # Current level state
        self.current_level_index = 0
        self.chain_idxs = []
        self.active_chain = 0
        self.chain_bag = []
        self._rng = random.Random()
        self._level_random_seed = 0
        if self.levels:
            start = self.levels[0]["fixed"].get("start_trial", 0)
            self.chain_idxs = [start] * len(self.levels[0]["objects"])
            self._reseed_rng_for_level()
            self.active_chain = self._level_start_object(self.levels[0])
            self._refill_chain_bag(exclude=self.active_chain)

        # Consecutive-correct counter, session-wide. Nothing reads it yet: it
        # is the input for the particle density (capped, constant in
        # constants.rs) once that lands.
        self.correct_streak = 0

        # Progress values pushed ahead of the real chain-index update so the
        # game can animate them during the door animation. None = use the real
        # values. See _projected_progress.
        self.pending_progress = None

        # Frame tracking
        self.current_frame = -1
        self.last_write_head = 0

        # FSM
        self.fsm_state = ControllerState.INIT
        self.trial_proceeding = TrialProceeding.ADVANCE

        # Special commands
        self._start = False
        self._time_win_expired = False

        # Wall-clock anchor for the MAX_SESSION_DURATION_S cap. Set on the
        # first transition into PLAYING and never reset.
        self.session_start_time = None

        # Per-trial tracking
        self.nr_attempts = 0
        self.trial_start_time = 0.0
        self.game_time_unresponsive = 0.0
        self.trial_start_state = None
        self.trial_start_orient = None
        self._frame_zero = None
        self._render_frame_zero = None
        # Preallocated, reused across all trials of a level. log_frame mutates
        # the row at self.frame_log[logged_fn] in place. save_trial_log copies
        # frame_log[0:frame_log_len] into a compact retained dict at trial end.
        self.frame_log = [_empty_frame_row() for _ in range(MAX_TRIAL_FRAMES)]
        self.frame_log_len = 0
        self._frame_log_overflow_warned = False
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

        # Keyboard listener (evdev, daemon thread). Wayland-safe; works on
        # X11 and bare TTYs too. See top-of-file note.
        self._kbd_device = _find_keyboard_device()
        if self._kbd_device is None:
            print("Error: no keyboard device found under /dev/input/event*.")
            print("Check that:")
            print("  - You are in the 'input' group:  id | grep input")
            print("  - If not:  sudo usermod -aG input $USER  (then log out/in)")
            print("  - Or pin a device with:  MONKEY_KEYBOARD_PATH=/dev/input/eventN")
            sys.exit(1)
        print(f"Keyboard listener: {self._kbd_device.path}  ({self._kbd_device.name})")
        threading.Thread(target=self._input_loop, daemon=True).start()

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

    def _level_start_object(self, level=None):
        """Resolve the initial active_chain for a level.
        `start_object`: -1 → controller picks uniformly at random over
        chains; >= 0 → use that chain index (clamped)."""
        lv = level if level is not None else self.level
        n = len(lv["objects"])
        v = lv["fixed"].get("start_object", -1)
        if v < 0:
            return self._rng.randrange(n)
        return max(0, min(int(v), n - 1))

    def _level_complete(self):
        n = len(self.level["trials"])
        return all(idx >= n for idx in self.chain_idxs)

    def _reseed_rng_for_level(self):
        """Reseed `self._rng` from the active level's `random_seed` (saved in
        `self._level_random_seed` and logged). `-1` → draw a fresh u32 from
        system entropy so the run stays non-reproducible but the resolved
        value is still recorded for post-hoc replay."""
        cfg = self.level["fixed"].get("random_seed", -1)
        if cfg is None or int(cfg) < 0:
            seed = random.SystemRandom().getrandbits(32)
        else:
            seed = int(cfg) & 0xFFFFFFFF
        self._level_random_seed = seed
        self._rng.seed(seed)

    def _refill_chain_bag(self, exclude=None):
        """Shuffle a fresh bag of chain indices to draw from. Excludes chains
        already at terminal idx when `remove_completed_chains` is set. The
        `exclude` arg drops one specific index (used at level start to avoid
        the starter reappearing as the next pick of the same cycle)."""
        n = len(self.level["trials"])
        remove_done = bool(self.level["fixed"].get("remove_completed_chains", False))
        indices = [
            i for i in range(len(self.level["objects"]))
            if (not remove_done) or self.chain_idxs[i] < n
        ]
        if exclude is not None:
            indices = [i for i in indices if i != exclude]
        self._rng.shuffle(indices)
        self.chain_bag = indices

    def _draw_next_chain(self):
        """Pop the next chain from the shuffled bag. Refills when empty so
        each cycle visits every (eligible) chain once before any repeats."""
        if not self.chain_bag:
            self._refill_chain_bag()
        if not self.chain_bag:
            return
        self.active_chain = self.chain_bag.pop()

    def game_state_fields(self, flat):
        """Return only the game-state keys (no controller meta)."""
        return {k: v for k, v in flat.items() if k not in CONTROLLER_META_FIELDS}

    def write_config_on_state(self, flat, state):
        """Overlay flat trial game-state config onto a base state dict."""
        for key, value in flat.items():
            if key not in CONTROLLER_META_FIELDS:
                state[key] = value
        return state

    # ── Level chain (top-of-screen circles) ────────────────────────────────
    # One circle per level; the first `_level_chain_done()` of them are filled.
    # Levels are played in order and the session ends on the last one, so the
    # number completed is simply the current index.
    def _level_chain_done(self):
        return self.current_level_index

    def _level_chain_size(self):
        return len(self.levels)

    # ── Left bar: mean trial position across chains, 0 -> 1 over the level ──
    # Reaches exactly 1 when every chain is terminal, i.e. when the level is
    # complete. Sent as value/max over a fixed integer scale because the SHM
    # fields are u32.
    def _level_progress_frac(self):
        total = len(self.level["trials"]) * len(self.level["objects"])
        if total <= 0:
            return 0.0
        done = sum(max(0, idx) for idx in self.chain_idxs)
        return max(0.0, min(1.0, done / total))

    # The chain index only advances at TRIAL_COMPLETE, which happens *after*
    # the door animation has finished — too late for the game to animate the
    # change during it. So on the attempt that ends the trial we push the
    # progress the trial is about to land on, and keep pushing it until the
    # real update catches up (at which point both agree and it is cleared).
    def _projected_progress(self, delta):
        n = len(self.level["trials"])
        total = n * len(self.level["objects"])
        projected = [
            max(0, min(v + delta, n)) if i == self.active_chain else v
            for i, v in enumerate(self.chain_idxs)
        ]
        done = sum(max(0, v) for v in projected)
        level_done = all(v >= n for v in projected)
        return {
            "value": round(max(0.0, min(1.0, done / total)) * LEVEL_PROGRESS_SCALE) if total > 0 else 0,
            "chain_done": self.current_level_index + (1 if level_done else 0),
        }

    # `fixed.score_bar_max == 0` keeps its old meaning: hide the bar.
    def _level_progress_max(self):
        return 0 if int(self.level["fixed"].get("score_bar_max", 10)) == 0 else LEVEL_PROGRESS_SCALE

    # Fraction of the session still left (1 -> 0), for the game's round clock.
    # Full until the first trial starts and `session_start_time` is anchored.
    def _session_time_left(self):
        if self.session_start_time is None or MAX_SESSION_DURATION_S <= 0:
            return 1.0
        left = 1.0 - (time.time() - self.session_start_time) / MAX_SESSION_DURATION_S
        return max(0.0, min(1.0, left))

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

    _CMD_KEYS = (
        "rotate_left", "rotate_right", "zoom_in", "zoom_out",
        "check", "reset", "toggle_blank", "toggle_stop_rendering",
        "animation_door", "animation_all_door", "animation_colored", "shake",
    )

    def write_commands(self, commands=None):
        if commands is None:
            data_to_write = {**self.inputs, **self.triggers}
        else:
            data_to_write = commands
        # Backfill any missing key with False so PyO3 doesn't reject the call.
        for k in self._CMD_KEYS:
            data_to_write.setdefault(k, False)
        self.shm_wrapper.write_commands(**data_to_write)
        cmds_snapshot = dict(data_to_write)
        self.reset_triggers()
        return cmds_snapshot
    
    def write_no_commands(self):
        cmds = {k: False for k in self._CMD_KEYS}
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

        if logged_fn < 0 or logged_fn >= MAX_TRIAL_FRAMES:
            if not self._frame_log_overflow_warned:
                print(f"[LOG] frame_log overflow: logged_fn={logged_fn} exceeds MAX_TRIAL_FRAMES={MAX_TRIAL_FRAMES}; skipping")
                self._frame_log_overflow_warned = True
            return

        # Mutate the preallocated row in place — no allocations during the trial.
        row = self.frame_log[logged_fn]
        sr = row["state_read"]
        for k in _LOGGED_STATE_FIELDS_LIST:
            sr[k] = state_read.get(k)
        if "frame_number" in sr:
            sr["frame_number"] = logged_fn
        if "render_frame_number" in sr:
            sr["render_frame_number"] = logged_rfn

        cs = row["commands_sent"]
        if commands_sent:
            for k in _CMD_KEYS:
                cs[k] = bool(commands_sent.get(k, False))
        else:
            for k in _CMD_KEYS:
                cs[k] = False

        if logged_fn + 1 > self.frame_log_len:
            self.frame_log_len = logged_fn + 1

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
        # Sanity-check value: 1 / mean(Δpresent) from the frames we actually
        # logged. Useful for catching VRR / dropped-frame mismatches against
        # the OS-reported `session_info.refresh_rate_hz` — not authoritative.
        refresh_hz_measured = (1.0 / mean) if mean > 0 else None
        return {
            "present_dt_mean_ms": round(mean * 1000, 3),
            "present_dt_std_ms":  round(std  * 1000, 3),
            "refresh_rate_hz_measured": round(refresh_hz_measured, 3) if refresh_hz_measured is not None else None,
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
        # Iterate the preallocated scratch buffer directly (rows are already
        # ordered by logged_fn = slot index).
        elapsed_time_anim = 0.0
        elapsed_time_no_anim = 0.0
        for i in range(1, self.frame_log_len):
            prev_sr = self.frame_log[i - 1]["state_read"]
            cur_sr = self.frame_log[i]["state_read"]
            p0 = prev_sr.get("present_elapsed_secs")
            p1 = cur_sr.get("present_elapsed_secs")
            if p0 is None or p1 is None:
                continue
            dt = float(p1) - float(p0)
            if dt <= 0:
                continue
            if prev_sr.get("is_animating"):
                elapsed_time_anim += dt
            else:
                elapsed_time_no_anim += dt

        # Build compact retained frames dict sized to actual frames used.
        # Deep-copy each row so the scratch buffer can be reused by the next
        # trial without aliasing this trial's saved log.
        frames_compact = {}
        frames_for_health = []
        for i in range(self.frame_log_len):
            r = self.frame_log[i]
            frames_compact[str(i)] = {
                "state_read": dict(r["state_read"]),
                "commands_sent": dict(r["commands_sent"]),
            }
            frames_for_health.append({
                "present_elapsed_secs": r["state_read"].get("present_elapsed_secs"),
                "render_frame_number":  r["state_read"].get("render_frame_number"),
            })

        log = {
            "level_index": self.current_level_index,
            "active_chain": self.active_chain,
            "trial_index_in_chain": self._trial_idx(),
            "trial_run_counter": self.trial_run_counter,
            "trial_config": {k: v for k, v in self.flat_trial.items() if k != "start_orient"},
            "start_orient": self.trial_start_orient,
            "level_random_seed": self._level_random_seed,
            "outcome": outcome,
            "nr_attempts": self.nr_attempts,
            "elapsed_time_no_anim": elapsed_time_no_anim,
            "elapsed_time_anim": elapsed_time_anim,
            "timestamp_start": trial_start_dt.isoformat(),
            "timestamp_end": end_dt.isoformat(),
            "session_info": self.session_info,
            "win_event": self.win_event,
            "frames": frames_compact,
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
            "level_random_seed": self._level_random_seed,
            "win_event": self.win_event,
            "file": trial_filename,
            "_frames_for_health": frames_for_health,
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
                    logged_fn = fn if self._frame_zero is None else fn - self._frame_zero
                    if logged_fn >= self.frame_log_len:
                        self.log_frame(s, None)

            # Use the latest state for FSM dispatch
            self.current_state = states[-1]
            
            
            self.current_frame = self.current_state.get("frame_number", 0)

            pending = self.pending_progress
            self.current_state["progress_bar_cur_size"] = (
                pending["chain_done"] if pending else self._level_chain_done())
            self.current_state["progress_bar_size"] = self._level_chain_size()
            self.current_state["score_bar_value"] = (
                pending["value"] if pending else round(self._level_progress_frac() * LEVEL_PROGRESS_SCALE))
            self.current_state["score_bar_max"] = self._level_progress_max()
            self.current_state["session_time_left"] = self._session_time_left()
            self.current_state["shake_amplitude"] = float(self.level["fixed"].get("shake_amplitude", 0.5))
            self.current_state["shake_duration"] = float(self.level["fixed"].get("shake_duration", 1.0))
            
            # Clear commands
            self.write_no_commands()

            # Session-duration cap: finalize current level run and stop.
            if (self.session_start_time is not None
                    and time.time() - self.session_start_time >= MAX_SESSION_DURATION_S):
                print(f"[SESSION] Reached {MAX_SESSION_DURATION_MIN}-minute cap → stopping")
                self._finalize_level_run_safe("timeout")
                self._running = False
                break

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

        # Input thread is a daemon — it dies with the process. The read_loop
        # call inside it is blocking; closing the device unblocks it.
        try:
            self._kbd_device.close()
        except Exception:
            pass
        print("[FSM] Controller stopped.")

    # FSM handlers (command logic unchanged)
    def _handle_init(self):
        # Wait for the game's startup countdown to finish before issuing reset.
        # Otherwise the end-of-countdown despawn-all (check_scene_ready) wipes
        # the trial pyramid we just spawned, leaving an empty scene on trial 0.
        if not self.current_state.get("is_scene_ready", False):
            return
        print("[FSM] INIT → issuing blank_screen + stop_rendering + load trial")
        flat = self.flat_trial
        print(f"[FSM] Level {self.current_level_index} chain {self.active_chain} "
              f"trial {self._trial_idx()}: {self.game_state_fields(flat)}")

        default_state = self.shm_wrapper.read_default_game_state()
        trial_state = self.write_config_on_state(flat, default_state)
        # Sample start orientation randomly from the 6 evenly-spaced door angles.
        # Editor stores a sentinel (-1) for this field; the real value is chosen
        # here and recorded into the trial log so analyses can recover it.
        trial_state["start_orient"] = self._rng.choice(START_ORIENTS)
        self.trial_start_orient = float(trial_state["start_orient"])
        trial_state["progress_bar_cur_size"] = self._level_chain_done()
        trial_state["progress_bar_size"] = self._level_chain_size()
        trial_state["score_bar_value"] = round(self._level_progress_frac() * LEVEL_PROGRESS_SCALE)
        trial_state["score_bar_max"] = self._level_progress_max()
        trial_state["session_time_left"] = self._session_time_left()

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
        # frame_log buffer reused across trials — rewind cursor only. Old row
        # contents past frame_log_len are never read.
        self.frame_log_len = 0
        self._frame_log_overflow_warned = False
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
            if self.session_start_time is None:
                self.session_start_time = time.time()
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

            show_all = bool(flat.get("show_all", False))

            correct = cosine_current > cosine_threshold

            # Is this the attempt that ends the trial? A correct alignment
            # always is (the game sets win_elapsed_secs); a wrong one is when
            # it exhausts the retroceed budget. `in_win_budget` is evaluated on
            # paused game time, so it reads the same now as after the animation.
            if correct or self.nr_attempts == retroceeds_threshold:
                self.pending_progress = self._projected_progress(
                    (1 if in_win_budget else 0) if correct else -1)

            if correct:
                self.correct_streak += 1
                shake = False
            else:
                self.correct_streak = max(0, self.correct_streak - 1)
                shake = True

            # Since animating stop rendering
            cmds =  {
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "toggle_blank": False,
                    "toggle_stop_rendering": not self.current_state.get("is_rendering_stopped", False),
                    "animation_door": True,
                    "animation_all_door": False, "animation_colored": False,
                    "shake": shake,
                }
            if (self.nr_attempts) == retroceeds_threshold and cosine_current < cosine_threshold:
                print(f"[PLAY] Attempt {self.nr_attempts} == {retroceeds_threshold} → retroceed")
                # show_all flips the retroceed light from single-door red to
                # all-doors red. In the existing game color logic, all-doors-red
                # corresponds to (colored=True, all_door=True).
                if show_all:
                    cmds["animation_all_door"] = True
                    cmds["animation_colored"] = True
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
            elif in_win_budget and cosine_current > cosine_threshold:
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
            new_idx = min(idx + 1, n)
        elif self.trial_proceeding == TrialProceeding.RETROCEED:
            new_idx = max(0, idx - 1)
        else:  # STAY
            new_idx = idx

        # Update trial counter
        self.trial_run_counter += 1

        self._set_trial_idx(new_idx)
        self.pending_progress = None   # the real values now match the pre-pushed ones

        # Advance to next level if all chains exhausted
        if self._level_complete():
            self._finalize_level_run("completed")
            self.current_level_index = (self.current_level_index + 1) % self.total_levels
            start = self.level["fixed"].get("start_trial", 0)
            self.chain_idxs = [start] * len(self.level["objects"])
            self._reseed_rng_for_level()
            self.active_chain = self._level_start_object()
            self._refill_chain_bag(exclude=self.active_chain)
            print(f"[LEVEL] Level complete → level {self.current_level_index}")
            return

        self._draw_next_chain()

    def _resync_with_game(self):
        self.current_frame = -1
        self.last_write_head = self.shm_wrapper.frame_write_head()
        self.shm_wrapper.resync_seq()  # Reset seq counter to current ack (handles game restart)
        self.game_time_unresponsive = 0.0
        self.fsm_state = ControllerState.INIT

    def _input_loop(self):
        """Background thread: read evdev events, update inputs/triggers.

        Same semantics as the old pynput on_key_press/on_key_release:
        - arrow keys hold-state into self.inputs
        - SPACE / R / Q debounced one-shot via self.pressed_keys (codes)
        - autorepeat events (ev.value == 2) are ignored

        Wayland-safe — reads /dev/input directly.
        """
        KEY_TO_INPUT = {
            ecodes.KEY_LEFT:  "rotate_left",
            ecodes.KEY_RIGHT: "rotate_right",
            ecodes.KEY_UP:    "zoom_in",
            ecodes.KEY_DOWN:  "zoom_out",
        }
        try:
            for ev in self._kbd_device.read_loop():
                if not self._running:
                    return
                if ev.type != ecodes.EV_KEY:
                    continue
                is_press = ev.value == 1
                is_release = ev.value == 0
                # ev.value == 2 is autorepeat; we intentionally ignore it so
                # one-shot triggers don't re-fire while a key is held.

                if ev.code in KEY_TO_INPUT:
                    if is_press:
                        self.inputs[KEY_TO_INPUT[ev.code]] = True
                        self.pressed_keys.add(ev.code)
                    elif is_release:
                        self.inputs[KEY_TO_INPUT[ev.code]] = False
                        self.pressed_keys.discard(ev.code)
                    continue

                if ev.code == ecodes.KEY_SPACE:
                    if is_press and ev.code not in self.pressed_keys:
                        self.triggers["check"] = True
                        self.pressed_keys.add(ev.code)
                    elif is_release:
                        self.triggers["check"] = False
                        self.pressed_keys.discard(ev.code)
                    continue

                if ev.code == ecodes.KEY_R:
                    if is_press and ev.code not in self.pressed_keys:
                        self._start = True
                        self.pressed_keys.add(ev.code)
                    elif is_release:
                        self._start = False
                        self.pressed_keys.discard(ev.code)
                    continue

                if ev.code == ecodes.KEY_Q:
                    if is_press and ev.code not in self.pressed_keys:
                        self._running = False
                        self.pressed_keys.add(ev.code)
                    elif is_release:
                        self.pressed_keys.discard(ev.code)
                    continue
        except OSError as e:
            # Device closed (graceful shutdown) or disconnected.
            print(f"[INPUT] evdev loop exiting: {e}")


if __name__ == "__main__":
    app = MonkeyGameController()
    app.loop()
