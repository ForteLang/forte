# Forte lang 仕様 v0 (ドラフト)

Status: Draft v0.1 / 2026-07-02
対応要求: SRS-LANG-001..008 / 上位: 05-detailed-design.md §1

本書は Phase 0 実装のための最小仕様。構文はリファレンス曲の移植(0.6)による
実地検証を経て v1 で固定する。**太字の DECISION** は実装前に確定が必要な項目。

---

## 1. 設計原理

1. **すべてが値** — ノート、パターン、進行、トラック、デバイス、曲はすべて式が返す値。
2. **決定論** — 実行時 I/O・時計・非シード乱数は存在しない。プログラムの意味は
   「イベント列+レンダーグラフ」への純粋な写像である。
3. **2 層** — Score 層(宣言的・コンパイル時に完全展開)と DSP 層(サンプル毎の
   手続き的カーネル)。同一言語のサブセットとして提供し、DSP 層のみ `process` を持つ。
4. **単位は型** — 拍・秒・Hz・dB・ピッチは別型。裸の数値との混同はコンパイルエラー。
5. **diff 可能** — `forte fmt` による唯一の正規形。1 ファイル 1 モジュール。

## 2. 字句

- エンコーディング: UTF-8。識別子: `[a-zA-Z_][a-zA-Z0-9_]*`(v0 は ASCII のみ)。
- コメント: `//` 行、`/* */` ブロック。
- 数値リテラルに単位サフィックスを許す: `92bpm`, `4bars`, `1/8beat`, `440Hz`,
  `-0.3dB`, `20ms`, `0.5`(無次元)。
- 音楽リテラル(バッククォート DSL、§5):
  `beat` … ステップ列 / `notes` … ノート列 / `prog` … コード進行。
- ピッチリテラル: `C4`, `F#3`, `Bb2`(オクターブは中央 C=C4)。

## 3. 型システム(v0 コア)

```
基本    : Bool, Int, Float, String(コンパイル時のみ)
単位付き: Beats, Bars, Sec, Hz, Db, Bpm, Pitch, Velocity(0..1)
音楽    : Note{pitch, start: Beats, dur: Beats, vel},
          Pattern = List<Note>(長さ: Beats 付き),
          Chord, Progression = List<(Chord, Beats)>
信号    : Audio(チャネル数は型パラメータ: Audio<1>, Audio<2>), Control
構造    : Track, Bus, Song, Section
デバイス: Instrument(Note→Audio), Effect(Audio→Audio), NoteFx(Pattern→Pattern)
アセット: RecordedAudio(来歴検証済みのマイク録音のみ。§8)
汎用    : List<T>, Map<K,V>, Option<T>, 関数型 (T)->U, レコード型 {a: T, b: U}
```

- 変換は明示のみ: `beats(2.0)`, `sec(1.5)`, `(1/8).beats * 3` など。
  `Beats → Sec` の変換は tempo が確定するコンテキスト(song 内)でのみ可能。
- **DECISION-T1**: ジェネリクスの範囲(v0 は `List<T>` と関数の単相化のみで開始し、
  ユーザー定義ジェネリクスは v1 に送る案を推奨)。

## 4. モジュールと import

```forte
// 外部依存(forte.toml の [deps] と forte.lock で解決)
import { tr909, Kick }  from "@rhythm/tr909@^2.1"
// ローカル
import { hook }         from "./sections/hook.forte"
// 録音アセット(来歴検証がコンパイル時に走る)
import vocalTake        from "../assets/vocal_take3.frec"
```

- 循環 import はエラー。公開は `pub` キーワード。
- モジュールのトップレベルは宣言のみ(式の実行はない)。
- public レジストリへの公開時はソース必須(SRS-PKG-003)。

## 5. Score 層

### 5.1 曲の構造

```forte
pub song "Aozora" {
  tempo 92bpm
  meter 4/4
  key   D maj

  let kick   = beat`x--- x--- x-x- x---`          // 1 小節、16 分解像度
  let chords = prog`Dmaj7 | Bm7 | Em7 A7`          // '|' が小節区切り

  section verse = bars(1..16)
  section hook  = bars(17..32)

  track Drums {
    instrument tr909(kick: .deep)
    play kick at verse.repeat()                     // セクション全体に反復
  }

  track Keys {
    instrument juno(voices: 8)
    play arp(chords, style: .updown, rate: 1/8beat) at hook
    automate cutoff from 0.2 to 0.8 over hook       // パラメータ名は型検査される
  }

  track Vocal {
    audio vocalTake at bars(17)                     // RecordedAudio の配置
    insert comp(ratio: 3.0, threshold: -18dB)
  }

  bus Master {
    insert limiter(ceiling: -0.3dB)
  }
}
```

### 5.2 意味論

- `song` ブロックはコンパイル時に評価され、**イベント列**(サンプル精度の
  ノート/オートメーション/クリップ配置)と**レンダーグラフ**(instrument/effect/bus の
  接続)に完全展開される。
- 制御構造(`let` / `fn` / `if` / `for` / `map` / `repeat`)はすべてコンパイル時。
  実行時に評価されるのは DSP 層の `process` のみ。
- ルーティング既定: `track` → 暗黙の `Master`。send/return は
  `return Space { insert reverb(...) }` ブロック+トラック内の `send Space 0.35`
  (**DECISION-S1 解決済 — v0 実装に準拠**)。
- 乱数: `random(seed: 42)` が返す純粋な生成器のみ。シードは省略不可。
- `song` は値なので、関数が `Song` を返す・変奏を `map` で作る等が可能
  (アルゴリズム作曲はこの経路で行う)。

### 5.3 音楽リテラルの意味

- `beat` … `x`=ヒット、`-`=休符、`X`=アクセント、空白=グルーピング(無意味)。
  解像度はリテラル長から推論(1 小節を等分)。**DECISION-S2**: 3 連符等の非 2 冪。
- `notes` … `notes\`C4:1/4 E4:1/4 G4:1/2\``(ピッチ:長さ)。
- `prog` … コード名と `|`(小節区切り。1 小節内の複数コードは時間を等分)。
  `Progression` 値になり、パターン関数 `chords(x)` / `arp(x, rate:, style: "up|down|updown")` /
  `bass(x, rate:)` の入力になる。裸の `prog` はブロックコードとして鳴る。
  クオリティ: (メジャー), m, min, 7, maj7, m7, min7, dim, aug, sus2, sus4。
  **進行が一級の値であることが類似検索(SRS-PLY-002)の基盤**。
- `section verse = bars(1..8)` で名前付き区間を定義し、`play x at verse` で参照する。

## 6. DSP 層

```forte
pub device MonoSaw : Instrument {
  param cutoff: Hz = 800Hz in 20Hz..18kHz    // Hub/可視化はこの宣言から UI を導出
  param res:    Float = 0.3 in 0.0..0.99

  state phase: Float = 0.0
  state svf:   Svf   = Svf.lowpass()

  on note_on(n)  { phase = 0.0 }

  process(out: &mut Frame<1>, ctx: &Ctx) {
    let s = std.osc.saw_blep(&mut phase, ctx.pitch_hz)
    out[0] = svf.run(s, cutoff, res) * ctx.env()
  }
}
```

- `process` は 1 サンプル(または 1 フレーム)毎に呼ばれる唯一の実行時コード。
  割り当て・再帰・無限ループ不可(コンパイル時に停止性を保証できる構文サブセット:
  上限付き `for` のみ)。
- `state` はボイス毎に複製される。`param` は Score 層から `automate` 可能。
- 数学関数は `std.math`(= forte-core の dmath、libm 固定)のみ。
  **決定論スパイク(07)により、この規約で native/wasm ビット同一が実証済み。**
- v0 実装はインタープリタ(Rust のオペレータ木)で開始(SDD §2)。

## 7. 標準ライブラリ `@std` (v0 収録)

| モジュール | 内容(dawcore からの移植元) |
| --- | --- |
| std.osc | polyBLEP saw/square/tri, sine (oscillator.rs) |
| std.env | ADSR (envelope.rs) |
| std.filter | TPT SVF, OnePole (filter.rs) |
| std.fx | delay, FDN reverb, drive, EQ3, limiter (effects.rs) |
| std.inst | Polymer 相当のリファレンスシンセ (synth.rs/voice.rs) |
| std.note | arp, transpose, repeat, quantize (device.rs の NoteFx) |
| std.math | sin/cos/tan/exp/tanh/powf (dmath.rs) |
| std.rand | シード付き xorshift |

サンプラー(sampler.rs)は `RecordedAudio` 専用に制限して移植する(外部ファイル
再生を持たない。SYS-REC-001)。

## 8. アセット参照と来歴

- `import x from "*.frec"` は `RecordedAudio` 型。コンパイル時に
  (a) CAS 実体の存在、(b) ed25519 署名、(c) `device_class ∈ {microphone}` を検証。
  失敗は `E-PROV-001`。
- `.frec` ポインタ形式は SDD §4.1 に定義。

## 9. 診断とエラー

- エラーコード体系: `E-TYPE-*`(型)、`E-TIME-*`(拍/秒の不整合)、
  `E-PROV-*`(来歴)、`E-DSP-*`(process 制約違反)、`E-MOD-*`(import)。
- メッセージは音楽語彙で(SRS-LANG-008)。例:
  `E-TIME-002: Track 'Vocal' の 3 小節目: Pattern(3/4 拍分)が meter 4/4 と一致しません`

## 10. 文法スケッチ (EBNF 抜粋)

```
file        := { import } { decl }
import      := "import" ( "{" ident {"," ident} "}" | ident ) "from" string
decl        := ["pub"] ( song | device | fnDecl | letDecl )
song        := "song" string "{" { songItem } "}"
songItem    := tempo | meter | key | letDecl | sectionDecl | track | bus | route
track       := "track" ident "{" { trackItem } "}"
trackItem   := instrument | audioPlace | play | insert | automate
device      := "device" ident ":" deviceKind "{" { param | state | handler | process } "}"
play        := "play" expr "at" expr
automate    := "automate" ident "from" expr "to" expr "over" expr
expr        := literal | musicLit | ident | call | lambda | binop | ...
musicLit    := ("beat"|"notes"|"prog") "`" raw "`"
```

## 11. v0 で作らないもの(v1 以降)

- ユーザー定義ジェネリクス、trait 相当の抽象
- マクロ / メタプログラミング
- Score 層のリアルタイム入力反映(ライブコーディング) — 設計上は可能だが後回し
- MIDI 2.0、マイクロチューニング(型は Pitch に拡張余地を残す)

## 12. 未決事項一覧

| ID | 内容 | 期限 |
| --- | --- | --- |
| DECISION-T1 | ジェネリクスの範囲 | パーサ実装前 |
| ~~DECISION-S1~~ | ~~send/return ルーティング構文~~ → **解決: `return Name {}` + `send Name level`** | 済 |
| DECISION-S2 | 非 2 冪分割(3 連)の beat リテラル表現 | リファレンス曲移植時 |
| DECISION-S3 | `section` の反復(A-B-A)の一級表現(単純な `section` は実装済) | 同上 |
| DECISION-D1 | `process` のフレーム粒度(1 サンプル vs 小ブロック) | インタープリタ実装時 |
