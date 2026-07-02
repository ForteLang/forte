// Generic runner for wasm32-wasip1 binaries under Node's built-in WASI.
// Usage: node run-wasi.mjs <wasm> <preopens-json> <args-json>
//   e.g. node run-wasi.mjs forte.wasm '{"/proj":"."}' '["forte","build","/proj/song.forte"]'
import { WASI } from 'node:wasi';
import { readFileSync } from 'node:fs';

const [wasmPath, preopensJson, argsJson] = process.argv.slice(2);
const wasi = new WASI({
  version: 'preview1',
  args: JSON.parse(argsJson),
  preopens: JSON.parse(preopensJson),
});
const wasm = await WebAssembly.compile(readFileSync(wasmPath));
const instance = await WebAssembly.instantiate(wasm, wasi.getImportObject());
wasi.start(instance);
