# ソフトウェア詳細設計 (SDD) — Forte

Status: Draft v0.1 / 2026-07-02
上位文書: 04-software-architecture.md (SAD)
本書は Phase 0–2(ロードマップ参照)の実装対象を詳細化する。以降のフェーズは追補する。

---

## 1. Forte lang 言語スケッチ

構文は確定仕様ではなく、言語設計の意図を示す参照例である(Phase 0 で仕様化)。

```forte
// song.forte — 曲はコードである
import { tr909 }        from "@rhythm/tr909@^2.1"        // fork 系譜が forte.lock に残る
import { juno }         from "@keys/juno-strings@1.0"
import { tape, limiter} from "@fx/std-master@^3"
import { section }      from "@arrange/pop-skeleton@0.4"  // 曲の骨子もモジュール
import vocalTake        from "../assets/vocal_take3.frec" // 来歴付き録音のみ参照可能

song "Aozora" {
  tempo 92
  meter 4/4
  key   D maj

  // パターンはデータであり値である
  let kick  = beat`x--- x--- x-x- x---`
  let chord = prog`Dmaj7 | Bm7 | Em7 A7`   // 進行も一級の値 → 類似検索の対象になる

  track Drums {
    instrument tr909(kick: .deep, hat: .tight)
    play kick at bars(1..32)
    automate hat.decay from 0.2 to 0.6 over bars(17..24)
  }

  track Keys {
    instrument juno(voices: 8)
    play arp(chord, style: .updown, rate: 1/8) at section.verse
  }

  track Vocal {
    audio vocalTake                 // MIDI とマイク由来アセット以外は型エラー
    insert tape(drive: 0.25)
  }

  bus Master {
    insert limiter(ceiling: -0.3dB)
  }
}
```

```forte
// DSP 層の例 — 音源も同じ言語のサブセットで書く(public 公開はソース必須)
device MonoSaw : Instrument {
  param cutoff: Hz = 800.0 in 20.0..18_000.0
  state phase: f32 = 0.0
  state svf:   Svf = Svf::lowpass()

  on note(n: Note) { phase = 0.0 }

  process(frame: &mut Frame, ctx: &Ctx) {
    let s = saw_blep(&mut phase, ctx.pitch_hz)
    frame.mono( svf.run(s, cutoff, q: 0.7) * ctx.env() )
  }
}
```

### 1.1 主要設計点

- **時間の型**: `bars/beats`(拍) と `sec`(秒) は別型。混合は明示変換のみ(SRS-LANG-004)。
- **コンパイル時展開**: Score 層の制御構造(repeat/map/if)はコンパイル時に評価され、
  イベント列とレンダーグラフに完全展開される。実行時の任意コードは DSP 層の
  `process` のみ(決定論とビルド速度の要)。
- **乱数**: `random(seed:)` のみ。シードは forte.lock に固定され、ビルド再現に含まれる。
- **アセット参照**: `import x from "*.frec"` は型 `RecordedAudio` を持ち、
  来歴ブロック検証(署名+ハッシュ)がコンパイル時に走る。検証失敗はエラー(SRS-REC-003)。
- **正規形**: `forte fmt` が唯一の整形を定める。AST 正規形は類似検索(進行抽出)の基盤。

## 2. コンパイラパイプライン

```
.forte ──parse──► AST ──resolve(imports, forte.lock)──► 型検査
   ──lower──► IR(イベント列 + グラフ定義 + DSPカーネル)
   ──codegen──► native: cranelift or LLVM / wasm: wasm32 モジュール
   ──cache──► モジュール単位のコンパイルキャッシュ(コンテンツハッシュキー)
```

- 差分ビルド: 変更モジュールと依存下流のみ再 lower/codegen。イベント列の差分から
  「変わったトラック/リージョン」を特定しエンジンに部分差し替えを指示(SRS-LANG-007)。
- DSP カーネルの codegen は Phase 0 ではインタープリタ(Rust 実装のオペレータ木)で開始し、
  Phase 2 で JIT/事前コンパイルに移行してよい(決定論規約は両者で同一の数値経路を要求)。

## 3. forte-core(エンジン)詳細

### 3.1 レンダーグラフ

```rust
struct Graph {
    nodes: Vec<Node>,          // Source(instrument) / Fx / Bus / Meter / Sink
    edges: Vec<(NodeId, PortId, NodeId, PortId)>, // audio / control
    order: Vec<NodeId>,        // トポロジカル順(コンパイラが決定・固定)
}
```

- 実行順はコンパイラが決定しグラフに焼き込む(実行時ソートなし=順序決定論)。
- ノードの処理は 128 サンプルブロック単位。サンプル精度イベント(ノート、
  オートメーション点)はブロック内オフセット付きでノードに配送(dawcore 方式)。
- ホットスワップ: 新旧グラフの差分ノードのみ置換。置換ノードは 10ms 等電力
  クロスフェード。状態(フィルタ履歴・ボイス)は NodeId が一致し型が同じなら移送する
  (SRS-CORE-006)。

### 3.2 スレッド/メモリ規律(dawcore 踏襲)

- RT スレッド(cpal コールバック / AudioWorklet process): 割り当て・ロック・syscall なし。
- 制御: SPSC リング(native: ringbuf クレート / web: SAB 上の自前 wait-free リング。
  ringbuf.js と同レイアウト)。Hot メッセージは Copy、構造物は Box 移送+ガベージ返却。
- 読み出し: メーター/再生位置/アンダーラン数は atomics publish。

### 3.3 浮動小数点決定論規約(D-11)

1. サンプル型は f32 固定。中間アキュムレータは f64 可(ただし使用箇所を規約で固定)。
2. `-ffast-math` 系最適化禁止。wasm/native 双方で FMA 縮約を禁止
   (Rust: デフォルトで縮約なし。`mul_add` は明示使用のみ=両ターゲットで同一)。
3. 超越関数(sin/exp/log/pow/tanh)は libm 依存禁止、自前の多項式近似実装
   `forte_math` を唯一の実装とする。
4. denormal は各フィルタ状態で明示 flush(加算オフセット法または量子化)。
   ※ CPU の FTZ フラグに依存しない(wasm に存在しないため)。
5. 並列レンダリングはトラック単位のワークスチールを行うが、**ミックスの加算順序は
   グラフ焼き込み順で固定**(決定論的リダクション)。Phase 0–1 は単スレッドで開始。
6. 検証: リファレンスコーパスの native/wasm 出力 SHA-256 一致を CI ゲートとする。

### 3.4 C ABI (forte_ffi)

```c
ForteCtx*  forte_open(const char* project_dir);
int        forte_build(ForteCtx*, const ForteBuildOpts*, ForteBuildResult* out);
int        forte_play_start(ForteCtx*, uint32_t sample_rate);
int        forte_eval(ForteCtx*, const char* changed_file);   // ホットリロード
void       forte_meter_read(ForteCtx*, ForteMeters* out);
void       forte_close(ForteCtx*);
```

ML/解析ツール(Python 等)からのレンダリング・特徴抽出利用を想定(創業者要件)。

## 4. 録音サブシステム

### 4.1 `.frec` ポインタ+CAS 実体(D-08)

```json
// assets/vocal_take3.frec (リポジトリに入るのはこのポインタのみ)
{
  "hash": "sha256:ab12…",           // CAS 上の PCM 実体
  "format": {"codec": "pcm_f32le", "rate": 48000, "ch": 1},
  "provenance": {
    "session": "uuid",  "device_class": "microphone",
    "recorded_at": "2026-07-02T04:12:33Z", "recorded_by": "user:shusuke",
    "input_chain": ["gate(-60dB)"],   // 収録時に掛けたモニタ系(記録のみ)
    "sig": "ed25519:…"               // デバイスローカル鍵の署名(SRS-SEC-002)
  }
}
```

- コンパイラは (a) ハッシュ実体の存在、(b) 署名検証、(c) `device_class ∈ {microphone,
  midi-render}` を検査。不合格は型エラー `E-PROV-001`。
- 録音書き込み: [入力 AudioWorklet tap] → SAB リング(容量 8 秒) → asset Worker が
  1 秒毎に OPFS へ append + リカバリジャーナル更新。クラッシュ後は
  ジャーナルからテイク復元(RSK-01)。

### 4.2 ループバック較正(SRS-REC-004)

チャープ信号を出力→入力で受け、相互相関でピーク検出。5 回試行の中央値を
`calibration.json` に保存(デバイスペア毎)。録音配置時に
`recorded_pos - (rtl - output_latency_reported)` で補正。目標 ±1ms。

## 5. Hub 詳細(Phase 2 対象)

### 5.1 fork 制約の実装(SRS-HUB-002)

- git smart HTTP の前段認可: `upload-pack`(clone/fetch)は
  (a) 所有者・コラボレータ、(b) fork 済みユーザーの fork リポジトリのみ許可。
- `POST /repos/{id}/fork` がサーバー内複製+`forked_from` エッジ作成+
  fork 者への read/write 付与を原子的に行う。
- Web UI のコードブラウズは public 全員可(読めるが持ち出しは fork のみ、が原則)。

### 5.2 リリースパイプライン(SRS-HUB-004)

```
tag push → webhook → build farm(コンテナ, ネットワーク遮断, forte-core 固定版)
  → forte build → SHA-256 比較(提出 build.manifest.json と一致?)
  → 一致: Release ノード作成+ストリーミング用エンコード(Opus セグメント)+指紋登録
  → 不一致: 拒否+差分レポート(決定論の破れは我々のバグとして扱う)
```

### 5.3 系譜集計(D-12)

- 再生イベント `(release, listener, duration)` を append-only ログに記録。
- 日次バッチ: release → forte.lock の依存閉包+performed エッジへ、
  減衰係数付きで貢献ポイントを按分(係数は将来のガバナンス項目。初期は記録のみ)。

## 6. VSCode 拡張 / Web エディタ

- 拡張は TS 実装。forte-lsp(native バイナリ)を spawn。再生は拡張ホスト内で
  forte_ffi を叩くヘルパープロセス(クラッシュ隔離)。
- 可視化 Webview: コンパイラが吐く `viz.json`(イベント列・グラフ・メーターチャネル)を
  Canvas 描画。クリック→ `sourceMap` で該当コード行へジャンプ(SRS-VIS-001)。
- Web 版: 同一の viz.json/LSP を wasm で動かす。エディタ状態は OPFS、
  Hub 同期は git(isomorphic-git or wasm-git)で行う。

## 7. エラー/例外方針

- RT 経路: panic 禁止。全ノードは飽和/NaN ガード(NaN 検出でノードをバイパスし
  エラーイベントを publish — 曲全体を落とさない)。
- コンパイラ: エラーは音楽語彙で(SRS-LANG-008)。エラーコード体系 `E-<領域>-<番号>`。
- Hub: リリース検証失敗・署名不一致はユーザー向けに完全な差分説明を返す(信頼の担保)。

## 8. 未確定事項(Phase 0 で決める)

1. 構文の最終形(上記スケッチのユーザーテスト)
2. DSP 層の実行方式初期値(インタープリタ vs 事前 codegen)
3. wasm-git vs 独自同期プロトコル
4. `@std` 標準ライブラリの初期収録(dawcore 由来: polymer 系シンセ、SVF、EQ、
   delay、FDN リバーブ、sampler → ただし sampler は録音アセット専用に制限)
5. 進行(`prog`)の正規形と類似検索インデックスの設計
