# ソフトウェア要求仕様 (SRS) — Forte

Status: Draft v0.1 / 2026-07-02
上位文書: 02-system-requirements.md (SYS)
下位文書: 04-software-architecture.md (SAD), 05-detailed-design.md (SDD)

表記: SRS-<コンポーネント>-<番号> [→ トレース先 SYS]。
コンポーネント: LANG(言語処理系), PKG(パッケージ管理), CORE(オーディオエンジン),
BLD(ビルド), LSP(エディタ支援), VIS(可視化), WEB(Webエディタ/実行環境),
REC(録音), HUB(ハブ), PLY(プレイヤー), SEC(セキュリティ)。

---

## 1. 言語処理系 (LANG)

- **SRS-LANG-001** [→SYS-LNG-001] Forte lang は以下の一級概念を持つ:
  `song`(曲), `track`, `pattern`(ノート列), `instrument`, `effect`, `bus`,
  `automation`, `asset`(録音参照), `module`(再利用単位)。
- **SRS-LANG-002** [→SYS-LNG-003] ソースは UTF-8 テキスト(拡張子 `.forte`)。
  1 ファイル 1 モジュール。フォーマッタ(`forte fmt`)を標準提供し、正規形を一意にする
  (diff/マージの安定化)。
- **SRS-LANG-003** [→SYS-ENG-001] 言語は**決定論的**である: 実行時乱数・時刻・外部 I/O を
  持たない。乱数は明示シード必須(`random(seed: …)`)。
- **SRS-LANG-004** [→SYS-LNG-001] 静的型付け。主要型: `Note`, `Pattern`, `Audio`(信号),
  `Control`(制御信号), `Time`(拍/秒の単位付き), `Pitch`, `Db`, `Params`。
  単位の混同(拍と秒、dB と線形)を型エラーとして検出する。
- **SRS-LANG-005** [→SYS-LNG-002] `import` は `@scope/name@semver` 形式の外部依存と
  相対パスのローカル依存をサポートする。
- **SRS-LANG-006** [→SYS-LNG-004] DSP を言語内で記述するための低レベル層
  (サンプル単位処理、状態変数、フィルタプリミティブ)を持ち、コンパイラが
  ネイティブ/WASM 双方に落とす。高レベル層(曲・アレンジ)は宣言的に記述する。
- **SRS-LANG-007** [→SYS-EDT-002] インクリメンタルコンパイル: モジュール単位のキャッシュを持ち、
  1 モジュール変更の再コンパイル+音反映が 1 秒以内。
- **SRS-LANG-008** エラーメッセージは音楽家向けの語彙で出す
  (例: 「Track 'Vocal' の 3 小節目: Pattern の長さが拍子 4/4 と一致しません」)。

## 2. パッケージ管理 (PKG)

- **SRS-PKG-001** [→SYS-LNG-002] マニフェスト `forte.toml`(名前、バージョン、依存、
  ライセンス、公開範囲)とロックファイル `forte.lock`(全依存の解決済みコミットハッシュ)。
- **SRS-PKG-002** [→SYS-HUB-002,003] public 依存の取得は Hub の fork API を経由し、
  取得の事実が系譜に記録される。レジストリからの匿名ダウンロードは存在しない。
- **SRS-PKG-003** [→SYS-LNG-004] public 公開時にソース必須。WASM のみのモジュールは
  private でのみ利用可。
- **SRS-PKG-004** [→SYS-ENG-004] ロックファイルには依存の**コンテンツハッシュ**を含め、
  改竄・すり替えを検出する。

## 3. オーディオエンジン (CORE)

- **SRS-CORE-001** [→SYS-ENG-002] エンジンはコンパイル済みプロジェクトから
  **レンダーグラフ**(ノード=instrument/effect/bus、エッジ=audio/control)を構築し、
  リアルタイム・オフライン共通のコードパスで処理する。
- **SRS-CORE-002** [→SYS-ENG-003] リアルタイム経路は割り当て・ロック・システムコールなし
  (既存 dawcore の規律を踏襲)。UI/制御からの変更はロックフリー SPSC リング経由、
  置換された構造はガベージチャネルで非 RT スレッドに返却して解放する。
- **SRS-CORE-003** [→SYS-ENG-001] 浮動小数点決定論規約: f32 固定、FMA 無効化または
  明示的 fma のみ、超越関数は自前実装(libm 固定)、denormal は明示 flush、
  並列化は結合順序を固定した決定論的リダクションのみ。
- **SRS-CORE-004** [→SYS-NFR-005] ターゲット: native(cdylib + C ABI)と wasm32
  (AudioWorklet 内動作)。単一 Rust ソース。※実装言語は SAD の決定 D-01 参照。
- **SRS-CORE-005** [→SYS-ENG-002] サンプル精度のスケジューラ、テンポ/拍子マップ、
  ループ、オートメーション(ブロックレート+サンプル精度イベント)をサポート。
- **SRS-CORE-006** [→SYS-EDT-002] ホットリロード: 新レンダーグラフへの差し替えは
  再生位置・稼働ボイスを可能な範囲で維持し、クリックノイズなしで行う
  (クロスフェード 10ms 以内)。
- **SRS-CORE-007** [→SYS-NFR-003] パフォーマンス計測(コールバック使用率、
  アンダーランカウンタ)を常時公開する。

## 4. ビルド (BLD)

- **SRS-BLD-001** [→SYS-ENG-001] `forte build` は WAV(および Opus)を出力し、
  出力の SHA-256 を**ビルド証明**として `build.manifest.json` に記録する。
- **SRS-BLD-002** [→SYS-ENG-004] `build.manifest.json` は全依存(コミット+fork系譜 ID)、
  全アセット(ハッシュ+収録来歴)、エンジンバージョン、ビルド設定を含む。
- **SRS-BLD-003** [→SYS-HUB-004] open-stems ビルド: バス/トラック単位のステム群+
  ミックス定義を成果物とするビルドプロファイル。
- **SRS-BLD-004** [→SYS-NFR-004] フルビルド 5 倍速以上(実時間比)、差分ビルド 1 秒以内。

## 5. エディタ支援 (LSP) / 可視化 (VIS)

- **SRS-LSP-001** [→SYS-EDT-001] LSP サーバー: 補完(モジュール・パラメータ・音名)、
  型診断、定義ジャンプ、リネーム、ホバー(パラメータの単位・範囲)。
- **SRS-LSP-002** [→SYS-EDT-001] VSCode 拡張: シンタックスハイライト、LSP 接続、
  再生コントロール(再生/停止/ループ範囲)、ビルドタスク。
- **SRS-VIS-001** [→SYS-EDT-003] 可視化ビュー(読み取り専用): ピアノロール、
  アレンジ概観、波形/スペクトラム、ミキサー(レベルメーター)、レンダーグラフ、系譜。
  各ビューはソース位置と双方向リンク(ノートをクリック→該当コード行へ)。
- **SRS-VIS-002** [→SYS-EDT-002] 可視化は再生と同期し 60fps を目標とする(描画は
  オーディオに影響しないこと)。

## 6. Web エディタ / ブラウザ実行 (WEB)

- **SRS-WEB-001** [→SYS-EDT-004] Monaco ベースの Web エディタ+同一 LSP(WASM 動作)。
- **SRS-WEB-002** [→SYS-ENG-003] ブラウザ再生: AudioWorklet(シンク)+ WASM エンジン+
  SharedArrayBuffer リング(ringbuf.js 型)。COOP/COEP 配備。
- **SRS-WEB-003** [→SYS-NFR-001] プロジェクトとアセットは OPFS に保存(Worker +
  SyncAccessHandle)。オフラインで編集・ビルド・再生が完結する PWA。
- **SRS-WEB-004** [→SYS-NFR-002] 縮退マトリクス: Safari は Web MIDI 不可・
  ストレージ 7 日消去のため「クラウド同期必須+MIDI 入力なし」の縮退モードを明示する。
- **SRS-WEB-005** [→SYS-GOV-003] ローカルプロジェクトの zip エクスポート/インポート
  (git bundle 互換)を提供する。

## 7. 録音 (REC)

- **SRS-REC-001** [→SYS-REC-001] 入力デバイスは MIDI(Web MIDI / CoreMIDI 等)と
  マイク/ライン(getUserMedia / native)のみ列挙する。ファイルドロップ・
  オーディオ import の UI/API を実装しない。
- **SRS-REC-002** [→SYS-REC-002] 録音は AudioWorklet で PCM 直取りし
  (MediaRecorder 不使用)、SAB リング→ Worker → OPFS/ディスクへ逐次書き込み。
  タブ/プロセスクラッシュ後も直前までのテイクが回復できる。
- **SRS-REC-003** [→SYS-REC-002] 録音アセット形式 `.frec`: PCM+来歴ブロック
  (セッション ID、入力デバイス種別、収録時刻、収録者 ID、クライアント署名)。
  来歴ブロックのないオーディオ参照はコンパイルエラー。
- **SRS-REC-004** [→SYS-REC-003] ループバック較正ウィザード(出力→入力の往復遅延を
  実測し ±1ms 精度で補正値を保存)。録音は補正値でタイムライン配置される。
- **SRS-REC-005** [→SYS-REC-001] getUserMedia は
  echoCancellation/noiseSuppression/autoGainControl をすべて false で開く。
  効かないブラウザ(既知バグ)では警告を表示する。
- **SRS-REC-006** [→SYS-REC-004] 演奏 fork モード: open-stems リリースを fork し、
  録音トラックの追加のみを行う最小 GUI(再生+録音+テイク選択+パンチイン)。

## 8. Hub (HUB)

- **SRS-HUB-001** [→SYS-HUB-001] git 互換ホスティング(smart HTTP)。private は
  通常の git 運用可。
- **SRS-HUB-002** [→SYS-HUB-002] public リポジトリ: git clone/fetch を認可層で拒否し、
  fork API(系譜記録+所有権付与)経由でのみ複製を提供する。
- **SRS-HUB-003** [→SYS-HUB-002,003] 系譜グラフ DB: ノード=リポジトリ/リリース/アセット/
  ユーザー、エッジ=fork/depends/performed/released。公開 API で照会可能。
- **SRS-HUB-004** [→SYS-HUB-004] リリースパイプライン: タグ push → ビルドファームで
  クリーンルーム決定論ビルド → 提出されたビルド証明とハッシュ照合 → 一致で公開。
  不一致は公開拒否(再現性の強制)。
- **SRS-HUB-005** [→SYS-HUB-005] 配信はストリーミングのみ(セグメント化+署名 URL)。
  ダウンロード API を持たない(完全な複製防止は不可能である前提で、規約+検出で補完)。
- **SRS-HUB-006** [→SYS-REC-005] 音響指紋(リリース音源全件)を保持し、新規アセット/
  リリースとの照合ジョブ+通報フローを持つ。
- **SRS-HUB-007** [→SYS-HUB-006] ポイント台帳の**データ基盤のみ**先行実装:
  再生イベント→系譜への貢献集計(バッチ)。換金・消費機能は実装しない(将来)。
- **SRS-HUB-008** [→SYS-PLY-002] 曲ページ: 系譜(fork 元/先、使用モジュール、演奏者)、
  バージョン一覧(歌い手違い・リミックス)、コードブラウズ(public)。

## 9. プレイヤー (PLY)

- **SRS-PLY-001** [→SYS-PLY-001] Web プレイヤー(ログイン不要再生、ゲイン正規化)。
- **SRS-PLY-002** [→SYS-PLY-003] 類似検索 v1: 使用モジュール・コード進行(言語 AST から
  抽出した進行の正規形)・テンポ/キーでの検索。埋め込みベースの類似は v2。

## 10. セキュリティ / プライバシー (SEC)

- **SRS-SEC-001** [→SYS-GOV-002] private リポジトリ/アセットは保存時暗号化、
  アクセス監査ログ。運営の閲覧は明示的同意フローなしに不可。
- **SRS-SEC-002** [→RSK-02] クライアント署名鍵はデバイスローカルに生成し、
  来歴ブロックに署名する。鍵の登録/失効を Hub で管理。
- **SRS-SEC-003** WASM モジュール(サードパーティ音源)はサンドボックス実行
  (メモリ隔離、ホスト API は音声処理のみ、ファイル/ネットワークアクセスなし)。

## 付録 A: トレーサビリティマトリクス(抜粋)

| SYS | SRS |
| --- | --- |
| SYS-LNG-001 | SRS-LANG-001/004/006 |
| SYS-LNG-003 | SRS-LANG-002 |
| SYS-ENG-001 | SRS-LANG-003, SRS-CORE-003, SRS-BLD-001, SRS-HUB-004 |
| SYS-ENG-002 | SRS-CORE-001/005/006 |
| SYS-ENG-003 | SRS-CORE-002, SRS-WEB-002 |
| SYS-ENG-004 | SRS-BLD-002, SRS-PKG-004 |
| SYS-EDT-001 | SRS-LSP-001/002 |
| SYS-EDT-002 | SRS-LANG-007, SRS-CORE-006, SRS-VIS-002 |
| SYS-EDT-003 | SRS-VIS-001 |
| SYS-EDT-004 | SRS-WEB-001/002/003 |
| SYS-HUB-002 | SRS-HUB-002/003, SRS-PKG-002 |
| SYS-HUB-004 | SRS-HUB-004, SRS-BLD-003 |
| SYS-HUB-005 | SRS-HUB-005 |
| SYS-REC-001 | SRS-REC-001/005 |
| SYS-REC-002 | SRS-REC-002/003, SRS-SEC-002 |
| SYS-REC-003 | SRS-REC-004 |
| SYS-REC-004 | SRS-REC-006 |
| SYS-REC-005 | SRS-HUB-006 |
| SYS-GOV-002 | SRS-SEC-001 |
| SYS-GOV-003 | SRS-WEB-005 |
| SYS-NFR-001 | SRS-WEB-003 |
| SYS-NFR-004 | SRS-BLD-004, SRS-LANG-007 |
| SYS-NFR-005 | SRS-CORE-004 |
