import math
import sys
import time
import json
import os
import random
import datetime
from pynput import keyboard
from enum import Enum, auto

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build the shared library with 'cargo build --release -p shared --features python' and copy the resulting '.so' to controller_python/monkey_shared.so.")
    sys.exit(1)

# Constants imported from shared/src/constants.rs via monkey_shared
REFRESH_RATE_HZ = monkey_shared.REFRESH_RATE_HZ
WIN_BLANK_DURATION_FRAMES = monkey_shared.WIN_BLANK_DURATION_FRAMES
POLLING_RATE_TIME_S = 1  # time of polling in between of game controller
GAME_UNRESPONSIVENESS_THRESHOLD_S = 3.0  # time threshold to consider game unresponsive to restart trial
COLOR_SUGGESTION_COS_SIM = math.cos(math.pi / 6)  # cosine threshold for suggesting a colored light hint

# Six evenly-spaced start orientations (one per door of the hexagonal base)
START_ORIENTS = [k * 2.0 * math.pi / 6.0 for k in range(6)]

# Controller-only metadata fields (not written to game shared memory)
CONTROLLER_META_FIELDS = {
    "nr_attempts_to_win",
    "nr_attempts_suggestion",
    "nr_attempts_to_retroceed",
    "elapsed_time_to_win",
    "elapsed_time_to_retroceed",
}

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
    "decorations_color": [[float]],
    "cosine_alignment_threshold": float,
    "door_anim_fade_out": float,
    "door_anim_stay_open": float,
    "door_anim_fade_in": float,
    "main_spotlight_intensity": float,
    "ambient_brightness": float,
    "max_spotlight_intensity": float,
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
    # Fixed fields (base_radius, height, start_orient, lighting, animation)
    for k, v in fixed.items():
        if k != "pr_switching_chain":
            flat[k] = v
    # Controller meta fields
    for k, v in trial_cfg.items():
        flat[k] = v
    return flat


def load_levels(trials_path="trials_config/trials.jsonl"):
    """Load levels from new JSONL format. Each line is a level with objects/trials/fixed."""
    levels = []
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    trial_file = os.path.join(parent_dir, trials_path)

    try:
        with open(trial_file, "r") as f:
            for line_num, line in enumerate(f, 1):
                line = line.strip()
                if not line:
                    continue
                level = json.loads(line)
                # Validate structure
                if "objects" not in level or "trials" not in level or "fixed" not in level:
                    print(f"Warning: line {line_num} missing objects/trials/fixed, skipping")
                    continue
                if len(level["objects"]) < 2:
                    print(f"Warning: line {line_num} needs at least 2 objects, skipping")
                    continue
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
    LOADING_TRIAL = auto()
    PLAYING = auto()
    WAITING_ANIMATION_START = auto()
    WAITING_ANIMATION_END = auto()
    TRIAL_COMPLETE = auto()


class TrialProceeding(Enum):
    ADVANCE = auto()
    STAY = auto()
    RETROCEED = auto()


class MonkeyGameController:
    def __init__(self):
        self.pressed_keys = set()

        # Shared-memory handle
        try:
            self.shm_wrapper = monkey_shared.SharedMemoryWrapper("monkey_game")
            print("Connected to shared memory interface.")
        except Exception as exc:
            print(f"SHM Connection Error: {exc}")
            sys.exit(1)

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
            "blank_screen": False,
            "stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        }

        # Level configuration
        self.levels = load_levels()
        self.total_levels = len(self.levels)

        # Current level state
        self.current_level_index = 0
        self.chain_a_idx = 0   # position in trials list for chain A (object[0])
        self.chain_b_idx = 0   # position in trials list for chain B (object[1])
        self.active_chain = 0  # 0 = chain A, 1 = chain B

        # Frame tracking
        self.current_frame = -1

        # FSM
        self.fsm_state = ControllerState.INIT
        self.trial_proceeding = TrialProceeding.ADVANCE

        # Special commands
        self._start = False
        self._time_win_expired = False
        self._time_retroceed_expired = False

        # Per-trial tracking
        self.nr_attempts = 0
        self.trial_start_time = 0.0
        self.game_time_unresponsive = 0.0
        self.trial_start_state = None
        self.frame_log = {}
        self.trial_run_counter = 0

        # Output directory for logs
        script_dir = os.path.dirname(os.path.abspath(__file__))
        parent_dir = os.path.dirname(script_dir)
        self.log_dir = os.path.join(parent_dir, "out", "trial_logs")
        os.makedirs(self.log_dir, exist_ok=True)

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
        trial_idx = self.chain_a_idx if self.active_chain == 0 else self.chain_b_idx
        trial_idx = min(trial_idx, len(self.level["trials"]) - 1)
        trial_cfg = self.level["trials"][trial_idx]
        return expand_flat_trial(obj, trial_cfg, self.level["fixed"])

    def _trial_idx(self):
        return self.chain_a_idx if self.active_chain == 0 else self.chain_b_idx

    def _set_trial_idx(self, val):
        if self.active_chain == 0:
            self.chain_a_idx = val
        else:
            self.chain_b_idx = val

    def _level_complete(self):
        n = len(self.level["trials"])
        return self.chain_a_idx >= n and self.chain_b_idx >= n

    def _maybe_switch_chain(self):
        pr = self.level["fixed"].get("pr_switching_chain", 0.5)
        # Only switch to the other chain if it still has trials remaining
        other = 1 - self.active_chain
        other_idx = self.chain_b_idx if self.active_chain == 0 else self.chain_a_idx
        if other_idx < len(self.level["trials"]) and random.random() < pr:
            self.active_chain = other

    def game_state_fields(self, flat):
        """Return only the game-state keys (no controller meta)."""
        return {k: v for k, v in flat.items() if k not in CONTROLLER_META_FIELDS}

    def write_config_on_state(self, flat, state):
        """Overlay flat trial game-state config onto a base state dict."""
        for key, value in flat.items():
            if key not in CONTROLLER_META_FIELDS:
                state[key] = value
        return state

    # Progress bar: sum of trial indices across all objects in the current level.
    # Size = trials_per_level × number_of_objects. Resets to 0 on level change.
    def _progress_bar_cur(self):
        return self.chain_a_idx + self.chain_b_idx

    def _progress_bar_size(self):
        return len(self.level["trials"]) * len(self.level["objects"])

    # Command helpers
    def check_has_finished(self, state):
        trial = self.flat_trial
        nr_attempts = state.get("nr_attempts", 0)
        nr_attempts_to_retroceed = trial.get("nr_attempts_to_retroceed", 0)
        time_elapsed = state.get("elapsed_secs", 0.0)
        elapsed_time_to_retroceed = trial.get("elapsed_time_to_retroceed", 0.0)
        return (
            state.get("win_elapsed_secs", 0.0) != 0.0
            or nr_attempts >= nr_attempts_to_retroceed
            or time_elapsed >= elapsed_time_to_retroceed
        )

    def reset_commands(self):
        self.inputs = {k: False for k in self.inputs}
        self.triggers = {k: False for k in self.triggers}

    def reset_triggers(self):
        self.triggers = {k: False for k in self.triggers}

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
            "blank_screen": False,
            "stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        }
        self.shm_wrapper.write_commands(**cmds)
        return cmds

    def write_game_state(self, state):
        self.shm_wrapper.write_game_state(**state)

    LOGGED_STATE_FIELDS = {
        "frame_number",
        "elapsed_secs",
        "camera_radius",
        "camera_position",
        "nr_attempts",
        "cosine_alignment",
        "current_angle",
        "is_animating",
        "win_elapsed_secs",
    }

    def log_frame(self, state_read, commands_sent):
        filtered_state = {k: v for k, v in state_read.items() if k in self.LOGGED_STATE_FIELDS}
        entry = {"state_read": filtered_state, "commands_sent": commands_sent}
        self.frame_log[str(self.current_frame)] = entry

    def save_trial_log(self, outcome):
        elapsed = time.time() - self.trial_start_time
        log = {
            "level_index": self.current_level_index,
            "active_chain": self.active_chain,
            "trial_index_in_chain": self._trial_idx(),
            "trial_config": self.flat_trial,
            "outcome": outcome,
            "nr_attempts": self.nr_attempts,
            "elapsed_time": round(elapsed, 4),
            "timestamp_start": datetime.datetime.fromtimestamp(self.trial_start_time).isoformat(),
            "timestamp_end": datetime.datetime.now().isoformat(),
            "frames": self.frame_log,
        }
        filename = f"trial_{self.current_level_index:03d}_run_{self.trial_run_counter:04d}.json"
        filepath = os.path.join(self.log_dir, filename)
        try:
            with open(filepath, "w") as f:
                json.dump(log, f, indent=2, default=str)
            print(f"[LOG] Saved trial log → {filepath}")
        except Exception as e:
            print(f"[LOG] Failed to save log: {e}")

    # Main loop
    def loop(self):
        print("[FSM] Controller loop started")
        while self._running:
            state = self.shm_wrapper.read_game_state()
            current_frame = state.get("frame_number", 0)

            if self.current_frame == -1:
                self.current_frame = current_frame
                print(f"[FSM] Starting at frame {self.current_frame}")
                continue

            if current_frame == self.current_frame:
                self.game_time_unresponsive += POLLING_RATE_TIME_S / 1000.0
                time.sleep(POLLING_RATE_TIME_S / 1000.0)
                if self.game_time_unresponsive >= GAME_UNRESPONSIVENESS_THRESHOLD_S or self.current_frame == 0:
                    print(f"[FSM] Game unresponsive for {self.game_time_unresponsive:.1f}s, resyncing...")
                    self._resync_with_game()
                continue

            self.current_frame = current_frame

            state["progress_bar_cur_size"] = self._progress_bar_cur()
            state["progress_bar_size"] = self._progress_bar_size()

            if self.fsm_state == ControllerState.INIT:
                self._handle_init()
            elif self.fsm_state == ControllerState.WAITING_FOR_START:
                self._handle_waiting_for_start(state)
            elif self.fsm_state == ControllerState.PLAYING:
                self._handle_playing(state)
            elif self.fsm_state == ControllerState.WAITING_ANIMATION_START:
                self._handle_waiting_animation_start(state)
            elif self.fsm_state == ControllerState.WAITING_ANIMATION_END:
                self._handle_waiting_animation_end(state)
            elif self.fsm_state == ControllerState.TRIAL_COMPLETE:
                self._handle_trial_complete(state)

            time.sleep(POLLING_RATE_TIME_S / 1000.0)
            self.game_time_unresponsive = 0.0

        self.listener.stop()
        print("[FSM] Controller stopped.")

    # FSM handlers (command logic unchanged)
    def _handle_init(self):
        print("[FSM] INIT → issuing blank_screen + stop_rendering")
        flat = self.flat_trial
        print(f"[FSM] Level {self.current_level_index} chain {self.active_chain} "
              f"trial {self._trial_idx()}: {self.game_state_fields(flat)}")

        default_state = self.shm_wrapper.read_default_game_state()
        trial_state = self.write_config_on_state(flat, default_state)
        # Sample start orientation randomly from the 6 evenly-spaced door angles
        trial_state["start_orient"] = random.choice(START_ORIENTS)
        trial_state["progress_bar_cur_size"] = self._progress_bar_cur()
        trial_state["progress_bar_size"] = self._progress_bar_size()

        state_old = self.shm_wrapper.read_game_state()

        self.write_game_state(trial_state)
        self.trial_start_state = trial_state

        print(f"state old {state_old.get('is_blank', False)} and stop {state_old.get('is_rendering_stopped', False)}")
        self.write_commands({
            "rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": True,
            "blank_screen": not state_old.get("is_blank", False),
            "stop_rendering": not state_old.get("is_rendering_stopped", False),
            "animation_door": False,
            "animation_all_door": False,
            "animation_colored": False,
        })

        self.nr_attempts = 0
        self.trial_start_time = time.time()
        self.frame_log = {}
        self.trial_run_counter += 1
        self._time_win_expired = False
        self._time_retroceed_expired = False

        self.fsm_state = ControllerState.WAITING_FOR_START
        self.reset_commands()
        print("[FSM] → WAITING_FOR_START  (press 'r' to begin)")

    def _handle_waiting_for_start(self, state):
        if not state.get("is_scene_ready", False):
            print("[FSM] Waiting for scene to be ready...")
            self.write_no_commands()
            return
        if self._start:
            cmds = self.write_commands({
                "rotate_left": False,
                "rotate_right": False,
                "zoom_in": False,
                "zoom_out": False,
                "check": False,
                "reset": True,
                "blank_screen": True,
                "stop_rendering": True,
                "animation_door": False,
                "animation_all_door": False,
                "animation_colored": False,
            })
            self.fsm_state = ControllerState.PLAYING
            self.log_frame(state, cmds)
            print(f"[FSM] R pressed → PLAYING (level {self.current_level_index} chain {self.active_chain} trial {self._trial_idx()})")
            return
        self.write_no_commands()

    def _handle_playing(self, state):
        flat = self.flat_trial
        time_elapsed = state.get("elapsed_secs", 0.0)

        is_win = (
            time_elapsed < flat.get("elapsed_time_to_win", 0.0)
            and self.nr_attempts < flat.get("nr_attempts_to_win", 0)
        )
        is_stay = (
            not is_win
            and time_elapsed < flat.get("elapsed_time_to_retroceed", 0.0)
            and self.nr_attempts < flat.get("nr_attempts_to_retroceed", 0)
        )

        if is_win:
            self.trial_proceeding = TrialProceeding.ADVANCE
        elif is_stay:
            self.trial_proceeding = TrialProceeding.STAY
        else:
            self.trial_proceeding = TrialProceeding.RETROCEED

        # Time based event
        # One-time: time-to-win exceeded → animate all lights white
        if time_elapsed > flat.get("elapsed_time_to_win", 0.0) and not self._time_win_expired:
            """
            print(f"[TIME] Time to win exceeded ({time_elapsed:.1f}s), triggering animation")
            self._time_win_expired = True
            cmds = self.write_commands({
                "rotate_left": False, "rotate_right": False,
                "zoom_in": False, "zoom_out": False,
                "check": True, "reset": False, "blank_screen": False,
                "stop_rendering": True, "animation_door": True,
                "animation_all_door": True, "animation_colored": False,
            })
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return
            """

        # One-time: time-to-retroceed exceeded -> animate correct light in red
        if time_elapsed > flat.get("elapsed_time_to_retroceed", 0.0) and not self._time_retroceed_expired:
            print(f"[TIME] Time to retroceed exceeded ({time_elapsed:.1f}s), triggering animation")
            self._time_retroceed_expired = True
            cmds = self.write_commands({
                "rotate_left": False, "rotate_right": False,
                "zoom_in": False, "zoom_out": False,
                "check": True, "reset": False, "blank_screen": False,
                "stop_rendering": True, "animation_door": True,
                "animation_all_door": False, "animation_colored": True,
            })
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return

        if self.check_has_finished(state):
            print(f"[FSM] Check finished with outcome {self.trial_proceeding.name} → TRIAL_COMPLETE")
            self.log_frame(state, {**self.inputs, **self.triggers})
            self.fsm_state = ControllerState.TRIAL_COMPLETE
            return

        # Input based triggers
        if self.triggers["check"]:
            suggestion_threshold = flat.get("nr_attempts_suggestion", 0)
            retroceeds_threshold = flat.get("nr_attempts_to_retroceed", 0)
            cosine_current = state.get("cosine_alignment", 0.0)
            cosine_threshold = flat.get("cosine_alignment_threshold", 0.0)

            # Exceeded number of attempt: animate red light and retroceed
            if (self.nr_attempts + 1) == retroceeds_threshold and cosine_current < cosine_threshold:
                print(f"[PLAY] Attempt {self.nr_attempts} == {retroceeds_threshold} → retroceed")
                cmds = self.write_commands({
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "blank_screen": False,
                    "stop_rendering": True, "animation_door": True,
                    "animation_all_door": False, "animation_colored": False,
                })
                self.fsm_state = ControllerState.WAITING_ANIMATION_START
                self.nr_attempts += 1
                print("[FSM] → WAITING_ANIMATION_START")
                self.log_frame(state, cmds)
                return
            # Suggestion available and can play: animate depending on cosine alignment
            elif (self.nr_attempts < suggestion_threshold and cosine_current < cosine_threshold) or \
                 (self.trial_proceeding == TrialProceeding.STAY and cosine_current > cosine_threshold):
                colored_light = cosine_current > COLOR_SUGGESTION_COS_SIM
                cmds = self.write_commands({
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "blank_screen": False,
                    "stop_rendering": True, "animation_door": True,
                    "animation_all_door": False, "animation_colored": colored_light,
                })
            # Won: animate green light
            elif is_win and cosine_current > cosine_threshold and self.nr_attempts < suggestion_threshold:
                cmds = self.write_commands({
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "blank_screen": False,
                    "stop_rendering": True, "animation_door": True,
                    "animation_all_door": False, "animation_colored": True,
                })
            # No suggestions available but can still play: animate all lights with red
            else:
                cmds = self.write_commands({
                    "rotate_left": False, "rotate_right": False,
                    "zoom_in": False, "zoom_out": False,
                    "check": True, "reset": False, "blank_screen": False,
                    "stop_rendering": True, "animation_door": True,
                    "animation_all_door": True, "animation_colored": False,
                })

            self.nr_attempts += 1
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return

        cmds = self.write_commands()
        self.log_frame(state, cmds)

    def _handle_waiting_animation_start(self, state):
        if state.get("is_animating", False):
            print("[FSM] Animation started → WAITING_ANIMATION_END")
            self.fsm_state = ControllerState.WAITING_ANIMATION_END

        cmds = self.write_commands({
            "rotate_left": False, "rotate_right": False,
            "zoom_in": False, "zoom_out": False,
            "check": False, "reset": False, "blank_screen": False,
            "stop_rendering": False, "animation_door": False,
            "animation_all_door": False, "animation_colored": False,
        })

        if self.check_has_finished(state):
            self._handle_trial_index_update()

        self.write_game_state(state)
        self.log_frame(state, cmds)

    def _handle_waiting_animation_end(self, state):
        if not state.get("is_animating", True):
            print("[FSM] Animation finished → issuing stop_rendering (resume)")
            self.reset_commands()
            cmds = self.write_commands({
                "rotate_left": False, "rotate_right": False,
                "zoom_in": False, "zoom_out": False,
                "check": False, "reset": False, "blank_screen": False,
                "stop_rendering": True, "animation_door": False,
                "animation_all_door": False, "animation_colored": False,
            })
            self.write_game_state(state)
            self.log_frame(state, cmds)
            self.fsm_state = ControllerState.PLAYING
            print("[FSM] → PLAYING")
            return

        cmds = self.write_commands({
            "rotate_left": False, "rotate_right": False,
            "zoom_in": False, "zoom_out": False,
            "check": False, "reset": False, "blank_screen": False,
            "stop_rendering": False, "animation_door": False,
            "animation_all_door": False, "animation_colored": False,
        })
        self.write_game_state(state)
        self.log_frame(state, cmds)

    def _handle_trial_complete(self, state):
        flat = self.flat_trial
        elapsed = state.get("elapsed_secs", 0.0)
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

        self._set_trial_idx(new_idx)

        # Advance to next level if both chains exhausted
        if self._level_complete():
            self.current_level_index = (self.current_level_index + 1) % self.total_levels
            self.chain_a_idx = 0
            self.chain_b_idx = 0
            self.active_chain = 0
            print(f"[LEVEL] Level complete → level {self.current_level_index}")
            return

        # If current chain is done, force switch to the other
        if self._trial_idx() >= n:
            self.active_chain = 1 - self.active_chain
            print(f"[CHAIN] Chain exhausted, switching to chain {self.active_chain}")
        else:
            self._maybe_switch_chain()

    def _resync_with_game(self):
        self.current_frame = -1
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
