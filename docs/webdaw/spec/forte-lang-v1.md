# Forte lang 仕様 v1

Status: **実装準拠**(この文書はリポジトリの実装が受理する言語を正確に記述する)。
v0 ドラフト(forte-lang-v0.md)は設計意図・将来構想を含む上位文書として残す。
対応実装: `crates/fortelang`(パーサ/検査/コンパイラ)、検証: `cargo test -p fortelang`。

---

## 1. ファイル構造

```
file := { import } { device } [ song ]
```

- `song` を持つファイル = **曲**。持たないファイル = **デバイスライブラリ**
  (import 可能。`forte check` は全デバイスを既定値でインスタンス化して検証する)。
- 評価は完全にコンパイル時。実行時 I/O・時計・非シード乱数は言語に存在しない。

## 2. 字句

- エンコーディング UTF-8。識別子 `[A-Za-z_@][A-Za-z0-9_@#]*`。
- コメント: `// 行末まで`、`/* … */`(複数行可)。
- 数値: `12`、`0.5`、負号は前置 `-`。**単位サフィックス**は数値に密着して書く:
  `96bpm` など(v1 で意味を持つのは `bpm` のみ。他は無視されず検査対象)。
- 文字列: `"…"`(1 行)。
- 音楽リテラル: `beat` / `notes` / `prog` の直後のバッククォート `` `…` ``(複数行可)。
- 記号: `{ } ( ) : , / - .. . =`

## 3. 文法 (EBNF、実装準拠)

```ebnf
file      = { import } { device } [ song ] ;
import    = "import" "{" ident { "," ident } "}" "from" string     (* モジュール *)
          | "import" ident "from" string ;                          (* .frec アセット *)
device    = "device" ident [ ":" "Instrument" ] "{" { devItem } "}" ;
devItem   = "param" ident "=" num [ "in" num ".." num ]
          | "node" ident "=" nodeExpr
          | "out" nodeExpr ;
nodeExpr  = ident "(" [ ident ":" nodeArg { "," ident ":" nodeArg } ] ")"
          | "note" "." ident                                        (* freq | gate | vel *)
          | ident ;                                                 (* node 名 / param 名 *)
nodeArg   = string | num | nodeExpr ;
song      = "song" string "{" { songItem } "}" ;
songItem  = "tempo" num | "meter" num "/" num | "key" ident ident
          | "let" ident "=" musicLit
          | "section" ident "=" "bars" "(" num ".." num ")"
          | track | return ;
track     = "track" ident "{" { trackItem } "}" ;
trackItem = "instrument" call | "insert" call
          | "play" patternExpr atRef
          | "audio" ident atRef
          | "send" ident num
          | "volume" num | "pan" num ;
return    = "return" ident "{" { "insert" call | "volume" num | "pan" num } "}" ;
call      = ident [ "(" [ ident ":" ( num | string ) { "," … } ] ")" ] ;
patternExpr = musicLit | ident
            | ident "(" patternExpr { "," ident ":" ( num | string ) } ")" ;
atRef     = "at" ( "bars" "(" num ".." num ")" | ident ) ;
musicLit  = ( "beat" | "notes" | "prog" ) "`" raw "`" ;
num       = [ "-" ] NUMBER [ UNIT ] ;
```

## 4. 意味論

### 4.1 song ヘッダ

| 要素 | 意味 | 制約 |
| --- | --- | --- |
| `tempo 96bpm` | テンポ | **必須**。20..400(E-TIME-003) |
| `meter 4/4` | 拍子 | 分母 2/4/8/16(E-TIME-004)。エンジン拍 = 分子×4/分母 |
| `key D minor` | キー | ルート C..B(+#/b)、スケール major/minor/dorian/phrygian/lydian/mixolydian/locrian/harmonicminor/chromatic |

### 4.2 配置

- 小節は **1 始まり・両端含む**: `bars(1..8)` = 小節 1〜8。
- `section verse = bars(1..8)` で名前付けし `at verse` で参照(E-MOD-003)。
- クリップ内容は配置区間内でループする(パターン長 < 区間長のとき)。

### 4.3 音楽リテラル

| リテラル | 内容 | 生成 |
| --- | --- | --- |
| `` beat`x--- X-x-` `` | `x`=ヒット, `X`=アクセント(vel 120), `-`=休符。空白は視覚グルーピング | ステップ数で 1 小節を等分。長さ=ステップの 60% |
| `` notes`C4:1/2 [E4 G4]:1 _:1` `` | `ピッチ:長さ`(拍)。`[…]`=和音、`_`=休符、長さは `1` `0.5` `1/2` | 逐次配置。C4 = MIDI 60 |
| `` prog`Em \| C G \| D` `` | `\|`=小節。1 小節内の複数コードは時間を等分 | ChordEvent 列。裸で play するとブロックコード |

コードクオリティ: (無印=メジャー), `m`, `min`, `7`, `maj7`, `m7`, `min7`, `dim`,
`aug`, `sus2`, `sus4`。

### 4.4 パターン関数(進行 → 演奏)

| 関数 | 引数 | ボイシング |
| --- | --- | --- |
| `chords(p)` | — | 全構成音をコード長で保持(ルート oct3, vel 90) |
| `bass(p, rate: 0.5)` | rate 省略時 1 コード 1 音 | ルート音 oct2, vel 100 |
| `arp(p, rate: 0.5, style: "up\|down\|updown")` | rate は 0<r≤1 小節 | 構成音 oct4 を巡回, vel 95 |

### 4.5 デバイス DSL(音源をコードで定義)

`param` はインスタンス化時に束縛(範囲は `in lo..hi`、既定 0..1)。グラフは
ボイス毎インタープリタに展開され、ポリフォニー(8 声・最古スチール)・
エンベロープ解放はエンジンが担う。

| プリミティブ | 信号入力(既定) | パラメータ(既定) |
| --- | --- | --- |
| `osc` | `freq`(note.freq) | `shape`: sine/saw/square/tri |
| `lfo` | — | `rate` 0..1(=0.05..12Hz), `shape`: sine/tri/saw/square |
| `adsr` | `gate`(note.gate) | `a` .05, `d` .3, `s` .6, `r` .25(正規化) |
| `svf` | `in`(必須), `mod`(±4oct) | `cutoff` .65(=30..18kHz 指数), `reso` .2 |
| `gain` | `in`(必須), `mod`(0..2 倍) | `level` .8 |
| `mix` | `a`, `b`(必須) | — |

信号ソース: `note.freq`(Hz) / `note.gate` / `note.vel`、宣言済み `node` 名
(前方参照不可 E-GRID-002)、入れ子呼び出し。数値位置には `param` 名を書ける。

### 4.6 ビルトインデバイス

| instrument | パラメータ |
| --- | --- |
| `sampler(sample: "Kick"\|"Snare"\|"Hat")` | gain, attack, decay, sustain, release, pitch |
| `polymer` | wave(sine/saw/square/tri), cutoff, reso, attack, decay, sustain, release, detune, sub, filtenv |
| `grid()` | 既定パッチのモジュラー音源 |

| effect | パラメータ |
| --- | --- |
| `filter` | type(lp/hp/bp/notch), cutoff, reso |
| `eq` | low, mid, high |
| `drive` | drive(別名 amount) |
| `delay` | time, fdbk(別名 feedback), mix |
| `reverb` | size, decay, mix |

数値ノブはすべて正規化 0..1(範囲外は E-TYPE-002)。volume 0..1、pan -1..1、
send レベル 0..1。

### 4.7 録音アセット(.frec)

- `import take from "./take1.frec"` → `audio take at bars(2..3)`。
- **来歴のないオーディオは参照すら不能**(E-PROV-001): ヘッダの provenance に
  `device_class`(microphone / midi-render), `recorded_at`, `by`, `session`,
  `sig` が必須。ループバック較正値は `latency_samples` として同梱される。
- レイアウト: `FREC1\n` + u32-le ヘッダ長 + JSON ヘッダ + f32-le PCM。
  レート 8k..192k、1..2ch(ステレオはモノミックスで再生)。
- 外部オーディオ(WAV/MP3 等)の import は**文法ごと存在しない**。

### 4.8 モジュール解決

- パスは import 元ファイルからの相対。再帰解決、循環は E-MOD-007。
- 名前がない場合はライブラリの実エクスポートを列挙(E-MOD-006)。
- 環境: CLI/LSP=ファイルシステム、ブラウザ=エディタのファイルマップ(OPFS+同梱)。

## 5. 決定論の契約

1. 同一ソース+同一アセット → **ビット同一のビルド**(native x86_64 / wasm32-wasip1 /
   ブラウザ wasm で検証済み。`scripts/determinism_test.sh` が CI ゲート)。
2. 成立条件: f32 固定、超越関数は libm 単一実装(dawcore::dmath)、
   レンダーは 48kHz・512 サンプルブロック・8 拍テール。
3. ビルド証明: 出力ダイジェスト = 全サンプルの f32 LE ビットパターンの
   FNV-1a 64(v1。製品版は SHA-256 へ)。`build.manifest.json` と Hub release
   台帳に記録され、`forte hub verify` / ブラウザの Verify が再現照合する。

## 6. 正規形(fmt)

`forte fmt` が唯一の整形: ブレース深度×2 スペースのインデント、行末空白なし、
空行は 1 行まで、末尾改行 1 つ。文字列・音楽リテラル・コメントは不変。
**整形前後の字句列が一致しない場合は適用を拒否**(E-FMT-001)——意味を変える
整形は構造的に不可能。

## 7. 診断カタログ

| 系列 | 意味 |
| --- | --- |
| E-LEX-001..005 | 字句(未閉の文字列/リテラル/ブロックコメント、不正文字) |
| E-PARSE-001..019 | 構文(各構文要素の期待と実際) |
| E-TYPE-001..005 | 値(単位、0..1 範囲、文字列/数値の取り違え、選択肢外) |
| E-TIME-001..004 | 時間(小節範囲、rate、tempo、拍子) |
| E-SONG-001..004 | 曲構造(tempo 必須、キー、track なし、song なし) |
| E-MOD-001..007 | 名前解決(パターン/section/return/import/循環) |
| E-DEV-001..008 | デバイス(未知、パラメータ、ビルトインサンプル、衝突) |
| E-GRID-001..006 | デバイス DSL(必須入力、前方参照、信号/数値、未知プリミティブ) |
| E-PAT-001..003 | パターン関数(prog 必須、引数、入れ子) |
| E-BEAT / E-NOTE / E-PROG | 各リテラルの内容 |
| E-PROV-001..003 | 録音来歴(必須ブロック、.frec 限定、未 import) |
| E-FMT-001 | フォーマッタの安全弁 |

メッセージは音楽家の語彙・日本語・位置付き。「使えるもの」を必ず列挙する。

## 8. v0 ドラフトからの差分 / 未実装

- 実装済みで v0 から確定: send/return 構文(DECISION-S1)、prog クオリティ集合、
  デバイス DSL はノードグラフ形式(任意式 `process` は将来)。
- 未実装(v2 候補): ユーザー定義ジェネリクス、`automate`(オートメーション)、
  3 連符 beat リテラル(DECISION-S2)、セクション反復の一級表現(DECISION-S3)、
  単位型の完全な検査(Hz/dB/ms)、`route` 明示ルーティング、エフェクトの
  device DSL(現状 Instrument のみ)、ed25519 署名の実検証。
