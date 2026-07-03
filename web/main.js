// Forte web editor: a main-thread wasm instance handles compile/diagnostics/
// build digest/viz; an AudioWorklet instance handles playback with hot reload.
// Songs autosave to OPFS (local-first): close the tab, come back, keep working.

import { Viz } from './viz.js';
import { Store } from './storage.js';
import { encodeFrec, toBase64 } from './frec.js';

const $ = (id) => document.getElementById(id);
const status = (t) => ($('status').textContent = t);
const viz = new Viz($('viz'));
const BUILTINS = [
  'first-light.forte',
  'slow-circles.forte',
  'night-parade.forte',
  'handmade.forte', // imports from devices/warm.forte
];
// bundled device libraries, importable from any song
const MODULE_LIBS = ['devices/warm.forte'];
let bundledModules = {};

// ---- main-thread compiler instance -----------------------------------------
let wasmBytes, main;
async function initWasm() {
  wasmBytes = await (await fetch('forte.wasm')).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(wasmBytes.slice(0), {});
  main = { e: instance.exports };
  main.ctx = main.e.fw_new(48000);
}
// module map = bundled libraries + every OPFS file (so local songs can split
// out their own device libraries and import them); recorded takes ride along
// as base64
let modulesJson = '{}';
let assetsJson = '{}';
async function refreshModules() {
  const map = { ...bundledModules };
  const assets = {};
  if (store) {
    for (const name of await store.list()) {
      map[name] = await store.read(name);
    }
    for (const name of await store.list('.frec')) {
      assets[name] = toBase64(await store.readBytes(name));
    }
  }
  modulesJson = JSON.stringify(map);
  assetsJson = JSON.stringify(assets);
}
function stage(inst, json, commit) {
  const bytes = new TextEncoder().encode(json);
  const ptr = inst.e.fw_modules_prepare(inst.ctx, bytes.length);
  new Uint8Array(inst.e.memory.buffer, ptr, bytes.length).set(bytes);
  commit(inst.ctx);
}
function setModules(inst) {
  stage(inst, modulesJson, inst.e.fw_modules_commit);
  stage(inst, assetsJson, inst.e.fw_assets_commit);
}

function mainCompile(text) {
  setModules(main);
  const bytes = new TextEncoder().encode(text);
  const ptr = main.e.fw_src_prepare(main.ctx, bytes.length);
  new Uint8Array(main.e.memory.buffer, ptr, bytes.length).set(bytes);
  const n = main.e.fw_compile(main.ctx);
  const dp = main.e.fw_diags_ptr(main.ctx);
  const dl = main.e.fw_diags_len(main.ctx);
  const diags = JSON.parse(new TextDecoder().decode(new Uint8Array(main.e.memory.buffer, dp, dl)));
  if (n === 0) {
    const vp = main.e.fw_viz_ptr(main.ctx);
    const vl = main.e.fw_viz_len(main.ctx);
    viz.setData(JSON.parse(new TextDecoder().decode(new Uint8Array(main.e.memory.buffer, vp, vl))));
    window.__vizTracks = viz.data?.tracks?.length ?? 0;
  }
  return { ok: n === 0, diags };
}

// ---- editor (Monaco if the CDN is reachable, plain textarea otherwise) ------
const fallback = $('fallback');
let getText = () => fallback.value;
let setText = (t) => (fallback.value = t);
let onChange = () => {};
fallback.addEventListener('input', () => onChange());
window.__forteGetText = () => getText();
window.__forteCompileCheck = (src) => {
  // compile arbitrary source in the main wasm instance without touching the
  // editor (used by tests); restores the editor's project afterwards
  const r = mainCompile(src);
  mainCompile(getText());
  return r.ok;
};

async function tryMonaco(initial) {
  try {
    const base = 'https://cdn.jsdelivr.net/npm/monaco-editor@0.49.0/min';
    await new Promise((res, rej) => {
      const s = document.createElement('script');
      s.src = `${base}/vs/loader.js`;
      s.onload = res;
      s.onerror = rej;
      setTimeout(rej, 4000);
      document.head.appendChild(s);
    });
    require.config({ paths: { vs: `${base}/vs` } });
    await new Promise((res, rej) => require(['vs/editor/editor.main'], res, rej));
    monaco.languages.register({ id: 'forte' });
    monaco.languages.setMonarchTokensProvider('forte', {
      tokenizer: {
        root: [
          [/\/\/.*/, 'comment'],
          [/\b(song|track|return|section|let|instrument|insert|play|at|send|volume|pan|tempo|meter|key|bars|automate|modulate|from|to|over|with)\b/, 'keyword'],
          [/\b(chords|arp|bass|sampler|polymer|grid|filter|eq|drive|delay|reverb|beat|notes|prog)\b/, 'type'],
          [/"[^"]*"/, 'string'],
          [/`[^`]*`/, 'string.backtick'],
          [/-?\d+(\.\d+)?\w*/, 'number'],
        ],
      },
    });
    fallback.remove();
    const ed = monaco.editor.create($('editor-host'), {
      value: initial,
      language: 'forte',
      theme: 'vs-dark',
      fontSize: 13,
      minimap: { enabled: false },
      automaticLayout: true,
    });
    getText = () => ed.getValue();
    setText = (t) => ed.setValue(t);
    ed.onDidChangeModelContent(() => onChange());
    window.__forteSetMarkers = (diags) => {
      monaco.editor.setModelMarkers(
        ed.getModel(),
        'forte',
        diags.map((d) => ({
          startLineNumber: d.line, startColumn: d.col,
          endLineNumber: d.line, endColumn: d.col + 1,
          severity: monaco.MarkerSeverity.Error,
          message: `[${d.code}] ${d.message}`,
        }))
      );
    };
    return true;
  } catch {
    return false; // offline: keep the textarea
  }
}

// ---- audio ------------------------------------------------------------------
let ac, node;
async function ensureAudio() {
  if (ac) return;
  ac = new AudioContext({ sampleRate: 48000, latencyHint: 'interactive' });
  // worklet module loads bypass the service worker (Chromium limitation), so
  // fetch through the SW cache ourselves and load from a blob URL — this is
  // what keeps playback working offline.
  const src = await (await fetch('worklet.js')).text();
  const blobUrl = URL.createObjectURL(new Blob([src], { type: 'text/javascript' }));
  await ac.audioWorklet.addModule(blobUrl);
  node = new AudioWorkletNode(ac, 'forte', { outputChannelCount: [2] });
  node.connect(ac.destination);
  await new Promise((res) => {
    node.port.onmessage = (e) => e.data.kind === 'ready' && res();
    node.port.postMessage({ cmd: 'init', wasm: wasmBytes.slice(0) });
  });
  node.port.onmessage = (e) => {
    const m = e.data;
    if (m.kind === 'pos') {
      status(`bar ${Math.floor(m.beats / 4) + 1}.${Math.floor(m.beats % 4) + 1} | peak ${m.peak.toFixed(2)}`);
      viz.setPlayhead(m.beats);
    }
  };
  node.port.postMessage({ cmd: 'src', text: getText(), modules: modulesJson, assets: assetsJson });
}

// ---- recording (mic → provenance-stamped .frec in OPFS) ----------------------
// Chunks stream through rec-worker.js to OPFS as they arrive, so a crashed
// tab loses at most the final second; recoverCrashedTake() picks it up on
// the next boot (SRS-REC-002).
let rec = null; // { ctx, stream, worker, rate, session, startedAt }

async function saveTake(pcm, rate, provenance) {
  const takes = store ? await store.list('.frec') : [];
  const name = `assets/take-${takes.length + 1}.frec`;
  await store?.writeBytes(name, encodeFrec(rate, 1, pcm, provenance));
  await refreshModules();
  document.body.dataset.lastTake = name;
  return name;
}

async function recStart() {
  const worker = new Worker('rec-worker.js');
  const session = crypto.randomUUID();
  const startedAt = new Date().toISOString();
  // the constraints matter: without them the browser applies phone-call
  // processing that ruins music takes (SRS-REC-005)
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
  });
  const ctx = new AudioContext({ sampleRate: 48000 });
  await new Promise((res) => {
    worker.onmessage = (e) => e.data.kind === 'started' && res();
    worker.postMessage({ cmd: 'start', rate: ctx.sampleRate, startedAt, session });
  });
  const src = await (await fetch('recorder.js')).text();
  await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })));
  const node = new AudioWorkletNode(ctx, 'forte-rec');
  node.port.onmessage = (e) =>
    worker.postMessage({ cmd: 'chunk', data: e.data.data }, [e.data.data.buffer]);
  ctx.createMediaStreamSource(stream).connect(node);
  rec = { ctx, stream, worker, rate: ctx.sampleRate, session, startedAt };
  $('rec').textContent = '■ 録音停止';
  document.body.dataset.rec = 'on';
  status('recording…');
}

async function recStop() {
  const { ctx, stream, worker, rate, session, startedAt } = rec;
  rec = null;
  stream.getTracks().forEach((t) => t.stop());
  await ctx.close();
  $('rec').textContent = '● Rec';
  document.body.dataset.rec = 'off';
  await new Promise((res) => {
    worker.onmessage = (e) => e.data.kind === 'stopped' && res();
    worker.postMessage({ cmd: 'stop' });
  });
  worker.terminate();
  const bytes = await store.readBytes('assets/.recording.pcm');
  const pcm = new Float32Array(bytes.buffer, 0, Math.floor(bytes.byteLength / 4));
  const calib = JSON.parse(localStorage.getItem('forte.calibration') || 'null');
  const provenance = {
    device_class: 'microphone',
    recorded_at: startedAt,
    by: 'user:web',
    session,
    sig: 'webcrypto:stub', // real device keys arrive with Hub accounts
    // measured round-trip latency travels with the take, so any consumer can
    // compensate placement (SRS-REC-004)
    ...(calib ? { latency_samples: calib.rtl_samples, latency_confidence: calib.confidence } : {}),
  };
  const name = await saveTake(pcm, rate, provenance);
  await store.remove('assets/.recording.pcm').catch(() => {});
  await store.remove('assets/.recording.json').catch(() => {});
  status(`saved ${name} (${(pcm.length / rate).toFixed(1)}s) — import ${name.split('/')[1].replace('.frec', '').replace('-', '_')} from "./${name}"`);
}

/// A leftover .recording.* pair means the tab died mid-take: turn what was
/// flushed into a real take instead of losing it.
async function recoverCrashedTake() {
  if (!store) return;
  let journal, bytes;
  try {
    journal = JSON.parse(await store.read('assets/.recording.json'));
    bytes = await store.readBytes('assets/.recording.pcm');
  } catch {
    return; // nothing to recover
  }
  try {
    if (bytes.byteLength >= 4) {
      const pcm = new Float32Array(bytes.buffer, 0, Math.floor(bytes.byteLength / 4));
      const name = await saveTake(pcm, journal.rate || 48000, {
        device_class: 'microphone',
        recorded_at: journal.started_at || new Date().toISOString(),
        by: 'user:web',
        session: journal.session || 'recovered',
        sig: 'webcrypto:stub',
        recovered: true,
      });
      document.body.dataset.recovered = 'ok';
      status(`前回のクラッシュから録音を復元しました: ${name}`);
    }
  } finally {
    await store.remove('assets/.recording.pcm').catch(() => {});
    await store.remove('assets/.recording.json').catch(() => {});
  }
}

// ---- files (OPFS, local-first) ----------------------------------------------
let store = null;
let currentName = BUILTINS[0];

async function localNames() {
  return store ? store.list() : [];
}

async function refreshFileList() {
  const locals = await localNames();
  const sel = $('file');
  sel.innerHTML = '';
  const add = (value, label) => {
    const o = document.createElement('option');
    o.value = value;
    o.textContent = label;
    sel.appendChild(o);
  };
  for (const n of locals) add(n, `● ${n}`);
  for (const n of BUILTINS) if (!locals.includes(n)) add(n, `demo: ${n}`);
  sel.value = currentName;
}

async function loadSong(name) {
  currentName = name;
  localStorage.setItem('forte.last', name);
  const locals = await localNames();
  let text;
  if (locals.includes(name)) {
    text = await store.read(name);
  } else {
    text = await (await fetch(`../songs/${name}`)).text();
  }
  setText(text);
  recompile(0);
}

let saveTimer;
function autosave() {
  if (!store) return;
  clearTimeout(saveTimer);
  $('saved').textContent = '● …';
  saveTimer = setTimeout(async () => {
    await store.write(currentName, getText());
    $('saved').textContent = '✓ saved';
    refreshFileList();
    refreshModules(); // local files are importable modules
  }, 500);
}

// ---- performance capture: play keys/MIDI, get code back (roadmap 1.4) --------
// A performance in Forte is not an opaque event recording — it comes back as
// a notes literal you can read, edit and commit.
let perf = null; // { t0, tempo, active: Map<pitch, startBeats>, events: [] }
const KEY_TO_PITCH = {
  a: 60, w: 61, s: 62, e: 63, d: 64, f: 65, t: 66,
  g: 67, y: 68, h: 69, u: 70, j: 71, k: 72, o: 73, l: 74,
};

function perfBeats() {
  return ((performance.now() - perf.t0) / 1000) * ((viz.data?.tempo ?? 120) / 60);
}
function perfNote(on, pitch) {
  if (!perf) return;
  node?.port.postMessage({ cmd: 'note', on, pitch, vel: 0.85 }); // live monitor
  if (on && !perf.active.has(pitch)) {
    perf.active.set(pitch, perfBeats());
  } else if (!on && perf.active.has(pitch)) {
    const start = perf.active.get(pitch);
    perf.active.delete(pitch);
    perf.events.push([start, Math.max(perfBeats() - start, 0.05), pitch]);
  }
}
function onPerfKey(e) {
  if (e.repeat || e.target.tagName === 'TEXTAREA' || e.target.tagName === 'INPUT') return;
  const pitch = KEY_TO_PITCH[e.key?.toLowerCase()];
  if (pitch === undefined) return;
  e.preventDefault();
  perfNote(e.type === 'keydown', pitch);
}

async function performToggle() {
  if (perf) {
    // stop: flush held notes, transcribe in wasm, hand the code back
    for (const [pitch] of [...perf.active]) perfNote(false, pitch);
    window.removeEventListener('keydown', onPerfKey, true);
    window.removeEventListener('keyup', onPerfKey, true);
    const events = perf.events;
    perf = null;
    $('perform').textContent = '🎹 Perform';
    if (!events.length) {
      status('演奏なし');
      return;
    }
    const flat = new Float32Array(events.length * 3);
    events.forEach(([s, l, p], i) => flat.set([s, l, p], i * 3));
    const ptr = main.e.fw_perform_buf(main.ctx, events.length);
    new Float32Array(main.e.memory.buffer, ptr, flat.length).set(flat);
    const len = main.e.fw_transcribe(main.ctx, 0.25); // 1/16 grid
    const body = new TextDecoder().decode(
      new Uint8Array(main.e.memory.buffer, main.e.fw_transcribe_ptr(main.ctx), len)
    );
    const code = `notes\`${body}\``;
    document.body.dataset.performCode = code;
    const div = document.createElement('div');
    div.className = 'ok';
    div.style.userSelect = 'all';
    div.textContent = `🎹 ${code}`;
    $('diags').prepend(div);
    status('書き起こしました(下の診断欄からコピーして play に貼ってください)');
    return;
  }
  await ensureAudio();
  await ac.resume();
  perf = { t0: performance.now(), active: new Map(), events: [] };
  window.addEventListener('keydown', onPerfKey, true);
  window.addEventListener('keyup', onPerfKey, true);
  // hardware MIDI when the browser has it (Chromium); PC keys always work
  try {
    const midi = await navigator.requestMIDIAccess?.();
    midi?.inputs.forEach((input) => {
      input.onmidimessage = (m) => {
        const [st, pitch, vel] = m.data;
        const kind = st & 0xf0;
        if (kind === 0x90 && vel > 0) perfNote(true, pitch);
        else if (kind === 0x80 || (kind === 0x90 && vel === 0)) perfNote(false, pitch);
      };
    });
  } catch { /* no MIDI permission — keyboard still works */ }
  $('perform').textContent = '■ 演奏終了';
  status('演奏モード: A〜K が白鍵、W/E/T/Y/U が黒鍵(MIDI 鍵盤も可)');
}

// ---- loopback calibration (SRS-REC-004) ---------------------------------------
// Play a chirp through the speakers while recording the mic on the SAME
// AudioContext clock; the wasm cross-correlator finds where it landed.
// rtl = (found position in recording) - (when we played it). Browsers cannot
// report this number truthfully, so we measure it.
async function calibrate() {
  status('calibrating…');
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { echoCancellation: false, noiseSuppression: false, autoGainControl: false },
  });
  const ctx = new AudioContext({ sampleRate: 48000 });
  const src = await (await fetch('recorder.js')).text();
  await ctx.audioWorklet.addModule(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })));
  const node = new AudioWorkletNode(ctx, 'forte-rec');
  let firstFrame = null;
  const chunks = [];
  node.port.onmessage = (e) => {
    if (firstFrame === null) firstFrame = e.data.frame;
    chunks.push(e.data.data);
  };
  ctx.createMediaStreamSource(stream).connect(node);

  // probe from the same wasm code the tests verify
  const probePtr = main.e.fw_calib_probe(main.ctx, 48000, 0.15);
  const probeLen = main.e.fw_calib_probe_len(main.ctx);
  const probe = new Float32Array(main.e.memory.buffer, probePtr, probeLen).slice();
  const buf = ctx.createBuffer(1, probe.length, 48000);
  buf.copyToChannel(probe, 0);
  const player = ctx.createBufferSource();
  player.buffer = buf;
  player.connect(ctx.destination);
  const startAt = ctx.currentTime + 0.25;
  player.start(startAt);

  await new Promise((r) => setTimeout(r, 1200));
  stream.getTracks().forEach((t) => t.stop());
  await ctx.close();

  const total = chunks.reduce((n, c) => n + c.length, 0);
  const recPtr = main.e.fw_calib_rec(main.ctx, total);
  let off = 0;
  for (const c of chunks) {
    new Float32Array(main.e.memory.buffer, recPtr + off * 4, c.length).set(c);
    off += c.length;
  }
  const lag = main.e.fw_calib_run(main.ctx);
  document.body.dataset.calib = lag >= 0 ? 'ok' : 'nodetect';
  if (lag < 0) {
    status('較正: プローブ音を検出できませんでした(スピーカー→マイクの経路を確認)');
    return;
  }
  const conf = main.e.fw_calib_confidence(main.ctx);
  const playedAtFrame = Math.round(startAt * 48000) - firstFrame;
  const rtl = lag - playedAtFrame;
  localStorage.setItem(
    'forte.calibration',
    JSON.stringify({ rtl_samples: rtl, rate: 48000, confidence: conf, at: new Date().toISOString() })
  );
  status(`較正完了: 往復 ${((rtl / 48000) * 1000).toFixed(1)}ms (信頼度 ${conf.toFixed(2)}) — 以後のテイクに記録されます`);
}

// ---- wiring -------------------------------------------------------------------
function showDiags(diags) {
  const el = $('diags');
  el.innerHTML = '';
  if (!diags.length) {
    el.innerHTML = '<div class="ok">✓ コンパイル OK</div>';
  } else {
    for (const d of diags) {
      const div = document.createElement('div');
      div.className = 'd';
      div.textContent = `${d.line}:${d.col} [${d.code}] ${d.message}`;
      el.appendChild(div);
    }
  }
  window.__forteSetMarkers?.(diags);
}

let debounce;
function recompile(delay = 300) {
  clearTimeout(debounce);
  debounce = setTimeout(() => {
    const { ok, diags } = mainCompile(getText());
    showDiags(diags);
    document.body.dataset.compiled = ok ? 'ok' : 'error';
    if (ok && node) node.port.postMessage({ cmd: 'src', text: getText(), modules: modulesJson, assets: assetsJson }); // hot reload
  }, delay);
}

const NEW_TEMPLATE = `song "Untitled" {
  tempo 120bpm
  meter 4/4

  track Drums {
    instrument sampler(sample: "Kick")
    play beat\`x--- x--- x--- x---\` at bars(1..4)
  }
}
`;

async function boot() {
  navigator.serviceWorker?.register('sw.js').catch(() => {});
  await initWasm();
  try {
    store = await new Store().init();
  } catch {
    store = null; // OPFS unavailable: still fully usable, just no persistence
  }
  for (const lib of MODULE_LIBS) {
    try {
      bundledModules[lib] = await (await fetch(`../songs/${lib}`)).text();
    } catch { /* offline without cache: song imports will diagnose */ }
  }
  await refreshModules();
  const last = localStorage.getItem('forte.last');
  const locals = await localNames();
  currentName =
    last && (locals.includes(last) || BUILTINS.includes(last)) ? last : BUILTINS[0];

  const initialText = locals.includes(currentName)
    ? await store.read(currentName)
    : await (await fetch(`../songs/${currentName}`)).text();
  setText(initialText);
  await tryMonaco(initialText);
  onChange = () => {
    autosave();
    recompile();
  };
  await refreshFileList();
  recompile(0);
  status('ready');
  await recoverCrashedTake();

  $('file').onchange = (e) => loadSong(e.target.value);
  $('new').onclick = async () => {
    const name = prompt('曲名 (例: my-song)');
    if (!name || !store) return;
    const file = `${name.replace(/[^\w-]/g, '-')}.forte`;
    await store.write(file, NEW_TEMPLATE);
    await refreshFileList();
    loadSong(file);
  };
  $('delete').onclick = async () => {
    if (!store) return;
    const locals = await localNames();
    if (!locals.includes(currentName)) return;
    if (!confirm(`ローカルの ${currentName} を削除しますか?`)) return;
    await store.remove(currentName);
    await refreshFileList();
    loadSong(BUILTINS.includes(currentName) ? currentName : BUILTINS[0]);
  };
  $('play').onclick = async () => {
    await ensureAudio();
    await ac.resume();
    node.port.postMessage({ cmd: 'play' });
  };
  $('stop').onclick = () => node?.port.postMessage({ cmd: 'stop' });
  $('rec').onclick = () => (rec ? recStop() : recStart()).catch((e) => status(`rec: ${e.message}`));
  $('calib').onclick = () =>
    calibrate().catch((e) => {
      document.body.dataset.calib = 'fail';
      status(`calib: ${e.message}`);
    });
  $('perform').onclick = () => performToggle().catch((e) => status(`perform: ${e.message}`));
  $('digest').onclick = () => {
    status('building…');
    setTimeout(() => {
      // wasm i64 returns arrive as *signed* BigInt; render as unsigned hex
      const d = BigInt.asUintN(64, main.e.fw_digest(main.ctx));
      $('digest-out').textContent = d.toString(16).padStart(16, '0');
      status('ready');
    }, 30);
  };
}
boot();
