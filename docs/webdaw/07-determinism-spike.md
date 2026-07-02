# 決定論スパイク結果 (Phase 0.4)

実施日: 2026-07-02 / 結果: **成功 — native と wasm32 でビット同一のレンダリングを達成**
対応要求: SYS-ENG-001, SRS-CORE-003 (D-11)

## 方法

既存 dawcore のデモプロジェクト(シンセ+サンプラー+エフェクト+モジュレータ+
メトロノイズなしの 20 秒アレンジ)を、同一エンジン(`bounce` と同じ経路)で
オフラインレンダリングし、全サンプルの f32 ビットパターンのダイジェスト
(FNV-1a 64)を比較した。

- native: x86_64-unknown-linux-gnu(rustc 1.94.1, release)
- wasm: wasm32-wasip1(同 rustc)、Node 22 の WASI で実行
- 再現: `scripts/determinism_test.sh`(検証コード: `crates/dawcore/examples/determinism.rs`)

## 結果

| 段階 | f32 digest (native / wasm) | 一致 |
| --- | --- | --- |
| 修正前(std の float メソッド) | `a287cd7994449b0a` / `52b1fa18e9084db2` | ✗ |
| 修正後(libm 統一) | `aa68277c9dbb8161` / `aa68277c9dbb8161` | **✓ ビット同一** |

修正前の不一致の実態(1,920,000 サンプル中):
- ビット不一致 63.9%、ただし**最大絶対差 1.49e-7(≈ -136 dBFS、不可聴)**
- 16bit 量子化後は 0.018% が 1 LSB 差のみ
- 原因: `f32::sin/cos/tan/exp/tanh/powf` が native では glibc、wasm では
  compiler-builtins に解決され、実装が異なるため(最初の差はサンプル 6 で発生)

## 修正内容

`crates/dawcore/src/dmath.rs` を新設し、超越関数 6 種を純 Rust の `libm`
クレートに固定。DSP 内の呼び出し 18 箇所を `crate::dmath::*` に置換。
`sqrt/abs/floor/round/fract/min/max` は IEEE 正確(全ターゲット同一)のため変更不要。
既存テスト 13 件はすべて合格。

## 結論と含意

1. **SYS-ENG-001(クロスターゲット決定論)は達成可能**。ロードマップの
   後退プラン(wasm 統一への縮退)は不要。
2. D-11 の規約のうち「超越関数の単一実装」が唯一の実際の障害だった。
   FMA 縮約・denormal は今回のコード経路では問題にならなかった
   (Rust は既定で縮約せず、wasm は subnormal を完全サポート)。
3. 決定論 CI ゲートの原型が `scripts/determinism_test.sh` として動作している。
   forte-core 開発ではこれを PR ゲート化する(リファレンスコーパスに拡張)。

## 残リスク(継続監視)

- aarch64(Apple Silicon)native は未検証 — Rust の f32 演算は IEEE 準拠のため
  一致する見込みだが、CI マトリクスに追加して確認する。
- 将来のマルチスレッドレンダリングでは加算順序の固定(D-11 §5)が必要になる。
- wasm の NaN ビットパターンは非正規(nondeterministic canonicalization)—
  音声経路に NaN を流さない規約(SDD §7 の NaN ガード)で回避する。
- `libm` クレートのバージョン更新で数値が変わりうる → forte.lock 相当で
  エンジンバージョンに数値実装のバージョンを含める(SRS-BLD-002 に反映済み)。
