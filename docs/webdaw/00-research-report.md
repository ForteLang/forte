# Web DAW 市場・技術調査レポート

調査日: 2026-07-02
手法: マルチエージェント Web 調査(検索 → 一次ソース取得 → クレーム単位の敵対的検証(3票制) → 統合)。
検証済みクレームは 23/25(2 件反証・除外)。本文中の確度表記は検証結果に基づく。

この文書は、Web アプリとして動く DAW を新規開発するにあたっての
(1) 競合分析、(2) 利用可能なオープンソース技術、(3) Web プラットフォーム技術の成熟度、
(4) 市場の空白と差別化機会 をまとめたものであり、後続の IEC 62304 型ドキュメント
(システム要求 → ソフトウェア要求 → アーキテクチャ設計 → 詳細設計)の入力となる。

---

## 1. 競合環境(商用 Web DAW)

### 1.1 プレイヤー一覧

| 製品 | ポジション | 価格 | 技術 | 強み | 弱み |
| --- | --- | --- | --- | --- | --- |
| **BandLab** | 無料・ソーシャル・オールインワン。登録 1 億人超(2024 年末) | DAW 完全無料。Membership $14.95/月等で収益化(ARR 約 $48M) | Web Audio + クラウド同期 + ネイティブモバイル | 無料での機能量(無制限トラック、24,000+ ループ、AI マスタリング、Splitter、SongStarter)が圧倒的 | プロ向け精密編集(コンピング、深いオートメーション)の欠如。本格制作はデスクトップ移行前提 |
| **Soundtrap** (Spotify → 2023 年創業者へ売却) | 初心者・教育・ポッドキャスト。「協働型クラウドスタジオ」 | 無料 5 トラック、$9.99〜17.99/月。教育 50 シート $249/年〜 | Web Audio ベース | リアルタイム共同編集の完成度、教育向け管理、Auto-Tune、レイテンシ較正機能 | 無料版が実質デモ。有料でも無料 BandLab に機能で劣る。Spotify が手放した(クリエイターツール戦略撤退) |
| **Soundation** | 初心者〜中級のループベース制作 | Free(3 プロジェクト/1GB)〜Pro $29.99/月 | Flash → NaCl → **WASM Threads を世界初実装**(最大 6 スレッドで約 300% 性能向上) | 軽快な WASM エンジン、安価 | 上位機能の課金ゲート、オフライン不可、第三者プラグイン非対応 |
| **Amped Studio** | ビートメーカー〜「本格寄り」。Chromebook 訴求 | 無料〜$12.99/月 | Web Audio + WAM | **WAM 対応(OBXD/DEXED 同梱+ショップ)、VST3 サポート(有料)、商用唯一級のオフライン PWA** | UI の親切さとコミュニティが弱い |
| **Audiotool** | モジュラー配線型。エレクトロニック系ホビイスト。MAU 30 万+ | **完全無料**(異例) | Flash(2008)→ HTML5。Worker で DSP → SAB リングバッファ → AudioWorklet のマルチコア設計 | 独自のモジュラー体験、強いコミュニティ、新ベータで開発者 API「Nexus」 | 学習コスト、大規模プロジェクトの性能、録音/編集が弱い、収益基盤不透明 |
| **Ableton** (Learning Music/Synths, Note, Move) | ブラウザは教育とファネルに限定。フル Web DAW を意図的に作らない | 教材無料、Note $5.99 買い切り | Learning Synths は **Max/MSP RNBO → Web コンパイル**の先行事例 | ブランド、Cloud 経由の Note→Live 導線 | Web フル DAW の空白を残している(= 参入余地) |
| **Splice** | 素材サブスクが本業。クラウド DAW(Splice Studio)からは**撤退** | $12.99〜39.99/月 | — | 素材供給+モバイル着想(CoSo) | 「Web フル DAW より素材供給が収益的に合理的」と判断した実例 |
| **Sesh.fm** | ビートメーカー向け「マルチプレイヤー DAW」 | 無料 + Pro 買い切り志向 | — | リアルタイムカーソル共有、バージョン管理、AI ステム分離無料 | 新興でエコシステム小 |

### 1.2 死んだ製品からの教訓(重要)

- **AudioSauna**: Flash 終了と共に消滅。プラットフォーム基盤への依存がプロダクトの死に直結。
- **Endlesss**(2024 年 5 月閉鎖): サーバー停止でアプリ自体が機能不能に。ユーザーは期日までに自作音源を DL するよう告知された。CDM の総括は「SaaS 音楽ツールはサービスが終わればツールも作品も消える」。→ **クラウド専用アーキテクチャは作品の可用性リスク**。
- **WavTool**(2024 年 11 月停止 → 2025 年 6 月 Suno が買収): 「GPT-4 搭載の世界初テキスト駆動 DAW」。単体事業として存続できず、ユーザーは約 7 ヶ月ツールにアクセス不能のまま放置された。チームは Suno Studio(2025 年 9 月ローンチ、$30/月)の母体に。

### 1.3 横断検証の結果

1. **第三者プラグイン**: WAM 2.0 という標準がありながら、商用実装は Amped Studio ほぼ唯一。BandLab/Soundtrap/Soundation はプラグインエコシステムを持たない。
2. **オフライン PWA**: Amped Studio がほぼ唯一の商用実装。他は常時接続前提。
3. **プロ級レコーディング**: どの商用 Web DAW も未達。テイク管理・コンピング UI を備えた商用 Web DAW は確認できなかった。ブラウザの往復レイテンシ(ベスト約 14〜30ms)と正確なレイテンシ報告 API の欠如が原因。
4. **日本市場**: BandLab が「無料で始める DTM」として圧倒的推奨。ブラウザ DAW は「入門・スケッチ用」で本格制作は Cubase/Studio One/Logic への移行が前提という論調。日本語 UI・教材はほぼ空白。

### 1.4 市場の空白(証拠から推論される未充足ニーズ)

1. **中級〜プロ向け Web DAW の不在** — 全プレイヤーが初心者/教育/スケッチに収斂。
2. **プラグインエコシステム** — WAM 2.0 実装が Amped と学術のみ。開発者が収益を得られる Web プラグインマーケットは未開拓。
3. **オフライン/ローカルファースト** — Endlesss/WavTool の死が「所有できる Web DAW」の価値を実証。
4. **低遅延録音技術** — WASM+AudioWorklet+マルチスレッドを録音品質(較正・補正・コンピング)に振り向けたプレイヤー不在。
5. **持続可能なビジネスモデル** — 完全無料(Audiotool)は持続性に疑問、サブスク疲れも顕在。
6. **「生成 AI 後」の編集需要** — 生成 AI と本格編集を中立的立場で橋渡しする Web DAW は空席。
7. **日本語圏ローカライズ** — GIGA スクール(Chromebook)との相性が良い教育市場が手つかず。

---

## 2. オープンソース・ビルディングブロック

### 2.1 Web DAW 本体(検証済み・確度高)

| プロジェクト | 概要 | ライセンス | 利用可否の判断 |
| --- | --- | --- | --- |
| **openDAW** (Audiotool 創業者 André Michelle) | TypeScript 製次世代 Web DAW。教育・プライバシー重視 | **AGPL v3 + 商用のデュアル** | プロトタイプ段階。オーディオエンジンは未 WASM 化(TS が AudioWorklet 上で動作)、WASM エンジンは 2026 Q2 予定・1.0 は Q3 予定。土台採用は時期尚早、**設計参考+動向追跡対象** |
| **GridSound** | Web Audio 製ブラウザ DAW。活発にメンテ(2026 年 6 月 v1.58.5) | **AGPL-3.0**、かつ「half open-source」(バックエンド非公開) | コード流用は AGPL 义務が発生。参考実装として閲覧価値のみ |
| **Signal** | Web MIDI エディタ | — | ピアノロール UI の参考 |
| **WAM-studio** | WAM 2.0 のリファレンス DAW(学術) | OSS | WAM ホスト実装の参考 |

**要注意**: 再利用候補の主要 OSS Web DAW は軒並み AGPL 系。プロプライエタリ製品に組み込むなら商用ライセンス取得(openDAW)かソース公開が必要。**エンジンは自前開発が現実的**。

### 2.2 プラグイン標準: Web Audio Modules 2.0(検証済み・確度高)

- Web Audio API には VST/AU/AAX/LV2 相当の高レベルプラグイン抽象が**存在しない**(これが「Web に VST がない」の根本原因)。
- **WAM 2.0**(2015 年開始、2021 年 v2.0)がこのギャップを埋める「Web 版 VST」標準。DSP+UI コンポーネント、パラメータオートメーション、MIDI、状態保存/読込をサポート。
- ホスト統合: メタデータ JSON 取得 → ES Module 動的 import → 標準 AudioNode として接続(DSP は AudioWorklet 上の WamProcessor)。
- エコシステム: 多数のプラグイン/ホスト、DAW 2 つ(WAM-studio、商用 Amped Studio)、Sequencer.party 等。ただし規模は OSS プラグイン数十個のニッチで、W3C 標準ではない。
- **C/C++/Faust/Csound を WASM にコンパイルして WAM 化できる**。Faust オンライン IDE には WAM 2.0 エクスポート(wam2-ts / wam2-poly-ts、ポリフォニック MIDI 対応)がある。

### 2.3 ライブラリ・DSP 資産

| 資産 | 用途 | 状態 |
| --- | --- | --- |
| **Tone.js** | Transport(サンプル精度スケジューリング)、シンセ群、Sampler、エフェクト | 活発(2026-07-01 に v15.5.26、14.7k スター)。ただし本格 DAW エンジンには抽象が高すぎる面もあり、UI 層/プロトタイプ向き |
| **ringbuf.js** (Paul Adenot) | SAB 上の wait-free SPSC リングバッファ | W3C Web Audio 仕様共同エディタによる実装。エンジンの中核部品 |
| **Faust** | DSP 言語 → WASM/WAM | 成熟。エフェクト/シンセの量産に有効 |
| **Csound-WASM** | 同上 | 成熟 |
| **RNBO** (Cycling '74) | Max パッチ → Web | 商用だが実績あり(Ableton Learning Synths) |
| Rust クレート(fundsp, oxisynth, dasp 等) | Rust DSP → WASM | 本リポジトリの既存 dawcore(Rust 製エンジン)を wasm32 ターゲットでコンパイルする路線と親和 |
| **Magenta.js** | ブラウザ内 MIDI 生成(MusicVAE 等) | 実質メンテナンスモードだが、ブラウザで動く数少ない MIDI 生成資産 |
| **demucs-rs / demucs-web / demucs-onnx** | ブラウザ内ステム分離(WASM/WebGPU、完全クライアントサイド) | 2026 年時点で実用段階。モデル約 172MB |
| **Transformers.js v3 / onnxruntime-web** | WebGPU 推論(musicgen-small 等) | 短いサンプル生成が現実的 |

---

## 3. Web プラットフォーム技術の成熟度

### 3.1 オーディオエンジン(検証済み・確度高)

Chrome チーム公式のデザインパターンが確立している:

- **実時間予算**: レンダー量子は **128 フレーム固定**、44.1kHz でコールバックあたり**約 3ms**。超えると可聴グリッチ。(Chrome の `renderSizeHint` はオリジントライアル段階)
- **AudioWorklet + WASM**: C/C++/Rust 資産の持ち込み + JS JIT/GC オーバーヘッドの排除。
- **重量級エンジンの標準形**: AudioWorklet + SharedArrayBuffer + Atomics + 専用 Worker。MessagePort は割り当てとレイテンシのため実時間オーディオに不適。AudioWorklet は「オーディオシンク」として振る舞い、DSP は Worker 側で実行(Audiotool が実運用)。
- **前提条件**: SAB には COOP/COEP によるクロスオリジン分離が必要(配備・埋め込み・サードパーティ資産読込に制約)。

### 3.2 レイテンシ実測値

| 環境 | 往復レイテンシ |
| --- | --- |
| Chrome デフォルト | 約 67ms |
| Firefox デフォルト | 約 55ms |
| Chrome チューニング済(latencyHint:0、EC/NS/AGC オフ) | **約 19ms** |
| Firefox 同上 | **約 14ms** |
| ネイティブ ASIO/CoreAudio | < 10ms(1 桁 ms) |

- チューニング済みブラウザは**ネイティブ比 +10〜15ms のハンデ**。ソフト音源演奏は可能圏、スルーモニタリング+エフェクトは厳しい。
- `outputLatency` / `MediaTrackSettings.latency` は信頼できない値を返す既知問題 → **ループバック較正が実務解**(Soundtrap が較正機能を提供する唯一級の例)。
- 録音時の必須制約: `echoCancellation/noiseSuppression/autoGainControl: false`(未指定だと通話向け処理が音楽録音を破壊)。Chrome/Safari に制約が効かないバグ履歴あり。iOS Safari は 44.1kHz 明示指定が必要。
- 高品質録音は MediaRecorder ではなく **AudioWorklet で PCM 直取り**が推奨。

### 3.3 ストレージ

- **OPFS SyncAccessHandle(Worker 専用)**: 100MB 書き込み約 90ms(≈1.1GB/s)、IndexedDB の約 9 倍速。録音ストリームの逐次書き込みに最適。全主要ブラウザ対応。
- **クォータ**: Chrome はディスクの 60%。Firefox は 10GiB(persist で拡大)。**Safari は「7 日間非利用で全消去」(ITP)** → Safari ではクラウド同期必須。
- **File System Access API(実フォルダ読み書き)は Chromium 限定**。Firefox/Safari はフォールバック必要。

### 3.4 その他 I/O

- **Web MIDI**: Chrome/Edge/Firefox ○。**Safari は全バージョン非対応**(iOS は全ブラウザ WebKit 強制のため不可)。USB MIDI 実測約 1ms、BLE MIDI は +10〜30ms ジッタ。
- **WebCodecs AudioEncoder**: エクスポート本命は Opus + WAV フォールバック。AAC は Safari/Chrome 限定(Linux 不可)、Firefox は AAC エンコード不可。
- **WebGPU**: リアルタイム(128 サンプル毎)の GPU DSP はディスパッチ遅延のため**非現実的**。リアルタイム DSP は WASM(+SIMD)一択。**オフライン処理(ステム分離、解析、マスタリング)は WebGPU で実用域**。

### 3.5 協調編集・ローカルファースト

- **CRDT 三強**: Yjs(最大エコシステム、純 JS)/ Automerge 3.0(完全履歴 DAG、メモリ 10〜100 倍改善)/ **Loro 1.x(ベンチ首位、MovableList/Movable Tree 標準搭載 — クリップのドラッグ&ドロップ・トラック階層に最適)**。
- **Figma 方式**(サーバ権威 + プロパティ単位 LWW、CRDT ではない)は「クリップ=オブジェクト、パラメータ=プロパティ」の DAW モデルと極めて相性が良い。
- **波形データは CRDT に入れない**が業界の共通結論 → コンテンツアドレス(SHA-256)参照で分離し、blob は OPFS + オブジェクトストレージで同期。
- DAW 特有の「アンドゥは自分の操作のみ」は Yjs/Loro の UndoManager が標準サポート。
- 同期インフラ: PartyKit(Cloudflare 買収済、Durable Objects)、Liveblocks、Jazz(FileStream で blob 同期内蔵)、PowerSync/ElectricSQL。

---

## 4. AI 音楽制作トレンド(2024–2026)

### 4.1 市場構造

- **ステム分離と AI マスタリングは「テーブルステークス」**。Logic 11(Stem Splitter/Mastering Assistant)、FL 2025、Ableton 12.3(ローカル実行ステム分離)、BandLab(全部無料)が標準装備済み。あっても加点なし、ないと減点。
- **Tracklib 調査(2025 年 11 月)**: プロデューサーの AI 利用は約 25–32%。内訳は**ステム分離 73.9%、マスタリング/EQ 45.5%**、フル楽曲生成は**わずか 3%**。**80% 超が AI 生成楽曲に反対**。
- **Suno Studio**(WavTool 買収 → 2025 年 9 月ローンチ、$30/月): 「Generative Audio Workstation」。生成起点のブラウザ DAW。「DAW の皮を被ったジェネレーター」との辛口評価もあり、既存 DAW ユーザーの代替には遠い。公式パブリック API なし。
- **法務**: UMG×Udio 和解(2025/10、生成曲 DL 不可の囲い込み化)、Warner×Suno 和解(2025/11)、**Sony は未和解で 2026 年 7 月にサマリージャッジメント審理予定**。無許諾学習系と組むのはブランドリスク。
- **「商用セーフ」の成立例**: Stable Audio 2.5(AudioSparx 完全ライセンスデータ学習、API 提供)、Lyria RealTime(Gemini API)、Magenta RealTime 2(オープンウェイト、230M の Small は MacBook Air でも動作)。

### 4.2 評価される AI vs ギミック

- **評価される**: ステム分離、マスタリングアシスタント(出発点として)、Logic Session Players 型の「編集可能な素材を返す伴奏生成」、text-to-sample(短い素材)、音声修復。
- **敵視される**: フル楽曲ワンショット生成、プロンプトだけの「作曲」、学習元不明モデル、オーディオ解析なしのチャットボット(FL Gopher への薄い反応)。

### 4.3 現実的な AI 差別化ポジション(調査エージェントの評価)

1. **「プライバシー保証つきローカル AI」**(最有力) — WebGPU/WASM でステム分離等を完全クライアントサイド実行。「音声を一切アップロードしない AI」は未発表曲の漏洩を懸念するプロの信頼に直結。FL(クラウド必須)/Ableton(ローカルを売りに)の流れとも整合。
2. **アシスト特化・生成非依存** — 分離(74% が使う)+API マスタリング(LANDR/Music.ai が API 開放済)+「編集可能な MIDI を返す」伴奏生成。ブラウザ完結 MIDI 生成は Magenta.js 以降決定版不在の**空白地帯**。
3. **生成を載せるなら商用セーフな text-to-sample 限定** — Stable Audio 2.5 API で「生成素材は 100% ライセンス済みデータ由来」を宣言。
4. **リアルタイム・インタラクティブ生成(中期)** — Lyria RealTime / Magenta RT 2 による「ジャム相手」はどの DAW も本格搭載していない。

---

## 5. 統合的な結論

### 5.1 技術スタックの確立事項(確度高)

```
UI (TypeScript / any framework)
  │  コマンド/スナップショット
  ▼
プロジェクトモデル: サーバ権威 LWW or CRDT (Loro) + コンテンツアドレス blob
  │  lock-free SPSC ring (ringbuf.js 型, SharedArrayBuffer + Atomics)
  ▼
オーディオエンジン: WASM (Rust/C++) — 専用 Worker + AudioWorklet(シンク)
  │  プラグイン: WAM 2.0 ホスト(AudioWorklet 内 WamProcessor)
  ▼
ストレージ: OPFS SyncAccessHandle(Worker) + クラウド同期 / File System Access(Chromium)
AI: WebGPU/WASM オフライン推論(ステム分離等) — リアルタイム DSP は WASM のみ
配備: COOP/COEP(クロスオリジン分離)必須、PWA
```

- 本リポジトリの **Rust 製 dawcore(ロックフリー設計・オフラインテスト済)は wasm32 コンパイルでこのアーキテクチャにほぼそのまま適合**する(cpal → AudioWorklet ブリッジへの置換、ringbuf クレート → SAB リングへの置換が主な作業)。
- ブラウザ間格差が大きい(Safari: Web MIDI 不可、7 日消去、WebCodecs 制限)。**フル機能は Chromium、他は縮退運転**の段階的戦略が現実的。

### 5.2 差別化候補(議論用)

| # | 候補 | 根拠となる空白 | リスク |
| --- | --- | --- | --- |
| A | **ローカルファーストの「所有できる Web DAW」**: オフライン PWA、OPFS+実フォルダ保存、オープンなプロジェクト形式、サービス終了でも作品が残る設計 | Endlesss/WavTool の死、Amped 以外オフライン不在、サブスク疲れ | 同期インフラの複雑さ。クラウド囲い込みによる収益化と緊張関係 |
| B | **プライバシー保証つきローカル AI**: ブラウザ内ステム分離(WebGPU)、編集可能 MIDI を返すアシスト、商用セーフ text-to-sample | AI 差別化ポジション 1–3。「アップロードしない AI」は Web DAW では逆説的に新しい | モデルサイズ(~170MB)の配信、WebGPU 非対応環境 |
| C | **中級〜プロ向けの本格録音/編集**: ループバック較正、レイテンシ補正、テイク管理/コンピング、精密ミキシング | 市場の空白 #1・#4。商用 Web DAW で未達 | ブラウザのレイテンシ天井(+10〜15ms)。ハード寄りの検証コスト |
| D | **「音楽の Figma」= リアルタイム共同編集 + WAM プラグインエコシステム**: Loro/LWW 同期、カーソル共有、開発者マーケット | 市場の空白 #2、Soundtrap 以外に本格協調なし、WAM 実装 1 社のみ | ネットワーク効果が出るまでの立ち上げ、マーケット運営コスト |
| E | (補助軸)**日本語圏・教育(GIGA スクール/Chromebook)** | 市場の空白 #7 | 教育営業チャネルが別事業 |

これらは排他ではないが、「最初の 1 本」を決めることが要求仕様の優先度・アーキテクチャのトレードオフ(例: ローカルファースト vs リアルタイム協調はどちらを既定にするか)を規定する。

---

## 6. 未解決の調査課題

- openDAW の WASM エンジン(2026 Q2 予定)と 1.0(Q3)が予定通り出るか — 出れば商用ライセンス込みで土台候補になりうるため追跡。
- Soundtrap/BandLab の同期プロトコルの詳細(非公開)。
- Chrome `renderSizeHint`(レンダー量子可変化)のオリジントライアル進捗。
- Sony v. Suno のサマリージャッジメント(2026 年 7 月審理予定)の帰結 — AI 機能の法務前提が変わる。

---

## 主要ソース(抜粋)

- Chrome: Audio Worklet Design Pattern — developer.chrome.com/blog/audio-worklet-design-pattern/
- ringbuf.js(Paul Adenot) — github.com/padenot/ringbuf.js
- WAM 2.0 論文(Buffa et al., WWW '22)— dl.acm.org/doi/fullHtml/10.1145/3487553.3524225
- openDAW — github.com/andremichelle/openDAW / GridSound — github.com/gridsound/daw
- W3C/SMPTE Media Production Workshop(Soundtrap のレイテンシ講演)— w3.org/2021/03/media-production-workshop/
- ブラウザ実測レイテンシ — jefftk.com/p/browser-audio-latency
- OPFS 性能 — rxdb.info/rx-storage-opfs.html / renderlog.in
- CRDT ベンチ — github.com/dmonad/crdt-benchmarks / loro.dev/docs/performance
- Figma のマルチプレイヤー — figma.com/blog/how-figmas-multiplayer-technology-works/
- Endlesss 閉鎖の総括 — cdm.link/endlesss-discontinued/
- Suno×WavTool — suno.com/blog/suno-acquires-wavtool / techcrunch.com(2025-06-26)
- Tracklib プロデューサー調査(2025-11)— musicbusinessworldwide.com
- demucs-rs(ブラウザ内ステム分離)— github.com/nikhilunni/demucs-rs
- Amped Studio PWA/WAM — ampedstudio.com
- 各項の詳細な出典は本文中の記載を参照。
