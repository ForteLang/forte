# Forte for VSCode

`.forte` の曲をコードとして書くための拡張。

- **診断**: 入力中にコンパイラのエラー(E-* コード)がその場に出る(`forte lsp`)
- **シンタックスハイライト**: キーワード、`beat`/`notes`/`prog` リテラル、デバイス名
- **コマンド**:
  - `Forte: Play (hot reload)` — ループ再生。**保存するたびに再生を止めずに反映**
  - `Forte: Build` — WAV + build.manifest.json を出力
  - `Forte: Stop Playback`

## セットアップ

1. CLI をビルド: リポジトリルートで `cargo build --release -p fortelang`
2. 設定 `forte.path` に `<repo>/target/release/forte` を指定
   (PATH に置くなら不要)
3. この拡張をビルド: `npm install && npm run compile`
4. 開発実行: この `editor/vscode-forte` フォルダを VSCode で開き **F5**
   (Extension Development Host)。`.vsix` にするなら `npx vsce package`
