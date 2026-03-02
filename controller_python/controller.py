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

# Controller-only metadata fields (not written to game shared memory)
CONTROLLER_META_FIELDS = {
    "nr_attempts_to_win",
    "nr_attempts_suggestion",
    "nr_attempts_to_retroceed",
    "elapsed_time_to_win",
    "elapsed_time_to_retroceed",
}

# Game-state schema (fields written to shared memory)
state_schema = {
    "decorations_seeds": [int],
    "base_radius": float,
    "height": float,
    "start_orient": float,
    "target_door": int,
    "colors": [[int]],
    "decorations_count": [int],
    "decorations_size": [float],
    "decorations_shape": [int], 
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

        # Trial configuration
        self.trials = load_trials()
        self.trials_length = len(self.trials)

        self.current_trial_index = 0

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

    def check_has_finished(self, state):
        nr_attempts = state.get("nr_attempts", 0)
        nr_attempts_to_retroceed = self.trial.get("nr_attempts_to_retroceed", 0)
        time_elapsed = state.get("elapsed_secs", 0.0)
        elapsed_time_to_retroceed = self.trial.get("elapsed_time_to_retroceed", 0.0)

        return state.get("win_elapsed_secs", 0.0) != 0.0 or \
               nr_attempts >= nr_attempts_to_retroceed or \
               time_elapsed >= elapsed_time_to_retroceed

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
            "animation_colored": False,
        }
        self.shm_wrapper.write_commands(**cmds)
        return cmds
    
    def write_game_state(self, state):
        self.shm_wrapper.write_game_state(**state)

    # Fields from the game state that change frame-to-frame and are worth logging
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
        """Record one frame of data in the trial log."""
        filtered_state = {k: v for k, v in state_read.items() if k in self.LOGGED_STATE_FIELDS}
        entry = {
            "state_read": filtered_state,
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

    def _handle_init(self):
        """Issue black screen + pause rendering, then wait for R."""
        print("[FSM] INIT → issuing blank_screen + stop_rendering")

        # Init trial
        trial_cfg = self.trial
        print(f"[FSM] Loading trial {self.current_trial_index}: {self.game_state_fields(trial_cfg)}")

        # Build fresh default state and overlay trial config
        default_state = self.shm_wrapper.read_default_game_state()
        trial_state = self.write_config_on_state(trial_cfg, default_state)

        # Read previous state of game machine to get state of it
        state_old = self.shm_wrapper.read_game_state()

        self.write_game_state(trial_state)
        self.trial_start_state = trial_state

        # Write commands to setup beginning of trial

        print(f"state old {state_old.get('is_blank', False)} and stop {state_old.get('is_rendering_stopped', False)}")
        self.write_commands(
            {"rotate_left": False,
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

        # Initialise per-trial tracking
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
        """Idle on black screen until experimenter presses R."""
        if self._start:
            # Turn off black screen and start rendering timing
            cmds = self.write_commands(
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
                "animation_colored": False,
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


        time_elapsed = state.get("elapsed_secs", 0.0)

        is_win = time_elapsed < self.trial.get("elapsed_time_to_win", 0.0) and self.nr_attempts < self.trial.get("nr_attempts_to_win", 0)
        is_stay = not is_win and time_elapsed < self.trial.get("elapsed_time_to_retroceed", 0.0) and \
                  self.nr_attempts < self.trial.get("nr_attempts_to_retroceed", 0)
        
        # Set state of advancement of trial
        if is_win:
            self.trial_proceeding = TrialProceeding.ADVANCE
        elif is_stay:
            self.trial_proceeding = TrialProceeding.STAY
        else:
            self.trial_proceeding = TrialProceeding.RETROCEED

        # One time set when Time for winning exceeded but not to retroceed, blink all the light on white
        if time_elapsed > self.trial.get("elapsed_time_to_win", 0.0) and not self._time_win_expired:
            print(f"[TIME] Time to win exceeded ({time_elapsed:.1f}s > {self.trial.get('elapsed_time_to_win', 0.0)}s), triggering animation")
            self._time_win_expired = True
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
                    "animation_all_door": True,
                    "animation_colored": False}
                )

            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return
            
        # Time for retroceed triggered, animate all lights in red and terminate
        elif (time_elapsed > self.trial.get("elapsed_time_to_retroceed", 0.0) and not self._time_retroceed_expired):
            print(f"[TIME] Time to retroceed exceeded ({time_elapsed:.1f}s > {self.trial.get('elapsed_time_to_retroceed', 0.0)}s), triggering animation")
            self._time_retroceed_expired = True
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
                    "animation_all_door": True,
                    "animation_colored": True}
                )

            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return
        
        # Terminate everything if won or attempts overexceeded or time to retroceed exceeded
        if self.check_has_finished(state):
            print(f"[FSM] Check finished with outcome {self.trial_proceeding.name} → TRIAL_COMPLETE")
            self.log_frame(state, {**self.inputs, **self.triggers})
            self.fsm_state = ControllerState.TRIAL_COMPLETE
            print("[FSM] Win detected → TRIAL_COMPLETE")
            return

        # Check triggered
        if self.triggers["check"]:
            trial_cfg = self.trial
            suggestion_threshold = trial_cfg.get("nr_attempts_suggestion", 0)
            retroceeds_threshold = trial_cfg.get("nr_attempts_to_retroceed", 0)
            # Flag to detect a win
            cosine_current_alignment = state.get("cosine_alignment", 0.0)
            cosine_alingment_treshold = trial_cfg.get("cosine_alignment_threshold", 0.0)

            # Current attempt is the last and fail
            if (self.nr_attempts + 1) == retroceeds_threshold and cosine_current_alignment < cosine_alingment_treshold:
                print(f"[PLAY] Attempt {self.nr_attempts} == {retroceeds_threshold} → retroceed")
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
                    "animation_all_door": True,
                    "animation_colored": True}
                )
                self.fsm_state = ControllerState.WAITING_ANIMATION_START
                self.log_frame(state, cmds)
                return
            # Single light not colored: either suggest in or winning with staying in current level
            elif (self.nr_attempts < suggestion_threshold and cosine_current_alignment < cosine_alingment_treshold) or \
                (self.trial_proceeding == TrialProceeding.STAY and cosine_current_alignment > cosine_alingment_treshold):
                print(f"[PLAY] Attempt {self.nr_attempts + 1} < {suggestion_threshold} and cosine_alignment={cosine_current_alignment:.2f} < {cosine_alingment_treshold:.2f} → hint (animation_door + pause)")  
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
                    "animation_all_door": False,
                    "animation_colored": False,
                    }
                )
            # Color single light: winning and proceeding to next level
            elif is_win and cosine_current_alignment > cosine_alingment_treshold and self.nr_attempts < suggestion_threshold :
                print(f"[PLAY] Attempt {self.nr_attempts + 1} < {suggestion_threshold} and cosine_alignment={cosine_current_alignment:.2f} < {cosine_alingment_treshold:.2f} → win with hint (animation_door + pause)")
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
                    "animation_all_door": False,
                    "animation_colored": True,
                    }
                )
            # All lights colored:
            else:
                # Real check
                print(f"[PLAY] Attempt {self.nr_attempts + 1} < {suggestion_threshold} and cosine_alignment={cosine_current_alignment:.2f} > {cosine_alingment_treshold:.2f} → check without hint")
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
                    "animation_all_door": True,
                    "animation_colored": False}
                )

            self.nr_attempts += 1
            self.fsm_state = ControllerState.WAITING_ANIMATION_START
            print("[FSM] → WAITING_ANIMATION_START")
            self.write_commands(cmds)
            self.log_frame(state, cmds)
            return 

        # Normal playing
        cmds = self.write_commands()
        self.log_frame(state, cmds)

        return

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
            "animation_all_door": False,
            "animation_colored": False}
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
                "animation_all_door": False,
                "animation_colored": False}
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
            "animation_all_door": False,
            "animation_colored": False}
        )
        self.write_game_state(state)
        self.log_frame(state, cmds)

    def _handle_trial_complete(self, state):
        """Evaluate performance and decide advance / stay / retroceed."""
        elapsed = self.trial.get("elapsed_secs", 0.0)

        nr_to_win = self.trial.get("nr_attempts_to_win", 999)
        nr_to_retro = self.trial.get("nr_attempts_to_retroceed", 999)
        time_to_win = self.trial.get("elapsed_time_to_win", 9999.0)
        time_to_retro = self.trial.get("elapsed_time_to_retroceed", 9999.0)

        print(f"[EVAL] attempts={self.nr_attempts} elapsed={elapsed:.1f}s | "
              f"win<={nr_to_win}/{time_to_win}s  retro>={nr_to_retro}/{time_to_retro}s")

        # Decide outcome
        if self.trial_proceeding == TrialProceeding.ADVANCE:
            outcome = "advance"
            next_index = (self.current_trial_index + 1) % self.trials_length
            print(f"[EVAL] ADVANCE → trial {next_index}")
        elif self.trial_proceeding == TrialProceeding.RETROCEED:
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
