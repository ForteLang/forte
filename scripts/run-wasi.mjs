// Run a wasm32-wasip1 binary under Node's built-in WASI (used by
// scripts/determinism_test.sh). Usage: node run-wasi.mjs <wasm> <scratch-dir>
import { WASI } from 'node:wasi';
import { readFileSync } from 'node:fs';

const [wasmPath, scratchDir] = process.argv.slice(2);
const wasi = new WASI({
  version: 'preview1',
  args: ['determinism', '/scratch/wasm.f32'],
  preopens: { '/scratch': scratchDir },
});
const wasm = await WebAssembly.compile(readFileSync(wasmPath));
const instance = await WebAssembly.instantiate(wasm, wasi.getImportObject());
wasi.start(instance);
