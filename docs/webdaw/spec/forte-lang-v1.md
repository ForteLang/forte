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
songItem  = "tempo" num | "swing" num | "meter" num "/" num | "key" ident ident
          | "let" ident "=" musicLit
          | "section" ident "=" "bars" "(" num ".." num ")"
          | track | return ;
track     = "track" ident "{" { trackItem } "}" ;
trackItem = "instrument" call | "insert" call
          | "play" patternExpr atRef
          | "audio" ident atRef
          | "send" ident num
          | "automate" ident "from" num "to" num "over" overRef
          | "modulate" ident "with" call
          | "volume" num | "pan" num ;
overRef   = "bars" "(" num ".." num ")" | ident ;                   (* セクション名 *)
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
| `swing 0.62` | 偶数位置の 16 分を遅らせる(MPC 表記: 0.5=ストレート、0.66≒シャッフル、範囲 0.5..0.8)。グリッド上の音のみ対象 |
| `meter 4/4` | 拍子 | 分母 2/4/8/16(E-TIME-004)。エンジン拍 = 分子×4/分母 |
| `key D minor` | キー | ルート C..B(+#/b)、スケール major/minor/dorian/phrygian/lydian/mixolydian/locrian/harmonicminor/chromatic |

### 4.2 配置

- 小節は **1 始まり・両端含む**: `bars(1..8)` = 小節 1〜8。
- `section verse = bars(1..8)` で名前付けし `at verse` で参照(E-MOD-003)。
- クリップ内容は配置区間内でループする(パターン長 < 区間長のとき)。

### 4.3 音楽リテラル

| リテラル | 内容 | 生成 |
| --- | --- | --- |
| `` beat`x--- X.x-` `` | `x`=ヒット(vel 100), `X`=アクセント(120), `.`=ゴースト(55), `-`=休符。空白は視覚グルーピング | ステップ数で 1 小節を等分。長さ=ステップの 60%。ベロシティは全音源でゲインに反映(100 = 等倍) |
| `` notes`C4:1/2 [E4 G4]:1 _:1` `` | `ピッチ:長さ`(拍)。`[…]`=和音、`_`=休符、長さは `1` `0.5` `1/2` | 逐次配置。C4 = MIDI 60 |
| `` notes`C2!:1/4 C2~:1/4 D2:1/2` `` | `!`=アクセント(vel 120)、`~`=タイ: 次の音までゲートを保持。mono/glide の楽器ではスライドになる(303 の記法)。両方付けるなら `C2!~` | タイは長さ 102% で重ねる |
| `` prog`Em \| C G \| D` `` | `\|`=小節。1 小節内の複数コードは時間を等分 | ChordEvent 列。裸で play するとブロックコード |

コードクオリティ: (無印=メジャー), `m`, `min`, `7`, `maj7`, `m7`, `min7`, `dim`,
`aug`, `sus2`, `sus4`。

### 4.4 パターン関数(進行 → 演奏)

| 関数 | 引数 | ボイシング |
| --- | --- | --- |
| `chords(p)` | — | 全構成音をコード長で保持(ルート oct3, vel 90) |
| `bass(p, rate: 0.5)` | rate 省略時 1 コード 1 音 | ルート音 oct2, vel 100 |
| `arp(p, rate: 0.5, style: "up\|down\|updown")` | rate は 0<r≤1 小節 | 構成音 oct4 を巡回, vel 95 |

### 4.5 デバイス DSL(音源とエフェクトをコードで定義)

`device 名前 : Instrument`(音源)または `device 名前 : Effect`(エフェクト、
`insert` で使う)。`param` はインスタンス化時に束縛(範囲は `in lo..hi`、
既定 0..1)。Instrument のグラフはボイス毎インタープリタに展開され、
ポリフォニー(8 声・最古スチール)・エンベロープ解放はエンジンが担う。
Effect のグラフはステレオ各チャンネルが独立の状態で同一グラフを評価する。

- Instrument の信号ソースは `note.freq / note.gate / note.vel`。
- Effect の信号ソースは **`audio.in`**(入力信号)。note.* は使えず
  (E-GRID-003)、`adsr` は gate の明示が必要(E-GRID-001)。
- Effect を instrument に、Instrument を insert に書くと E-DEV-009。
- **予約 param 名 `glide`**: 宣言すると mono/レガートになり、値がポルタメント秒。
  重なった(タイされた)ノートはリトリガーせず周波数がスライドする(303 のスライド)。

| プリミティブ | 信号入力(既定) | パラメータ(既定) |
| --- | --- | --- |
| `osc` | `freq`(note.freq), `mod`(±4oct), `pwm`(パルス幅 ±0.45) | `shape`: sine/saw/square/tri/pulse、`pw`(pulse の基準幅、既定 0.5) |
| `noise` | — | —(決定論: per-voice xorshift、ノート毎に再シード) |
| `lfo` | — | `rate` 0..1(=0.05..12Hz), `shape`: sine/tri/saw/square |
| `adsr` | `gate`(note.gate) | `a` .05, `d` .3, `s` .6, `r` .25(正規化) |
| `svf` | `in`(必須), `mod`(±4oct) | `cutoff` .65(=30..18kHz 指数), `reso` .2 |
| `shaper` | `in`(必須), `mod`(drive 加算) | `drive` .3, `mode`: tanh/clip/fold |
| `gain` | `in`(必須), `mod`(0..2 倍) | `level` .8 |
| `mix` | `a`, `b`(必須) | — |
| `sample` | —(Instrument 専用: Effect では E-GRID-003) | `take`(必須: 宣言済み take スロット), `start` 0, `end` 1, `loop`: off/on, `reverse`: off/on |

信号ソース: `note.freq`(Hz) / `note.gate` / `note.vel`、宣言済み `node` 名
(前方参照不可 E-GRID-002)、入れ子呼び出し。数値位置には `param` 名を書ける。

**take スロット(soundnote)**: device 冒頭の `take voice` は「使う側が録音を
差し込む」宣言。`sample(take: voice)` がそのテイクをグラフの音源として再生し
(基準 C4、演奏ノートに再ピッチ、ノートオン毎に先頭から)、svf/shaper/gain の
後段で加工できる。束縛は呼び出し側 `instrument MyVox(voice: myTake)`(未束縛は
E-DEV-002、Ident 以外は E-TYPE-004)。デバイス自体はテイクを持たないため、
ライブラリ単体の検証(`forte check lib.forte`)はスロット未束縛のまま通る —
publish した楽器に各自が自分の録音を差せる。

### 4.6 ビルトインデバイス

| instrument | パラメータ |
| --- | --- |
| `sampler(sample: "Kick"\|"Snare"\|"Hat")` | gain, attack, decay, sustain, release, pitch, start, end, loop("off"/"on"), reverse("off"/"on") |
| `sampler(take: <import した録音>, root: A3)` | 同上。録音テイクが楽器になる: `root` はテイクを演奏した音名(C2..C6)で、その音で弾くと原音、他はクロマチックに再ピッチ |
| `sampler(…, start: 0.25, end: 0.6, loop: "on", reverse: "on")` | 音作り: `start`/`end` は再生範囲(0..1 の割合)、`loop: "on"` はノート保持中に範囲をループ(短い範囲は持続音化)、`reverse: "on"` は逆再生。全てノートオン時に確定するため決定論を保つ |
| `kit(C2: kickTake, D2: snareTake, …)` | gain, attack, decay, sustain, release。音名キーが録音テイクをパッドに割り当てる(完全一致した音程のみ発音・原速再生・再ピッチなし)。`beat` リテラルは最低音のパッドを叩く |
| `polymer` | wave(sine/saw/square/tri), cutoff, reso, attack, decay, sustain, release, detune, sub, filtenv |
| `grid()` | 既定パッチのモジュラー音源 |

ビルトインの他に、標準楽器ライブラリ `lib/std/`(drums / percussion / bass /
keys / leads / pads / synths / fx の計 103 楽器)が同梱される。これらは言語機能ではなく §4.5 の
device DSL で書かれたユーザー空間のコードであり、通常の `import` で使う。

| effect | パラメータ |
| --- | --- |
| `filter` | type(lp/hp/bp/notch), cutoff, reso |
| `eq` | low, mid, high |
| `drive` | drive(別名 amount) |
| `delay` | time, fdbk(別名 feedback), mix |
| `reverb` | size, decay, mix |
| `comp` | thresh, ratio, attack, release, makeup — ステレオリンク・コンプレッサ |
| `chorus` | rate, depth, mix — L/R 直交位相の変調ディレイ |
| `pump` | amount, beats — テンポ同期ダッキング(サイドチェイン・ポンピングの決定論版。beats は 1 サイクルの拍数、既定 1)|
| `width` | amount — M/S ステレオ幅(0.5 が等倍。insert はパン前段なのでステレオ源に使う)|

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

### 4.9 オートメーションとモジュレーション

対象パラメータの解決は automate / modulate で共通(大文字小文字は区別しない):

- `volume`(automate のみ)/ instrument のパラメータ名 — ビルトイン
  (polymer / sampler)はパラメータ表、**自作 device は宣言した `param` が
  そのまま名前になる**。
- `<insert名>.<パラメータ>` — insert エフェクトを書いた名前で指す:
  `delay.mix`、`Muffle.cutoff`(自作 Effect の `param` も公開される)。
  同名 insert が複数あるときは最初のものに差さる。

未知の名前は「使えるもの」を列挙して E-AUTO-001 / E-LFO-001。

- `automate <param> from 0.2 to 0.8 over bars(1..8)` — 区間の頭から末尾へ
  線形ランプ(`over` にはセクション名も可)。値は 0..1(E-TYPE-002)。
  レーンがあるパラメータでは基準値はレーンに置き換わる: ランプ開始前は
  `from`、終了後は `to` を保持する。複数の `automate` は対象ごとに
  ひとつのレーンへ拍順でマージされる。
- `modulate <param> with <modulator>(…)` — パラメータにモジュレータを
  差し込む。種類は 4 つ(それ以外は E-PARSE-021):
  - `lfo(rate: 0.4, amount: 0.5, shape: "tri")` — 周期波。`rate` 0..1
    (0.05..8.05 Hz、省略時 0.3)、`shape` sine / tri / saw / square
    (省略時 sine)。
  - `steps(seq: "0.1 0.6 0.3 0.9", every: "1/16", amount: 0.5)` —
    ステップシーケンサ。`seq` は空白区切りの 0..1(E-TYPE-002)。
    `every`(1/2, 1/4, 1/8, 1/16。E-TYPE-005)を書くと**テンポ同期**:
    1 ステップ = その音価。省くと `rate` の自走周期で 1 周する。
  - `random(rate: 0.4, amount: 0.4, smooth: 0.5)` — サンプル&ホールド
    乱数(決定論: 同一ソースなら同一乱数列)。`smooth` 0..1 でステップ間を
    滑らかに補間。`every` でテンポ同期も可。
  - `adsr(a: 0.02, d: 0.4, s: 0.3, r: 0.1)` — **ノートゲート駆動**の外付け
    エンベロープ: トラックの音が鳴り始めると立ち上がり、無音になると
    リリースする(フィルターエンベロープの後付け)。各値 0..1
    (時間は 2 乗カーブで最大 3 秒)。ブロックレート評価。
  共通: `amount` -1..1 は**必須**(E-LFO-003)。揺れは基準値
  (automate レーンがあればその時点のレーン値)に amount 幅で乗り、
  0..1 に飽和する。ひとつのパラメータに `automate` と `modulate` を
  **重ねられる**(ランプの上にモジュレーションが乗る)し、`modulate` を
  複数並べてスタックすることもできる。

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
| E-PARSE-001..021 | 構文(各構文要素の期待と実際、automate/modulate の形) |
| E-TYPE-001..005 | 値(単位、0..1 範囲、文字列/数値の取り違え、選択肢外) |
| E-TIME-001..004 | 時間(小節範囲、rate、tempo、拍子) |
| E-SONG-001..004 | 曲構造(tempo 必須、キー、track なし、song なし) |
| E-MOD-001..007 | 名前解決(パターン/section/return/import/循環) |
| E-DEV-001..009 | デバイス(未知、パラメータ、ビルトインサンプル、衝突、Instrument/Effect の取り違え) |
| E-GRID-001..006 | デバイス DSL(必須入力、前方参照、信号/数値、未知プリミティブ) |
| E-PAT-001..003 | パターン関数(prog 必須、引数、入れ子) |
| E-BEAT / E-NOTE / E-PROG | 各リテラルの内容 |
| E-PROV-001..003 | 録音来歴(必須ブロック、.frec 限定、未 import) |
| E-AUTO-001 | automate(未知のパラメータ名。使えるものを列挙) |
| E-LFO-001..003 | modulate(パラメータ名、instrument なし、モジュレータ引数) |
| E-FMT-001 | フォーマッタの安全弁 |

メッセージは音楽家の語彙・日本語・位置付き。「使えるもの」を必ず列挙する。

## 8. v0 ドラフトからの差分 / 未実装

- 実装済みで v0 から確定: send/return 構文(DECISION-S1)、prog クオリティ集合、
  デバイス DSL はノードグラフ形式(任意式 `process` は将来)。
- 実装済み(v1.1): `automate`(volume + instrument / insert の全パラメータ、
  §4.9)、`modulate … with lfo / steps / random / adsr`(自作 device・
  自作 Effect の `param`、`<insert>.<param>` 含む、§4.9)。
- 未実装(v2 候補): ユーザー定義ジェネリクス、automate pan、マクロ
  (1 ノブ → 多パラメータ)、モジュレータ自体のオートメーション
  (`wobble.amount`)、song レベル共有モジュレータ、3 連符 beat リテラル
  (DECISION-S2)、セクション反復の一級表現(DECISION-S3)、
  単位型の完全な検査(Hz/dB/ms)、`route` 明示ルーティング、
  ed25519 署名の実検証。(エフェクトの device DSL は §4.5 で実装済み)
