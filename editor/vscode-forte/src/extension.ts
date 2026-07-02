// Forte VSCode extension: LSP diagnostics + play/build commands.
// The compiler is the single source of truth — this is a thin shell around
// the `forte` CLI (`forte lsp`, `forte play`, `forte build`).

import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from 'vscode-languageclient/node';

let client: LanguageClient | undefined;
let playTerminal: vscode.Terminal | undefined;

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
    })
  );
}

export async function deactivate() {
  playTerminal?.dispose();
  await client?.stop();
}
