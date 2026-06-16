/* tslint:disable */
/* eslint-disable */

export class WebSharedMemory {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Pointer to command_ack (AtomicU64, Game writes)
     */
    get_command_ack_ptr(): number;
    /**
     * Pointer to command_seq (AtomicU64, Controller writes)
     */
    get_command_seq_ptr(): number;
    /**
     * Byte offsets of every field inside SharedCommands (relative to its start).
     */
    get_commands_offsets(): any;
    /**
     * Pointer to SharedCommands (Controller → Game)
     */
    get_commands_ptr(): number;
    /**
     * Return default values of SharedGameState::new() as a JS object.
     * Equivalent to Python's `read_default_game_state()`.
     */
    get_default_game_state(): any;
    /**
     * Pointer to frame_ring_buffer.entries[0] (first SharedGameState slot)
     */
    get_frame_buffer_entries_ptr(): number;
    /**
     * Byte size of one ring buffer entry (= SharedGameState).
     */
    get_frame_buffer_entry_stride(): number;
    /**
     * Number of slots in the ring buffer.
     */
    get_frame_buffer_size(): number;
    /**
     * Pointer to frame_ring_buffer.write_head (AtomicU64)
     */
    get_frame_buffer_write_head_ptr(): number;
    /**
     * Byte offsets of every field inside SharedGameState (works for both
     * game_structure_game and game_structure_control since they have identical layout).
     */
    get_game_state_offsets(): any;
    /**
     * Pointer to game_structure_control (Controller → Game: controller writes, game reads on reset)
     */
    get_game_structure_control_ptr(): number;
    /**
     * Pointer to game_structure_game (Game → Controller: game writes, controller reads)
     */
    get_game_structure_game_ptr(): number;
    /**
     * Get base pointer to SharedMemory
     */
    get_ptr(): number;
    constructor(ptr: number);
}

/**
 * Single object containing every cross-controller constant — values, field
 * lists, FSM labels. Mirrors the attributes exposed by the Python module so
 * `controller.py` and `controller_main.js` can pull the same source.
 */
export function controller_constants(): any;

/**
 * Allocate the shared memory on Rust side and return pointer.
 * JS will use this pointer to create a view.
 */
export function create_shared_memory_wasm(): number;

/**
 * Constants and defaults consumed by the trial editor (trial_editor.html).
 * Lets the editor import the same source of truth as the game and the
 * controllers, instead of hand-mirroring values from shared/src/lib.rs.
 */
export function editor_constants(): any;

/**
 * Return the byte-size of SharedGameState so JS knows the extent of each region.
 */
export function shared_game_state_byte_size(): number;

/**
 * WASM entry point – call this manually from JS after create_shared_memory_wasm()
 */
export function wasm_main(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly wasm_main: () => void;
    readonly __wbg_websharedmemory_free: (a: number, b: number) => void;
    readonly controller_constants: () => number;
    readonly create_shared_memory_wasm: () => number;
    readonly editor_constants: () => number;
    readonly shared_game_state_byte_size: () => number;
    readonly websharedmemory_get_command_ack_ptr: (a: number) => number;
    readonly websharedmemory_get_command_seq_ptr: (a: number) => number;
    readonly websharedmemory_get_commands_offsets: (a: number) => number;
    readonly websharedmemory_get_commands_ptr: (a: number) => number;
    readonly websharedmemory_get_default_game_state: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_entries_ptr: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_entry_stride: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_size: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_write_head_ptr: (a: number) => number;
    readonly websharedmemory_get_game_state_offsets: (a: number) => number;
    readonly websharedmemory_get_game_structure_control_ptr: (a: number) => number;
    readonly websharedmemory_get_game_structure_game_ptr: (a: number) => number;
    readonly websharedmemory_new: (a: number) => number;
    readonly websharedmemory_get_ptr: (a: number) => number;
    readonly __wasm_bindgen_func_elem_195664: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_4644: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_4646: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_108481: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_4: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_5: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_6: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_7: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_8: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_9: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4646_10: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4655: (a: number, b: number, c: number) => void;
    readonly __wasm_bindgen_func_elem_4651: (a: number, b: number) => void;
    readonly __wasm_bindgen_func_elem_92928: (a: number, b: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export5: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
