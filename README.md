# Forte (仮称) — compose music as code

**音楽制作を「コード・fork・ビルド・リリース」によるオープン開発の世界へ。**
曲も、パターンも、コード進行も、そして音源そのものも、読める・直せる・fork できる
ソースコード(`.forte`)。ビルドは決定論的で、同じコミットからは native / wasm /
ブラウザのどこでも**ビット同一のオーディオ**が再現される。リリースの正しさは
誰でも(ブラウザのタブからでも)再検証できる。

**📖 使い方ガイド(チュートリアル): [docs/GUIDE.md](docs/GUIDE.md)** /
言語リファレンス: [docs/webdaw/spec/forte-lang-v1.md](docs/webdaw/spec/forte-lang-v1.md) /
ビジョン・要求仕様・アーキテクチャ: [docs/webdaw/](docs/webdaw/README.md)
(IEC 62304 型のドキュメント体系)。

```forte
import { WarmLead, SubBass } from "./devices/warm.forte"

song "Handmade" {
  tempo 100bpm
  key G minor
  let line = prog`Gm | Eb | Bb | F`

  track Lead {
    instrument WarmLead(cutoff: 0.7, vib: 0.35)
    insert delay(time: 0.3, fdbk: 0.3, mix: 0.25)
    play arp(line, rate: 0.5, style: "updown") at bars(5..12)
  }
}
```

## Quickstart

```bash
cargo install --path crates/fortelang   # `forte` コマンドが入る(~/.cargo/bin)

forte repl                              # ★打った行がその場で鳴る
forte check songs/first-light.forte     # 検証(エラーは音楽の語彙+行番号)
forte play  songs/first-light.forte     # ライブ再生。保存するたび即反映
forte build songs/first-light.forte     # WAV + ビルド証明(digest 入り)
```

REPL はこんな感じ:

```
forte> beat`x--- x-x-`                     ← 即ループ再生
♪ playing (120 bpm, loop 32 beats)
forte> let theme = prog`Am | F | C | G`
forte> arp(theme, rate: 0.25, style: "updown")
♪ playing
forte> :inst polymer(wave: "saw")          ← 音色を差し替え(鳴りっぱなし)
forte> :fx reverb(mix: 0.3)
forte> :save jam.forte                     ← ジャムがそのまま曲ファイルになる
```

**ブラウザエディタ**(タイプ中診断・AudioWorklet 再生・OPFS 自動保存・完全オフライン PWA):

```bash
scripts/build_web.sh
python3 -m http.server 8000   # リポジトリルートで
# → http://localhost:8000/web/
```

**Hub**(fork 系譜レジストリ: 取得は fork のみ、来歴は構造的に記録される):

```bash
export FORTE_HUB=~/.forte-hub
forte hub publish songs/handmade.forte   # import ごとスナップショット
forte hub release handmade               # 決定論ビルド → ダイジェストを台帳へ
forte hub verify handmade                # 誰でも再現検証できる
forte hub serve                          # → http://localhost:8000/web/hub.html で系譜をディグる
```

**VSCode**: `editor/vscode-forte/`(シンタックスハイライト+ `forte lsp` 診断+
Play/Build コマンド)。

## リポジトリ構成

```
crates/dawcore    リアルタイムエンジン+DSP(ロックフリー、決定論、no GUI)
crates/fortelang  言語: lexer/parser/検査、コンパイラ、CLI(check/build/play/lsp/hub)
crates/forteweb   ブラウザ用 C-ABI wasm(コンパイル・再生・ビルド証明)
web/              ブラウザエディタ+Hub 系譜ページ(PWA)
editor/           VSCode 拡張
songs/            リファレンス曲 4 曲+デバイスライブラリ
docs/webdaw/      ビジョン/SYS/SRS/SAD/SDD/ロードマップ+調査レポート
scripts/          決定論ゲート・ブラウザ E2E
```

## テスト

```bash
cargo test -p dawcore -p fortelang     # エンジン+言語+Hub(23 tests)
scripts/determinism_test.sh            # native/wasm ビット同一ゲート(2 段)
node scripts/web_e2e.mjs               # ブラウザ E2E 8 項目(要 playwright)
node scripts/hub_e2e.mjs               # Hub E2E 6 項目
```

---

エンジン(`dawcore`)は本リポジトリの前身である Bitwig Studio 風 DAW の実装から
流用しており、その規律(音声スレッドで割り当てない・ロックしない、UI→audio は
ロックフリーリング、オフラインとリアルタイムが同一エンジン)が Forte の決定論
ビルドの土台になっている。
