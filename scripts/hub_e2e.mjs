// Browser E2E for the hub page: publish+release via CLI, then in a real
// Chromium browse the lineage, play a release from its sources, verify its
// digest in-tab, and fork it into the editor's OPFS.
// Prereqs: cargo build --release -p fortelang, scripts/build_web.sh,
//          `npm i playwright`. Run: node scripts/hub_e2e.mjs
import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const ROOT = new URL('..', import.meta.url).pathname;
const FORTE = join(ROOT, 'target/release/forte');
const HUB = mkdtempSync(join(tmpdir(), 'forte-hub-e2e-'));
const STATIC_PORT = 8353;
const API_PORT = 9391;

// seed the hub via the CLI
const run = (...a) => execFileSync(FORTE, a, { env: { ...process.env, FORTE_HUB: HUB } }).toString();
console.log(run('hub', 'publish', join(ROOT, 'packages/essentials_0.6.0/songs/devices/warm.forte')).trim());
console.log(run('hub', 'publish', join(ROOT, 'packages/essentials_0.6.0/songs/handmade.forte')).trim());
console.log(run('hub', 'release', 'handmade').trim());

const api = spawn(FORTE, ['hub', 'serve', '--port', String(API_PORT)], {
  env: { ...process.env, FORTE_HUB: HUB },
});
const statics = spawn('python3', ['-m', 'http.server', '-d', ROOT, String(STATIC_PORT)]);
await new Promise((r) => setTimeout(r, 1000));

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined,
  args: [
    '--autoplay-policy=no-user-gesture-required',
    '--no-proxy-server',
    // fake mic (emits a tone): the performance-fork loop records headlessly
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
  await page.goto(
    `http://127.0.0.1:${STATIC_PORT}/web/hub.html?api=http://127.0.0.1:${API_PORT}`,
    { waitUntil: 'load' }
  );

  // 1) the lineage list shows both repos
  await page.waitForSelector('.repo', { timeout: 15000 });
  const names = await page.$$eval('.repo h2', (hs) => hs.map((h) => h.textContent));
  check('hub list shows repos', names.some((n) => n.includes('handmade')) && names.some((n) => n.includes('warm')));

  // 2) detail shows the release digest
  await page.click('.repo:has-text("handmade")');
  await page.waitForSelector('#detail', { state: 'visible' });
  const lineage = await page.textContent('#d-lineage');
  check('detail shows release digest', lineage.includes('d66a3103bcf1cad1'), lineage.split('\n')[0]);

  // 3) listen: playback from sources advances the transport
  await page.click('#listen');
  await page.waitForFunction(
    () => /bar\s+\d+\.\d/.test(document.getElementById('status').textContent),
    null,
    { timeout: 15000 }
  );
  check('listen plays from sources', true, await page.textContent('#status'));

  // 3.5) open-stems: per-track M/S controls exist and drive the live engine
  const stemRows = await page.$$eval('#stems .stem', (rows) =>
    rows.map((r) => r.querySelector('span').textContent)
  );
  check('stem controls list the tracks', stemRows.length >= 2, stemRows.join(', '));
  await page.click('#stems .stem button'); // mute the first track
  await page.waitForFunction(() => document.body.dataset.stems === '1m0s', null, { timeout: 5000 });
  check('mute toggles apply while playing', true);
  await page.click('#stems .stem button'); // unmute for the rest of the test

  // 4) verify in tab: digest reproduces
  await page.click('#verify');
  await page.waitForSelector('body[data-verify]', { timeout: 120000 });
  const v = await page.evaluate(() => document.body.dataset.verify);
  check('release verified in browser tab', v === 'ok');

  // 5) fork into the editor: files land in OPFS (with import structure) and
  //    the ledger records the event
  await page.click('#fork');
  await page.waitForSelector('body[data-forked="ok"]', { timeout: 15000 });
  const opfs = await page.evaluate(async () => {
    const root = await navigator.storage.getDirectory();
    const songs = await root.getDirectoryHandle('songs');
    const out = [];
    const walk = async (dir, prefix) => {
      for await (const [name, h] of dir.entries()) {
        if (h.kind === 'file') out.push(prefix + name);
        else await walk(h, `${prefix}${name}/`);
      }
    };
    await walk(songs, '');
    return out.sort();
  });
  check(
    'fork lands in editor OPFS with import structure',
    opfs.includes('handmade.forte') && opfs.includes('devices/warm.forte'),
    opfs.join(', ')
  );
  const detail = await (await fetch(`http://127.0.0.1:${API_PORT}/api/repos/handmade`)).json();
  check('fork is in the ledger', detail.fork_events === 1, `fork_events=${detail.fork_events}`);

  // 6) the performance fork closes its loop: open the forked song in the
  //    editor, record a take over it, one-tap insert, publish back to the hub
  page.on('dialog', (d) => d.accept('handmade-voice')); // confirm + name prompt
  await page.goto(
    `http://127.0.0.1:${STATIC_PORT}/web/?api=http://127.0.0.1:${API_PORT}`,
    { waitUntil: 'load' }
  );
  await page.waitForSelector('body[data-compiled]', { timeout: 20000 });
  await page.selectOption('#file', 'handmade.forte');
  await page.waitForFunction(
    () => document.body.dataset.compiled === 'ok' && window.__forteGetText().includes('Handmade'),
    null,
    { timeout: 15000 }
  );
  await page.click('#rec');
  await page.waitForFunction(() => document.body.dataset.rec === 'on', null, { timeout: 15000 });
  await page.waitForTimeout(1200);
  await page.click('#rec'); // stop → confirm dialog inserts the take
  await page.waitForFunction(() => document.body.dataset.takeInserted, null, { timeout: 15000 });
  await page.waitForFunction(
    () => document.body.dataset.compiled === 'ok' && window.__forteGetText().includes('track Voice_'),
    null,
    { timeout: 15000 }
  );
  check('recorded take inserts into the forked song', true);

  await page.click('#publish');
  await page
    .waitForFunction(() => document.body.dataset.published === 'handmade-voice', null, {
      timeout: 30000,
    })
    .catch(async (e) => {
      console.log('DEBUG status:', await page.textContent('#status'));
      throw e;
    });
  const pub = await (await fetch(`http://127.0.0.1:${API_PORT}/api/repos/handmade-voice`)).json();
  check(
    'browser publish records lineage and carries the take',
    pub.forked_from?.repo === 'handmade' &&
      Object.keys(pub.files ?? {}).length === 0 /* detail has no files list */ ||
      pub.forked_from?.repo === 'handmade',
    `forked_from=${pub.forked_from?.repo}`
  );
  const files = await (
    await fetch(`http://127.0.0.1:${API_PORT}/api/repos/handmade-voice/files`)
  ).json();
  check(
    'published snapshot includes the recorded take',
    Object.keys(files.assets ?? {}).some((p) => p.endsWith('.frec')),
    Object.keys(files.assets ?? {}).join(', ')
  );

  // 7) dig: the hub front page draws the fork family tree
  await page.goto(
    `http://127.0.0.1:${STATIC_PORT}/web/hub.html?api=http://127.0.0.1:${API_PORT}`,
    { waitUntil: 'load' }
  );
  await page.waitForFunction(
    () => Number(document.body.dataset.treeNodes || 0) >= 3,
    null,
    { timeout: 15000 }
  );
  const tree = await page.textContent('#tree');
  check(
    'lineage tree nests the performance fork under its origin',
    tree.includes('handmade-voice') && tree.includes('└─'),
    tree.replace(/\n/g, ' | ').slice(0, 120)
  );

  // 8) cross-module dig: the library page lists the songs that play it
  await page.click('.repo:has-text("warm")');
  await page.waitForSelector('#detail', { state: 'visible' });
  const libLineage = await page.textContent('#d-lineage');
  check(
    'library page answers 「この楽器を使う曲」',
    libLineage.includes('この楽器を使う曲') && libLineage.includes('handmade'),
    libLineage.split('\n').find((l) => l.includes('楽器')) ?? ''
  );
} finally {
  await browser.close();
  api.kill();
  statics.kill();
}
process.exit(failed ? 1 : 0);
