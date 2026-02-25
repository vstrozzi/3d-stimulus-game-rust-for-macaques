import sys
import time
import math
import json
import os
from pynput import keyboard
from enum import Enum, auto

from transitions import Machine

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build the shared library with 'cargo build --release -p shared --features python' and copy the resulting '.so' to controller_python/monkey_shared.so.")
    sys.exit(1)

# Constants imported from shared/src/constants.rs via monkey_shared
REFRESH_RATE_HZ = monkey_shared.REFRESH_RATE_HZ
WIN_BLANK_DURATION_FRAMES = monkey_shared.WIN_BLANK_DURATION_FRAMES
POLLING_RATE_FSM = 10 # ms

# State config schema
state_schema = {
    "decoration_seeds": [int], # size 3
    "base_radius": float,
    "height": float,
    "start_orient": float,
    "target_door": int,
    "colors": [[int]], # size 4x3
    "decorations_count": [int], # size 4
    "decorations_size": [float], # size 4
    "cosine_alignment_threshold": float,
    "door_anim_fade_out": int,
    "door_anim_stay_open": int,
    "door_anim_fade_in": int,
    "main_spotlight_intensity": float,
    "ambient_brightness": float,
    "max_spotlight_intensity": float,
}

# Validate schema name
def validate_data_on_schema(schema, data):
    for key_d, key_sch in zip(data.keys(), schema.keys()):
        if key_d != key_sch:
            print(f"Key mismatch: expected '{key_sch}', got '{key_d}'")
            return False
    return True

# Load trials from the path as dictionary
def load_trials(trials_path="trials.jsonl"):
    """Load trials from JSONL file."""
    trials = []
    # Try relative to script directory first
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    trial_file = os.path.join(parent_dir, trials_path)

    try:
        with open(trial_file, 'r') as f:
            for line in f:
                line = line.strip()
                if line:
                    t = json.loads(line)
                    if validate_data_on_schema(state_schema, t):
                        trials.append(t)
                    else:
                        print(f"Warning: Skipping trial with invalid structure: {t}")

        print(f"Loaded {len(trials)} trials from {trial_file}")
    except Exception as e:
        print(f"Failed to load trials: {e}")
    return trials

class MonkeyGameController:
    def __init__(self):
        # Game State FSM
        self.states = ['init', 'playing', 'won']
        self.machine = Machine(model=self, states=self.states, initial='init')

        # Transitions
        self.machine.add_transition('start_game', 'init', 'playing', conditions='', after='trigger_reset_config')

        self.machine.add_transition('win_game', 'playing', 'won',)
        self.machine.add_transition('start_anim', 'won', 'animating')
        self.machine.add_transition('stop_rendering', '*', 'stop_rendering')
        self.machine.add_transition('start_blank', 'animating', 'blank_screen')
        self.machine.add_transition('reset_game', 'blank_screen', 'playing')
        self.machine.add_transition('force_reset', '*', 'playing')
        self.machine.add_transition('force_anim', 'playing', 'animating')

        self.pressed_keys = set()
        # Get the shared memory
        try:
            self.shm_wrapper = monkey_shared.SharedMemoryWrapper("monkey_game")
            print("Connected to shared memory interface.")
        except Exception as exc:
            print(f"SHM Connection Error: {exc}")

        # Inputs continous
        self.inputs = {
            "rotate_left": False, "rotate_right": False,
            "zoom_in": False, "zoom_out": False
        }
        # Inputs triggers
        self.triggers = {
            "check": False, "reset": False,
            "blank_screen": False, "stop_rendering": False,
            "animation_door": False
        }

        # Configuration
        self.trials = load_trials()
        self.current_trial_index = 0
        self.trials_length = len(self.trials)

        # Current frame
        self.current_frame = 0
        self.current_state = None

        self._running = True

        # Keyboard listener runs in its own threa
        self.listener = keyboard.Listener(
            on_press=self.on_key_press,
            on_release=self.on_key_release
        )
        self.listener.start()

    def run(self):
        while self._running:
            self.loop()
            time.sleep(POLLING_RATE_FSM / 1000.0)
        self.listener.stop()

    def loop(self):
        # Read game state
        state = self.shm_wrapper.read_game_state()
        # Inputs are read with interruput
        current_frame = state.get("frame_number", 0)

        # Check if the game has updated
        if current_frame == self.current_frame:
            # Reloop since no update by the game
            return

        print(state["win_elapsed_secs"])
        self.current_frame = current_frame
        # Process inputs that redefines the state
        if self.triggers["reset"]:
            # If won advance the state
            if self.check_has_won(state):
                self.current_trial_index = (self.current_trial_index + 1) % self.trials_length

            # Load the default game state and apply current config
            state = self.shm_wrapper.read_default_game_state()
            state = self.write_config_on_state(self.trials[self.current_trial_index], state)

        # TODO: Handle inputs
        #if self.inputs["check"]:

        print(self.inputs, self.triggers)
        # Write commands
        self.write_game_state(state)
        # Write game state
        self.write_commands()

        #Clean commands

    def reset_commands(self):
        self.inputs = {k: False for k in self.inputs}
        self.triggers = {k: False for k in self.triggers}

    def write_commands(self):
        # Write to SHM
        self.shm_wrapper.write_commands(**self.inputs, **self.triggers)

        # Clear triggers
        for k in self.triggers: self.triggers[k] = False

    def write_game_state(self, state):
        self.shm_wrapper.write_game_state(**state)

    def write_config_on_state(self, config, state):
        for key in config:
            state[key] = config[key]
        return state

    def check_has_won(self, state):
        return state["win_elapsed_secs"] != 0.0

    def on_key_release(self, key):
        self.pressed_keys.discard(key)

        if key == keyboard.Key.left: self.inputs["rotate_left"] = False
        if key == keyboard.Key.right: self.inputs["rotate_right"] = False

        if key == keyboard.Key.up: self.inputs["zoom_in"] = False
        if key == keyboard.Key.down: self.inputs["zoom_out"] = False

        if key == keyboard.Key.space: self.triggers["check"] = False
        if hasattr(key, 'char'):
            if key.char == 'd': self.triggers["animation_door"] = False
            if key.char == 'r': self.triggers["reset"] = False

    def on_key_press(self, key):
        if key == keyboard.Key.left: self.inputs["rotate_left"] = True
        elif key == keyboard.Key.right: self.inputs["rotate_right"] = True

        if key == keyboard.Key.up: self.inputs["zoom_in"] = True
        elif key == keyboard.Key.down: self.inputs["zoom_out"] = True

        if key == keyboard.Key.space:
            self.triggers["check"] = True

        if hasattr(key, 'char'):
            if key.char == 'd':
                self.triggers["animation_door"] = True
            if key.char == 'r' and key not in self.pressed_keys:
                print(self.pressed_keys)
                print("Reset trigger toggled")
                self.triggers["reset"] = True
            if key.char == 'b':
                self.triggers["blank_screen"] = not self.triggers["blank_screen"]
            if key.char == 'p':
                self.triggers["stop_rendering"] = not self.triggers["stop_rendering"]
            if key.char == 'q':
                self._running = False

        # Add pressed keys to current preset
        self.pressed_keys.add(key)

    # Update a state to the default values not set by config
    def state_update_to_default(self, state):

        print(f"Transitioning from {event.transition.source} to {event.transition.dest} on event '{event.event.name}'")

if __name__ == "__main__":
    app = MonkeyGameController()
    app.run()


