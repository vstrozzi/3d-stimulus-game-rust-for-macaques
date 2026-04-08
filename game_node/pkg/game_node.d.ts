/* tslint:disable */
/* eslint-disable */

export class WebSharedMemory {
    free(): void;
    [Symbol.dispose](): void;
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
 * Allocate the shared memory on Rust side and return pointer.
 * JS will use this pointer to create a view.
 */
export function create_shared_memory_wasm(): number;

/**
 * REFRESH_RATE_HZ from constants.rs — mirrors Python's monkey_shared.REFRESH_RATE_HZ
 */
export function refresh_rate_hz(): number;

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
    readonly websharedmemory_new: (a: number) => number;
    readonly websharedmemory_get_commands_ptr: (a: number) => number;
    readonly websharedmemory_get_game_structure_game_ptr: (a: number) => number;
    readonly websharedmemory_get_game_structure_control_ptr: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_write_head_ptr: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_entries_ptr: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_entry_stride: (a: number) => number;
    readonly websharedmemory_get_frame_buffer_size: (a: number) => number;
    readonly websharedmemory_get_commands_offsets: (a: number) => any;
    readonly websharedmemory_get_game_state_offsets: (a: number) => any;
    readonly websharedmemory_get_default_game_state: (a: number) => any;
    readonly __wbg_websharedmemory_free: (a: number, b: number) => void;
    readonly refresh_rate_hz: () => number;
    readonly shared_game_state_byte_size: () => number;
    readonly create_shared_memory_wasm: () => number;
    readonly websharedmemory_get_ptr: (a: number) => number;
    readonly wasm_bindgen__closure__destroy__h008ef40e414b05ea: (a: number, b: number) => void;
    readonly wasm_bindgen__closure__destroy__h1460c50773e8b475: (a: number, b: number) => void;
    readonly wasm_bindgen__closure__destroy__h7f28661cd7c808ff: (a: number, b: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h75024a8abfa83955: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__heace45efc15e9340: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_2: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_3: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_4: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_5: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_6: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_7: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h05be767a89874804_8: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h79af2cf5ebf3e6cc: (a: number, b: number, c: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h14051faa26fee3bc: (a: number, b: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h985676e499e3a96b: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
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
