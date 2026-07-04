// Forte Studio: LSP diagnostics, play/build, REPL, the read-only arrangement
// view, song history (VCS) and the fork-only Hub. The compiler and the CLI
// are the single source of truth — this is a thin shell around `forte`.

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

function hubFlags(): string[] {
  const dir = vscode.workspace.getConfiguration('forte').get<string>('hub');
  return dir ? ['--hub', dir] : [];
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

// ---- Hub view: listen, fork, publish — the fork-only ecosystem -------------

class HubItem extends vscode.TreeItem {
  constructor(
    public readonly repo: string,
    v: number,
    kind: string,
    author: string,
    forkedFrom: string | undefined,
    releases: number
  ) {
    super(`${repo} v${v}`, vscode.TreeItemCollapsibleState.None);
    this.description = `${kind} · ${author}${forkedFrom ? ` · ⑂ ${forkedFrom}` : ''}`;
    this.tooltip = releases ? `releases: ${releases}` : undefined;
    this.contextValue = 'hubRepo';
    this.iconPath = new vscode.ThemeIcon(kind === 'library' ? 'library' : 'music');
  }
}

class HubProvider implements vscode.TreeDataProvider<HubItem> {
  private ev = new vscode.EventEmitter<void>();
  readonly onDidChangeTreeData = this.ev.event;
  refresh() {
    this.ev.fire();
  }
  getTreeItem(e: HubItem) {
    return e;
  }
  async getChildren(): Promise<HubItem[]> {
    try {
      const data: {
        repos: {
          name: string;
          v: number;
          kind: string;
          author: string;
          forked_from: { repo: string; v: number } | null;
          releases: number;
        }[];
      } = JSON.parse(await forte(['hub', 'list', '--json', ...hubFlags()], repoCwd()));
      return data.repos.map(
        (r) =>
          new HubItem(
            r.name,
            r.v,
            r.kind,
            r.author,
            r.forked_from ? `${r.forked_from.repo} v${r.forked_from.v}` : undefined,
            r.releases
          )
      );
    } catch {
      return [];
    }
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

  // --- Forte Studio: History (VCS) + Hub sidebars ---------------------------
  const history = new HistoryProvider();
  const hub = new HubProvider();
  const err = (e: unknown) => vscode.window.showErrorMessage(`Forte: ${(e as Error).message}`);

  context.subscriptions.push(
    vscode.window.createTreeView('forteHistory', { treeDataProvider: history }),
    vscode.window.createTreeView('forteHub', { treeDataProvider: hub }),
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
    }),

    vscode.commands.registerCommand('forte.refreshHub', () => hub.refresh()),
    vscode.commands.registerCommand('forte.hubListen', async (item: HubItem) => {
      try {
        const entry = await forte(['hub', 'entry', item.repo, ...hubFlags()], repoCwd());
        playTerminal?.dispose();
        playTerminal = vscode.window.createTerminal(`Forte: ${item.repo}`);
        playTerminal.show(true);
        playTerminal.sendText(`${fortePath()} play "${entry}"`);
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.hubFork', async (item: HubItem) => {
      const root = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
      const dest = await vscode.window.showInputBox({
        prompt: 'fork 先フォルダ',
        value: root ? path.join(root, 'forks', item.repo) : item.repo,
      });
      if (!dest) return;
      try {
        const out = await forte(['hub', 'fork', item.repo, dest, ...hubFlags()], repoCwd());
        const open = await vscode.window.showInformationMessage(out, 'フォルダを開く');
        if (open) {
          await vscode.commands.executeCommand('vscode.openFolder', vscode.Uri.file(dest), {
            forceNewWindow: true,
          });
        }
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.hubLineage', async (item: HubItem) => {
      try {
        await showReport(
          `forte hub lineage ${item.repo}`,
          await forte(['hub', 'lineage', item.repo, ...hubFlags()], repoCwd())
        );
      } catch (e) {
        err(e);
      }
    }),
    vscode.commands.registerCommand('forte.hubVerify', (item: HubItem) =>
      vscode.window.withProgress(
        { location: vscode.ProgressLocation.Notification, title: `${item.repo} を検証中…` },
        async () => {
          try {
            vscode.window.showInformationMessage(
              await forte(['hub', 'verify', item.repo, ...hubFlags()], repoCwd())
            );
          } catch (e) {
            err(e);
          }
        }
      )
    ),
    vscode.commands.registerCommand('forte.hubPublish', async () => {
      const file = activeForteFile();
      if (!file) return;
      const name = await vscode.window.showInputBox({
        prompt: 'hub 上の名前(空なら ファイル名)',
        value: path.basename(file, '.forte'),
      });
      if (name === undefined) return;
      try {
        const args = ['hub', 'publish', file, ...(name ? ['--as', name] : []), ...hubFlags()];
        vscode.window.showInformationMessage(await forte(args, repoCwd()));
        hub.refresh();
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
  #brand { position: fixed; top: 8px; right: 12px; display: flex; gap: 6px;
    align-items: center; color: #e8b34c; font: 11px/1 sans-serif;
    letter-spacing: 2px; opacity: 0.75; pointer-events: none; }
</style></head>
<body><canvas id="c"></canvas><div id="err"></div>
<div id="brand"><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" width="18" height="18">
<path d="M 78 24 C 65 20 58 30 55 46 L 46 98 C 43 114 33 122 22 117" fill="none" stroke="#e8b34c" stroke-width="9" stroke-linecap="round"/>
<path d="M 24 66 L 58 66 L 63 56 L 70 78 L 76 50 L 83 80 L 89 60 L 93 66 L 104 66" fill="none" stroke="#4fb6c8" stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>
</svg>FORTE</div>
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
