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

const NATIVE_DIGEST = '10a443f96fc027cf'; // forte build songs/first-light.forte
const PORT = 8329;
const ROOT = new URL('..', import.meta.url).pathname;

const server = spawn('python3', ['-m', 'http.server', '-d', ROOT, String(PORT)]);
await new Promise((r) => setTimeout(r, 800));

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM || undefined,
  args: ['--autoplay-policy=no-user-gesture-required', '--no-proxy-server'],
});
const page = await (await browser.newContext()).newPage();
page.on('pageerror', (e) => console.log('pageerror:', e.message));

let failed = false;
const check = (name, ok, detail = '') => {
  console.log(`${ok ? '✓' : '✗'} ${name}${detail ? ` — ${detail}` : ''}`);
  if (!ok) failed = true;
};

try {
  await page.goto(`http://127.0.0.1:${PORT}/web/`, { waitUntil: 'load' });

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
} finally {
  await browser.close();
  server.kill();
}
process.exit(failed ? 1 : 0);
