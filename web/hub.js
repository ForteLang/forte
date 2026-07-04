// Forte Hub browser page: browse lineage, play a release straight from its
// sources, verify its digest in-tab, and fork it into the local editor.

import { Store } from './storage.js';
import { fromBase64 } from './frec.js';

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
function compileIn(inst, entrySrc, files, assets) {
  put(inst, inst.e.fw_modules_prepare, inst.e.fw_modules_commit, JSON.stringify(files));
  put(inst, inst.e.fw_modules_prepare, inst.e.fw_assets_commit, JSON.stringify(assets || {}));
  put(inst, inst.e.fw_src_prepare, null, entrySrc);
  return inst.e.fw_compile(inst.ctx);
}

let filterAuthor = null; // performer cross-cut

// ---- list view ---------------------------------------------------------------
function treeNode(n, prefix, isLast, out) {
  const conn = prefix === '' ? '' : prefix + (isLast ? '└─ ' : '├─ ');
  const row = document.createElement('div');
  row.className = 'tnode';
  const badges = [
    n.kind === 'library' ? '📚' : '♪',
    n.releases ? `<span class="badge">release×${n.releases}</span>` : '',
    n.plays ? `<span class="badge">▶${n.plays}</span>` : '',
  ].join(' ');
  row.innerHTML = `${conn}${badges} ${n.name} <span class="who">v${n.v} by ${n.author}</span>`;
  row.onclick = () => showDetail(n.name);
  out.appendChild(row);
  const childPrefix = prefix === '' ? '  ' : prefix + (isLast ? '   ' : '│  ');
  (n.children || []).forEach((c, i) => treeNode(c, childPrefix, i === (n.children.length - 1), out));
}

async function showTree() {
  try {
    const { roots } = await (await fetch(`${API}/api/lineage`)).json();
    const el = $('tree');
    el.innerHTML = '';
    roots.forEach((r) => treeNode(r, '', true, el));
    document.body.dataset.treeNodes = String(el.querySelectorAll('.tnode').length);
  } catch { /* older server: tree stays empty */ }
}

async function showList() {
  $('detail').style.display = 'none';
  $('list-wrap').style.display = 'block';
  showTree();
  const { repos } = await (await fetch(`${API}/api/repos`)).json();
  $('list').innerHTML = repos.length ? '' : 'hub は空です';
  for (const r of repos) {
    const div = document.createElement('div');
    div.className = 'repo';
    div.innerHTML = `<h2>${r.name} <span class="badge ${r.kind}">${r.kind}</span>
      ${r.forked_from ? `<span class="badge">⑂ ${r.forked_from.repo} v${r.forked_from.v}</span>` : ''}
      ${r.releases ? `<span class="badge">releases: ${r.releases}</span>` : ''}</h2>
      <div class="meta">v${r.v} by <a href="#" class="author" data-author="${r.author}">${r.author}</a>${r.devices.length ? ` — devices: ${r.devices.join(', ')}` : ''}</div>`;
    div.onclick = (e) => {
      // performer cross-cut: clicking an author filters the list to them
      if (e.target.classList?.contains('author')) {
        e.preventDefault();
        e.stopPropagation();
        filterAuthor = filterAuthor === r.author ? null : r.author;
        showList();
        return;
      }
      showDetail(r.name);
    };
    if (filterAuthor && r.author !== filterAuthor) div.style.display = 'none';
    $('list').appendChild(div);
  }
  document.body.dataset.authorFilter = filterAuthor || '';
  if (filterAuthor) {
    const note = document.createElement('div');
    note.className = 'meta';
    note.innerHTML = `by ${filterAuthor} で絞り込み中 — <a href="#" id="clear-author">解除</a>`;
    note.querySelector('#clear-author').onclick = (e) => {
      e.preventDefault();
      filterAuthor = null;
      showList();
    };
    $('list').prepend(note);
  }
}

// ---- detail view ---------------------------------------------------------------
let current = null; // { repo detail, files, entrySrc }
let ac, node;
// open-stems: listener-side stem states (applied live to the worklet)
let muted = new Set();
let soloed = new Set();

function sendStem(cmd, track, on) {
  node?.port.postMessage({ cmd, track, on });
}

function renderStems() {
  const el = $('stems');
  el.innerHTML = '';
  muted = new Set();
  soloed = new Set();
  const viz = current?.viz;
  if (!viz?.tracks) return;
  viz.tracks.forEach((t, i) => {
    if (t.fx) return; // returns follow their senders
    const row = document.createElement('div');
    row.className = 'stem';
    const m = document.createElement('button');
    m.textContent = 'M';
    m.title = 'ミュート';
    m.onclick = () => {
      muted.has(i) ? muted.delete(i) : muted.add(i);
      m.classList.toggle('on', muted.has(i));
      sendStem('mute', i, muted.has(i));
      document.body.dataset.stems = `${muted.size}m${soloed.size}s`;
    };
    const s = document.createElement('button');
    s.textContent = 'S';
    s.title = 'ソロ(歌入れ練習: Vocal を M、または自分のパートを S)';
    s.onclick = () => {
      soloed.has(i) ? soloed.delete(i) : soloed.add(i);
      s.classList.toggle('on', soloed.has(i));
      sendStem('solo', i, soloed.has(i));
      document.body.dataset.stems = `${muted.size}m${soloed.size}s`;
    };
    const label = document.createElement('span');
    label.textContent = t.name;
    row.append(m, s, label);
    el.append(row);
  });
}

async function showDetail(name) {
  const repo = await (await fetch(`${API}/api/repos/${name}`)).json();
  const { files, assets } = await (await fetch(`${API}/api/repos/${name}/files`)).json();
  const entrySrc = files[repo.entry];
  current = { repo, files, assets: assets || {}, entrySrc };

  // compile once on the main thread for the track list (stem controls)
  try {
    const inst = await wasm();
    if (compileIn(inst, entrySrc, files, current.assets) === 0) {
      const vp = inst.e.fw_viz_ptr(inst.ctx);
      const vl = inst.e.fw_viz_len(inst.ctx);
      current.viz = JSON.parse(
        new TextDecoder().decode(new Uint8Array(inst.e.memory.buffer, vp, vl))
      );
    }
  } catch { /* stem controls are optional */ }

  $('list-wrap').style.display = 'none';
  $('detail').style.display = 'block';
  $('d-name').textContent = `${name} v${repo.v} [${repo.kind}] by ${repo.author}`;

  const lin = [];
  if (repo.forked_from) lin.push(`└─ forked from: ${repo.forked_from.repo} v${repo.forked_from.v}`);
  for (const f of repo.forks) lin.push(`⑂ fork -> ${f.name} v${f.v}`);
  if (repo.plays) lin.push(`▶ plays: ${repo.plays}`);
  for (const s of repo.similar ?? [])
    lin.push(`♪ 同じ進行: <a href="#" class="similar" data-name="${s.name}">${s.name}</a> (キー非依存)`);
  // cross-module dig: instruments link to the library that defines them,
  // libraries list the songs that play them
  if (repo.uses?.length) {
    const parts = repo.uses.map((u) =>
      repo.device_sources?.[u]
        ? `<a href="#" class="similar" data-name="${repo.device_sources[u]}">${u}</a>`
        : u
    );
    lin.push(`🎛 使っている楽器: ${parts.join(', ')}`);
  }
  for (const user of repo.used_by ?? [])
    lin.push(`🎛 この楽器を使う曲: <a href="#" class="similar" data-name="${user}">${user}</a>`);
  for (const rel of repo.releases)
    lin.push(`release v${rel.v}: digest ${rel.digest} (${rel.seconds.toFixed(1)}s, verified ${rel.verified}回) <span id="vbadge-${rel.v}"></span>`);
  lin.push(`fork events: ${repo.fork_events}`);
  $('d-lineage').innerHTML = lin.join('<br>');

  renderStems();
  $('d-code').innerHTML = '';
  for (const [rel, text] of Object.entries(files)) {
    const pre = document.createElement('pre');
    pre.textContent = `// ${rel}\n${text}`;
    $('d-code').appendChild(pre);
  }
  status(repo.kind === 'song' ? '' : '(library — 再生対象は song)');
  for (const a of document.querySelectorAll('a.similar')) {
    a.onclick = (e) => {
      e.preventDefault();
      showDetail(a.dataset.name);
    };
  }
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
  node.port.postMessage({
    cmd: 'src',
    text: current.entrySrc,
    modules: JSON.stringify(current.files),
    assets: JSON.stringify(current.assets),
  });
  await ac.resume();
  // re-apply the listener's stem states to the fresh engine
  for (const i of muted) sendStem('mute', i, true);
  for (const i of soloed) sendStem('solo', i, true);
  node.port.postMessage({ cmd: 'play' });
  // listens are ledger events — the raw data the contribution economy
  // will be computed from (SRS-HUB-007)
  fetch(`${API}/api/repos/${current.repo.name}/play?by=browser`, { method: 'POST' }).catch(() => {});
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
  const n = compileIn(inst, current.entrySrc, current.files, current.assets);
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
    for (const [rel, b64] of Object.entries(fork.assets || {})) {
      await store.writeBytes(rel, fromBase64(b64)); // recorded takes
    }
    // provenance by construction: a re-publish of this copy must record
    // forked_from — the stamp travels with the files
    await store.write('.forte-lineage.json', JSON.stringify(fork.origin, null, 2));
    status(`forked — エディタ(index.html)の一覧に入りました`);
    document.body.dataset.forked = 'ok';
  } catch {
    status('fork は台帳に記録されましたが OPFS 保存に失敗しました');
  }
};

showList();
