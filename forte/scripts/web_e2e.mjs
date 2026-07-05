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

const NATIVE_DIGEST = '1f1e8e0e873a42fc'; // forte build packages/essentials_0.6.0/songs/first-light.forte
const NATIVE_DIGEST_HANDMADE = 'd66a3103bcf1cad1'; // forte build packages/essentials_0.6.0/songs/handmade.forte
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
    '../../packages/essentials_0.6.0/albums/first-light/04-smiley-acid.fortesong'
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
  check('player loads the .fortesong meta', trackName === 'Smiley Acid', trackName);
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
} finally {
  await browser.close();
  server.kill();
}
process.exit(failed ? 1 : 0);
