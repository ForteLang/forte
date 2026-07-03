// Forte Hub browser page: browse lineage, play a release straight from its
// sources, verify its digest in-tab, and fork it into the local editor.

import { Store } from './storage.js';

const API = new URLSearchParams(location.search).get('api') || 'http://127.0.0.1:9377';
const $ = (id) => document.getElementById(id);
const status = (t) => ($('status').textContent = t);

let wasmBytes;
async function wasm() {
  wasmBytes ??= await (await fetch('forte.wasm')).arrayBuffer();
  const { instance } = await WebAssembly.instantiate(wasmBytes.slice(0), {});
  const e = instance.exports;
  return { e, ctx: e.fw_new(48000) };
}
function put(inst, prepare, commit, text) {
  const bytes = new TextEncoder().encode(text);
  const ptr = prepare(inst.ctx, bytes.length);
  new Uint8Array(inst.e.memory.buffer, ptr, bytes.length).set(bytes);
  return commit ? commit(inst.ctx) : 0;
}
function compileIn(inst, entrySrc, files) {
  put(inst, inst.e.fw_modules_prepare, inst.e.fw_modules_commit, JSON.stringify(files));
  put(inst, inst.e.fw_src_prepare, null, entrySrc);
  return inst.e.fw_compile(inst.ctx);
}

// ---- list view ---------------------------------------------------------------
async function showList() {
  $('detail').style.display = 'none';
  $('list').style.display = 'block';
  const { repos } = await (await fetch(`${API}/api/repos`)).json();
  $('list').innerHTML = repos.length ? '' : 'hub は空です';
  for (const r of repos) {
    const div = document.createElement('div');
    div.className = 'repo';
    div.innerHTML = `<h2>${r.name} <span class="badge ${r.kind}">${r.kind}</span>
      ${r.forked_from ? `<span class="badge">⑂ ${r.forked_from.repo} v${r.forked_from.v}</span>` : ''}
      ${r.releases ? `<span class="badge">releases: ${r.releases}</span>` : ''}</h2>
      <div class="meta">v${r.v} by ${r.author}${r.devices.length ? ` — devices: ${r.devices.join(', ')}` : ''}</div>`;
    div.onclick = () => showDetail(r.name);
    $('list').appendChild(div);
  }
}

// ---- detail view ---------------------------------------------------------------
let current = null; // { repo detail, files, entrySrc }
let ac, node;

async function showDetail(name) {
  const repo = await (await fetch(`${API}/api/repos/${name}`)).json();
  const { files } = await (await fetch(`${API}/api/repos/${name}/files`)).json();
  const entrySrc = files[repo.entry];
  current = { repo, files, entrySrc };

  $('list').style.display = 'none';
  $('detail').style.display = 'block';
  $('d-name').textContent = `${name} v${repo.v} [${repo.kind}] by ${repo.author}`;

  const lin = [];
  if (repo.forked_from) lin.push(`└─ forked from: ${repo.forked_from.repo} v${repo.forked_from.v}`);
  for (const f of repo.forks) lin.push(`⑂ fork -> ${f.name} v${f.v}`);
  for (const rel of repo.releases)
    lin.push(`release v${rel.v}: digest ${rel.digest} (${rel.seconds.toFixed(1)}s, verified ${rel.verified}回) <span id="vbadge-${rel.v}"></span>`);
  lin.push(`fork events: ${repo.fork_events}`);
  $('d-lineage').innerHTML = lin.join('<br>');

  $('d-code').innerHTML = '';
  for (const [rel, text] of Object.entries(files)) {
    const pre = document.createElement('pre');
    pre.textContent = `// ${rel}\n${text}`;
    $('d-code').appendChild(pre);
  }
  status(repo.kind === 'song' ? '' : '(library — 再生対象は song)');
}

$('back').onclick = (e) => {
  e.preventDefault();
  showList();
};

// listen: compile the sources in a worklet and play — the "release" needs no
// audio file because the code IS the recording
$('listen').onclick = async () => {
  if (!current) return;
  if (!ac) {
    ac = new AudioContext({ sampleRate: 48000 });
    const src = await (await fetch('worklet.js')).text();
    await ac.audioWorklet.addModule(URL.createObjectURL(new Blob([src], { type: 'text/javascript' })));
    node = new AudioWorkletNode(ac, 'forte', { outputChannelCount: [2] });
    node.connect(ac.destination);
    wasmBytes ??= await (await fetch('forte.wasm')).arrayBuffer();
    await new Promise((res) => {
      node.port.onmessage = (e) => e.data.kind === 'ready' && res();
      node.port.postMessage({ cmd: 'init', wasm: wasmBytes.slice(0) });
    });
    node.port.onmessage = (e) => {
      if (e.data.kind === 'pos')
        status(`bar ${Math.floor(e.data.beats / 4) + 1}.${Math.floor(e.data.beats % 4) + 1}`);
    };
  }
  node.port.postMessage({ cmd: 'src', text: current.entrySrc, modules: JSON.stringify(current.files) });
  await ac.resume();
  node.port.postMessage({ cmd: 'play' });
};
$('stop').onclick = () => node?.port.postMessage({ cmd: 'stop' });

// verify: rebuild in-tab and compare with the ledger digest
$('verify').onclick = async () => {
  if (!current?.repo.releases?.length) {
    status('release がありません');
    return;
  }
  status('rebuilding…');
  await new Promise((r) => setTimeout(r, 30));
  const inst = await wasm();
  const n = compileIn(inst, current.entrySrc, current.files);
  if (n !== 0) {
    status('コンパイル失敗');
    return;
  }
  const digest = BigInt.asUintN(64, inst.e.fw_digest(inst.ctx)).toString(16).padStart(16, '0');
  const rel = current.repo.releases.at(-1);
  const badge = document.getElementById(`vbadge-${rel.v}`);
  const ok = digest === rel.digest;
  if (badge) {
    badge.className = `badge ${ok ? 'verified' : 'failed'}`;
    badge.textContent = ok ? 'VERIFIED IN THIS TAB' : `MISMATCH (${digest})`;
  }
  status(ok ? '再現一致 ✓' : '不一致');
  document.body.dataset.verify = ok ? 'ok' : 'fail';
};

// fork: ledger the event on the hub, then drop the files into the local
// editor's OPFS — provenance stamp included
$('fork').onclick = async () => {
  if (!current) return;
  const res = await fetch(`${API}/api/repos/${current.repo.name}/fork?by=browser`, { method: 'POST' });
  const fork = await res.json();
  try {
    const store = await new Store().init();
    for (const [rel, text] of Object.entries(fork.files)) {
      await store.write(rel, text); // nested paths keep import structure
    }
    status(`forked — エディタ(index.html)の一覧に入りました`);
    document.body.dataset.forked = 'ok';
  } catch {
    status('fork は台帳に記録されましたが OPFS 保存に失敗しました');
  }
};

showList();
