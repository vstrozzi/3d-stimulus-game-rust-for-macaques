import sys
import time
import json
import os
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

# ── Controller-only metadata fields (not written to game shared memory) ──────
CONTROLLER_META_FIELDS = {
    "nr_attempts_to_win",
    "nr_attempts_suggestion",
    "nr_attempts_to_retroceed",
    "elapsed_time_to_win",
    "elapsed_time_to_retroceed",
}

# ── Game-state schema (fields written to shared memory) ──────────────────────
state_schema = {
    "decoration_seeds": [int],
    "base_radius": float,
    "height": float,
    "start_orient": float,
    "target_door": int,
    "colors": [[int]],
    "decorations_count": [int],
    "decorations_size": [float],
    "cosine_alignment_threshold": float,
    "door_anim_fade_out": int,
    "door_anim_stay_open": int,
    "door_anim_fade_in": int,
    "main_spotlight_intensity": float,
    "ambient_brightness": float,
    "max_spotlight_intensity": float,
}


def validate_data_on_schema(schema, data):
    """Check that every key required by *schema* is present in *data*."""
    for key_sch in schema.keys():
        if key_sch not in data:
            print(f"Missing required key: '{key_sch}'")
            return False
    return True


def load_trials(trials_path="trials.jsonl"):
    """Load trials from JSONL.  Returns a list of dicts."""
    trials = []
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    trial_file = os.path.join(parent_dir, trials_path)

    try:
        with open(trial_file, "r") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                t = json.loads(line)
                if validate_data_on_schema(state_schema, t):
                    # Validate that controller metadata is also present
                    missing_meta = CONTROLLER_META_FIELDS - set(t.keys())
                    if missing_meta:
                        print(f"Warning: trial missing controller fields {missing_meta}, using defaults")
                    trials.append(t)
                else:
                    print(f"Warning: Skipping trial with invalid structure: {t}")

        print(f"Loaded {len(trials)} trials from {trial_file}")
    except Exception as e:
        print(f"Failed to load trials: {e}")
    return trials


# ── FSM states ───────────────────────────────────────────────────────────────
class ControllerState(Enum):
    INIT = auto()
    WAITING_FOR_START = auto()
    LOADING_TRIAL = auto()
    PLAYING = auto()
    WAITING_ANIMATION_START = auto()
    WAITING_ANIMATION_END = auto()
    TRIAL_COMPLETE = auto()


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
        }

        # Trial configuration
        self.trials = load_trials()
        self.trials_length = len(self.trials)

        self.current_trial_index = 0

        # Frame tracking
        self.current_frame = -1

        # FSM
        self.fsm_state = ControllerState.INIT

        # Special commands (START)
        self._start = False

        # Per-trial tracking
        self.nr_attempts = 0
        self.trial_start_time = 0.0
        self.game_time_unresponsive = 0.0
        self.trial_start_state = None
        self.frame_log = {}  # frame_number -> {state_read, commands_sent}
        self.trial_run_counter = 0  # monotonically increasing across entire session

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

    # ── helpers ───────────────────────────────────────────────────────────
    @property
    def trial(self):
        """Current trial config dict."""
        return self.trials[self.current_trial_index]

    def game_state_fields(self, config):
        """Return only the game-state keys from a trial config (no controller meta)."""
        return {k: v for k, v in config.items() if k not in CONTROLLER_META_FIELDS}

    def write_config_on_state(self, config, state):
        """Overlay trial game-state config onto a base state dict."""
        for key, value in config.items():
            if key not in CONTROLLER_META_FIELDS:
                state[key] = value
        return state

    def check_has_won(self, state):
        return state.get("win_elapsed_secs", 0.0) != 0.0

    def reset_commands(self):
        self.inputs = {k: False for k in self.inputs}
        self.triggers = {k: False for k in self.triggers}

    def reset_triggers(self):
        self.triggers = {k: False for k in self.triggers}

    def write_commands(self, commands=None):
        if commands is None:
            # Use internal commands
            data_to_write = {**self.inputs, **self.triggers}
        else:
            data_to_write = commands

        self.shm_wrapper.write_commands(**data_to_write)
        
        # Snapshot for logging
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
        }
        self.shm_wrapper.write_commands(**cmds)
        return cmds
    
    def write_game_state(self, state):
        self.shm_wrapper.write_game_state(**state)

    def log_frame(self, state_read, commands_sent):
        """Record one frame of data in the trial log."""
        entry = {
            "state_read": state_read,
            "commands_sent": commands_sent,
        }
        self.frame_log[str(self.current_frame)] = entry

    def save_trial_log(self, outcome):
        """Persist the accumulated frame log for the current trial."""
        elapsed = time.time() - self.trial_start_time
        log = {
            "trial_index": self.current_trial_index,
            "trial_config": self.trial,
            "outcome": outcome,
            "nr_attempts": self.nr_attempts,
            "elapsed_time": round(elapsed, 4),
            "timestamp_start": datetime.datetime.fromtimestamp(
                self.trial_start_time
            ).isoformat(),
            "timestamp_end": datetime.datetime.now().isoformat(),
            "frames": self.frame_log,
        }

        filename = f"trial_{self.current_trial_index:03d}_run_{self.trial_run_counter:04d}.json"
        filepath = os.path.join(self.log_dir, filename)
        try:
            with open(filepath, "w") as f:
                json.dump(log, f, indent=2, default=str)
            print(f"[LOG] Saved trial log → {filepath}")
        except Exception as e:
            print(f"[LOG] Failed to save log: {e}")

    def loop(self):
        print("[FSM] Controller loop started")
        while self._running:
            # Read current game state from shared memory
            state = self.shm_wrapper.read_game_state()

            current_frame = state.get("frame_number", 0)

            # Sync game frame and wait for next one
            if self.current_frame == -1:
                self.current_frame = current_frame
                print(f"[FSM] Starting at frame {self.current_frame}")
                continue

            # Wait for a new game frame
            if current_frame == self.current_frame:
                self.game_time_unresponsive += POLLING_RATE_TIME_S / 1000.0
                time.sleep(POLLING_RATE_TIME_S / 1000.0)
                # Game unresponsive, resync FSM            
                if self.game_time_unresponsive >= GAME_UNRESPONSIVENESS_THRESHOLD_S or self.current_frame == 0:
                    print(f"[FSM] Game unresponsive for {self.game_time_unresponsive:.1f}s, resyncing...")
                    self._resync_with_game()
                continue
            
            # Update frame 
            self.current_frame = current_frame

            # State transitions
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

    # ── state handlers ────────────────────────────────────────────────────

    def _handle_init(self):
        """Issue black screen + pause rendering, then wait for R."""
        print("[FSM] INIT → issuing blank_screen + stop_rendering")

        # Init trial
        trial_cfg = self.trial
        print(f"[FSM] Loading trial {self.current_trial_index}: {self.game_state_fields(trial_cfg)}")

        # Build fresh default state and overlay trial config
        default_state = self.shm_wrapper.read_default_game_state()
        trial_state = self.write_config_on_state(trial_cfg, default_state)
        self.write_game_state(trial_state)
        self.trial_start_state = trial_state

        # Write commands to setup beginning of trial
        self.write_commands(
            {"rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": True,
            "blank_screen": True,
            "stop_rendering": True,
            "animation_door": False,
            "animation_all_door": False,    
            })

        # Initialise per-trial tracking
        self.nr_attempts = 0
        self.trial_start_time = time.time()
        self.frame_log = {}
        self.trial_run_counter += 1


        self.fsm_state = ControllerState.WAITING_FOR_START
        self.reset_commands()
        print("[FSM] → WAITING_FOR_START  (press 'r' to begin)")

    def _handle_waiting_for_start(self, state):
        """Idle on black screen until experimenter presses R."""
        if self._start:
            # Turn off black screen and start rendering timing
            cmds = self.write_commands(
                {"rotate_left": False,
                "rotate_right": False,
                "zoom_in": False,
                "zoom_out": False,
                "check": False,
                "reset": False,
                "blank_screen": True,
                "stop_rendering": True,
                "animation_door": False,
                "animation_all_door": False,    
            }
            )
            self.fsm_state = ControllerState.PLAYING
            self.log_frame(state, cmds)

            print(f"[FSM] R pressed → LOADING_TRIAL (trial {self.current_trial_index})")
            return
        # If not dont write any new command
        cmds = self.write_no_commands()

    def _handle_playing(self, state):
        """Main gameplay: relay inputs, intercept check for hint logic, detect win."""
        # ── Win detection ─────────────────────────────────────────────
        if self.check_has_won(state):
            self.log_frame(state, {**self.inputs, **self.triggers})
            self.fsm_state = ControllerState.TRIAL_COMPLETE
            print("[FSM] Win detected → TRIAL_COMPLETE")
            return

        # ── Handle space (check / hint) ──────────────────────────────
        if self.triggers["check"]:
            trial_cfg = self.trial
            suggestion_threshold = trial_cfg.get("nr_attempts_suggestion", 0)

            if self.nr_attempts < suggestion_threshold:
                # Hint mode: show door animation while paused
                print(f"[PLAY] Attempt {self.nr_attempts + 1} < {suggestion_threshold} → hint (animation_door + pause)")
                # Don't sent any new commands
                cmds = self.write_commands(
                    {"rotate_left": False,
                    "rotate_right": False,
                    "zoom_in": False,
                    "zoom_out": False,
                    "check": True,
                    "reset": False,
                    "blank_screen": False,
                    "stop_rendering": True,
                    "animation_door": True,
                    "animation_all_door": False}
                )

            else:
                # Real check
                print(f"[PLAY] Attempt {self.nr_attempts + 1} >= {suggestion_threshold} → check")
                cmds = self.write_commands(
                    {"rotate_left": False,
                    "rotate_right": False,
                    "zoom_in": False,
                    "zoom_out": False,
                    "check": True,
                    "reset": False,
                    "blank_screen": False,
                    "stop_rendering": False,
                    "animation_door": True,
                    "animation_all_door": True}
                )

            self.nr_attempts += 1
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return 
        
        # Any other case the game is playing
        cmds = self.write_commands()
        self.log_frame(state, cmds)

    def _handle_waiting_animation_start(self, state):
        """Wait for the game to acknowledge the animation (is_animating → True)."""
        if state.get("is_animating", False):
            print("[FSM] Animation started → WAITING_ANIMATION_END")
            self.fsm_state = ControllerState.WAITING_ANIMATION_END

        # Don't sent any new commands
        cmds = self.write_commands(
            {"rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": False,
            "blank_screen": False,
            "stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False}
        )
        self.write_game_state(state)
        self.log_frame(state, cmds)

    def _handle_waiting_animation_end(self, state):
        """Wait for door animation to finish (is_animating → False), then resume."""
        if not state.get("is_animating", True):
            # Animation done, resume rendering
            print("[FSM] Animation finished → issuing stop_rendering (resume)")
            self.reset_commands()

            # Resume rendering new commands
            cmds = self.write_commands(
                {"rotate_left": False,
                "rotate_right": False,
                "zoom_in": False,
                "zoom_out": False,
                "check": False,
                "reset": False,
                "blank_screen": False,
                "stop_rendering": True,
                "animation_door": False,
                "animation_all_door": False}
            )
            self.write_game_state(state)
            self.log_frame(state, cmds)
            self.fsm_state = ControllerState.PLAYING
            print("[FSM] → PLAYING")
            return

        # Don't sent any new commands, it's animating
        cmds = self.write_commands(
            {"rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
            "check": False,
            "reset": False,
            "blank_screen": False,
            "stop_rendering": False,
            "animation_door": False,
            "animation_all_door": False}
        )
        self.write_game_state(state)
        self.log_frame(state, cmds)

    def _handle_trial_complete(self, state):
        """Evaluate performance and decide advance / stay / retroceed."""
        elapsed = time.time() - self.trial_start_time
        trial_cfg = self.trial

        nr_to_win = trial_cfg.get("nr_attempts_to_win", 999)
        nr_to_retro = trial_cfg.get("nr_attempts_to_retroceed", 999)
        time_to_win = trial_cfg.get("elapsed_time_to_win", 9999.0)
        time_to_retro = trial_cfg.get("elapsed_time_to_retroceed", 9999.0)

        print(f"[EVAL] attempts={self.nr_attempts} elapsed={elapsed:.1f}s | "
              f"win<={nr_to_win}/{time_to_win}s  retro>={nr_to_retro}/{time_to_retro}s")

        # Decide outcome
        if self.nr_attempts <= nr_to_win and elapsed <= time_to_win:
            outcome = "advance"
            next_index = (self.current_trial_index + 1) % self.trials_length
            print(f"[EVAL] ADVANCE → trial {next_index}")
        elif self.nr_attempts >= nr_to_retro or elapsed >= time_to_retro:
            outcome = "retroceed"
            next_index = min((self.current_trial_index - 1) % self.trials_length, 0)
            print(f"[EVAL] RETROCEED → trial {next_index}")
        else:
            outcome = "stay"
            next_index = self.current_trial_index
            print(f"[EVAL] STAY → trial {next_index}")

        # Save trial log
        self.save_trial_log(outcome)

        # Update trial index
        self.current_trial_index = next_index

        self._resync_with_game()
    # ── keyboard handlers ─────────────────────────────────────────────────

    def _resync_with_game(self):
        self.current_frame = -1
        self.game_time_unresponsive = 0.0
        self.fsm_state = ControllerState.INIT

    def on_key_press(self, key):
        # Continuous inputs
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
        if  hasattr(key, 'char') and key.char == "r":
            self._start = False

if __name__ == "__main__":
    app = MonkeyGameController()
    app.loop()
