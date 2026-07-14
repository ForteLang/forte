// Forte Studio: LSP diagnostics, play/build, REPL, a drag-editable
// arrangement view, a beat grid and song history (VCS). The compiler and the
// CLI are the single source of truth — this is a thin shell around `forte`.
// Architecture (ADR D-13): VS Code IS the Studio shell — git, GitHub, AI
// assistants and terminals come from the host; this extension owns only what
// is unique to Forte: the code↔GUI projections over the lossless edit layer
// (`forte edit`), playback and the listening instruments.

import { execFile } from 'child_process';
import * as path from 'path';
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

/** Run the forte CLI, resolving stdout (trimmed). */
function forte(args: string[], cwd?: string): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(fortePath(), args, { cwd, maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) reject(new Error((stderr || String(err)).trim()));
      else resolve(stdout.trim());
    });
  });
}

/** Directory whose enclosing .forte repo VCS commands act on. */
function repoCwd(): string | undefined {
  const doc = vscode.window.activeTextEditor?.document;
  if (doc && !doc.isUntitled && doc.uri.scheme === 'file') return path.dirname(doc.fileName);
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
}

async function showReport(title: string, body: string) {
  const doc = await vscode.workspace.openTextDocument({
    content: `// ${title}\n${body}\n`,
    language: 'plaintext',
  });
  await vscode.window.showTextDocument(doc, { preview: true, viewColumn: vscode.ViewColumn.Beside });
}

// ---- History view: the song's commits, diffed in music vocabulary ----------

class CommitItem extends vscode.TreeItem {
  constructor(public readonly hash: string, n: number, message: string, author: string, merge: boolean) {
    super(`#${n} ${message}`, vscode.TreeItemCollapsibleState.None);
    this.description = `${author}${merge ? ' (merge)' : ''}`;
    this.tooltip = hash;
    this.contextValue = 'commit';
    this.iconPath = new vscode.ThemeIcon(merge ? 'git-merge' : 'git-commit');
  }
}

class HistoryProvider implements vscode.TreeDataProvider<CommitItem> {
  private ev = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.ev.event;
  refresh() {
    this.ev.fire();
  }
  getTreeItem(e: CommitItem) {
    return e;
  }
  async getChildren(): Promise<CommitItem[]> {
    const cwd = repoCwd();
    if (!cwd) return [];
    try {
      const log: { hash: string; n: number; message: string; author: string; parents: string[] }[] =
        JSON.parse(await forte(['log', '--json'], cwd));
      return log.map((c) => new CommitItem(c.hash, c.n, c.message, c.author, c.parents.length > 1));
    } catch {
      return []; // not a repo yet — the view stays empty
    }
  }
}



// ---- Blocks view: the workspace's blocks, playable and traceable ----------
class BlockItem extends vscode.TreeItem {
  constructor(
    public readonly file: vscode.Uri | null,
    public readonly block: string | null,
    label: string,
    collapsible: vscode.TreeItemCollapsibleState
  ) {
    super(label, collapsible);
    if (block && file) {
      this.contextValue = 'forteBlock';
      this.iconPath = new vscode.ThemeIcon('symbol-namespace');
      this.command = {
        command: 'vscode.open',
        title: 'open',
        arguments: [file],
      };
    } else {
      this.iconPath = new vscode.ThemeIcon('file-code');
    }
  }
}

class BlocksProvider implements vscode.TreeDataProvider<BlockItem> {
  private ev = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.ev.event;
  refresh() {
    this.ev.fire();
  }
  getTreeItem(e: BlockItem) {
    return e;
  }
  async getChildren(parent?: BlockItem): Promise<BlockItem[]> {
    if (parent) {
      if (!parent.file || parent.block) return [];
      const text = new TextDecoder().decode(await vscode.workspace.fs.readFile(parent.file));
      return [...text.matchAll(/^\s*block\s+([A-Za-z_@][\w@#]*)/gm)].map(
        (m) => new BlockItem(parent.file, m[1], m[1], vscode.TreeItemCollapsibleState.None)
      );
    }
    const files = await vscode.workspace.findFiles('**/*.forte', '**/target/**');
    const out: BlockItem[] = [];
    for (const f of files.sort((a, b) => a.fsPath.localeCompare(b.fsPath))) {
      const text = new TextDecoder().decode(await vscode.workspace.fs.readFile(f));
      if (/^\s*block\s+[A-Za-z_@]/m.test(text)) {
        out.push(
          new BlockItem(
            f,
            null,
            vscode.workspace.asRelativePath(f),
            vscode.TreeItemCollapsibleState.Collapsed
          )
        );
      }
    }
    return out;
  }
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
  // Panels steal focus from the text editor, so remember the last .forte
  // document: "Forte: Beat Grid" must work while the arrangement has focus.
  let lastForteFile: string | undefined;
  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor((e) => {
      if (e?.document.languageId === 'forte') lastForteFile = e.document.fileName;
    })
  );
  const activeForteFile = (): string | undefined => {
    const doc = vscode.window.activeTextEditor?.document;
    if (doc?.languageId === 'forte') {
      doc.save();
      lastForteFile = doc.fileName;
      return doc.fileName;
    }
    if (lastForteFile) return lastForteFile;
    vscode.window.showErrorMessage('Forte: open a .forte file first.');
    return undefined;
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
    // --- beat grid: pattern literals as clickable step rows ------------------
    vscode.commands.registerCommand('forte.showGrid', () => {
      const file = activeForteFile();
      if (!file) return;
      openGrid(context, file);
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.languageId !== 'forte') return;
      if (vizPanel) refreshViz(doc.fileName);
      if (gridPanel) refreshGrid(doc.fileName);
    })
  );

  // --- Forte Studio: History (VCS) + Blocks sidebars -------------------------
  const history = new HistoryProvider();
  const blocks = new BlocksProvider();
  const err = (e: unknown) => vscode.window.showErrorMessage(`Forte: ${(e as Error).message}`);

  context.subscriptions.push(
    vscode.window.createTreeView('forteHistory', { treeDataProvider: history }),
    vscode.window.createTreeView('forteBlocks', { treeDataProvider: blocks }),
    vscode.commands.registerCommand('forte.refreshBlocks', () => blocks.refresh()),
    vscode.commands.registerCommand('forte.blockListen', (item: BlockItem) => {
      if (!item?.file || !item.block) return;
      playTerminal?.dispose();
      playTerminal = vscode.window.createTerminal(`Forte: ${item.block}`);
      playTerminal.show(true);
      playTerminal.sendText(`${fortePath()} play "${item.file.fsPath}" --block ${item.block}`);
    }),
    vscode.commands.registerCommand('forte.blockRefs', async (item: BlockItem) => {
      if (!item?.block) return;
      const files = await vscode.workspace.findFiles('**/*.forte', '**/target/**');
      const refs: vscode.Uri[] = [];
      const needle = new RegExp(`(play\\s+${item.block}\\b|import\\s*\\{[^}]*\\b${item.block}\\b)`);
      for (const f of files) {
        if (item.file && f.fsPath === item.file.fsPath) continue;
        const text = new TextDecoder().decode(await vscode.workspace.fs.readFile(f));
        if (needle.test(text)) refs.push(f);
      }
      if (!refs.length) {
        vscode.window.showInformationMessage(`${item.block} を使う曲は(まだ)ありません`);
        return;
      }
      const pick = await vscode.window.showQuickPick(
        refs.map((f) => vscode.workspace.asRelativePath(f)),
        { title: `${item.block} を使っている場所(${refs.length})` }
      );
      if (pick) {
        const target = refs.find((f) => vscode.workspace.asRelativePath(f) === pick);
        if (target) vscode.window.showTextDocument(target);
      }
    }),
    vscode.window.onDidChangeActiveTextEditor(() => history.refresh()),

    vscode.commands.registerCommand('forte.refreshHistory', () => history.refresh()),
    vscode.commands.registerCommand('forte.commit', async () => {
      const cwd = repoCwd();
      if (!cwd) return;
      const msg = await vscode.window.showInputBox({ prompt: 'コミットメッセージ' });
      if (msg === undefined) return;
      try {
        await vscode.workspace.saveAll();
        // first commit needs a repo — create one on the fly
        const out = await forte(['commit', '-m', msg || 'edit'], cwd).catch(async (e: Error) => {
          if (!e.message.includes('リポジトリではありません')) throw e;
          await forte(['init'], cwd);
          return forte(['commit', '-m', msg || 'edit'], cwd);
        });
        vscode.window.setStatusBarMessage(`Forte: ${out}`, 5000);
        history.refresh();
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.diffCommit', async (item: CommitItem) => {
      try {
        const report = await forte(['diff', item.hash], repoCwd());
        await showReport(`forte diff ${item.hash.slice(0, 8)} → 作業ツリー`, report);
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.checkoutCommit', async (item: CommitItem) => {
      const ok = await vscode.window.showWarningMessage(
        `${item.label} の状態に戻しますか?(未コミットの変更があると拒否されます)`,
        { modal: true },
        '戻す'
      );
      if (ok !== '戻す') return;
      try {
        const out = await forte(['checkout', item.hash], repoCwd());
        vscode.window.setStatusBarMessage(`Forte: ${out}`, 5000);
        history.refresh();
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.merge', async () => {
      const branch = await vscode.window.showInputBox({ prompt: 'マージするブランチ名' });
      if (!branch) return;
      try {
        const out = await forte(['merge', branch], repoCwd());
        await showReport(`forte merge ${branch}`, out);
        history.refresh();
      } catch (e) {
        err(e);
      }
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

let vizFile: string | undefined;
let gridPanel: vscode.WebviewPanel | undefined;
let gridFile: string | undefined;

/** Apply a structured edit through the lossless edit layer and save.
 *  The op goes through the DOCUMENT (WorkspaceEdit), so it lands on the
 *  editor's undo stack like any keystroke — Cmd+Z undoes a GUI gesture. */
async function applyEditOp(file: string, op: unknown): Promise<void> {
  const doc = await vscode.workspace.openTextDocument(file);
  if (doc.isDirty) await doc.save();
  const out = await new Promise<string>((resolve, reject) => {
    execFile(
      fortePath(),
      ['edit', file, JSON.stringify(op)],
      { maxBuffer: 16 * 1024 * 1024 },
      (err, stdout, stderr) => (err ? reject(new Error((stderr || String(err)).trim())) : resolve(stdout))
    );
  });
  const edit = new vscode.WorkspaceEdit();
  edit.replace(
    doc.uri,
    new vscode.Range(doc.positionAt(0), doc.positionAt(doc.getText().length)),
    out
  );
  await vscode.workspace.applyEdit(edit);
  await doc.save(); // save → the panels refresh via onDidSaveTextDocument
}

function refreshGrid(file: string) {
  gridFile = file;
  execFile(fortePath(), ['edit', file, '--sites'], { maxBuffer: 16 * 1024 * 1024 }, (err, stdout, stderr) => {
    if (!gridPanel) return;
    if (err) {
      gridPanel.webview.postMessage({ kind: 'error', message: stderr || String(err) });
    } else {
      gridPanel.webview.postMessage({ kind: 'sites', data: JSON.parse(stdout) });
    }
  });
}

function openGrid(context: vscode.ExtensionContext, file: string) {
  if (!gridPanel) {
    gridPanel = vscode.window.createWebviewPanel(
      'forteGrid',
      'Forte: Beat Grid',
      vscode.ViewColumn.Beside,
      { enableScripts: true, retainContextWhenHidden: true }
    );
    gridPanel.onDidDispose(() => (gridPanel = undefined), null, context.subscriptions);
    gridPanel.webview.onDidReceiveMessage(
      async (m) => {
        if (m?.kind === 'edit' && gridFile) {
          try {
            await applyEditOp(gridFile, m.op);
          } catch (e) {
            vscode.window.showErrorMessage(`Forte: ${(e as Error).message}`);
            refreshGrid(gridFile); // resync the stale view
          }
        } else if (m?.kind === 'jump' && typeof m.line === 'number' && m.line >= 1 && gridFile) {
          const doc = await vscode.workspace.openTextDocument(gridFile);
          const ed = await vscode.window.showTextDocument(doc, { viewColumn: vscode.ViewColumn.One });
          const pos = new vscode.Position(m.line - 1, 0);
          ed.selection = new vscode.Selection(pos, pos);
          ed.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenter);
        }
      },
      null,
      context.subscriptions
    );
    gridPanel.webview.html = GRID_HTML;
  }
  refreshGrid(file);
}

function refreshViz(file: string) {
  vizFile = file;
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
    // clicks jump to source; drags re-place the play through the edit layer
    vizPanel.webview.onDidReceiveMessage(
      async (m) => {
        if (!vizFile) return;
        if (m?.kind === 'move' && typeof m.line === 'number' && Array.isArray(m.bars)) {
          try {
            await applyEditOp(vizFile, { op: 'move_at_line', line: m.line, bars: m.bars });
          } catch (e) {
            vscode.window.showErrorMessage(`Forte: ${(e as Error).message}`);
            refreshViz(vizFile);
          }
          return;
        }
        if (m?.kind !== 'jump' || typeof m.line !== 'number' || m.line < 1) return;
        const doc = await vscode.workspace.openTextDocument(vizFile);
        const ed = await vscode.window.showTextDocument(doc, {
          viewColumn: vscode.ViewColumn.One,
          preserveFocus: false,
        });
        const pos = new vscode.Position(m.line - 1, 0);
        ed.selection = new vscode.Selection(pos, pos);
        ed.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenter);
      },
      null,
      context.subscriptions
    );
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
  #brand { position: fixed; top: 8px; right: 12px; display: flex; gap: 6px;
    align-items: center; color: #e8b34c; font: 11px/1 sans-serif;
    letter-spacing: 2px; opacity: 0.75; pointer-events: none; }
</style></head>
<body><canvas id="c"></canvas><div id="err"></div>
<div id="brand"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="18" height="18">
<path d="M 78 24 C 65 20 58 30 55 46 L 46 98 C 43 114 33 122 22 117" fill="none" stroke="#e8b34c" stroke-width="9" stroke-linecap="round"/>
<path d="M 24 66 L 58 66 L 63 56 L 70 78 L 76 50 L 83 80 L 89 60 L 93 66 L 104 66" fill="none" stroke="#e8b34c" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>
</svg>FORTE</div>
<script>
const vscodeApi = acquireVsCodeApi();
const canvas = document.getElementById('c');
const g = canvas.getContext('2d');
let data = null;
let mode = 'arrange';   // 'arrange' | 'piano'
let rollTrack = 0;
window.addEventListener('message', (e) => {
  const m = e.data;
  if (m.kind === 'viz') { data = m.data; document.getElementById('err').textContent = ''; draw(); }
  else if (m.kind === 'error') { document.getElementById('err').textContent = m.message; }
});
new ResizeObserver(draw).observe(canvas);
// lane header click → piano roll; clip click → code jump; clip DRAG →
// re-place the play through the edit layer (bar-snapped, dashed ghost)
let ghost = null;   // {track, start, duration}
let drag = null;    // {track, clip, x0, moved, snapped}
let dragJustEnded = false;
function hitClip(x, y) {
  if (!data || !data.tracks?.length || mode === 'piano') return null;
  const rulerH = 16, headerW = 92;
  const laneH = (canvas.clientHeight - rulerH) / data.tracks.length;
  const i = Math.floor((y - rulerH) / laneH);
  const t = data.tracks[i];
  if (!t || x < headerW) return null;
  const span = Math.max(data.lengthBeats, data.beatsPerBar);
  const beats = ((x - headerW) / (canvas.clientWidth - headerW)) * span;
  const clip = t.clips.find((c) => beats >= c.start && beats <= c.start + c.duration);
  return clip ? { track: i, clip } : null;
}
canvas.addEventListener('mousedown', (ev) => {
  const rect = canvas.getBoundingClientRect();
  const hit = hitClip(ev.clientX - rect.left, ev.clientY - rect.top);
  if (hit) drag = { ...hit, x0: ev.clientX, moved: false };
});
window.addEventListener('mousemove', (ev) => {
  if (!drag || !data) return;
  const dx = ev.clientX - drag.x0;
  if (!drag.moved && Math.abs(dx) < 4) return;
  drag.moved = true;
  const headerW = 92;
  const span = Math.max(data.lengthBeats, data.beatsPerBar);
  const pxPerBeat = (canvas.clientWidth - headerW) / span;
  const bpb = data.beatsPerBar;
  drag.snapped = Math.max(0, Math.round((drag.clip.start + dx / pxPerBeat) / bpb)) * bpb;
  ghost = { track: drag.track, start: drag.snapped, duration: drag.clip.duration };
  draw();
});
window.addEventListener('mouseup', () => {
  if (!drag) return;
  const { clip, moved, snapped } = drag;
  drag = null;
  dragJustEnded = moved;
  ghost = null;
  draw();
  if (!moved || snapped === undefined || snapped === clip.start || !clip.line) return;
  const bpb = data.beatsPerBar;
  const a = Math.round(snapped / bpb) + 1;
  const durBars = Math.max(1, Math.round(clip.duration / bpb));
  vscodeApi.postMessage({ kind: 'move', line: clip.line, bars: [a, a + durBars - 1] });
});
canvas.addEventListener('click', (ev) => {
  if (dragJustEnded) { dragJustEnded = false; return; }
  if (!data || !data.tracks?.length) return;
  const rect = canvas.getBoundingClientRect();
  const x = ev.clientX - rect.left, y = ev.clientY - rect.top;
  if (mode === 'piano') { mode = 'arrange'; draw(); return; }
  const rulerH = 16, headerW = 92;
  const laneH = (canvas.clientHeight - rulerH) / data.tracks.length;
  const i = Math.floor((y - rulerH) / laneH);
  const t = data.tracks[i];
  if (!t) return;
  if (x < headerW) { mode = 'piano'; rollTrack = i; draw(); return; }
  const span = Math.max(data.lengthBeats, data.beatsPerBar);
  const beats = ((x - headerW) / (canvas.clientWidth - headerW)) * span;
  const clip = t.clips.find((c) => beats >= c.start && beats <= c.start + c.duration);
  const line = (clip && clip.line) || t.line || 0;
  if (line > 0) vscodeApi.postMessage({ kind: 'jump', line });
});
function draw() {
  const dpr = devicePixelRatio || 1;
  const w = canvas.clientWidth, h = canvas.clientHeight;
  canvas.width = w * dpr; canvas.height = h * dpr;
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, w, h);
  if (!data || !data.tracks?.length) return;
  if (mode === 'piano') return drawPianoRoll(w, h);
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
  if (ghost) {
    const y = rulerH + ghost.track * laneH;
    g.save();
    g.setLineDash([4, 3]);
    g.strokeStyle = '#e8b34c';
    g.strokeRect(bx(ghost.start) + 0.5, y + 2.5, bx(ghost.start + ghost.duration) - bx(ghost.start) - 1, laneH - 6);
    g.restore();
  }
}
// Piano roll of one track: pitch rows over time, loops unrolled, velocity
// as opacity — the same projection web/viz.js draws.
function drawPianoRoll(w, h) {
  const t = data.tracks[rollTrack];
  if (!t) return;
  const headerW = 34, rulerH = 16;
  const span = Math.max(data.lengthBeats, data.beatsPerBar);
  const bx = (beats) => headerW + ((w - headerW) * beats) / span;
  const notes = [];
  let lo = 127, hi = 0;
  for (const c of t.clips) {
    for (let off = 0; off < c.duration; off += c.length) {
      for (const [p, s, len, vel] of c.notes) {
        if (off + s >= c.duration) continue;
        notes.push([p, c.start + off + s, Math.min(len, c.duration - off - s), vel ?? 0.8]);
        if (p < lo) lo = p;
        if (p > hi) hi = p;
      }
    }
  }
  g.fillStyle = '#8a919e'; g.font = '10px sans-serif'; g.textBaseline = 'top';
  g.fillText('♪ ' + t.name + ' — piano roll (click to go back)', headerW + 6, 2);
  if (!notes.length) return;
  lo = Math.max(0, lo - 2); hi = Math.min(127, hi + 2);
  const rows = hi - lo + 1;
  const rowH = (h - rulerH) / rows;
  const py = (p) => rulerH + (hi - p) * rowH;
  for (let p = lo; p <= hi; p++) {
    const black = [1, 3, 6, 8, 10].includes(p % 12);
    g.fillStyle = black ? 'rgba(0,0,0,0.22)' : 'rgba(255,255,255,0.02)';
    g.fillRect(headerW, py(p), w - headerW, rowH);
    if (p % 12 === 0 && rowH >= 5) {
      g.fillStyle = '#565d69'; g.font = '8px monospace'; g.textBaseline = 'middle';
      g.fillText('C' + (Math.floor(p / 12) - 1), 4, py(p) + rowH / 2);
    }
  }
  for (let bnum = 0; bnum * data.beatsPerBar <= span; bnum++) {
    const x = bx(bnum * data.beatsPerBar);
    g.strokeStyle = bnum % 4 === 0 ? '#2f3440' : '#232730';
    g.beginPath(); g.moveTo(x, rulerH); g.lineTo(x, h); g.stroke();
  }
  const [r, gg, b] = t.color;
  for (const [p, s, len, vel] of notes) {
    const x0 = bx(s);
    const nw = Math.max(2, bx(s + len) - x0);
    g.fillStyle = 'rgba(' + r + ',' + gg + ',' + b + ',' + (0.35 + 0.65 * vel) + ')';
    g.fillRect(x0, py(p) + 0.5, nw, Math.max(1.5, rowH - 1));
  }
}
</script></body></html>`;

// Beat grid: every `beat` literal as a row of step cells. A click cycles
// - → x → X → . and posts the exact set_pattern op back; the extension runs
// it through `forte edit`, so the diff touches only the literal's contents.
const GRID_HTML = /* html */ `<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  html, body { margin: 0; background: #14161b; color: #d7dae0;
    font: 12px/1.6 ui-monospace, monospace; }
  #wrap { padding: 10px 14px; }
  #err { color: #e06c75; white-space: pre-wrap; }
  .hint { color: #565d69; }
  .row { margin-bottom: 10px; }
  .label { display: block; color: #8a919e; margin-bottom: 3px; cursor: pointer;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .label:hover { color: #d7dae0; }
  .cells { display: flex; gap: 2px; flex-wrap: wrap; }
  .cells button { padding: 0; width: 18px; height: 18px; flex: none;
    font: 10px ui-monospace, monospace; border-radius: 3px;
    border: 1px solid #3a3f4b; background: #22262e; color: #6a7280; cursor: pointer; }
  .cells button:nth-child(4n+1):not(:first-child) { margin-left: 5px; }
  .cells button.hit { background: #e8b34c; color: #16181d; border-color: #e8b34c; }
  .cells button.acc { background: #f6cd7c; color: #16181d; border-color: #f6cd7c; font-weight: 700; }
  .cells button.ghost { background: #5a4a2a; color: #d7dae0; border-color: #6a5a33; }
</style></head>
<body><div id="wrap"><div class="hint">—</div></div><div id="err"></div>
<script>
const vscodeApi = acquireVsCodeApi();
const STEP_CLASS = { X: 'acc', x: 'hit', '.': 'ghost', '-': '' };
const STEP_NEXT = { '-': 'x', x: 'X', X: '.', '.': '-' };
function parseSteps(raw) {
  const s = raw.replace(/\\s+/g, '');
  const out = [];
  for (let i = 0; i < s.length; i++) {
    let t = s[i];
    if (s[i + 1] === '*') {
      let j = i + 2, d = '';
      while (j < s.length && /\\d/.test(s[j])) d += s[j++];
      t += '*' + d;
      i = j - 1;
    }
    out.push(t);
  }
  return out;
}
function cycle(t) {
  const head = STEP_NEXT[t[0]] ?? 'x';
  return head === '-' ? '-' : head + t.slice(1);
}
function join(steps) {
  if (steps.length % 4 !== 0) return steps.join('');
  const groups = [];
  for (let i = 0; i < steps.length; i += 4) groups.push(steps.slice(i, i + 4).join(''));
  return groups.join(' ');
}
window.addEventListener('message', (e) => {
  const m = e.data;
  if (m.kind === 'error') { document.getElementById('err').textContent = m.message; return; }
  if (m.kind !== 'sites') return;
  document.getElementById('err').textContent = '';
  const el = document.getElementById('wrap');
  el.textContent = '';
  const rows = m.data.filter((s) => s.kind === 'beat' && !s.raw.trim().startsWith('euclid('));
  if (!rows.length) {
    el.innerHTML = '<div class="hint">beat リテラルがありません</div>';
    return;
  }
  for (const site of rows) {
    const row = document.createElement('div');
    row.className = 'row';
    const label = document.createElement('span');
    label.className = 'label';
    const where = site.path?.length ? site.path.join('/') + ' · ' : '';
    label.textContent = site.let_name
      ? where + 'let ' + site.let_name
      : where + site.track + (site.at ? ' @' + site.at : '');
    label.title = site.line + ' 行目へジャンプ';
    label.onclick = () => vscodeApi.postMessage({ kind: 'jump', line: site.line });
    row.appendChild(label);
    const cells = document.createElement('div');
    cells.className = 'cells';
    const steps = parseSteps(site.raw);
    steps.forEach((step, i) => {
      const b = document.createElement('button');
      b.textContent = step.length > 1 ? step[0] + '*' : step === '-' ? '' : step;
      b.title = step;
      b.className = STEP_CLASS[step[0]] ?? '';
      b.onclick = () => {
        const next = steps.slice();
        next[i] = cycle(step);
        const op = { op: 'set_pattern', path: site.path ?? [], value: join(next) };
        if (site.let_name) op.let_name = site.let_name;
        else { op.track = site.track; op.play = site.play; }
        vscodeApi.postMessage({ kind: 'edit', op });
      };
      cells.appendChild(b);
    });
    row.appendChild(cells);
    el.appendChild(row);
  }
});
</script></body></html>`;

export async function deactivate() {
  playTerminal?.dispose();
  await client?.stop();
}
