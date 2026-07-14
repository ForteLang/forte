// Browser E2E for the Forte web editor. Verifies, in a real Chromium:
//   1. the page compiles the reference song on boot,
//   2. the in-browser build digest equals the native CLI digest (bit-identity),
//   3. live diagnostics appear when the source is broken,
//   4. AudioWorklet playback advances the transport.
//
// Prereqs: scripts/build_web.sh, `npm i playwright`, a chromium binary
// (CHROMIUM env var, or Playwright's default download).
// Run: node scripts/web_e2e.mjs
import { chromium } from 'playwright';
import { spawn } from 'node:child_process';

// forte build packages/essentials_0.6.0/songs/first-light.forte
// (rebaselined 2026-07 with the prisma unison/spread params — #126)
const NATIVE_DIGEST = '9716a94698961fbd';
// forte build packages/essentials_0.6.0/songs/handmade.forte
// (rebaselined 2026-07: prisma gained unison/spread params — #126 — which
// shifted every prisma song's digest; forte-test.lock moved with it)
const NATIVE_DIGEST_HANDMADE = 'a7827674a4035229';
const PORT = 8329;
const ROOT = new URL('../..', import.meta.url).pathname;

const server = spawn('python3', ['-m', 'http.server', '-d', ROOT, String(PORT)]);
await new Promise((r) => setTimeout(r, 800));

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined,
  args: [
    '--autoplay-policy=no-user-gesture-required',
    '--no-proxy-server',
    // fake mic (emits a tone) so the recording path is testable headlessly
    '--use-fake-device-for-media-stream',
    '--use-fake-ui-for-media-stream',
  ],
});
const page = await (await browser.newContext()).newPage();
page.on('pageerror', (e) => console.log('pageerror:', e.message));

let failed = false;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failed = true;
};

try {
  await page.goto(`http://127.0.0.1:${PORT}/forte/web/`, { waitUntil: 'load' });

  await page.waitForSelector('body[data-compiled="ok"]', { timeout: 15000 });
  check('boot compile', true);

  await page.click('#digest');
  await page.waitForFunction(
    () => document.getElementById('digest-out').textContent !== '—',
    null,
    { timeout: 60000 }
  );
  const digest = await page.textContent('#digest-out');
  check('browser digest == native', digest === NATIVE_DIGEST, digest);

  const diagText = await page.evaluate(async () => {
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value = ta.value.replace('cutoff:', 'cutof:');
      ta.dispatchEvent(new Event('input'));
    } else {
      const ed = monaco.editor.getModels()[0];
      ed.setValue(ed.getValue().replace('cutoff:', 'cutof:'));
    }
    await new Promise((r) => setTimeout(r, 600));
    return document.getElementById('diags').textContent;
  });
  check('live diagnostics', diagText.includes('E-DEV-002'), diagText.slice(0, 60));

  // undo the typo before playing — otherwise the worklet (rightly) refuses
  // to load the broken source and the transport runs over silence
  await page.evaluate(async () => {
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value = ta.value.replace('cutof:', 'cutoff:');
      ta.dispatchEvent(new Event('input'));
    } else {
      const ed = monaco.editor.getModels()[0];
      ed.setValue(ed.getValue().replace('cutof:', 'cutoff:'));
    }
    await new Promise((r) => setTimeout(r, 600));
  });

  await page.click('#play');
  await page.waitForFunction(
    () => /bar\s+\d+\.\d/.test(document.getElementById('status').textContent),
    null,
    { timeout: 15000 }
  );
  const s1 = await page.textContent('#status');
  await page.waitForTimeout(1500);
  const s2 = await page.textContent('#status');
  check('worklet playback advances', s1 !== s2, `${s1} → ${s2}`);
  // …and actually makes sound. The transport once advanced over a silent
  // engine (TextEncoder missing in the worklet scope) and this suite was
  // blind to it — peak must be nonzero while the kick plays.
  const peak = parseFloat((s2.match(/peak\s+([\d.]+)/) || [])[1] ?? '0');
  check('worklet playback is audible', peak > 0.02, `peak ${peak}`);

  const vizTracks = await page.evaluate(() => window.__vizTracks ?? 0);
  check('arrangement view rendered', vizTracks >= 6, `${vizTracks} tracks`);

  // 4.5) visualization second wave: meters, code-jump, piano roll
  const vizBox = await page.evaluate(() => {
    const r = document.getElementById('viz').getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height, lanes: window.__vizTracks };
  });
  const meters = await page.evaluate(async () => {
    // pos messages feed viz.setPeaks — poll the viz for the live levels
    for (let i = 0; i < 30; i++) {
      const p = window.__forteViz?.peaks;
      if (p?.length) return { n: p.length, max: Math.max(...p) };
      await new Promise((r) => setTimeout(r, 100));
    }
    return null;
  });
  check(
    'per-track meters stream from the worklet',
    meters && meters.n >= 6 && meters.max > 0.01,
    meters ? `${meters.n} tracks, peak ${meters.max.toFixed(3)}` : 'no peaks'
  );
  // click a clip → the editor cursor jumps to that source line
  const laneH0 = (vizBox.h - 16) / vizBox.lanes;
  await page.mouse.click(vizBox.x + vizBox.w * 0.3, vizBox.y + 16 + laneH0 * 0.5);
  const cursorLine = await page.evaluate(() => {
    if (window.monaco) return monaco.editor.getEditors?.()[0]?.getPosition()?.lineNumber
      ?? window.__forteCursorLine ?? 0;
    const ta = document.getElementById('fallback');
    return ta ? ta.value.slice(0, ta.selectionStart).split('\n').length : 0;
  });
  check('clip click jumps the editor to the source line', cursorLine > 1, `line ${cursorLine}`);
  // click the lane header → piano roll; click again → back to arrange
  await page.mouse.click(vizBox.x + 40, vizBox.y + 16 + laneH0 * 0.5);
  const rollOn = await page.evaluate(() => window.__forteViz?.mode ?? 'unknown');
  await page.mouse.click(vizBox.x + 40, vizBox.y + 16 + laneH0 * 0.5);
  const rollOff = await page.evaluate(() => window.__forteViz?.mode ?? 'unknown');
  check('lane header toggles the piano roll', rollOn === 'piano' && rollOff === 'arrange', `${rollOn} → ${rollOff}`);
  // 4.6) edit→sound latency: a full in-browser recompile (the hot-reload
  // unit of work) must fit far inside the 1-second budget (issue #2)
  const compileMs = await page.evaluate(() => {
    const src = window.__forteGetText();
    const t0 = performance.now();
    window.__forteCompileCheck(src); // compiles twice (probe + restore)
    return (performance.now() - t0) / 2;
  });
  check('edit→compile under 1s in the browser', compileMs < 1000, `${compileMs.toFixed(0)}ms per compile`);

  // 4.7) the beat grid (Studio P0, #135): rows render from the song's beat
  // literals, and a cell click round-trips through the wasm edit layer into
  // the code — touching exactly the literal, nothing else
  const gridRows = await page.evaluate(() => Number(document.body.dataset.gridRows ?? 0));
  check('beat grid renders rows', gridRows >= 3, `${gridRows} rows`);
  const gridBefore = await page.evaluate(() => window.__forteGetText());
  const cell = '#grid .grid-row:first-child .grid-cells button:first-child';
  await page.click(cell); // kick step 1: x → X (accent)
  await page.waitForFunction(
    () => window.__forteGetText().includes('X--- x--- x-x- x---'),
    null,
    { timeout: 15000 }
  );
  const gridAfter = await page.evaluate(() => window.__forteGetText());
  const b = gridBefore.split('\n');
  const a = gridAfter.split('\n');
  const changed = a.filter((l, i) => l !== b[i]).length;
  check('grid click writes back exactly one line', a.length === b.length && changed === 1, `${changed} lines changed`);
  for (let i = 0; i < 3; i++) {
    // X → . → - → x: three more clicks cycle the cell back home
    await page.click(cell);
    await page.waitForTimeout(300);
  }
  const gridRestored = await page.evaluate(() => window.__forteGetText());
  check('grid cycle returns to the original pattern', gridRestored === gridBefore);

  // 4.8) arrange drag (Studio P0, #135): dragging a clip on the canvas
  // re-places the play statement it came from, snapped to bars, through
  // the same edit layer — the arrange view is a real editing surface
  const snareDragPoint = () =>
    page.evaluate(() => {
      const viz = window.__forteViz;
      const d = viz.data;
      const r = document.getElementById('viz').getBoundingClientRect();
      const { headerW, pxPerBeat } = viz.geom();
      const ti = d.tracks.findIndex((t) => t.name === 'Snare');
      const clip = d.tracks[ti].clips[0];
      const laneH = (r.height - 16) / d.tracks.length;
      return {
        x: r.left + headerW + (clip.start + clip.duration / 2) * pxPerBeat,
        y: r.top + 16 + laneH * (ti + 0.5),
        barPx: pxPerBeat * d.beatsPerBar,
      };
    });
  const d1 = await snareDragPoint();
  await page.mouse.move(d1.x, d1.y);
  await page.mouse.down();
  await page.mouse.move(d1.x - d1.barPx, d1.y, { steps: 6 });
  await page.mouse.up();
  await page.waitForFunction(
    () => window.__forteGetText().includes('play snare at bars(8..15)'),
    null,
    { timeout: 15000 }
  );
  check('clip drag re-places the play through the edit layer', true);
  const d2 = await snareDragPoint(); // recompiled: re-measure before undoing
  await page.mouse.move(d2.x, d2.y);
  await page.mouse.down();
  await page.mouse.move(d2.x + d2.barPx, d2.y, { steps: 6 });
  await page.mouse.up();
  await page.waitForFunction(
    () => window.__forteGetText().includes('play snare at bars(9..16)'),
    null,
    { timeout: 15000 }
  );
  check('drag back restores the original placement', true);

  // 5) local-first: edits autosave to OPFS and survive a reload
  await page.evaluate(async () => {
    const marker = '\n// persisted-marker\n';
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value += marker;
      ta.dispatchEvent(new Event('input'));
    } else {
      const ed = monaco.editor.getModels()[0];
      ed.setValue(ed.getValue() + marker);
    }
    await new Promise((r) => setTimeout(r, 1000)); // autosave debounce
  });
  await page.reload({ waitUntil: 'load' });
  await page.waitForSelector('body[data-compiled]', { timeout: 15000 });
  const persisted = await page.evaluate(() => window.__forteGetText());
  check('OPFS persistence across reload', persisted.includes('persisted-marker'));

  // 5.6) the repository lives in the browser: commit → musical diff → restore
  page.on('dialog', (d) => d.accept());
  await page.evaluate(async () => {
    // repair the deliberate typo from the diagnostics test: the semantic diff
    // only speaks music when both versions compile
    const edit = (t) => t.replace('cutof:', 'cutoff:');
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value = edit(ta.value);
      ta.dispatchEvent(new Event('input'));
    } else {
      const ed = monaco.editor.getModels()[0];
      ed.setValue(edit(ed.getValue()));
    }
    await new Promise((r) => setTimeout(r, 1000)); // autosave debounce
  });
  await page.fill('#commit-msg', '最初のスケッチ');
  await page.click('#commit');
  await page
    .waitForFunction(() => document.body.dataset.commits === '1', null, { timeout: 15000 })
    .catch(async (e) => {
      console.log('DEBUG status:', await page.textContent('#status'));
      console.log('DEBUG commits:', await page.evaluate(() => document.body.dataset.commits));
      throw e;
    });
  await page.evaluate(() => {
    const edit = (t) => t.replace('tempo 96bpm', 'tempo 132bpm');
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value = edit(ta.value);
      ta.dispatchEvent(new Event('input'));
    } else {
      const ed = monaco.editor.getModels()[0];
      ed.setValue(edit(ed.getValue()));
    }
  });
  await page.click('#vcs-log .commit a'); // "diff" of commit #1
  await page.waitForFunction(
    () => document.getElementById('vcs-diff').textContent.includes('tempo: 96 → 132 bpm'),
    null,
    { timeout: 15000 }
  );
  check('browser commit + semantic diff', true);
  await page.click('#vcs-log .commit a:nth-of-type(2)'); // "戻す" (restore)
  await page.waitForFunction(
    () => window.__forteGetText().includes('tempo 96bpm'),
    null,
    { timeout: 15000 }
  );
  check('restore returns the committed take', true);

  // 6) offline PWA: with the service worker active, the editor still boots,
  //    compiles and plays with the network cut
  await page.evaluate(() => navigator.serviceWorker.ready);
  await page.waitForTimeout(500); // let precache finish
  await page.context().setOffline(true);
  await page.reload({ waitUntil: 'load' });
  await page.waitForSelector('body[data-compiled]', { timeout: 15000 });
  await page.click('#play');
  const offlinePlays = await page
    .waitForFunction(
      () => /bar\s+\d+\.\d/.test(document.getElementById('status').textContent),
      null,
      { timeout: 15000 }
    )
    .then(() => true)
    .catch(() => false);
  check('offline PWA: boots, compiles and plays with network cut', offlinePlays);
  await page.context().setOffline(false);

  // 7) imports in the browser: handmade.forte pulls its instruments from
  //    devices/warm.forte and builds bit-identical to the native CLI
  await page.selectOption('#file', 'handmade.forte');
  await page.waitForFunction(
    () => document.body.dataset.compiled === 'ok' && window.__forteGetText().includes('Handmade'),
    null,
    { timeout: 15000 }
  );
  await page.click('#digest');
  await page.waitForFunction(
    () => /^[0-9a-f]{16}$/.test(document.getElementById('digest-out').textContent),
    null,
    { timeout: 60000 }
  );
  const digest2 = await page.textContent('#digest-out');
  check(
    'browser imports + device DSL digest == native',
    digest2 === NATIVE_DIGEST_HANDMADE,
    digest2
  );

  // 8) recording: capture the fake mic into a provenance-stamped .frec, then
  //    a song importing the take must compile (provenance validated in-wasm)
  await page.click('#rec');
  await page.waitForFunction(
    () => document.body.dataset.rec === 'on',
    null,
    { timeout: 15000 }
  );
  await page.waitForTimeout(1200);
  await page.click('#rec');
  await page.waitForFunction(() => document.body.dataset.lastTake, null, { timeout: 15000 });
  const take = await page.evaluate(() => document.body.dataset.lastTake);
  const recSong = [
    `import take from "./${take}"`,
    'song "Rec" {',
    '  tempo 120bpm',
    '  track Kick { instrument sampler(sample: "Kick") play beat`x---` at bars(1..2) }',
    '  track Voice { audio take at bars(1..2) }',
    '}',
    '',
  ].join('\n');
  await page.evaluate((text) => {
    const ta = document.getElementById('fallback');
    if (ta) {
      ta.value = text;
      ta.dispatchEvent(new Event('input'));
    } else {
      monaco.editor.getModels()[0].setValue(text);
    }
  }, recSong);
  await page.waitForFunction(
    () => document.body.dataset.compiled === 'ok' && window.__forteGetText().includes('Voice'),
    null,
    { timeout: 15000 }
  );
  check('recorded take imports and compiles in browser', true, take);

  // 9) calibration flow completes: the fake mic cannot hear the probe, so the
  //    honest outcome is a graceful "not detected" (never a bogus number)
  await page.click('#calib');
  await page.waitForSelector('body[data-calib]', { timeout: 30000 });
  const calib = await page.evaluate(() => document.body.dataset.calib);
  check(
    'calibration flow completes honestly',
    calib === 'nodetect', // fake mic can't hear the probe; a number here would be a lie
    `data-calib=${calib}`
  );

  // 9.5) performance capture: play PC keys, stop, get a notes literal back —
  //      and the generated code must itself compile
  await page.click('#perform');
  for (const key of ['a', 'd', 'g']) {
    await page.keyboard.down(key);
    await page.waitForTimeout(180);
    await page.keyboard.up(key);
    await page.waitForTimeout(80);
  }
  await page.click('#perform');
  await page.waitForFunction(() => document.body.dataset.performCode, null, { timeout: 15000 });
  const performCode = await page.evaluate(() => document.body.dataset.performCode);
  const perfCompiles = await page.evaluate(async (code) => {
    const src = `song "P" { tempo 120bpm track A { instrument prisma() play ${code} at bars(1..2) } }`;
    return window.__forteCompileCheck ? window.__forteCompileCheck(src) : null;
  }, performCode);
  check(
    'performance transcribes to compilable code',
    /notes`.*C4.*`/.test(performCode) && perfCompiles !== false,
    performCode.slice(0, 60)
  );

  // 10) crash recovery: reload mid-recording (the stop path never runs) —
  //     the streamed PCM must come back as a real take on next boot
  await page.click('#rec');
  await page.waitForFunction(
    () => document.body.dataset.rec === 'on',
    null,
    { timeout: 15000 }
  );
  await page.waitForTimeout(1500); // > one flush interval
  await page.reload({ waitUntil: 'load' }); // simulated crash
  await page.waitForSelector('body[data-recovered="ok"]', { timeout: 20000 });
  const recovered = await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const assets = await (await root.getDirectoryHandle('songs')).getDirectoryHandle('assets');
    const names = [];
    for await (const [n] of assets.entries()) names.push(n);
    return names.sort();
  });
  check(
    'crashed take recovered on boot',
    recovered.includes('take-2.frec') && !recovered.includes('.recording.pcm'),
    recovered.join(', ')
  );

  // 11) zero-install player: player.html?src=….fortesong unpacks the
  //     container, verifies the files digest, compiles and PLAYS — with a
  //     ../-climbing import (smiley-acid) to prove the module rebase
  const track = encodeURIComponent(
    '../../packages/essentials_0.6.0/albums/first-light/02-hello-world.fortesong'
  );
  await page.goto(`http://127.0.0.1:${PORT}/forte/web/player.html?src=${track}`, {
    waitUntil: 'load',
  });
  await page.waitForFunction(
    () => document.querySelectorAll('.trk').length === 1,
    null,
    { timeout: 15000 }
  );
  await page.click('#toggle');
  await page.waitForFunction(
    () => /\d+:\d+ \/ \d+:\d+/.test(document.getElementById('time').textContent),
    null,
    { timeout: 20000 }
  );
  const trackName = await page.textContent('#t-name');
  check('player loads the .fortesong meta', trackName === 'Hello World', trackName);
  const p1 = await page.textContent('#time');
  await page.waitForTimeout(1500);
  const p2 = await page.textContent('#time');
  check('player transport advances', p1 !== p2, `${p1} → ${p2}`);
  const audible = await page.evaluate(async () => {
    // tap the worklet's pos messages for a real peak reading
    return await new Promise((res) => {
      let peak = 0;
      const t = setTimeout(() => res(peak), 2500);
      const orig = node.port.onmessage;
      node.port.onmessage = (e) => {
        if (e.data.kind === 'pos') peak = Math.max(peak, e.data.peak);
        orig(e);
        if (peak > 0.02) {
          clearTimeout(t);
          res(peak);
        }
      };
    });
  });
  check('player playback is audible', audible > 0.02, `peak ${audible.toFixed(3)}`);

  // 12) the composer view: the arrangement canvas IS the block browser —
  //     click a track lane and the player reveals that block's source and
  //     the import line to steal it with
  await page.waitForFunction(
    () =>
      getComputedStyle(document.getElementById('arr-wrap')).display !== 'none' &&
      typeof vizData === 'object' &&
      vizData?.tracks?.length > 0,
    null,
    { timeout: 20000 }
  );
  const arrBox = await page.evaluate(() => {
    document.getElementById('arr').scrollIntoView({ block: 'center' });
    const r = document.getElementById('arr').getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height, lanes: vizData.tracks.length };
  });
  check('arrangement canvas rendered with lanes', arrBox.lanes >= 3, `${arrBox.lanes} lanes`);
  const laneH = (arrBox.h - 16) / arrBox.lanes;
  await page.mouse.click(arrBox.x + arrBox.w / 2, arrBox.y + 16 + laneH * 1.5);
  await page.waitForFunction(
    () => document.getElementById('blk-src-wrap').style.display === 'block',
    null,
    { timeout: 10000 }
  );
  const importLine = await page.textContent('#blk-import');
  check(
    'lane click reveals the block import line',
    importLine.startsWith('import {') || importLine.startsWith('block') || importLine.startsWith('root'),
    importLine.slice(0, 60)
  );
  const blkTitle = await page.textContent('#blk-title');
  check('lane click shows track + instrument + inserts', /—\s+\S/.test(blkTitle), blkTitle.slice(0, 80));
  await page.click('#full-src-btn');
  await page.waitForFunction(
    () => document.getElementById('blk-src').textContent.includes('song "'),
    null,
    { timeout: 5000 }
  );
  const fullSrc = await page.textContent('#blk-src');
  check('whole-piece source one click away', fullSrc.includes('import {'), `${fullSrc.length} chars`);

  // 13) transport: the progress bar seeks, and a (re)started track ALWAYS
  //     begins at 0:00 — no position bleed between songs
  const barBox = await page.evaluate(() => {
    document.getElementById('bar-wrap').scrollIntoView({ block: 'center' });
    const r = document.getElementById('bar-wrap').getBoundingClientRect();
    return { x: r.left, y: r.top + r.height / 2, w: r.width };
  });
  await page.mouse.click(barBox.x + barBox.w * 0.8, barBox.y);
  await page.waitForTimeout(800);
  const secsOf = (s) => {
    const [m, ss] = s.trim().split(':').map(Number);
    return m * 60 + ss;
  };
  const afterSeek = (await page.textContent('#time')).split(' / ').map(secsOf);
  check(
    'progress bar seeks to the clicked position',
    Math.abs(afterSeek[0] - afterSeek[1] * 0.8) < 10,
    `${afterSeek[0]}s of ${afterSeek[1]}s`
  );
  await page.click('#next');
  await page.waitForTimeout(1500);
  const afterNext = secsOf((await page.textContent('#time')).split(' / ')[0]);
  check('a newly started track begins at 0:00', afterNext < 8, `${afterNext}s`);

  // 10. volume boost: the knob drives the live gain node and persists via
  //     localStorage (playback-only; the render digest is upstream of it)
  const boosted = await page.evaluate(() => {
    const el = document.getElementById('boost');
    el.value = '2.5';
    el.dispatchEvent(new Event('input'));
    return { stored: localStorage.getItem('forte-boost'), label: document.getElementById('boost-val').textContent };
  });
  check(
    'boost control sets gain and persists',
    boosted.stored === '2.5' && boosted.label.startsWith('+8.0'),
    `stored=${boosted.stored} label=${boosted.label}`
  );
} finally {
  await browser.close();
  server.kill();
}
process.exit(failed ? 1 : 0);
