# ソフトウェアアーキテクチャ設計 (SAD) — Forte

Status: Draft v0.1 / 2026-07-02
上位文書: 03-software-requirements.md (SRS)
下位文書: 05-detailed-design.md (SDD)

---

## 1. アーキテクチャ上の決定 (Architecture Decision Records)

| ID | 決定 | 根拠 | 状態 |
| --- | --- | --- | --- |
| **D-01** | コアエンジン+コンパイラの実装言語は **Rust**(C ABI で API 化) | 既存 dawcore 資産(ロックフリー設計・DSP・オフラインレンダ)、wasm32 ツールチェーンの成熟、メモリ安全。創業者要望は「C++ で組み API 化」だが、要件(ネイティブコア+API+WASM)は Rust で同等以上に満たせる | **承認済 2026-07-02** |
| D-02 | Forte lang は**独自 DSL**(汎用言語への埋め込みではない) | 決定論(SRS-LANG-003)と入力制限(SRS-REC-001)を言語仕様レベルで強制するため。TS/Python 埋め込みでは任意 I/O を排除できない | **承認済 2026-07-02** |
| D-03 | 言語は「宣言的な曲記述層」+「DSP 記述層」の 2 層 | 作曲者には簡易な宣言層、音源開発者には低レベル層。単一言語内のサブセットとして提供 | 承認待ち |
| D-04 | リアルタイムとオフラインは**同一レンダーグラフ実装** | SYS-ENG-002。dawcore の bounce.rs 方式を踏襲 | 承認待ち |
| D-05 | ブラウザ実行は AudioWorklet(シンク)+ WASM + SAB リング | 業界標準パターン(00-research §3.1)。COOP/COEP 配備必須 | 承認待ち |
| D-06 | サードパーティ音源/エフェクトの配布形式は **Forte ソース**(public 必須)。コンパイル済み WASM は private のみ | ホワイトボックス原則(SYS-LNG-004)。WAM 2.0 は採用しない(バイナリ+任意 JS UI を許すため原則に反する)が、ホスト API 設計の参考にする | 承認待ち |
| D-07 | Hub の VCS は **git 互換**(独自 VCS を作らない)。fork 制約は認可層で実装 | git のエコシステム(diff/merge/歴史)をそのまま使う。public clone 拒否はサーバー側認可で実現(SRS-HUB-002) | 承認待ち |
| D-08 | 録音アセットはコンテンツアドレス(SHA-256)で git LFS 相当の別ストアに置き、リポジトリには参照+来歴のみ | CRDT/git にバイナリを入れない業界定石(00-research §3.5) | 承認待ち |
| D-09 | 系譜グラフは Hub のファーストクラスデータ(グラフ DB)。git の履歴とは独立に管理 | fork/depends/performed/released は git では表現できない | 承認待ち |
| D-10 | エディタ戦略は「VSCode 拡張が主、Web エディタ(Monaco)が従」で開始 | 中〜上級者ターゲット。LSP を共通化し二重開発を避ける | 承認待ち |
| D-11 | 決定論規約: f32、自前 libm、denormal flush、決定論的並列リダクション | SYS-ENG-001 の成立条件。SDD §4 に詳細。**スパイクで実証済み(07-determinism-spike.md): libm 統一のみで native/wasm ビット同一を達成** | 承認待ち |
| D-12 | ポイント経済は「イベント収集+系譜集計」のみ先行、経済ルールは後付け可能な台帳設計 | 法規制・ゲーム理論の検討前に不可逆な設計をしない | 承認待ち |

## 2. システム分割

```
┌─────────────────────────── SS1 ツールチェーン (Rust) ───────────────────────────┐
│                                                                                  │
│  forte-lang     パーサ / 型検査 / モジュール解決 / 正規化(fmt) / AST→IR         │
│  forte-compile  IR → レンダーグラフ定義 + DSP カーネル(native/wasm コード生成)   │
│  forte-core     レンダーグラフ実行系(RT/offline 共通)・スケジューラ・ミキサー    │
│                 ← 既存 dawcore の engine/dsp/bounce を改造して流用               │
│  forte-pkg      forte.toml / forte.lock / Hub fork API クライアント              │
│  forte-cli      build / play / fmt / test / publish                              │
│  forte-lsp      LSP サーバー(forte-lang を組み込み)                              │
│  C ABI: forte_ffi (libforte.so / .dylib / .dll) — ML/外部ツールから利用可能      │
└──────────────────────────────────────────────────────────────────────────────────┘

┌──────────── SS2 エディタ ────────────┐   ┌──────────────── SS3 Hub ────────────────┐
│ VSCode 拡張(TS)                      │   │ git ホスティング + 認可層(public=fork限定)│
│  ├ LSP クライアント                   │   │ 系譜グラフサービス(GraphDB)              │
│  ├ 再生コントロール / 可視化 Webview  │   │ アセットストア(CAS, S3系+署名URL)        │
│  └ ローカル forte-core (native)       │   │ ビルドファーム(決定論ビルド+検証)        │
│ Web エディタ(Monaco + WASM 一式)      │   │ ストリーミング配信 / プレイヤー           │
│  ├ forte-lsp (wasm)                   │   │ 指紋照合 / モデレーション                 │
│  ├ forte-core (wasm, AudioWorklet)    │   │ イベント台帳(再生→貢献集計)             │
│  └ OPFS プロジェクトストア             │   │ アカウント / 署名鍵管理                   │
└───────────────────────────────────────┘   └───────────────────────────────────────────┘
```

## 3. ランタイムアーキテクチャ(再生・録音経路)

### 3.1 ネイティブ(CLI / VSCode 拡張内)

```
forte-lang ──AST──► forte-compile ──graph+kernels──► forte-core
                                                        │
エディタ/CLI ──コマンド(SPSC ring)──► RT スレッド(cpal コールバック)
                                    ◄──ガベージ返却 / メーター(atomics)
録音: 入力コールバック ──SPSC──► 書き込みスレッド ──► .frec 逐次書き込み
```

### 3.2 ブラウザ

```
Main thread: Monaco / 可視化(Canvas/WebGPU) / トランスポート UI
   │ postMessage(制御) / SAB リング(オーディオ・メーター)
Worker(compile): forte-lang + forte-compile (wasm) — 差分ビルド
Worker(asset):   OPFS SyncAccessHandle — プロジェクト/録音の永続化
AudioWorklet:    forte-core (wasm) — 128 フレーム毎に SAB リングから
                 コマンド消費・レンダー・メーター publish
録音: AudioWorklet(入力 tap) ──SAB ring──► Worker(asset) ──► OPFS .frec
```

- グラフ差し替え(ホットリロード)は「新グラフを Worker 側で構築 → Box 相当を
  リング経由で移送 → RT 側で swap → 旧グラフをガベージ返却」(dawcore の既存プロトコルと同型)。
- COOP/COEP 必須。サードパーティ資産は同一オリジン配信(自社 CDN)に限定する。
  ※ D-06 により任意オリジンのプラグイン読込は存在しないため、WAM で問題になる
  クロスオリジン緊張は発生しない。

## 4. 言語アーキテクチャ(2 層構造, D-03)

| 層 | 対象ユーザー | 内容 | 実行形態 |
| --- | --- | --- | --- |
| **Score 層**(宣言的) | 作曲者 | song/track/pattern/arrangement/mix。時間は拍単位。制御フローは限定(map/repeat/条件はコンパイル時評価) | コンパイル時に完全展開 → イベント列+グラフ |
| **DSP 層**(手続き的) | 音源・エフェクト開発者 | `process(frame)` カーネル、状態変数、フィルタ/オシレータプリミティブ | native/wasm へコード生成、RT 実行 |

- 両層とも決定論(SRS-LANG-003)。I/O・時計・非シード乱数は言語に存在しない。
- Score 層は「コンパイル時に全展開できる」ことが決定論とビルド速度の鍵。
  生成的な作曲(アルゴリズム作曲)はシード付きで Score 層のコンパイル時関数として書ける。

## 5. データアーキテクチャ

### 5.1 リポジトリ内容物

```
song-repo/
  forte.toml          マニフェスト(名前・版・依存・ライセンス・公開範囲)
  forte.lock          解決済み依存(コミット+コンテンツハッシュ+系譜ID)
  src/*.forte         コード(曲・トラック・カスタムデバイス)
  assets/*.frec       録音参照ではなく実体は CAS。ここにはポインタファイル
                      (ハッシュ+来歴ブロック+署名)のみ置く (D-08)
  build.manifest.json 最新ビルド証明(出力ハッシュ+全来歴) — release 時に検証される
```

### 5.2 系譜グラフ(D-09)

- ノード: `User / Repo / Release / Asset / ModuleVersion`
- エッジ: `forked_from / depends_on(version) / performed_on / released_as / recorded_by`
- 不変条件: public Repo の複製操作は必ず `forked_from` エッジを生成する(SRS-HUB-002)。
- 再生イベントは `Release` に紐付き、バッチで依存閉包に沿って貢献度を按分集計する(D-12)。

## 6. 縮退マトリクス(ブラウザ)

| 機能 | Chromium | Firefox | Safari |
| --- | --- | --- | --- |
| 編集・ビルド・再生(WASM+AudioWorklet+SAB) | ○ | ○ | ○(要 COOP/COEP) |
| MIDI 入力 | ○ | ○ | **×(Web MIDI 非対応)→ 画面鍵盤のみ** |
| マイク録音(制約 off) | ○ | ○ | △(EC 制約バグ・44.1kHz 明示) |
| OPFS 永続 | ○ | ○ | △ **7 日消去 → Hub 同期を必須化** |
| 実フォルダ保存 | ○(FSA) | × zip DL | × zip DL |
| 推奨ポジション | フル機能 | ほぼフル | 「試す・聴く」+要同期 |

ネイティブ(CLI+VSCode)がプロ用途の一級環境であるため、ブラウザ格差は
「Web は入口・共有・軽作業」という位置づけで吸収する。

## 7. dawcore からの流用マップ

| dawcore(既存) | Forte での扱い |
| --- | --- |
| dsp/(polyBLEP, SVF, ADSR, delay, FDN reverb, sampler) | **流用**: 標準ライブラリ `@std/*` の DSP カーネル実装に転用 |
| engine.rs(サンプル精度スケジューラ、ミキサー) | **改造流用**: レンダーグラフ化(固定 3 ステージ → 任意グラフ) |
| command.rs(SPSC+ガベージチャネル) | **流用**: ホットリロード/制御プロトコルの基盤 |
| bounce.rs(オフラインレンダ) | **流用**: `forte build` の中核。決定論規約を追加適用 |
| model.rs(インデックス参照のプロジェクトモデル) | **廃棄**: モデルはコンパイラ出力(IR)に置き換わる |
| dawapp(egui UI) | **廃棄**(可視化の参考のみ)。編集 UI は作らない方針のため |
| tests/(オフラインレンダ検証) | **流用**: 決定論 CI(2 環境ハッシュ比較)に発展させる |

## 8. 検証アーキテクチャ

- **決定論 CI**: リファレンス曲コーパスを native(Linux x86_64)と wasm(Node)で
  ビルドし SHA-256 比較。PR 毎に実行(SYS-ENG-001)。
- **RT ベンチ**: アンダーランカウンタ+コールバック使用率をリファレンス曲で計測
  (SYS-NFR-003)。
- **ゴールデンオーディオテスト**: dawcore の visual test に相当する音の回帰テスト
  (出力ハッシュ固定。意図した変更時のみ更新)。
- **言語テスト**: `forte test`(モジュールの単体テスト: 期待イベント列/期待スペクトル)。
- **Hub 統合テスト**: fork 制約(clone 拒否)、リリース再現検証の E2E。
