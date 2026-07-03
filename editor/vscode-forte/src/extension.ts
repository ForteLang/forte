// Forte VSCode extension: LSP diagnostics, play/build, REPL integration and
// the read-only arrangement view. The compiler is the single source of truth
// — this is a thin shell around the `forte` CLI.

import { execFile } from 'child_process';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let playTerminal: vscode.Terminal | undefined;
let replTerminal: vscode.Terminal | undefined;
let vizPanel: vscode.WebviewPanel | undefined;

function fortePath(): string {
  return vscode.workspace.getConfiguration('forte').get<string>('path') ?? 'forte';
}

export async function activate(context: vscode.ExtensionContext) {
  // --- language server ------------------------------------------------------
  const serverOptions: ServerOptions = {
    command: fortePath(),
    args: ['lsp'],
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ language: 'forte' }],
  };
  client = new LanguageClient('forte', 'Forte Language Server', serverOptions, clientOptions);
  try {
    await client.start();
  } catch {
    vscode.window.showWarningMessage(
      `Forte: could not start "${fortePath()} lsp". Set forte.path to the built CLI ` +
        '(cargo build --release -p fortelang).'
    );
  }

  // --- commands -------------------------------------------------------------
  const activeForteFile = (): string | undefined => {
    const doc = vscode.window.activeTextEditor?.document;
    if (!doc || doc.languageId !== 'forte') {
      vscode.window.showErrorMessage('Forte: open a .forte file first.');
      return undefined;
    }
    doc.save();
    return doc.fileName;
  };

  context.subscriptions.push(
    vscode.commands.registerCommand('forte.play', () => {
      const file = activeForteFile();
      if (!file) return;
      playTerminal?.dispose();
      playTerminal = vscode.window.createTerminal('Forte Play');
      playTerminal.show(true);
      // hot reload: keep playing; saving the file swaps the new version in
      playTerminal.sendText(`${fortePath()} play "${file}"`);
    }),
    vscode.commands.registerCommand('forte.stop', () => {
      playTerminal?.dispose();
      playTerminal = undefined;
    }),
    vscode.commands.registerCommand('forte.build', () => {
      const file = activeForteFile();
      if (!file) return;
      const term = vscode.window.createTerminal('Forte Build');
      term.show(true);
      term.sendText(`${fortePath()} build "${file}"`);
    }),

    // --- REPL: a terminal jam session you can feed from the editor ----------
    vscode.commands.registerCommand('forte.repl', () => {
      ensureRepl().show(true);
    }),
    vscode.commands.registerCommand('forte.sendToRepl', () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) return;
      const sel = editor.selection;
      const text = sel.isEmpty ? editor.document.lineAt(sel.active.line).text : editor.document.getText(sel);
      if (!text.trim()) return;
      ensureRepl().sendText(text);
    }),

    // --- arrangement view: refreshed on every save ---------------------------
    vscode.commands.registerCommand('forte.showArrangement', () => {
      const file = activeForteFile();
      if (!file) return;
      openViz(context, file);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId === 'forte' && vizPanel) refreshViz(doc.fileName);
    })
  );
}

function ensureRepl(): vscode.Terminal {
  if (!replTerminal || replTerminal.exitStatus) {
    replTerminal = vscode.window.createTerminal('Forte REPL');
    replTerminal.sendText(`${fortePath()} repl`);
  }
  return replTerminal;
}

function refreshViz(file: string) {
  execFile(fortePath(), ['viz', file], { maxBuffer: 64 * 1024 * 1024 }, (err, stdout, stderr) => {
    if (!vizPanel) return;
    if (err) {
      vizPanel.webview.postMessage({ kind: 'error', message: stderr || String(err) });
    } else {
      vizPanel.webview.postMessage({ kind: 'viz', data: JSON.parse(stdout) });
    }
  });
}

function openViz(context: vscode.ExtensionContext, file: string) {
  if (!vizPanel) {
    vizPanel = vscode.window.createWebviewPanel(
      'forteViz',
      'Forte: Arrangement',
      vscode.ViewColumn.Beside,
      { enableScripts: true, retainContextWhenHidden: true }
    );
    vizPanel.onDidDispose(() => (vizPanel = undefined), null, context.subscriptions);
    vizPanel.webview.html = VIZ_HTML;
  }
  refreshViz(file);
}

// Self-contained read-only arrangement renderer (mirror of web/viz.js).
const VIZ_HTML = /* html */ `<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  html, body { margin: 0; height: 100%; background: #14161b; }
  canvas { width: 100vw; height: 100vh; display: block; }
  #err { position: fixed; top: 8px; left: 12px; color: #e06c75;
    font: 12px/1.5 monospace; white-space: pre-wrap; }
</style></head>
<body><canvas id="c"></canvas><div id="err"></div>
<script>
const canvas = document.getElementById('c');
const g = canvas.getContext('2d');
let data = null;
window.addEventListener('message', (e) => {
  const m = e.data;
  if (m.kind === 'viz') { data = m.data; document.getElementById('err').textContent = ''; draw(); }
  else if (m.kind === 'error') { document.getElementById('err').textContent = m.message; }
});
new ResizeObserver(draw).observe(canvas);
function draw() {
  const dpr = devicePixelRatio || 1;
  const w = canvas.clientWidth, h = canvas.clientHeight;
  canvas.width = w * dpr; canvas.height = h * dpr;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  if (!data || !data.tracks?.length) return;
  const headerW = 92, rulerH = 16;
  const laneH = (h - rulerH) / data.tracks.length;
  const span = Math.max(data.lengthBeats, data.beatsPerBar);
  const bx = (b) => headerW + ((w - headerW) * b) / span;
  g.font = '9px monospace'; g.textBaseline = 'top';
  for (let b = 0; b * data.beatsPerBar <= span; b++) {
    const x = bx(b * data.beatsPerBar);
    g.strokeStyle = b % 4 === 0 ? '#2f3440' : '#232730';
    g.beginPath(); g.moveTo(x, rulerH); g.lineTo(x, h); g.stroke();
    if (b % 4 === 0) { g.fillStyle = '#565d69'; g.fillText(String(b + 1), x + 3, 3); }
  }
  data.tracks.forEach((t, i) => {
    const y = rulerH + i * laneH;
    const [r, gg, b] = t.color;
    g.strokeStyle = '#20242c';
    g.beginPath(); g.moveTo(0, y + laneH); g.lineTo(w, y + laneH); g.stroke();
    g.fillStyle = '#8a919e'; g.font = '10px sans-serif'; g.textBaseline = 'middle';
    g.fillText(t.name + (t.fx ? ' ⟲' : ''), 8, y + laneH / 2, headerW - 14);
    for (const c of t.clips) {
      const x0 = bx(c.start), x1 = bx(c.start + c.duration);
      g.fillStyle = 'rgba(' + r + ',' + gg + ',' + b + ',0.22)';
      g.strokeStyle = 'rgb(' + r + ',' + gg + ',' + b + ')';
      g.fillRect(x0, y + 2, x1 - x0, laneH - 5);
      g.strokeRect(x0 + 0.5, y + 2.5, x1 - x0 - 1, laneH - 6);
      const pitches = c.notes.map((n) => n[0]);
      if (!pitches.length) continue;
      const lo = Math.min(...pitches), hi = Math.max(...pitches);
      const py = (p) => y + laneH - 6 - (hi === lo ? 0.5 : (p - lo) / (hi - lo)) * (laneH - 12);
      g.fillStyle = 'rgb(' + r + ',' + gg + ',' + b + ')';
      for (let off = 0; off < c.duration; off += c.length) {
        for (const [p, s, len] of c.notes) {
          if (off + s >= c.duration) continue;
          const nx = bx(c.start + off + s);
          const nw = Math.max(1.5, bx(Math.min(c.duration, off + s + len)) - bx(off + s));
          g.fillRect(nx, py(p), nw, 2);
        }
      }
    }
  });
}
</script></body></html>`;

export async function deactivate() {
  playTerminal?.dispose();
  await client?.stop();
}
