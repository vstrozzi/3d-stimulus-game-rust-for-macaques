/* tslint:disable */
/* eslint-disable */

/**
 * Helper wrapper for WASM side
 */
export class WebSharedMemory {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * Get pointer to SharedCommands (for writing commands from JS)
     */
    get_commands_ptr(): number;
    /**
     * Get pointer to SharedGameStructure (for reading/writing game state from JS)
     */
    get_game_structure_ptr(): number;
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
 * WASM entry point - call this manually from JS after create_shared_memory_wasm()
 */
export function wasm_main(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly wasm_main: () => void;
    readonly websharedmemory_new: (a: number) => number;
    readonly websharedmemory_get_commands_ptr: (a: number) => number;
    readonly websharedmemory_get_game_structure_ptr: (a: number) => number;
    readonly __wbg_websharedmemory_free: (a: number, b: number) => void;
    readonly create_shared_memory_wasm: () => number;
    readonly websharedmemory_get_ptr: (a: number) => number;
    readonly wasm_bindgen__closure__destroy__h05de16b877b82b7a: (a: number, b: number) => void;
    readonly wasm_bindgen__closure__destroy__hb69473973aca8f63: (a: number, b: number) => void;
    readonly wasm_bindgen__closure__destroy__he4caea5b818e1e89: (a: number, b: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h104a3925f357f864: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h108b8b23f051e164: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h4ec3345578019de4: (a: number, b: number, c: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h9bc97ee707d2470f: (a: number, b: number, c: any) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h644b49c99bfff8fa: (a: number, b: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h70b77d51203f2fd1: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
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
