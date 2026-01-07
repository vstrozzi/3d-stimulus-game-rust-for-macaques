/* tslint:disable */
/* eslint-disable */

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly main: (a: number, b: number) => number;
  readonly wasm_bindgen__convert__closures_____invoke__hb1a4afd1d907f0c6: (a: number, b: number, c: any, d: any) => void;
  readonly wasm_bindgen__closure__destroy__h0032b9a52451c724: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h2295e08e163a431b: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hed7404fa0e794b64: (a: number, b: number, c: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h66a5d2011b38d568: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__hd0c5b9e2f9017268: (a: number, b: number) => void;
  readonly wasm_bindgen__closure__destroy__ha2b3aa73fbfb85c5: (a: number, b: number) => void;
  readonly wasm_bindgen__convert__closures_____invoke__h649b46b7b005587e: (a: number, b: number, c: any) => void;
  readonly wasm_bindgen__closure__destroy__h85b85e3fdfdf6809: (a: number, b: number) => void;
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
