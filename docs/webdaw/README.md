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

## 承認待ちの意思決定(創業者判断)

- **D-01**: コア実装言語 — Rust(推奨、dawcore 資産流用)か C++ か
- **D-02**: 独自 DSL(推奨)か既存言語への埋め込みか
- 名称(Forte は仮)
- 系譜保存ライセンスの法的レビュー着手時期
