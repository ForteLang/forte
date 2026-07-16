// Forte web editor: a main-thread wasm instance handles compile/diagnostics/
// build digest/viz; an AudioWorklet instance handles playback with hot reload.
// Songs autosave to OPFS (local-first): close the tab, come back, keep working.

import { Viz } from './viz.js';
import { Store, ServerStore } from './storage.js';
import { encodeFrec, toBase64 } from './frec.js';
import { Vcs } from './vcs.js';

const $ = (id) => document.getElementById(id);
const status = (t) => ($('status').textContent = t);
// transient feedback that doesn't require watching the status corner
function toast(msg, kind = '') {
  const host = $('toasts');
  if (!host) return;
  const t = document.createElement('div');
  t.className = `toast ${kind}`;
  t.textContent = msg;
  host.appendChild(t);
  requestAnimationFrame(() => t.classList.add('show'));
  setTimeout(() => {
    t.classList.remove('show');
    setTimeout(() => t.remove(), 300);
  }, 3200);
}
const viz = new Viz($('viz'));
window.__forteViz = viz;
// Click = code-jump / piano-roll toggle. Drag on a clip = move the play it
// came from: the drop snaps to bars and writes back through the edit layer
// (move_at_line), so the arrange view is a real editing surface (#135).
let vizDrag = null; // {hit, x0, moved}
$('viz').addEventListener('mousedown', (ev) => {
  const rect = $('viz').getBoundingClientRect();
  const hit = viz.hitTest(ev.clientX - rect.left, ev.clientY - rect.top);
  if (hit?.kind === 'clip') {
    // grabbing the clip's right edge resizes it; anywhere else moves it
    const { headerW, pxPerBeat } = viz.geom();
    const rightX = headerW + (hit.start + hit.duration) * pxPerBeat;
    const resize = Math.abs(ev.clientX - rect.left - rightX) < 6;
    vizDrag = { hit, x0: ev.clientX, moved: false, resize };
  }
});
window.addEventListener('mousemove', (ev) => {
  if (!vizDrag) {
    // hover feedback: the right edge of a clip is a resize handle
    const rect = $('viz').getBoundingClientRect();
    if (ev.target === $('viz') && viz.data) {
      const hit = viz.hitTest(ev.clientX - rect.left, ev.clientY - rect.top);
      let cur = '';
      if (hit?.kind === 'clip') {
        const { headerW, pxPerBeat } = viz.geom();
        const rightX = headerW + (hit.start + hit.duration) * pxPerBeat;
        cur = Math.abs(ev.clientX - rect.left - rightX) < 6 ? 'ew-resize' : '';
      }
      $('viz').style.cursor = cur;
    }
    return;
  }
  const dx = ev.clientX - vizDrag.x0;
  if (!vizDrag.moved && Math.abs(dx) < 4) return; // still a click
  vizDrag.moved = true;
  const { pxPerBeat } = viz.geom();
  const bpb = viz.data.beatsPerBar;
  if (vizDrag.resize) {
    const dur = Math.max(bpb, Math.round((vizDrag.hit.duration + dx / pxPerBeat) / bpb) * bpb);
    vizDrag.snappedDur = dur;
    viz.setGhost({ track: vizDrag.hit.track, start: vizDrag.hit.start, duration: dur });
    return;
  }
  const snapped =
    Math.max(0, Math.round((vizDrag.hit.start + dx / pxPerBeat) / bpb)) * bpb;
  vizDrag.snapped = snapped;
  viz.setGhost({ track: vizDrag.hit.track, start: snapped, duration: vizDrag.hit.duration });
});
let vizDragJustEnded = false; // mouseup precedes click: remember one tick
window.addEventListener('mouseup', () => {
  if (!vizDrag) return;
  const { hit, moved, snapped, resize, snappedDur } = vizDrag;
  vizDrag = null;
  vizDragJustEnded = moved;
  viz.setGhost(null);
  if (!moved) return;
  const bpb = viz.data.beatsPerBar;
  // a placement longer than its block renders as several segments (loops),
  // all sharing one source line — the gesture applies to the WHOLE span
  const siblings = (viz.data?.tracks ?? [])
    .flatMap((t) => t.clips ?? [])
    .filter((c) => c.line === hit.line);
  const spanStart = Math.min(hit.start, ...siblings.map((c) => c.start));
  const spanEnd = Math.max(hit.start + hit.duration, ...siblings.map((c) => c.start + c.duration));
  if (resize) {
    if (snappedDur === undefined || snappedDur === hit.duration) return;
    const a = Math.round(spanStart / bpb) + 1;
    const newEnd = hit.start + snappedDur; // the grabbed segment's new end
    const durBars = Math.max(1, Math.round((newEnd - spanStart) / bpb));
    applyEdit({ op: 'move_at_line', line: hit.line, bars: [a, a + durBars - 1] });
    return;
  }
  if (snapped === undefined || snapped === hit.start) return;
  const newStart = spanStart + (snapped - hit.start); // move the span by the drag delta
  const a = Math.max(1, Math.round(newStart / bpb) + 1);
  const durBars = Math.max(1, Math.round((spanEnd - spanStart) / bpb));
  applyEdit({ op: 'move_at_line', line: hit.line, bars: [a, a + durBars - 1] });
});
$('viz').addEventListener('click', async (ev) => {
  if (vizDragJustEnded) {
    vizDragJustEnded = false; // a drag just ended, not a click
    return;
  }
  const rect = $('viz').getBoundingClientRect();
  // the top 15px is the ruler: a single click there seeks the playhead
  if (ev.clientY - rect.top <= 15 && viz.data) {
    const { headerW, pxPerBeat } = viz.geom();
    const bpb = viz.data.beatsPerBar || 4;
    const beats = Math.max(0, Math.round((ev.clientX - rect.left - headerW) / pxPerBeat / bpb) * bpb);
    await ensureAudio();
    node.port.postMessage({ cmd: 'seek', beats });
    viz.setPlayhead(beats);
    return;
  }
  const hit = viz.hitTest(ev.clientX - rect.left, ev.clientY - rect.top);
  if (!hit) return;
  if (hit.kind === 'header' || hit.kind === 'roll') {
    viz.togglePianoRoll(hit.track); // lane name → that track's piano roll
  } else if (hit.line > 0) {
    jumpToLine(hit.line); // clip / lane → the play/track (or import) line
  }
});
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
  const map = PROJECT ? {} : { ...bundledModules };
  const assets = {};
  if (store?.readAllText) {
    Object.assign(map, await store.readAllText());
    Object.assign(assets, await store.readAllAssets());
  } else if (store) {
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
// imports in the open buffer resolve from its own directory (project mode)
function currentBase() {
  return PROJECT ? currentName.split('/').slice(0, -1).join('/') : '';
}
function setModules(inst) {
  stage(inst, modulesJson, inst.e.fw_modules_commit);
  stage(inst, assetsJson, inst.e.fw_assets_commit);
  stage(inst, currentBase(), inst.e.fw_base_commit);
}

// ---- mixer (DAW-MIX-08): strips are a projection of the compiled tracks —
// fader / pan write back as set_track; M/S are engine-side monitor state
// for THIS session only (never written to code).
const monitor = { mute: new Set(), solo: new Set() };

// apply an edit op to the OPEN buffer silently; false = op didn't apply
function tryBufferEdit(op) {
  stageSrc(getText());
  stageJson(JSON.stringify(op));
  if (main.e.fw_edit(main.ctx) !== 0) return false;
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  replaceText(out);
  autosave();
  recompile(0);
  return true;
}

// which block file defines this track? (for tracks that arrive via
// `play <ImportedBlock>` — the mixer routes their edits to the block's home)
function trackHomeFile(name) {
  // placed blocks compile their tracks as "<Block>.<Track>"
  const dot = name.indexOf('.');
  const blockName = dot > 0 ? name.slice(0, dot) : null;
  const trackName = dot > 0 ? name.slice(dot + 1) : name;
  const text = getText();
  const scan = (matchBlock) => {
    for (const f of PROJECT?.blocks ?? []) {
      for (const b of f.blocks ?? []) {
        if (matchBlock && b.name !== blockName) continue;
        if (!(b.tracks ?? []).some((t) => t.name === trackName)) continue;
        if (text.includes(b.name)) return { file: f.file, block: b.name, track: trackName };
      }
    }
    return null;
  };
  // exact block first; aliases (play X as Y) fall back to the track name
  return (blockName && scan(true)) || scan(false);
}

// a track-scoped edit: try the open buffer first, else write to the block's
// own file on disk through the project API
async function routeTrackOp(trackName, op) {
  if (tryBufferEdit(op)) return true;
  const home = PROJECT && trackHomeFile(trackName);
  if (!home) {
    status(`edit: track '${trackName}' is not defined in this file or project`);
    return false;
  }
  const r = await fetch(`api/edit?path=${encodeURIComponent(home.file)}`, {
    method: 'POST',
    body: JSON.stringify({ ...op, track: home.track, path: [home.block] }),
  });
  const t = await r.text();
  if (!r.ok) {
    status(`edit: ${t}`);
    return false;
  }
  status(`→ written back to ${home.file} (${home.block})`);
  await refreshModules();
  recompile(0);
  return true;
}

// ---- inspector (set_arg knobs): instrument/insert args of one track ----
function autoSitesOf(text) {
  stageSrc(text);
  const n = main.e.fw_auto_sites(main.ctx);
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  stageSrc(getText());
  return n < 0 ? [] : JSON.parse(out);
}
function argSitesOf(text) {
  stageSrc(text);
  const n = main.e.fw_arg_sites(main.ctx);
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  stageSrc(getText());
  return n < 0 ? [] : JSON.parse(out);
}

// curated effect defaults for the "+ fx" picker (full list = the compiler's)
const FX_PRESETS = [
  ['filter', 'filter(type: "lp", cutoff: 0.6)'],
  ['eq', 'eq(low: 0, mid: 0, high: 0)'],
  ['drive', 'drive(amount: 0.3)'],
  ['delay', 'delay(time: 0.25, fdbk: 0.3, mix: 0.2)'],
  ['space', 'space(mix: 0.25)'],
  ['reverb', 'reverb(mix: 0.25)'],
  ['comp', 'comp()'],
  ['glue', 'glue()'],
  ['chorus', 'chorus()'],
  ['crush', 'crush(amount: 0.4)'],
  ['saturate', 'saturate(amount: 0.4)'],
  ['vinyl', 'vinyl()'],
  ['pump', 'pump()'],
  ['width', 'width()'],
  ['stutter', 'stutter()'],
  ['gate', 'gate()'],
  ['transient', 'transient()'],
  ['parcomp', 'parcomp()'],
  ['exciter', 'exciter()'],
  ['ringmod', 'ringmod()'],
  ['tapestop', 'tapestop()'],
  ['limiter', 'limiter()'],
];

// route an insert-chain op to the buffer or the track's home file
async function routeChainOps(home, siteTrack, ops) {
  if (home) {
    const r = await fetch(`api/edit?path=${encodeURIComponent(home.file)}`, {
      method: 'POST',
      body: JSON.stringify(ops),
    });
    if (!r.ok) return toast(await r.text(), 'err');
    await refreshModules();
    recompile(0);
  } else {
    applyEdit(ops.length === 1 ? ops[0] : ops);
  }
}

let inspTrack = null;
async function renderInspector() {
  const el = $('insp');
  if (!el) return;
  if (el.contains(document.activeElement)) return; // don't rebuild mid-typing
  el.textContent = '';
  if (!inspTrack) return;
  let sites = argSitesOf(getText()).filter((x) => x.track === inspTrack);
  let home = null;
  let homeText = null;
  if (!sites.length && PROJECT) {
    home = trackHomeFile(inspTrack);
    if (home) {
      homeText = await store.read(home.file);
      sites = argSitesOf(homeText).filter(
        (x) => x.track === home.track && x.path.includes(home.block)
      );
    }
  }
  const head = document.createElement('div');
  head.className = 'ihead';
  head.textContent = `inspector: ${inspTrack}${home ? ` — ${home.file}` : ''}`;
  el.appendChild(head);
  for (const site of sites) {
    const row = document.createElement('div');
    row.className = 'irow';
    const nm = document.createElement('span');
    nm.className = 'inm';
    nm.textContent = `${site.target === 'instrument' ? '♪' : 'fx'} ${site.name}`;
    nm.title = `${site.target} (line ${site.line})`;
    row.appendChild(nm);
    if (site.target.startsWith('insert:')) {
      const idx = Number(site.target.split(':')[1]);
      const chainLen = sites.filter((x) => x.target.startsWith('insert:')).length;
      const cb = (label, title, fn) => {
        const b = document.createElement('button');
        b.textContent = label;
        b.title = title;
        b.style.padding = '0 6px';
        b.onclick = fn;
        row.appendChild(b);
      };
      const base = { path: site.path, track: site.track };
      if (idx > 0)
        cb('▲', 'Move this effect earlier in the chain', () =>
          routeChainOps(home, site.track, [{ op: 'move_insert', ...base, from: idx, to: idx - 1 }]));
      if (idx < chainLen - 1)
        cb('▼', 'Move this effect later in the chain', () =>
          routeChainOps(home, site.track, [{ op: 'move_insert', ...base, from: idx, to: idx + 1 }]));
      cb('✕', 'Remove this effect (undoable in-buffer)', () =>
        routeChainOps(home, site.track, [{ op: 'remove_insert', ...base, index: idx }]));
    }
    if (site.target === 'instrument') {
      // swap the instrument itself: pick any palette entry by name
      const sel = document.createElement('select');
      sel.title = 'Swap the instrument (set_instrument)';
      const cur = document.createElement('option');
      cur.textContent = 'swap…';
      cur.value = '';
      sel.appendChild(cur);
      for (const inst of paletteInstruments()) {
        const o = document.createElement('option');
        o.value = JSON.stringify({ call: inst.call, name: inst.name, from: inst.from ?? null });
        o.textContent = inst.label + (inst.where && inst.where !== 'built-in' ? ` (${inst.where.split('_')[0]})` : '');
        sel.appendChild(o);
      }
      sel.onchange = async () => {
        if (!sel.value) return;
        const pick = JSON.parse(sel.value);
        sel.blur(); // let the panel re-render after the recompile
        const ops = [];
        // the import belongs in the FILE that holds the track (home-aware)
        const homeFile = home ? home.file : currentName;
        if (pick.from && pick.from !== homeFile) {
          ops.push({ op: 'add_import', names: [pick.name], from: relPath(homeFile, pick.from) });
        }
        ops.push({ op: 'set_instrument', path: site.path, track: site.track, call: pick.call });
        if (home) {
          const r = await fetch(`api/edit?path=${encodeURIComponent(home.file)}`, {
            method: 'POST',
            body: JSON.stringify(ops),
          });
          if (!r.ok) return status(`edit: ${await r.text()}`);
          status(`→ swapped the instrument in ${home.file}`);
          await refreshModules();
          recompile(0);
        } else {
          applyEdit(ops);
        }
      };
      row.appendChild(sel);
    }
    for (const a of site.args) {
      const lab = document.createElement('label');
      lab.textContent = a.arg;
      const inp = document.createElement('input');
      if (a.num !== undefined && a.num !== null) {
        inp.type = 'number';
        inp.step = '0.01';
        inp.value = a.num;
      } else {
        inp.type = 'text';
        inp.value = a.str ?? '';
        inp.size = 6;
      }
      inp.onchange = () => {
        inp.blur(); // allow the rebuild after recompile
        routeTrackOp(inspTrack, {
          op: 'set_arg',
          path: site.path,
          track: site.track,
          target: site.target,
          arg: a.arg,
          value: inp.type === 'number' ? Number(inp.value) : inp.value,
        });
      };
      lab.appendChild(inp);
      row.appendChild(lab);
    }
    el.appendChild(row);
  }
  if (sites.length) {
    // "+ fx" picker: append an effect to this track's chain
    const row = document.createElement('div');
    row.className = 'irow';
    const sel = document.createElement('select');
    sel.title = 'Add an effect to the end of the chain';
    const first = document.createElement('option');
    first.value = '';
    first.textContent = '+ fx…';
    sel.appendChild(first);
    for (const [name, call] of FX_PRESETS) {
      const o = document.createElement('option');
      o.value = call;
      o.textContent = name;
      sel.appendChild(o);
    }
    sel.onchange = () => {
      if (!sel.value) return;
      const call = sel.value;
      sel.blur();
      const s0 = sites[0];
      routeChainOps(home, s0.track, [
        { op: 'add_insert', path: s0.path, track: s0.track, call },
      ]).then(() => toast(`+ insert ${call.split('(')[0]}`, 'ok'));
    };
    row.appendChild(sel);
    el.appendChild(row);
  }
  // ---- automation rows: automate <target> from A to B over <range> ----
  const trackName = home ? home.track : inspTrack;
  const autos = autoSitesOf(homeText ?? getText()).filter(
    (x) => x.track === trackName && (!home || x.path.includes(home.block))
  );
  for (const a of autos) {
    const row = document.createElement('div');
    row.className = 'irow';
    const nm = document.createElement('span');
    nm.className = 'inm';
    nm.textContent = `⟿ ${a.target} @${a.at}`;
    nm.title = `automate (line ${a.line})`;
    row.appendChild(nm);
    const mkNum = (label, value) => {
      const lab = document.createElement('label');
      lab.textContent = label;
      const inp = document.createElement('input');
      inp.type = 'number';
      inp.step = '0.01';
      inp.value = value;
      lab.appendChild(inp);
      row.appendChild(lab);
      return inp;
    };
    const fromI = mkNum('from', a.from);
    const toI = mkNum('to', a.to);
    const commitAuto = () =>
      routeChainOps(home, trackName, [
        {
          op: 'set_automation',
          path: a.path,
          track: a.track,
          index: a.index,
          from: Number(fromI.value),
          to: Number(toI.value),
        },
      ]);
    fromI.onchange = () => { fromI.blur(); commitAuto(); };
    toI.onchange = () => { toI.blur(); commitAuto(); };
    const del = document.createElement('button');
    del.textContent = '✕';
    del.title = 'Remove this automation';
    del.style.padding = '0 6px';
    del.onclick = () =>
      routeChainOps(home, trackName, [
        { op: 'remove_automation', path: a.path, track: a.track, index: a.index },
      ]);
    row.appendChild(del);
    el.appendChild(row);
  }
  if (sites.length) {
    // "+ automate…" — targets come from the track's own knobs
    const row = document.createElement('div');
    row.className = 'irow';
    const sel = document.createElement('select');
    sel.title = 'Automate a parameter over a bar range (edit the range in code)';
    const first = document.createElement('option');
    first.value = '';
    first.textContent = '+ automate…';
    sel.appendChild(first);
    const targets = ['volume'];
    for (const site of sites) {
      for (const arg of site.args) {
        if (arg.num === undefined || arg.num === null) continue;
        targets.push(site.target === 'instrument' ? arg.arg : `${site.name}.${arg.arg}`);
      }
    }
    for (const t of [...new Set(targets)]) {
      const o = document.createElement('option');
      o.value = t;
      o.textContent = t;
      sel.appendChild(o);
    }
    sel.onchange = () => {
      if (!sel.value) return;
      const target = sel.value;
      sel.blur();
      const bpb = viz.data?.beatsPerBar || 4;
      const endBar = Math.max(1, Math.round((viz.data?.lengthBeats || 16) / bpb));
      const s0 = sites[0];
      routeChainOps(home, trackName, [
        { op: 'add_automation', path: s0.path, track: s0.track, target, from: 0, to: 1, bars: [1, endBar] },
      ]).then(() => toast(`+ automate ${target} from 0 to 1 over bars(1..${endBar})`, 'ok'));
    };
    row.appendChild(sel);
    el.appendChild(row);
  }
}
function sendMonitor(tracks) {
  if (!node) return;
  const anySolo = monitor.solo.size > 0;
  tracks.forEach((t, i) => {
    const off = anySolo ? !monitor.solo.has(t.name) : monitor.mute.has(t.name);
    node.port.postMessage({ cmd: 'mute', track: i, on: off });
  });
}
function renderMixer() {
  const el = $('mixer');
  if (!el) return;
  const tracks = viz.data?.tracks ?? [];
  el.textContent = '';
  for (const t of tracks) {
    if (t.fx) continue; // return tracks have no fader statement (v1)
    const strip = document.createElement('div');
    strip.className = 'strip';
    strip.dataset.track = t.name;
    const nm = document.createElement('div');
    nm.className = 'nm';
    nm.textContent = t.name;
    nm.title = `${t.name} — ${t.instrument} (click for the inspector)`;
    nm.style.cursor = 'pointer';
    nm.onclick = () => {
      inspTrack = inspTrack === t.name ? null : t.name;
      renderInspector().catch(() => {});
    };
    strip.appendChild(nm);
    const meter = document.createElement('div');
    meter.className = 'meter';
    meter.appendChild(document.createElement('i'));
    strip.appendChild(meter);
    const slider = (field, min, max, step, value) => {
      const lbl = document.createElement('div');
      lbl.className = 'lbl';
      const name = document.createElement('span');
      name.textContent = field;
      const val = document.createElement('span');
      val.textContent = Number(value).toFixed(2);
      lbl.append(name, val);
      strip.appendChild(lbl);
      const r = document.createElement('input');
      r.type = 'range';
      r.min = min;
      r.max = max;
      r.step = step;
      r.value = value;
      r.oninput = () => (val.textContent = Number(r.value).toFixed(2));
      // write on release: one clean splice per gesture, not per pixel
      r.onchange = () =>
        routeTrackOp(t.name, { op: 'set_track', track: t.name, field, value: Number(r.value) });
      strip.appendChild(r);
    };
    slider('volume', 0, 1, 0.01, t.volume ?? 0.8);
    slider('pan', -1, 1, 0.01, t.pan ?? 0);
    const ms = document.createElement('div');
    ms.className = 'ms';
    const mbtn = (label, set, title) => {
      const b = document.createElement('button');
      b.textContent = label;
      b.title = title;
      b.className = set.has(t.name) ? 'on' : '';
      b.onclick = () => {
        set.has(t.name) ? set.delete(t.name) : set.add(t.name);
        b.className = set.has(t.name) ? 'on' : '';
        sendMonitor(tracks);
      };
      ms.appendChild(b);
    };
    mbtn('M', monitor.mute, 'Mute (monitor only — never written to code)');
    mbtn('S', monitor.solo, 'Solo (monitor only)');
    {
      const b = document.createElement('button');
      b.textContent = '✕';
      b.title = 'Delete this track (removes the whole track block from the code)';
      b.onclick = () => {
        if (!confirm(`Delete track ${t.name}?`)) return;
        routeTrackOp(t.name, { op: 'remove_track', track: t.name });
      };
      ms.appendChild(b);
    }
    strip.appendChild(ms);
    el.appendChild(strip);
  }
  sendMonitor(tracks);
}

// ---- arrange zoom: the canvas grows wider than its dock and scrolls -------
let vizZoom = 1;
function applyZoom(z, focusClientX) {
  const wrap = document.getElementById('viz-wrap');
  const cv = $('viz');
  const wrapRect = wrap.getBoundingClientRect();
  const fx = focusClientX ?? wrapRect.left + wrap.clientWidth / 2;
  const before = viz.geom();
  // which beat sits under the focus point right now?
  const beat = (fx - cv.getBoundingClientRect().left - before.headerW) / before.pxPerBeat;
  vizZoom = Math.min(16, Math.max(1, z));
  cv.style.width = vizZoom === 1 ? '100%' : `${Math.round(wrap.clientWidth * vizZoom)}px`;
  requestAnimationFrame(() => {
    // keep that beat under the same viewport point
    const after = viz.geom();
    wrap.scrollLeft = Math.max(0, after.headerW + beat * after.pxPerBeat - (fx - wrapRect.left));
    renderSections();
  });
}

// ---- section bar (arrange ruler): loop a section, drag its end -----------
function currentSections() {
  if (!PROJECT) return [];
  const song = (PROJECT.songs ?? []).find((f) => f.file === currentName);
  if (song?.song) return (song.song.sections ?? []).map((x) => ({ ...x, path: [] }));
  const bf = (PROJECT.blocks ?? []).find((f) => f.file === currentName);
  const b = bf?.blocks?.[bf.blocks.length - 1];
  if (b) return (b.sections ?? []).map((x) => ({ ...x, path: [b.name] }));
  return [];
}

function setSectionLoop(sec) {
  const bpb = viz.data?.beatsPerBar || 4;
  if (!sec || loopRange?.name === sec.name) {
    loopRange = null;
    node?.port.postMessage({ cmd: 'loop', start: 0, end: viz.data?.lengthBeats ?? 0 });
    status('loop: full song');
  } else {
    loopRange = { name: sec.name, start: (sec.bars[0] - 1) * bpb, end: sec.bars[1] * bpb };
    node?.port.postMessage({ cmd: 'loop', start: loopRange.start, end: loopRange.end });
    status(`loop: ${sec.name} bars(${sec.bars[0]}..${sec.bars[1]}) — click again to clear`);
  }
  renderSections();
}

let sectDrag = null; // {sec, x0, span, barsPerPx}
function renderSections() {
  const el = $('sectbar');
  if (!el) return;
  const secs = currentSections();
  el.textContent = '';
  el.style.display = secs.length ? '' : 'none';
  el.style.width = `${$('viz').clientWidth}px`;
  el.style.right = 'auto';
  if (!secs.length) return;
  const { headerW, pxPerBeat } = viz.geom();
  const bpb = viz.data?.beatsPerBar || 4;
  for (const sec of secs) {
    const d = document.createElement('div');
    d.className = 'sect' + (loopRange?.name === sec.name ? ' on' : '');
    d.textContent = sec.name;
    d.title = `${sec.name} = bars(${sec.bars[0]}..${sec.bars[1]}) — click to loop / drag the right edge to resize`;
    const left = headerW + (sec.bars[0] - 1) * bpb * pxPerBeat;
    const width = (sec.bars[1] - sec.bars[0] + 1) * bpb * pxPerBeat;
    d.style.left = `${left}px`;
    d.style.width = `${Math.max(12, width)}px`;
    d.onmousedown = (ev) => {
      const r = d.getBoundingClientRect();
      if (r.right - ev.clientX < 7) {
        sectDrag = { sec, x0: ev.clientX, el: d, w0: r.width, pxPerBar: bpb * pxPerBeat };
        ev.preventDefault();
        ev.stopPropagation();
      }
    };
    d.onclick = (ev) => {
      if (sectDrag) return;
      ev.stopPropagation();
      setSectionLoop(sec);
    };
    el.appendChild(d);
  }
}
window.addEventListener('mousemove', (ev) => {
  if (!sectDrag) return;
  const w = Math.max(sectDrag.pxPerBar, sectDrag.w0 + (ev.clientX - sectDrag.x0));
  sectDrag.el.style.width = `${w}px`;
});
window.addEventListener('mouseup', (ev) => {
  if (!sectDrag) return;
  const d = sectDrag;
  sectDrag = null;
  const dx = ev.clientX - d.x0;
  if (Math.abs(dx) < 4) return;
  const addBars = Math.round(dx / d.pxPerBar);
  const newEnd = Math.max(d.sec.bars[0], d.sec.bars[1] + addBars);
  if (newEnd === d.sec.bars[1]) {
    renderSections();
    return;
  }
  applyEdit({ op: 'set_section', path: d.sec.path, name: d.sec.name, bars: [d.sec.bars[0], newEnd] });
});

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
    renderMixer();
    renderSections();
    renderInspector().catch(() => {});
    const bpm = $('bpm');
    if (bpm && document.activeElement !== bpm) bpm.value = viz.data?.tempo ?? '';
  }
  return { ok: n === 0, diags };
}

// ---- editor (Monaco if the CDN is reachable, plain textarea otherwise) ------
const fallback = $('fallback');
let getText = () => fallback.value;
let setText = (t) => (fallback.value = t);
// GUI edits go through replaceText so Ctrl+Z undoes a fader move / clip drag
// like any typed edit (one stack — DAW-HIS-01); the plain textarea has no
// undo API, so there it falls back to a plain set.
let replaceText = (t) => setText(t);
let onChange = () => {};
// code-jump: the visualization hands us 1-based source lines
let jumpToLine = (line) => {
  const text = fallback.value;
  let idx = 0;
  for (let i = 1; i < line; i++) {
    const nl = text.indexOf('\n', idx);
    if (nl < 0) break;
    idx = nl + 1;
  }
  fallback.focus();
  fallback.setSelectionRange(idx, idx);
};
fallback.addEventListener('input', () => onChange());
window.__forteGetText = () => getText();
window.__forteCompileCheck = (src) => {
  // compile arbitrary source in the main wasm instance without touching the
  // editor (used by tests); restores the editor's project afterwards
  const r = mainCompile(src);
  mainCompile(getText());
  return r.ok;
};

let monacoAbandoned = false;
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
    // this fetch had NO timeout — a slow CDN kept the app on "loading…"
    await new Promise((res, rej) => {
      require(['vs/editor/editor.main'], res, rej);
      setTimeout(() => rej(new Error('monaco timeout')), 8000);
    });
    if (monacoAbandoned) return false; // boot moved on without us
    monaco.languages.register({ id: 'forte' });
    monaco.languages.setMonarchTokensProvider('forte', {
      tokenizer: {
        root: [
          [/\/\/.*/, 'comment'],
          [/\b(song|track|return|section|let|instrument|insert|play|at|send|volume|pan|tempo|meter|key|bars|automate|modulate|from|to|over|with)\b/, 'keyword'],
          [/\b(chords|arp|bass|sampler|prisma|mesh|filter|eq|drive|delay|reverb|beat|notes|prog)\b/, 'type'],
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
    window.__forteUndo = () => {
      ed.focus();
      ed.trigger('toolbar', 'undo', null);
    };
    replaceText = (t) => {
      const model = ed.getModel();
      ed.pushUndoStop();
      model.pushEditOperations([], [{ range: model.getFullModelRange(), text: t }], () => null);
      ed.pushUndoStop();
    };
    jumpToLine = (line) => {
      ed.revealLineInCenter(line);
      ed.setPosition({ lineNumber: line, column: 1 });
      ed.focus();
    };
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
// The worklet scope has no TextEncoder, so all text crosses the port as bytes.
let loopRange = null; // {name, start, end} in beats — engine-side, not code
function encodeSrc() {
  const enc = new TextEncoder();
  return {
    cmd: 'src',
    text: enc.encode(getText()),
    modules: enc.encode(modulesJson),
    assets: enc.encode(assetsJson),
    base: enc.encode(currentBase()),
    loop: loopRange ? { start: loopRange.start, end: loopRange.end } : null,
  };
}
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
    if (m.kind === 'err') {
      status(`worklet error: ${m.message}`);
      return;
    }
    if (m.kind === 'compiled' && m.diagCount > 0) {
      status(`worklet compile failed: ${m.diagCount} diagnostics`);
      return;
    }
    if (m.kind === 'pos') {
      const bpb = viz.data?.beatsPerBar || 4;
      status(`bar ${Math.floor(m.beats / bpb) + 1}.${Math.floor(m.beats % bpb) + 1} | peak ${m.peak.toFixed(2)}`);
      if (m.peaks) {
        viz.setPeaks(m.peaks);
        const strips = $('mixer')?.children ?? [];
        const tracks = viz.data?.tracks ?? [];
        for (const strip of strips) {
          const i = tracks.findIndex((t) => t.name === strip.dataset.track);
          const bar = strip.querySelector('.meter i');
          if (bar && i >= 0) bar.style.width = `${Math.min(1, m.peaks[i] ?? 0) * 100}%`;
        }
      }
      viz.setPlayhead(m.beats);
      if (vizZoom > 1) {
        // keep the playhead visible while zoomed in
        const wrap = document.getElementById('viz-wrap');
        const { headerW, pxPerBeat } = viz.geom();
        const x = headerW + m.beats * pxPerBeat;
        if (x < wrap.scrollLeft + headerW || x > wrap.scrollLeft + wrap.clientWidth - 40) {
          wrap.scrollLeft = Math.max(0, x - headerW - 20);
        }
      }
    }
  };
  node.port.postMessage(encodeSrc());
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
  $('rec').textContent = '■ Stop rec';
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
    sig: 'webcrypto:stub', // real device keys arrive with signed releases
    // measured round-trip latency travels with the take, so any consumer can
    // compensate placement (SRS-REC-004)
    ...(calib ? { latency_samples: calib.rtl_samples, latency_confidence: calib.confidence } : {}),
  };
  const name = await saveTake(pcm, rate, provenance);
  await store.remove('assets/.recording.pcm').catch(() => {});
  await store.remove('assets/.recording.json').catch(() => {});
  status(`saved ${name} (${(pcm.length / rate).toFixed(1)}s)`);
  // performance fork, closed: one tap drops the take into the song
  if (confirm(`Insert take ${name} into this song? (adds an import + a Voice track)`)) {
    insertTake(name);
  }
}

/// Append `import take from …` and a Voice track playing it — the textual
/// equivalent of dragging a take onto the timeline.
function insertTake(path) {
  const ident = path.split('/').pop().replace('.frec', '').replace(/-/g, '');
  let text = getText();
  if (!text.includes(`from "./${path}"`)) {
    text = `import ${ident} from "./${path}"\n` + text;
  }
  const end = text.lastIndexOf('}');
  if (end >= 0) {
    const track = `\n  track Voice_${ident} {\n    audio ${ident} at bars(1..4)\n  }\n`;
    text = text.slice(0, end) + track + text.slice(end);
  }
  setText(text);
  autosave();
  recompile(0);
  document.body.dataset.takeInserted = path;
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
      status(`recovered a recording from the last crash: ${name}`);
    }
  } finally {
    await store.remove('assets/.recording.pcm').catch(() => {});
    await store.remove('assets/.recording.json').catch(() => {});
  }
}

// ---- files (OPFS, local-first) ----------------------------------------------
let store = null;
// project mode (forte daw): the inventory of the opened package, or null
// when running local-first on OPFS (forte browser / hosted).
let PROJECT = null;
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
  if (!PROJECT) for (const n of BUILTINS) if (!locals.includes(n)) add(n, `demo: ${n}`);
  sel.value = currentName;
  await refreshTree(locals);
}

// ---- file tree (the project explorer on the left) -----------------------------
// Nested paths render as indented rows under their directory; the current
// file is highlighted; .frec takes are listed but not openable (audio).
async function refreshTree(locals) {
  const el = $('tree');
  if (!el) return;
  locals = locals ?? (await localNames());
  const assets = store ? await store.list('.frec') : [];
  el.textContent = '';
  const section = (label) => {
    const d = document.createElement('div');
    d.className = 'sec';
    d.textContent = label;
    el.appendChild(d);
  };
  const INDENT = 12;
  const fileRow = (path, { asset = false, demo = false } = {}) => {
    const depth = path.split('/').length - 1;
    const d = document.createElement('div');
    d.className = 'f' + (!asset && path === currentName ? ' cur' : '') + (asset ? ' asset' : '');
    d.dataset.file = path;
    d.style.paddingLeft = `${12 + (depth + (asset ? 0 : 0)) * INDENT}px`;
    d.textContent = `${asset ? '📼' : demo ? '♪' : '●'} ${path.split('/').pop()}`;
    d.title = path;
    if (!asset) d.onclick = () => loadSong(path);
    el.appendChild(d);
  };
  const renderPaths = (paths, opts) => {
    let lastDirs = [];
    for (const p of paths) {
      const dirs = p.split('/').slice(0, -1);
      for (let i = 0; i < dirs.length; i++) {
        if (lastDirs[i] !== dirs[i]) {
          const d = document.createElement('div');
          d.className = 'dir';
          d.style.paddingLeft = `${12 + i * INDENT}px`;
          d.textContent = `${dirs[i]}/`;
          el.appendChild(d);
          lastDirs = lastDirs.slice(0, i); // deeper levels are new too
        }
      }
      lastDirs = dirs;
      fileRow(p, opts);
    }
  };
  if (PROJECT) {
    section(`package: ${PROJECT.name}${PROJECT.version ? ' ' + PROJECT.version : ''}`);
    const row = document.createElement('div');
    row.className = 'projbtns';
    const btn = (label, title, fn) => {
      const b = document.createElement('button');
      b.textContent = label;
      b.title = title;
      b.onclick = fn;
      row.appendChild(b);
    };
    btn('+song', 'Create a new song in songs/', () => newElement('song'));
    btn('+block', 'Create a new block in blocks/', () => newElement('block'));
    btn('+package', 'Vendor another package (forte package add)', addPackage);
    el.appendChild(row);
    await store.refresh?.();
    if (store.project) PROJECT = store.project;
    const libBlocks = (PROJECT.blocks || []).flatMap((f) =>
      (f.blocks || []).map((b) => ({ ...b, file: f.file }))
    );
    for (const pkg of PROJECT.packages ?? [])
      for (const f of pkg.blocks ?? [])
        for (const b of f.blocks ?? []) libBlocks.push({ ...b, file: f.file, pkg: pkg.name });
    if (libBlocks.length) {
      section('blocks');
      for (const b of libBlocks) {
        const d = document.createElement('div');
        d.className = 'f blk';
        d.draggable = true;
        d.ondragstart = (ev) => {
          ev.dataTransfer.setData('text/forte-block', JSON.stringify(b));
          ev.dataTransfer.effectAllowed = 'copy';
        };
        const label = document.createElement('span');
        label.textContent = `❐ ${b.name}${b.pkg ? ' ·' + b.pkg.split('_')[0] : ''}`;
        label.title = `${b.file}:${b.line} (${b.bars ?? '?'} bars) — click to open / drag onto the arrange to place`;
        label.onclick = () => loadSong(b.file);
        d.appendChild(label);
        const bbtn = (t, title, fn) => {
          const x = document.createElement('button');
          x.textContent = t;
          x.title = title;
          x.onclick = (e) => {
            e.stopPropagation();
            fn();
          };
          d.appendChild(x);
        };
        bbtn('▶', 'Audition this block standalone', () => auditionBlock(b));
        bbtn('+song', 'Import into the open song and place it', () => placeBlock(b));
        el.appendChild(d);
      }
    }
    section('instruments');
    const instRows = [];
    const filt = document.createElement('input');
    filt.className = 'palfilt';
    filt.placeholder = 'search (e.g. bass)';
    filt.value = window.__paletteFilter ?? '';
    filt.oninput = () => {
      window.__paletteFilter = filt.value;
      const q = filt.value.trim().toLowerCase();
      for (const { row, key } of instRows) row.style.display = !q || key.includes(q) ? '' : 'none';
    };
    el.appendChild(filt);
    for (const inst of paletteInstruments()) {
      const d = document.createElement('div');
      d.className = 'f blk';
      const label = document.createElement('span');
      label.textContent = `♪ ${inst.label}`;
      label.title = `${inst.call}${inst.where ? `(${inst.where})` : ''}`;
      d.appendChild(label);
      const bb = (t, title, fn) => {
        const b = document.createElement('button');
        b.textContent = t;
        b.title = title;
        b.onclick = (e) => {
          e.stopPropagation();
          fn();
        };
        d.appendChild(b);
      };
      bb('▶', 'Preview one bar', () => previewInstrument(inst).catch((e) => status(`preview: ${e.message}`)));
      bb('+tr', 'Add a track with this instrument to the open song / block', () => addTrackFromPalette(inst));
      instRows.push({ row: d, key: `${inst.label} ${inst.where ?? ''}`.toLowerCase() });
      el.appendChild(d);
    }
    {
      const q = (window.__paletteFilter ?? '').trim().toLowerCase();
      if (q) for (const { row, key } of instRows) row.style.display = key.includes(q) ? '' : 'none';
    }
    if (!(PROJECT.packages ?? []).length) {
      const d = document.createElement('div');
      d.className = 'f blk';
      const label = document.createElement('span');
      label.textContent = '📦 more instruments';
      label.title = "Vendor forte's starter package (essentials: 303s, junos, 909s, …)";
      d.appendChild(label);
      const b = document.createElement('button');
      b.textContent = '+';
      b.onclick = async (e) => {
        e.stopPropagation();
        const starters = await (await fetch('api/starters')).json().catch(() => []);
        if (!starters.length) return status('no starter packages found');
        status(`package add: ${starters[0].name}…`);
        const r = await fetch(`api/pkg?spec=${encodeURIComponent(starters[0].spec)}`, { method: 'POST' });
        if (r.ok) toast(`vendored ${starters[0].name} — the palette just grew`, 'ok');
        else toast(`pkg: ${(await r.text()).slice(0, 160)}`, 'err');
        await store.refresh?.();
        await refreshModules();
        await refreshFileList();
      };
      d.appendChild(b);
      el.appendChild(d);
    }
  }
  if (locals.length || assets.length) {
    section(PROJECT ? 'project' : 'local');
    renderPaths([...locals, ...assets].sort(), {});
    // assets got the local marker: restyle them
    for (const a of assets) {
      const row = el.querySelector(`[data-file="${CSS.escape(a)}"]`);
      if (row) {
        row.className = 'f asset';
        row.textContent = `📼 ${a.split('/').pop()}`;
        row.onclick = null;
      }
    }
  }
  const demos = PROJECT ? [] : BUILTINS.filter((n) => !locals.includes(n));
  if (demos.length) {
    section('demo');
    renderPaths(demos, { demo: true });
  }
  document.body.dataset.treeFiles = String(el.querySelectorAll('.f').length);
}

async function loadSong(name) {
  currentName = name;
  localStorage.setItem('forte.last', name);
  const locals = await localNames();
  let text;
  if (locals.includes(name) || PROJECT) {
    text = await store.read(name);
  } else {
    text = await (await fetch(`../../packages/essentials_0.6.0/songs/${name}`)).text();
  }
  setText(text);
  recompile(0);
  refreshFileList(); // dropdown value + tree highlight follow the open file
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

// ---- project gestures (forte daw): scaffold and vendor into the package ------
function uniqueName(base, taken) {
  let n = 1;
  while (taken.includes(n > 1 ? `${base}${n}` : base)) n++;
  return n > 1 ? `${base}${n}` : base;
}
async function newElement(kind) {
  const files = (await localNames()).map((f) => f.split('/').pop().replace('.forte', ''));
  const def = uniqueName(kind === 'block' ? 'Groove' : 'song', files);
  const name = prompt(kind === 'block' ? 'New block name:' : 'New song name:', def);
  if (!name) return;
  const r = await fetch(`api/new?kind=${kind}&name=${encodeURIComponent(name.trim())}`, { method: 'POST' });
  const t = await r.text();
  if (!r.ok) {
    status(`new: ${t}`);
    return;
  }
  await refreshModules();
  await refreshFileList();
  loadSong(JSON.parse(t).file);
}

async function addPackage() {
  const spec = prompt('Package to add (e.g. github:owner/repo or a local path)');
  if (!spec) return;
  status('package add… (cloning)');
  const r = await fetch(`api/pkg?spec=${encodeURIComponent(spec.trim())}`, { method: 'POST' });
  const t = await r.text();
  status(r.ok ? 'package added' : `pkg: ${t.slice(0, 200)}`);
  await store.refresh?.();
  await refreshModules();
  await refreshFileList();
}

// relative path from the open file's directory to another project file
function relPath(fromFile, toFile) {
  const a = fromFile.split('/').slice(0, -1);
  const b = toFile.split('/');
  while (a.length && b.length > 1 && a[0] === b[0]) {
    a.shift();
    b.shift();
  }
  return [...a.map(() => '..'), ...b].join('/');
}

// audition a block standalone: open its file (a block library compiles with
// its last block as the root) and start playback
async function auditionBlock(b) {
  await loadSong(b.file);
  await ensureAudio();
  await ac.resume();
  setTimeout(() => node?.port.postMessage({ cmd: 'play' }), 200);
}

// the library gesture: import the block into the OPEN SONG and place it —
// at `startBar` when given (drag & drop), else after the last used bar
function placeBlock(b, startBar) {
  if (!currentName.startsWith('songs/')) {
    status('placements go into a song — open one under songs/ first');
    return;
  }
  const beatsPerBar = viz.data?.beatsPerBar || 4;
  const usedBars = Math.round((viz.data?.lengthBeats || 0) / beatsPerBar);
  const start = startBar ?? usedBars + 1;
  const len = Math.max(1, b.bars || 4);
  const ops = [];
  if (b.file !== currentName) {
    ops.push({ op: 'add_import', names: [b.name], from: relPath(currentName, b.file) });
  }
  ops.push({ op: 'add_place', block: b.name, bars: [start, start + len - 1] });
  if (applyEdit(ops)) {
    toast(`placed ${b.name} at bars(${start}..${start + len - 1})`, 'ok');
    status(`placed: ${b.name} at bars(${start}..${start + len - 1})`);
    // the code opens on the new placement (D-15: drop lands you in the code)
    const line = getText().split('\n').findIndex((l) => l.includes(`play ${b.name} `) || l.trim().startsWith(`play ${b.name}`));
    if (line >= 0) jumpToLine(line + 1);
  }
}
$('viz').addEventListener('dragover', (ev) => {
  if ([...ev.dataTransfer.types].includes('text/forte-block')) ev.preventDefault();
});
$('viz').addEventListener('drop', (ev) => {
  const data = ev.dataTransfer.getData('text/forte-block');
  if (!data) return;
  ev.preventDefault();
  const b = JSON.parse(data);
  const rect = $('viz').getBoundingClientRect();
  const hit = viz.hitTest(ev.clientX - rect.left, ev.clientY - rect.top);
  if (hit?.kind === 'clip' && hit.line > 0) {
    // dropping onto a clip swaps WHICH block that placement plays.
    // the swap goes FIRST: add_import may insert a line, which would
    // shift the line this op addresses (ops apply sequentially)
    const ops = [{ op: 'set_place_block', line: hit.line, to: b.name }];
    if (b.file !== currentName) {
      ops.push({ op: 'add_import', names: [b.name], from: relPath(currentName, b.file) });
    }
    if (applyEdit(ops)) {
      toast(`this placement now plays ${b.name}`, 'ok');
      status(`swap: this placement now plays ${b.name}`);
      jumpToLine(hit.line);
    }
    return;
  }
  const { headerW, pxPerBeat } = viz.geom();
  const bpb = viz.data?.beatsPerBar || 4;
  const bar = Math.max(1, Math.round((ev.clientX - rect.left - headerW) / pxPerBeat / bpb) + 1);
  placeBlock(b, bar);
});
// right-click a clip = delete its placement (the arrange's remove gesture)
$('viz').addEventListener('contextmenu', (ev) => {
  const rect = $('viz').getBoundingClientRect();
  const hit = viz.hitTest(ev.clientX - rect.left, ev.clientY - rect.top);
  if (hit?.kind !== 'clip' || !(hit.line > 0)) return;
  ev.preventDefault();
  if (applyEdit({ op: 'remove_at_line', line: hit.line })) {
    toast('clip deleted — Ctrl+Z to undo', 'ok');
  }
});

// ---- instrument palette: built-ins + every device in the package ----------
const BUILTIN_INSTRUMENTS = [
  { label: 'Kick', call: 'sampler(sample: "Kick")', kind: 'beat', pat: 'x... x... x... x...' },
  { label: 'Snare', call: 'sampler(sample: "Snare")', kind: 'beat', pat: '.... x... .... x...' },
  { label: 'Hat', call: 'sampler(sample: "Hat")', kind: 'beat', pat: '..x. ..x. ..x. ..x.' },
  {
    label: 'Prisma',
    call: 'prisma(wave: "saw", cutoff: 0.4, sub: 0.5)',
    kind: 'notes',
    pat: 'C3:0.5 _:0.5 Eb3:0.5 _:0.5 G3:0.5 _:0.5 Eb3:0.5 _:0.5',
  },
  { label: 'Mesh', call: 'mesh()', kind: 'notes', pat: '[C3 Eb3 G3]:2 [Ab2 C3 Eb3]:2' },
];
const DEFAULT_NOTES_PAT = 'C3:0.5 _:0.5 Eb3:0.5 _:0.5 G3:0.5 _:0.5 Eb3:0.5 _:0.5';

function paletteInstruments() {
  const out = BUILTIN_INSTRUMENTS.map((b) => ({ ...b, where: 'built-in' }));
  const push = (file, d, where) => {
    if (d.kind !== 'Instrument') return;
    out.push({
      label: d.name,
      name: d.name,
      from: file,
      call: `${d.name}()`,
      kind: 'notes',
      pat: DEFAULT_NOTES_PAT,
      where,
    });
  };
  for (const f of PROJECT?.instruments ?? []) for (const d of f.devices ?? []) push(f.file, d, f.file);
  for (const pkg of PROJECT?.packages ?? [])
    for (const f of pkg.instruments ?? []) for (const d of f.devices ?? []) push(f.file, d, pkg.name);
  return out;
}

// preview: offline-render one bar of the instrument in the MAIN wasm
// instance (it has its own engine), then restore the user's project
let previewAc = null;
async function previewInstrument(inst) {
  const imp = inst.from ? `import { ${inst.name} } from "${inst.from}"\n` : '';
  const src = `${imp}song "prev" {\n  tempo 120bpm\n  track P {\n    instrument ${inst.call}\n    play ${inst.kind}\`${inst.pat}\` at bars(1..1)\n  }\n}`;
  // compile the preview with the project's module map, from the project root
  stage(main, modulesJson, main.e.fw_modules_commit);
  stage(main, assetsJson, main.e.fw_assets_commit);
  stage(main, '', main.e.fw_base_commit);
  const bytes = new TextEncoder().encode(src);
  const ptr = main.e.fw_src_prepare(main.ctx, bytes.length);
  new Uint8Array(main.e.memory.buffer, ptr, bytes.length).set(bytes);
  if (main.e.fw_compile(main.ctx) !== 0) {
    mainCompile(getText());
    status(`preview: ${inst.label} does not compile`);
    return;
  }
  const rate = 48000;
  const frames = rate * 2; // one bar at 120bpm
  const L = new Float32Array(frames);
  const R = new Float32Array(frames);
  main.e.fw_play(main.ctx);
  for (let i = 0; i < frames; i += 128) {
    const n = Math.min(128, frames - i);
    main.e.fw_process(main.ctx, n);
    L.set(new Float32Array(main.e.memory.buffer, main.e.fw_out_l(main.ctx), n), i);
    R.set(new Float32Array(main.e.memory.buffer, main.e.fw_out_r(main.ctx), n), i);
  }
  main.e.fw_stop(main.ctx);
  mainCompile(getText()); // the user's project comes back
  previewAc ??= new AudioContext({ sampleRate: rate });
  await previewAc.resume();
  const buf = previewAc.createBuffer(2, frames, rate);
  buf.copyToChannel(L, 0);
  buf.copyToChannel(R, 1);
  const srcNode = previewAc.createBufferSource();
  srcNode.buffer = buf;
  srcNode.connect(previewAc.destination);
  srcNode.start();
  status(`preview: ${inst.label}`);
}

// the palette gesture: a new track with this instrument + a starter pattern
function addTrackFromPalette(inst) {
  if (PROJECT && !currentName.startsWith('songs/') && !currentName.startsWith('blocks/')) {
    status('open a song or block first (the tree on the left, or +song / +block)');
    return;
  }
  const text = getText();
  const base = (inst.label || 'Track').replace(/[^A-Za-z0-9_]/g, '') || 'Track';
  let n = 1;
  while (new RegExp('track\\s+' + base + (n > 1 ? n : '') + '\\b').test(text)) n++;
  const name = base + (n > 1 ? n : '');
  const ops = [];
  if (inst.from) ops.push({ op: 'add_import', names: [inst.name], from: relPath(currentName, inst.from) });
  ops.push({
    op: 'add_track',
    name,
    instrument: inst.call,
    play: inst.kind + '`' + inst.pat + '` at bars(1..4)',
  });
  if (applyEdit(ops)) toast(`+ track ${name} (${inst.label}) — the grid / roll is ready`, 'ok');
}

// ---- history: the .forte repository lives in the browser too -----------------
// Commit snapshots every local file; the diff between any commit and the
// working tree is computed by the wasm compiler — in music vocabulary.
let vcs = null;

async function workingSnapshot() {
  const snap = {};
  if (store) {
    for (const name of await store.list()) snap[name] = await store.read(name);
  }
  snap[currentName] = getText(); // the buffer wins over the last autosave
  return snap;
}

function semdiff(oldSnap, newSnap) {
  stage(main, JSON.stringify({ old: oldSnap, new: newSnap }), main.e.fw_semdiff);
  const p = main.e.fw_semdiff_ptr(main.ctx);
  const l = main.e.fw_semdiff_len(main.ctx);
  return new TextDecoder().decode(new Uint8Array(main.e.memory.buffer, p, l));
}

async function refreshVcsLog() {
  if (!vcs) return;
  const log = await vcs.log();
  const el = $('vcs-log');
  el.textContent = '';
  for (const c of log) {
    const row = document.createElement('div');
    row.className = 'commit';
    const label = document.createElement('b');
    label.textContent = `#${c.n} ${c.message}`;
    label.title = c.hash;
    const diff = document.createElement('a');
    diff.textContent = 'diff';
    diff.title = 'Diff between this commit and the working copy — in music vocabulary';
    diff.onclick = async () => {
      const report = semdiff(await vcs.snapshotOf(c.hash), await workingSnapshot());
      const pre = $('vcs-diff');
      pre.hidden = false;
      pre.textContent = `#${c.n} → now\n${report}`;
    };
    const restore = document.createElement('a');
    restore.textContent = 'restore';
    restore.title = 'Restore this commit\'s files into the working copy';
    restore.onclick = async () => {
      if (!confirm(`Restore #${c.n} "${c.message}"? (uncommitted changes will be lost)`)) return;
      const snap = await vcs.snapshotOf(c.hash);
      for (const [path, text] of Object.entries(snap)) await store.write(path, text);
      await refreshModules();
      await refreshFileList();
      if (snap[currentName] !== undefined) setText(snap[currentName]);
      recompile(0);
      status(`restored #${c.n}`);
    };
    row.append(label, diff, restore);
    el.append(row);
  }
  document.body.dataset.commits = String(log.length);
}

// The working tree is clean when every committed file matches the store (with
// the open buffer standing in for its file) — same rule as the CLI's is_clean.
async function workingIsClean() {
  const head = await vcs.head();
  if (!head) return true;
  const committed = await vcs.snapshotOf(head);
  const work = await workingSnapshot();
  const kc = Object.keys(committed).sort();
  const kw = Object.keys(work).sort();
  return kc.join(' ') === kw.join(' ') && kc.every((k) => committed[k] === work[k]);
}

/// Make a snapshot THE working tree: tracked files not in it are removed,
/// the rest written, the editor follows (checkout / merge results land here).
async function applySnapshotToWorkingTree(snap) {
  for (const name of await store.list()) {
    if (!(name in snap)) await store.remove(name).catch(() => {});
  }
  for (const [path, text] of Object.entries(snap)) await store.write(path, text);
  await refreshModules();
  if (snap[currentName] !== undefined) {
    setText(snap[currentName]);
  } else {
    const first = Object.keys(snap).find((p) => p.endsWith('.forte'));
    if (first) {
      currentName = first;
      localStorage.setItem('forte.last', first);
      setText(snap[first]);
    }
  }
  await refreshFileList();
  recompile(0);
}

async function refreshGitBar() {
  if (!vcs) return;
  const cur = await vcs.headRef();
  const branches = await vcs.branches();
  const bsel = $('branch');
  bsel.innerHTML = '';
  for (const b of branches.length ? branches : [{ name: 'main' }]) {
    const o = document.createElement('option');
    o.value = b.name;
    o.textContent = b.name;
    bsel.appendChild(o);
  }
  bsel.value = cur ?? 'main';
  const msel = $('merge-from');
  msel.innerHTML = '';
  for (const b of branches.filter((b) => b.name !== cur)) {
    const o = document.createElement('option');
    o.value = b.name;
    o.textContent = b.name;
    msel.appendChild(o);
  }
  msel.disabled = $('merge').disabled = msel.options.length === 0;
  $('git-state').textContent = (await vcs.mergeHead())
    ? ' merging… (fix conflicts, then Commit)'
    : '';
  document.body.dataset.branch = cur ?? '';
}

async function initVcs() {
  if (!store) return;
  try {
    vcs = await new Vcs().init();
    await refreshVcsLog();
    await refreshGitBar();
  } catch {
    vcs = null; // OPFS unavailable: the panel stays empty
  }
  $('commit').onclick = async () => {
    if (!vcs) return;
    const msg = $('commit-msg').value.trim() || `edit ${currentName}`;
    try {
      await store.write(currentName, getText()); // commit what you hear
      const { n } = await vcs.commit(msg, await workingSnapshot());
      $('commit-msg').value = '';
      $('vcs-diff').hidden = true;
      await refreshVcsLog();
      await refreshGitBar();
      status(`commit #${n}: ${msg}`);
    } catch (e) {
      status(e.message);
    }
  };
  $('branch-new').onclick = async () => {
    if (!vcs) return;
    const name = (prompt('New branch name') || '').trim();
    if (!name) return;
    try {
      await vcs.createBranch(name); // = checkout -b: same tree, new ref
      await refreshGitBar();
      await refreshVcsLog();
      status(`created and switched to branch ${name}`);
    } catch (e) {
      status(e.message);
      await refreshGitBar();
    }
  };
  $('branch').onchange = async (e) => {
    if (!vcs) return;
    const target = e.target.value;
    if (target === (await vcs.headRef())) return;
    try {
      await store.write(currentName, getText()); // the buffer is part of the tree
      if (!(await workingIsClean())) {
        throw new Error('uncommitted changes — commit before switching');
      }
      const snap = await vcs.checkout(target);
      await applySnapshotToWorkingTree(snap);
      await refreshGitBar();
      await refreshVcsLog();
      status(`checkout: ${target}`);
    } catch (err) {
      status(err.message);
      await refreshGitBar();
    }
  };
  $('merge').onclick = async () => {
    if (!vcs) return;
    const from = $('merge-from').value;
    if (!from) return;
    try {
      await store.write(currentName, getText());
      if (!(await workingIsClean())) {
        throw new Error('uncommitted changes — commit before merging');
      }
      const r = await vcs.merge(from);
      await applySnapshotToWorkingTree(r.snapshot);
      await refreshGitBar();
      await refreshVcsLog();
      if (r.kind === 'conflict') {
        status(`conflicts — fix the <<<<<<< markers, then Commit: ${r.conflicts.join(' / ')}`);
      } else if (r.kind === 'fast-forward') {
        status(`fast-forward: merged ${from}`);
      } else {
        status(`merged ${from}`);
      }
    } catch (err) {
      status(err.message);
      await refreshGitBar();
    }
  };
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
      status('nothing played');
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
    status('transcribed — copy it from the panel below into a play statement');
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
  $('perform').textContent = '■ End perform';
  status('perform mode: A–K are white keys, W/E/T/Y/U black keys (MIDI keyboards work too)');
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
    status('calibration: could not detect the probe tone (check the speaker → mic path)');
    return;
  }
  const conf = main.e.fw_calib_confidence(main.ctx);
  const playedAtFrame = Math.round(startAt * 48000) - firstFrame;
  const rtl = lag - playedAtFrame;
  localStorage.setItem(
    'forte.calibration',
    JSON.stringify({ rtl_samples: rtl, rate: 48000, confidence: conf, at: new Date().toISOString() })
  );
  status(`calibrated: round trip ${((rtl / 48000) * 1000).toFixed(1)}ms (confidence ${conf.toFixed(2)}) — recorded on future takes`);
}

// ---- beat grid: the first GUI projection over the code (Studio P0, #135) -----
// The grid renders `beat` literals as clickable step rows and writes each
// click back through the wasm edit layer (fw_edit → fortelang::edit): the
// diff touches only the literal's contents, comments and layout survive.
function wasmText(p, l) {
  return new TextDecoder().decode(new Uint8Array(main.e.memory.buffer, p, l));
}
function stageSrc(text) {
  const bytes = new TextEncoder().encode(text);
  const ptr = main.e.fw_src_prepare(main.ctx, bytes.length);
  new Uint8Array(main.e.memory.buffer, ptr, bytes.length).set(bytes);
}
function stageJson(json) {
  const bytes = new TextEncoder().encode(json);
  const ptr = main.e.fw_modules_prepare(main.ctx, bytes.length);
  new Uint8Array(main.e.memory.buffer, ptr, bytes.length).set(bytes);
}

function applyEdit(op) {
  stageSrc(getText());
  stageJson(JSON.stringify(op));
  const r = main.e.fw_edit(main.ctx);
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  if (r !== 0) {
    status(`edit: ${out}`);
    toast(out, 'err');
    return false;
  }
  replaceText(out); // one undoable step; Monaco fires onChange → autosave + recompile
  autosave(); // the plain-textarea fallback does not
  recompile(0);
  return true;
}

// one step per hit/rest char, keeping `*N` ratchet suffixes attached
function parseBeatSteps(raw) {
  const s = raw.replace(/\s+/g, '');
  const out = [];
  for (let i = 0; i < s.length; i++) {
    let t = s[i];
    if (s[i + 1] === '*') {
      let j = i + 2, d = '';
      while (j < s.length && /\d/.test(s[j])) d += s[j++];
      t += `*${d}`;
      i = j - 1;
    }
    out.push(t);
  }
  return out;
}
const STEP_CLASS = { X: 'acc', x: 'hit', '.': 'ghost', '-': 'rest' };
const STEP_NEXT = { '-': 'x', x: 'X', X: '.', '.': '-' };
function cycleStep(t) {
  const head = STEP_NEXT[t[0]] ?? 'x';
  return head === '-' ? '-' : head + t.slice(1); // rests cannot ratchet
}
function joinSteps(steps) {
  if (steps.length % 4 !== 0) return steps.join('');
  const groups = [];
  for (let i = 0; i < steps.length; i += 4) groups.push(steps.slice(i, i + 4).join(''));
  return groups.join(' ');
}

// ---- piano roll (NOTE-01): notes literals become editable rolls ----------
function stageBytes(bytes) {
  const ptr = main.e.fw_modules_prepare(main.ctx, bytes.length);
  new Uint8Array(main.e.memory.buffer, ptr, bytes.length).set(bytes);
}
function notesParse(raw) {
  stageBytes(new TextEncoder().encode(raw));
  const n = main.e.fw_notes_parse(main.ctx);
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  return n < 0 ? null : JSON.parse(out);
}
function notesWrite(doc) {
  stageBytes(new TextEncoder().encode(JSON.stringify(doc)));
  const r = main.e.fw_notes_write(main.ctx);
  const out = wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx));
  if (r !== 0) {
    status(`roll: ${out}`);
    return null;
  }
  return out;
}
const ROLL_Q = 0.25; // grid quantum in beats
function renderRoll(el, site) {
  const doc = notesParse(site.raw);
  if (!doc) return;
  const row = document.createElement('div');
  row.className = 'grid-row roll-row';
  const label = document.createElement('span');
  label.className = 'grid-label';
  const where = site.path.length ? `${site.path.join('/')} · ` : '';
  label.textContent = site.let_name
    ? `${where}let ${site.let_name}`
    : `${where}${site.track}${site.at ? ` @${site.at}` : ''}`;
  label.title = `${label.textContent} (click → line ${site.line} / drag to draw a note, click a note to delete)`;
  label.onclick = () => jumpToLine(site.line);
  row.appendChild(label);

  const pitches = doc.notes.map((n) => n.pitch);
  const lo = Math.min(...(pitches.length ? pitches : [60])) - 4;
  const hi = Math.max(...(pitches.length ? pitches : [60])) + 4;
  const cols = Math.max(1, Math.round(doc.len / ROLL_Q));
  const CW = 14, RH = 8;
  const cv = document.createElement('canvas');
  const W = cols * CW + 1, H = (hi - lo + 1) * RH + 1;
  cv.width = W * devicePixelRatio;
  cv.height = H * devicePixelRatio;
  cv.style.width = `${W}px`;
  cv.style.height = `${H}px`;
  cv.className = 'roll';
  const g = cv.getContext('2d');
  g.scale(devicePixelRatio, devicePixelRatio);
  const bpb = viz.data?.beatsPerBar || 4;
  const draw = (ghost) => {
    g.clearRect(0, 0, W, H);
    g.fillStyle = '#14171c';
    g.fillRect(0, 0, W, H);
    for (let p = lo; p <= hi; p++) {
      if (p % 12 === 0) {
        g.fillStyle = '#1b1f26';
        g.fillRect(0, (hi - p) * RH, W, RH);
      }
    }
    for (let c = 0; c <= cols; c++) {
      const beats = c * ROLL_Q;
      g.strokeStyle = beats % bpb === 0 ? '#333a45' : beats % 1 === 0 ? '#232830' : '#1b1f26';
      g.beginPath();
      g.moveTo(c * CW + 0.5, 0);
      g.lineTo(c * CW + 0.5, H);
      g.stroke();
    }
    for (const n of doc.notes) {
      g.fillStyle = n.accent ? '#ffd479' : '#58a6ff';
      g.fillRect((n.start / ROLL_Q) * CW + 1, (hi - n.pitch) * RH + 1, (n.dur / ROLL_Q) * CW - 2, RH - 2);
    }
    if (ghost) {
      g.fillStyle = 'rgba(88,196,112,0.5)';
      g.fillRect((ghost.start / ROLL_Q) * CW + 1, (hi - ghost.pitch) * RH + 1, (ghost.dur / ROLL_Q) * CW - 2, RH - 2);
    }
  };
  draw();

  const cellAt = (ev) => {
    const r = cv.getBoundingClientRect();
    const col = Math.max(0, Math.min(cols - 1, Math.floor((ev.clientX - r.left) / CW)));
    const pitch = hi - Math.max(0, Math.min(hi - lo, Math.floor((ev.clientY - r.top) / RH)));
    return { t: col * ROLL_Q, pitch };
  };
  const noteAt = (t, pitch) =>
    doc.notes.find((n) => n.pitch === pitch && t >= n.start - 1e-6 && t < n.start + n.dur - 1e-6);
  const commit = () => {
    const value = notesWrite(doc);
    if (value === null) return false;
    const op = { op: 'set_pattern', path: site.path, value };
    if (site.let_name) op.let_name = site.let_name;
    else Object.assign(op, { track: site.track, play: site.play });
    return applyEdit(op);
  };
  let drag = null; // {t0, pitch, note, moved}
  cv.onmousedown = (ev) => {
    const { t, pitch } = cellAt(ev);
    drag = { t0: t, pitch, note: noteAt(t, pitch) ?? null, moved: false };
    ev.preventDefault();
  };
  cv.onmousemove = (ev) => {
    if (!drag) return;
    const { t, pitch } = cellAt(ev);
    if (t !== drag.t0 || pitch !== drag.pitch) drag.moved = true;
    if (drag.note) {
      // dragging a note carries it (start + pitch), duration preserved
      const start = Math.max(0, drag.note.start + (t - drag.t0));
      drag.carry = { start, pitch, dur: drag.note.dur };
      draw(drag.carry);
      return;
    }
    const start = Math.min(drag.t0, t);
    const dur = Math.abs(t - drag.t0) + ROLL_Q;
    draw({ start, dur, pitch: drag.pitch });
  };
  const finish = (ev) => {
    if (!drag) return;
    const d = drag;
    drag = null;
    const { t } = cellAt(ev);
    const hitNote = d.note;
    if (d.moved && hitNote && d.carry) {
      // move gesture: drop the note at the carried position
      const clash = doc.notes.some(
        (n) =>
          n !== hitNote &&
          d.carry.start < n.start + n.dur - 1e-6 &&
          n.start < d.carry.start + d.carry.dur - 1e-6 &&
          !(Math.abs(n.start - d.carry.start) < 1e-6 && Math.abs(n.dur - d.carry.dur) < 1e-6)
      );
      if (clash) {
        status('roll: the drop overlaps an existing note');
        draw();
        return;
      }
      hitNote.start = d.carry.start;
      hitNote.pitch = d.carry.pitch;
      if (!commit()) draw();
      return;
    }
    if (!d.moved && hitNote) {
      if (ev.shiftKey) {
        hitNote.accent = !hitNote.accent; // shift-click = accent (!)
        commit();
        return;
      }
      if (ev.altKey) {
        hitNote.tie = !hitNote.tie; // alt-click = tie (~, glide into the next)
        commit();
        return;
      }
      // plain click on a note = delete it
      doc.notes = doc.notes.filter((n) => n !== hitNote);
      commit();
      return;
    }
    if (hitNote) {
      draw();
      return; // a note was grabbed but not moved meaningfully: no-op
    }
    const start = Math.min(d.t0, t);
    const dur = Math.abs(t - d.t0) + ROLL_Q;
    // refuse overlaps the sequential notes grammar cannot express
    const clash = doc.notes.some(
      (n) => start < n.start + n.dur - 1e-6 && n.start < start + dur - 1e-6 &&
        !(Math.abs(n.start - start) < 1e-6 && Math.abs(n.dur - dur) < 1e-6)
    );
    if (clash) {
      status('roll: overlaps an existing note (same start + length would merge into a chord)');
      draw();
      return;
    }
    doc.notes.push({ start, dur, pitch: d.pitch, tie: false, accent: false });
    if (!commit()) draw();
  };
  cv.onmouseup = finish;
  cv.onmouseleave = (ev) => {
    if (drag?.moved) finish(ev);
    else if (drag) {
      drag = null;
      draw();
    }
  };
  row.appendChild(cv);
  el.appendChild(row);
}

function refreshGrid() {
  const el = $('grid');
  stageSrc(getText());
  const n = main.e.fw_pattern_sites(main.ctx);
  if (n < 0) return; // unparsable source: keep the last grid
  const sites = JSON.parse(wasmText(main.e.fw_edit_ptr(main.ctx), main.e.fw_edit_len(main.ctx)));
  const rows = sites.filter((s) => s.kind === 'beat' && !s.raw.trim().startsWith('euclid('));
  const rolls = sites.filter((s) => s.kind === 'notes');
  el.innerHTML = '';
  document.body.dataset.gridRows = String(rows.length + rolls.length);
  if (!rows.length && !rolls.length) {
    el.innerHTML = '<div class="hint">no beat / notes literals</div>';
    return;
  }
  for (const site of rows) {
    const row = document.createElement('div');
    row.className = 'grid-row';
    const label = document.createElement('span');
    label.className = 'grid-label';
    const where = site.path.length ? `${site.path.join('/')} · ` : '';
    label.textContent = site.let_name
      ? `${where}let ${site.let_name}`
      : `${where}${site.track}${site.at ? ` @${site.at}` : ''}`;
    label.title = `${label.textContent} (jump to line ${site.line})`;
    label.onclick = () => jumpToLine(site.line);
    row.appendChild(label);
    const cells = document.createElement('div');
    cells.className = 'grid-cells';
    const steps = parseBeatSteps(site.raw);
    steps.forEach((step, i) => {
      const b = document.createElement('button');
      b.textContent = step.length > 1 ? `${step[0]}*` : step === '-' ? '' : step;
      b.title = step;
      b.className = STEP_CLASS[step[0]] ?? '';
      b.onclick = () => {
        const next = steps.slice();
        next[i] = cycleStep(step);
        const op = { op: 'set_pattern', path: site.path, value: joinSteps(next) };
        if (site.let_name) op.let_name = site.let_name;
        else Object.assign(op, { track: site.track, play: site.play });
        applyEdit(op);
      };
      cells.appendChild(b);
    });
    row.appendChild(cells);
    el.appendChild(row);
  }
  for (const site of rolls) renderRoll(el, site);
}

// ---- wiring -------------------------------------------------------------------
function showDiags(diags) {
  const el = $('diags');
  el.innerHTML = '';
  if (!diags.length) {
    el.innerHTML = '<div class="ok">✓ compiled OK</div>';
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
    refreshGrid();
    document.body.dataset.compiled = ok ? 'ok' : 'error';
    if (ok && node) node.port.postMessage(encodeSrc()); // hot reload
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
  status('loading engine…');
  await initWasm();
  try {
    // forte daw serves the package as the project API — the store becomes
    // the real directory on disk (ADR D-15: the package IS the project)
    const srv = await new ServerStore().init();
    store = srv;
    PROJECT = srv.project;
    document.title = `forte daw — ${PROJECT.name}`;
    document.body.dataset.project = PROJECT.name;
  } catch {
    PROJECT = null;
  }
  if (!store) {
    try {
      store = await new Store().init();
    } catch {
      store = null; // OPFS unavailable: still fully usable, just no persistence
    }
  }
  if (!PROJECT) for (const lib of MODULE_LIBS) {
    try {
      bundledModules[lib] = await (await fetch(`../../packages/essentials_0.6.0/songs/${lib}`)).text();
    } catch { /* offline without cache: song imports will diagnose */ }
  }
  await refreshModules();
  const last = localStorage.getItem('forte.last');
  const locals = await localNames();
  if (PROJECT) {
    currentName =
      last && locals.includes(last)
        ? last
        : locals.find((f) => f.startsWith('songs/')) ??
          locals.find((f) => f.startsWith('blocks/')) ??
          locals[0] ??
          'package.forte';
  } else {
    currentName =
      last && (locals.includes(last) || BUILTINS.includes(last)) ? last : BUILTINS[0];
  }

  const initialText =
    locals.includes(currentName) || PROJECT
      ? await store.read(currentName)
      : await (await fetch(`../../packages/essentials_0.6.0/songs/${currentName}`)).text();
  setText(initialText);
  status('loading editor…');
  // the editor is a nice-to-have: never let its CDN stall the DAW
  await Promise.race([
    tryMonaco(initialText),
    new Promise((res) => setTimeout(() => { monacoAbandoned = true; res(false); }, 9000)),
  ]);
  status('reading project…');
  onChange = () => {
    autosave();
    recompile();
  };
  await refreshFileList();
  recompile(0);
  status('ready');
  await recoverCrashedTake();

  await initVcs();

  $('file').onchange = (e) => loadSong(e.target.value);
  $('new').onclick = async () => {
    if (PROJECT) return newElement('song'); // the project's songs live in songs/
    const name = prompt('Song name (e.g. my-song)');
    if (!name || !store) return;
    const file = `${name.replace(/[^\w-]/g, '-')}.forte`;
    await store.write(file, NEW_TEMPLATE);
    await refreshFileList();
    loadSong(file);
  };
  $('delete').onclick = async () => {
    if (PROJECT) return status('delete project files in your shell / git');
    if (!store) return;
    const locals = await localNames();
    if (!locals.includes(currentName)) return;
    if (!confirm(`Delete local ${currentName}?`)) return;
    await store.remove(currentName);
    await refreshFileList();
    loadSong(BUILTINS.includes(currentName) ? currentName : BUILTINS[0]);
  };
  let playing = false;
  const doPlay = async () => {
    await ensureAudio();
    await ac.resume();
    node.port.postMessage({ cmd: 'play' });
    playing = true;
  };
  const doStop = () => {
    node?.port.postMessage({ cmd: 'stop' });
    playing = false;
  };
  $('play').onclick = doPlay;
  $('stop').onclick = doStop;
  $('zoom-in').onclick = () => applyZoom(vizZoom * 1.5);
  $('zoom-out').onclick = () => applyZoom(vizZoom / 1.5);
  $('zoom-fit').onclick = () => applyZoom(1);
  $('viz').addEventListener('wheel', (ev) => {
    if (!ev.ctrlKey) return; // plain wheel keeps scrolling the dock
    ev.preventDefault();
    applyZoom(vizZoom * (ev.deltaY < 0 ? 1.25 : 0.8), ev.clientX);
  }, { passive: false });
  $('helpbtn').onclick = () => $('help').classList.toggle('show');
  $('help-welcome').onclick = () => {
    $('help').classList.remove('show');
    $('welcome').classList.add('show');
  };
  $('undo').onclick = () => {
    if (window.__forteUndo) window.__forteUndo();
    else {
      $('fallback')?.focus();
      document.execCommand('undo');
    }
  };
  const hideWelcome = () => $('welcome').classList.remove('show');
  $('welcome-close').onclick = hideWelcome;
  $('welcome-demo').onclick = async () => {
    hideWelcome();
    const r = await fetch('api/new?kind=demo&name=demo', { method: 'POST' });
    if (r.ok) {
      await refreshModules();
      await refreshFileList();
      await loadSong(JSON.parse(await r.text()).file);
    } else {
      await loadSong('songs/demo.forte').catch(() => {});
    }
    status('press space to play! touching the grid / roll / mixer rewrites the code');
  };
  $('welcome-block').onclick = () => {
    hideWelcome();
    newElement('block');
  };
  $('welcome-song').onclick = () => {
    hideWelcome();
    newElement('song');
  };
  // an empty package is a blank page problem: show the guided start
  if (PROJECT && !(PROJECT.songs ?? []).length && !(PROJECT.blocks ?? []).length) {
    $('welcome-title').textContent = `Forte DAW — ${PROJECT.name}`;
    $('welcome').classList.add('show');
  }
  const bpmBox = $('bpm');
  if (bpmBox) {
    bpmBox.onchange = () => {
      const v = Number(bpmBox.value);
      if (Number.isFinite(v) && v >= 20 && v <= 300) {
        applyEdit({ op: 'set_tempo', bpm: v });
      } else {
        bpmBox.value = viz.data?.tempo ?? '';
      }
    };
  }
  // space = play/stop, the DAW way — unless typing (editor, inputs) or performing
  window.addEventListener('keydown', (e) => {
    if (e.code !== 'Space' || e.repeat) return;
    if (perf) return; // perform mode owns the keyboard
    const t = document.activeElement;
    if (t && (t.tagName === 'TEXTAREA' || t.tagName === 'INPUT' || t.isContentEditable)) return;
    e.preventDefault();
    (playing ? doStop : doPlay)();
  });
  // double-click in the arrange = seek the playhead to that beat
  $('viz').addEventListener('dblclick', async (ev) => {
    const rect = $('viz').getBoundingClientRect();
    const { headerW, pxPerBeat } = viz.geom();
    const x = ev.clientX - rect.left;
    if (x <= headerW || !viz.data) return;
    const bpb = viz.data.beatsPerBar || 4;
    const beats = Math.max(0, Math.round((x - headerW) / pxPerBeat / bpb) * bpb);
    await ensureAudio();
    node.port.postMessage({ cmd: 'seek', beats });
    viz.setPlayhead(beats);
  });
  $('rec').onclick = () => (rec ? recStop() : recStart()).catch((e) => status(`rec: ${e.message}`));
  $('calib').onclick = () =>
    calibrate().catch((e) => {
      document.body.dataset.calib = 'fail';
      status(`calib: ${e.message}`);
    });
  $('perform').onclick = () => performToggle().catch((e) => status(`perform: ${e.message}`));
  $('packages').onclick = () => { location.href = 'catalog.html'; };
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
boot().catch((e) => {
  status(`boot failed: ${e.message}`);
  toast(`boot failed: ${e.message} — reload to retry`, 'err');
  console.error(e);
});
