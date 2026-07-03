# Forte 使い方ガイド

コードで作曲するための実践ガイド。上から順にやれば、ゼロから
「曲を書く → 聴きながら直す → 音源を自作する → 録音を混ぜる →
公開して fork される」まで一通り体験できます。

- 言語の正確なリファレンス: [webdaw/spec/forte-lang-v1.md](webdaw/spec/forte-lang-v1.md)
- 設計思想: [webdaw/01-vision.md](webdaw/01-vision.md)

---

## 0. セットアップ

必要なもの: **Rust ツールチェーン**(rustup)。Linux はオーディオ出力に
`libasound2-dev` が必要です(なくても動きます — 無音バックエンドで走ります)。

```bash
git clone <このリポジトリ>
cd <リポジトリ>
cargo install --path crates/fortelang    # `forte` コマンドが入ります
```

動作確認:

```bash
forte check songs/first-light.forte
# OK: song をコンパイルしました(6 tracks, tempo 96 bpm, 16 小節)
```

## 0.5 いきなり音を出す(REPL)

ファイルを作る前に、まず鳴らせます:

```
$ forte repl
forte> beat`x--- x-x-`                  # 打った瞬間からループ再生
forte> let theme = prog`Am | F | C | G`
forte> arp(theme, rate: 0.25, style: "updown")
forte> :inst polymer(wave: "saw", cutoff: 0.4)   # 鳴らしたまま音色替え
forte> :fx delay(time: 0.3, mix: 0.25)
forte> device Bloop : Instrument {      # 音源の自作も REPL で(複数行 OK)
  ...>   node o = osc(shape: "square")
  ...>   out gain(in: o, mod: adsr())
  ...> }
forte> :inst Bloop()
forte> :track Bass                      # ← トラックを重ねる(ループステーション)
forte:Bass> :inst polymer(wave: "saw", sub: 0.8)
forte:Bass> bass(theme, rate: 0.5)      # 鳴っているドラムの上に重なる
forte:Bass> :vol 0.7
forte:Bass> :undo                       # 一手戻る
forte:Bass> :save jam.forte             # 多重トラックの曲として保存
forte:Bass> :quit
```

`:help` で全コマンド。`:save` した曲は `forte play jam.forte` でそのまま続きを作れます。

## 1. 最初の曲(5 分)

`my-song.forte` を作ります:

```forte
song "My First" {
  tempo 120bpm
  meter 4/4
  key C major

  track Drums {
    instrument sampler(sample: "Kick")
    play beat`x--- x--- x--- x-x-` at bars(1..4)
  }

  track Keys {
    instrument polymer(wave: "square", cutoff: 0.5)
    play notes`C4:1 E4:1 G4:1 [C4 E4 G4]:1` at bars(1..4)
  }
}
```

```bash
forte check my-song.forte    # エラーは行番号+日本語で出ます
forte play  my-song.forte    # ループ再生開始
```

**再生しっぱなしのまま**エディタでファイルを編集して保存してください。
音が途切れずに変わります(ホットリロード)。これが Forte の基本ループです:
**聴きながら、コードで直す。**

ファイルに書き出すには:

```bash
forte build my-song.forte
# my-song.wav と my-song.manifest.json(ビルド証明)ができます
```

`manifest.json` の digest は「このコードからこの音が生まれた」証明で、
誰がどのマシン(ブラウザ含む)でビルドしても同じ値になります。

## 2. 言語チートシート

```forte
// コメント。/* ブロック */ も可

song "名前" {
  tempo 96bpm            // 必須
  meter 6/8              // 省略時 4/4
  key D minor            // 省略可

  // ---- パターンは値。let で名前を付けて使い回す ----
  let kick  = beat`x--- x-x-`             // x=音 X=アクセント -=休符。1小節を等分
  let melo  = notes`D4:1/2 F4:1/2 [A3 D4]:1 _:1`  // ピッチ:拍。[]=和音 _=休符
  let theme = prog`Dm | Bb F | C`         // コード進行。| が小節

  // ---- 曲の構造に名前を付ける ----
  section verse = bars(1..8)
  section hook  = bars(9..16)

  // ---- リターントラック(センド先) ----
  return Space { insert reverb(size: 0.7, decay: 0.6, mix: 1.0) }

  track Bass {
    instrument polymer(wave: "saw", cutoff: 0.3, sub: 0.7)
    insert drive(drive: 0.2)              // インサートは並べた順に掛かる
    volume 0.7
    pan -0.1
    play bass(theme, rate: 0.5) at verse  // 進行からベースラインを生成
  }

  track Keys {
    instrument polymer(wave: "tri")
    send Space 0.35                        // ポストフェーダーセンド
    play chords(theme) at verse            // ブロックコード
    play arp(theme, rate: 0.25, style: "updown") at hook  // アルペジオ
  }
}
```

- 小節は **1 始まり・両端含む**。パターンが区間より短ければループします。
- ノブ系の数値はぜんぶ **0..1 に正規化**(volume も cutoff も)。pan だけ -1..1。
- ビルトイン音源: `sampler(sample: "Kick"/"Snare"/"Hat")`, `polymer(…)`, `grid()`。
  エフェクト: `filter, eq, drive, delay, reverb`。パラメータ名を間違えると
  「使えるもの」を列挙してくれるので、覚えなくても書けます。

整形は `forte fmt my-song.forte`(意味を変えない保証付き)。

## 3. 音源を自作する(device)

シンセもコードです。ファイル冒頭(song の前)に書きます:

```forte
device MyLead : Instrument {
  param cutoff = 0.6 in 0.0..1.0          // 使う側から調整できるパラメータ

  node o   = osc(shape: "saw")             // freq 省略時は演奏ノートの音程
  node env = adsr(a: 0.03, d: 0.25, s: 0.6, r: 0.3)
  node vib = lfo(rate: 0.3, shape: "sine")
  node f   = svf(in: o, cutoff: cutoff, reso: 0.3, mod: vib)
  out gain(in: f, mod: env, level: 0.9)
}

song "..." {
  track Lead {
    instrument MyLead(cutoff: 0.75)        // 範囲チェック付きで束縛
    ...
  }
}
```

プリミティブは 6 つ: `osc / lfo / adsr / svf / gain / mix`。
信号は `note.freq / note.gate / note.vel`、node 名、入れ子呼び出しで配線します。
ポリフォニー(8 声)はエンジンが面倒を見ます。

## 4. ライブラリに分割して import

音源を別ファイルに切り出すと、複数の曲から使えます(そして将来 Hub で
fork される単位になります):

```forte
// devices/mylib.forte — song を持たないファイル = デバイスライブラリ
device MyLead : Instrument { ... }
device MyBass : Instrument { ... }
```

```forte
// my-song.forte
import { MyLead, MyBass } from "./devices/mylib.forte"
```

ライブラリ単体の検証: `forte check devices/mylib.forte`。

## 5. ブラウザエディタ

インストール不要で同じ言語・同じ音(ビット同一)が動きます:

```bash
scripts/build_web.sh                 # wasm をビルド(要 wasm32-unknown-unknown ターゲット)
python3 -m http.server 8000          # リポジトリルートで
# → http://localhost:8000/web/
```

| UI | 何をするか |
| --- | --- |
| 曲セレクタ / New / Del | OPFS(ブラウザ内ストレージ)の自分の曲+デモ曲。**編集は自動保存**され、タブを閉じても残る |
| ▶ Play / ■ Stop | AudioWorklet 再生。編集は再生を止めずに反映 |
| ● Rec | マイク録音 → 来歴付き `.frec` として保存(下記) |
| ⏱ Calib | ループバック較正: チャープを鳴らしてマイクで捕まえ、往復レイテンシを実測 |
| 🎹 Perform | 演奏モード: MIDI 鍵盤 or PC キー(A〜K=白鍵、W/E/T/Y/U=黒鍵)。停止すると演奏が `notes` コードに書き起こされる |
| Build digest | ブラウザ内でビルド証明を計算。CLI と同じ値になります |
| 下のアレンジビュー | 読み取り専用の可視化(編集の唯一の真実はコード) |

一度開けば**完全オフライン**で動きます(PWA)。Chromium 系推奨。

## 6. 録音(歌・生演奏)

Forte に「オーディオファイルの読み込み」はありません。音声の入口は
**マイク(と MIDI)だけ**で、録音には必ず来歴(いつ・誰が・どのデバイスで)が
刻まれます。ブラウザエディタで:

1. (推奨)⏱ Calib を一度実行 — 実測レイテンシが以後のテイクに記録されます
2. ● Rec → 演奏/歌う → ■ 録音停止
3. `assets/take-1.frec` が保存され、ステータスバーに import 行が出ます
4. 曲に貼る:

```forte
import take from "./assets/take-1.frec"
song "..." {
  track Voice {
    audio take at bars(5..8)      // instrument 不要。エフェクトは insert で
    insert reverb(mix: 0.2)
  }
}
```

来歴のない `.frec`(他所から持ち込んだ音)はコンパイルエラー E-PROV-001 に
なります。これは仕様であってバグではありません — Forte の信条です。

## 7. VSCode で書く

```bash
cd editor/vscode-forte
npm install && npm run compile
# このフォルダを VSCode で開いて F5(拡張開発ホスト)
```

設定 `forte.path` に `target/release/forte` の絶対パスを入れると:
- エラーが打ちながら赤線で出る(補完・ホバー・format-on-save も対応)
- コマンドパレットから **Forte: Play (hot reload)** / **Build** / **Stop**

## 8. Hub — 公開・fork・リリース

```bash
export FORTE_HUB=~/.forte-hub        # 置き場所(なければ ./.forte-hub)

forte hub publish my-song.forte      # import したライブラリごとスナップショット
forte hub list
forte hub fork mylib ./work/mylib    # ★取得はこれだけ。DL コマンドは存在しない
#   → 手元に来歴スタンプ(.forte-lineage.json)付きでコピーされる
#   → 改変して publish --as newname すると "forked from mylib v1" が自動記録

forte hub release my-song            # 決定論ビルド → digest を台帳に記録
forte hub verify  my-song            # 誰でも再現検証できる(改竄は MISMATCH)
forte hub lineage my-song            # 系譜: fork 元/先、リリース、検証回数
forte hub similar my-song            # 同じコード進行の曲(キーが違っても見つかる)
```

ブラウザで系譜をディグる:

```bash
forte hub serve                      # API: http://127.0.0.1:9377
# → http://localhost:8000/web/hub.html を開く
```

曲ページで **▶ Listen**(ソースからその場で再生)、**Verify in browser**
(リリースの digest を自分のタブで再現検証)、**Fork → エディタへ**
(fork が台帳に記録され、ファイルがエディタに入る)ができます。

## 9. 困ったとき

| 症状 | 対処 |
| --- | --- |
| `forte play` で音が出ない | 冒頭に `audio: 出力デバイスなし — 無音バックエンド` と出ていれば音声デバイスの問題。Linux は `apt install libasound2-dev` してリビルド |
| エラーの読み方 | `行:列 [E-XXX-nnn] メッセージ`。メッセージ内に「使えるもの」が列挙されます。コード体系は仕様 v1 の §7 |
| ブラウザで音が出ない | 最初の ▶ Play はユーザー操作が必要(ブラウザの自動再生制限)。Safari は制約が多く Chromium 推奨 |
| wasm ビルドが失敗 | `rustup target add wasm32-unknown-unknown` |
| 決定論を自分で確認したい | `rustup target add wasm32-wasip1` して `scripts/determinism_test.sh`(要 Node 20+) |
| テスト一式 | `cargo test -p dawcore -p fortelang` / ブラウザ E2E は `npm i playwright` 後に `node scripts/web_e2e.mjs` |

## 10. よくある質問

**Q. 手持ちの WAV やサンプルパックを読み込みたい。**
A. できません(仕様)。Forte は「出自の分からないオーディオ」を構造的に
排除することで、全ての音の来歴が辿れる世界を作っています。ドラムは
`sampler` のビルトイン音源か `device` で合成、歌・生演奏はマイク録音で。

**Q. GUI でノートを編集したい。**
A. しません(仕様)。編集の唯一の真実はコードです。代わりに可視化
(アレンジビュー)は読み取り専用で提供します。diff が読める・マージできる・
fork できるのはテキストだからです。

**Q. 同じコードなのに環境で音が変わらない?**
A. 変わりません。それが決定論ビルドで、リリース検証(`hub verify`)と
貢献証明の土台です。`build.manifest.json` の digest で確認できます。
