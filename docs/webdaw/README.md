# Forte (仮称) — ドキュメント体系

「コードで作曲し、fork 系譜で貢献が追跡される」音楽制作プラットフォームの設計文書。
IEC 62304 のプロセス規律(要求→アーキテクチャ→詳細設計のトレーサビリティ)を採用。

| # | 文書 | 内容 |
| --- | --- | --- |
| 00 | [research-report](00-research-report.md) | Web DAW 市場・技術調査(2026-07)。競合・OSS・プラットフォーム成熟度・AI トレンド |
| 01 | [vision](01-vision.md) | 製品ビジョン: 音楽のホワイトボックス化 / fork 系譜 / 決定論的ビルド |
| 02 | [system-requirements](02-system-requirements.md) | システム要求仕様(SYS)+リスク管理 |
| 03 | [software-requirements](03-software-requirements.md) | ソフトウェア要求仕様(SRS)+トレーサビリティ |
| 04 | [software-architecture](04-software-architecture.md) | アーキテクチャ設計(SAD)+意思決定記録(ADR) |
| 05 | [detailed-design](05-detailed-design.md) | 詳細設計(SDD): 言語スケッチ・エンジン・録音・Hub |
| 06 | [roadmap](06-roadmap.md) | 開発ロードマップ(Phase 0–5)+リスクレジスタ |
| 07 | [determinism-spike](07-determinism-spike.md) | Phase 0.4 スパイク結果: native/wasm ビット同一レンダリング達成 |
| spec | [forte-lang-v0](spec/forte-lang-v0.md) | Forte lang 言語仕様 v0 ドラフト |

## 実装の現在地

- **`crates/fortelang`** — 言語 v0 スライス: lexer/parser/検査(診断コード付き)、
  dawcore へのコンパイル、`forte check` / `forte build`(WAV + build.manifest.json)/
  `forte play`(リアルタイム再生+保存で即反映のホットリロード。音声デバイスがなければ
  無音バックエンドで走行)。
- **`songs/`** — リファレンス曲 3 曲(`first-light` 4/4、`slow-circles` 6/8、
  `night-parade`: prog 進行リテラル・section・send/return・chords/arp/bass)。
- **`editor/vscode-forte`** — VSCode 拡張: シンタックスハイライト、`forte lsp` に
  よるリアルタイム診断、Play(ホットリロード)/Build/Stop コマンド。
- **`web/` + `crates/forteweb`** — ブラウザエディタのプロトタイプ:
  メインスレッドの wasm がタイプ中診断とビルド証明、AudioWorklet 内の wasm が
  再生+保存なしホットリロード。`scripts/build_web.sh` でビルドし、リポジトリ
  ルートを静的配信して `/web/` を開く。E2E は `scripts/web_e2e.mjs`(実 Chromium で
  「ブラウザのビルドダイジェスト == ネイティブ CLI」まで検証済 = **native / wasip1 /
  ブラウザの三者ビット同一**)。
- **`scripts/determinism_test.sh`** — 決定論ゲート 2 段(エンジン単体 / forte build 経由)。
  どちらも native x86_64 と wasm32-wasip1 でビット同一を CI 検証できる。

## 意思決定の状態

- **D-01 承認済(2026-07-02)**: コアは Rust(C ABI で API 化)
- **D-02 承認済(2026-07-02)**: 独自 DSL
- 未決: 名称(Forte は仮)、系譜保存ライセンスの法的レビュー着手時期
