# Web DAW Market and Technology Research Report

Research date: 2026-07-02
Method: Multi-agent web research (search → primary-source retrieval → adversarial verification per claim (three-vote system) → synthesis).
Verified claims: 23/25 (2 refuted and excluded). Confidence annotations in the body are based on verification results.

This document summarizes (1) competitive analysis, (2) available open-source technologies,
(3) the maturity of web platform technologies, and (4) market gaps and differentiation
opportunities for developing a new DAW that runs as a web application. It serves as input
to the subsequent IEC 62304-style documents
(system requirements → software requirements → architecture design → detailed design).

---

## 1. Competitive Landscape (Commercial Web DAWs)

### 1.1 List of Players

| Product | Position | Pricing | Technology | Strengths | Weaknesses |
| --- | --- | --- | --- | --- | --- |
| **BandLab** | Free, social, all-in-one. Over 100M registered users (end of 2024) | DAW completely free. Monetized via Membership at $14.95/mo etc. (ARR ~$48M) | Web Audio + cloud sync + native mobile | Overwhelming feature volume for free (unlimited tracks, 24,000+ loops, AI mastering, Splitter, SongStarter) | Lacks pro-grade precision editing (comping, deep automation). Serious production assumes migration to desktop |
| **Soundtrap** (Spotify → sold back to founders in 2023) | Beginners, education, podcasts. "Collaborative cloud studio" | Free 5 tracks, $9.99–17.99/mo. Education 50 seats from $249/yr | Web Audio based | Polished real-time co-editing, education administration, Auto-Tune, latency calibration feature | Free tier is effectively a demo. Even paid tiers trail free BandLab on features. Spotify divested it (retreat from creator-tools strategy) |
| **Soundation** | Loop-based production for beginners to intermediates | Free (3 projects/1GB) to Pro $29.99/mo | Flash → NaCl → **world's first WASM Threads implementation** (up to 6 threads for ~300% performance gain) | Snappy WASM engine, inexpensive | Paywalled upper-tier features, no offline, no third-party plugins |
| **Amped Studio** | Beat makers to the "more serious" end. Chromebook appeal | Free to $12.99/mo | Web Audio + WAM | **WAM support (OBXD/DEXED bundled + shop), VST3 support (paid), the only commercial-grade offline PWA** | Weak UI friendliness and community |
| **Audiotool** | Modular patching style. Electronic-music hobbyists. 300K+ MAU | **Completely free** (unusual) | Flash (2008) → HTML5. Multicore design: DSP in Workers → SAB ring buffer → AudioWorklet | Unique modular experience, strong community, developer API "Nexus" in new beta | Learning curve, performance on large projects, weak recording/editing, unclear revenue base |
| **Ableton** (Learning Music/Synths, Note, Move) | Browser presence deliberately limited to education and funnel. Intentionally not building a full web DAW | Learning materials free, Note $5.99 one-time | Learning Synths is a pioneering case of **Max/MSP RNBO → Web compilation** | Brand, Note→Live pipeline via Cloud | Leaves the full web DAW space vacant (= room for entry) |
| **Splice** | Core business is sample subscription. **Withdrew** from cloud DAW (Splice Studio) | $12.99–39.99/mo | — | Sample supply + mobile ideation (CoSo) | A real-world case of judging "sample supply is more economically rational than a full web DAW" |
| **Sesh.fm** | "Multiplayer DAW" for beat makers | Free + Pro one-time-purchase oriented | — | Real-time cursor sharing, version control, free AI stem separation | New entrant with a small ecosystem |

### 1.2 Lessons from Dead Products (Important)

- **AudioSauna**: Vanished together with the end of Flash. Dependence on a platform foundation directly caused the product's death.
- **Endlesss** (shut down May 2024): When the servers stopped, the app itself became inoperable. Users were told to download their own creations by a deadline. CDM's post-mortem: "with SaaS music tools, when the service ends, both the tool and the works disappear." → **Cloud-only architecture is an availability risk for users' works**.
- **WavTool** (halted November 2024 → acquired by Suno in June 2025): "The world's first text-driven DAW powered by GPT-4." Could not survive as a standalone business; users were left without access to the tool for roughly 7 months. The team became the nucleus of Suno Studio (launched September 2025, $30/mo).

### 1.3 Results of Cross-Verification

1. **Third-party plugins**: Despite the existence of the WAM 2.0 standard, Amped Studio is virtually the only commercial implementation. BandLab/Soundtrap/Soundation have no plugin ecosystem.
2. **Offline PWA**: Amped Studio is virtually the only commercial implementation. Others assume an always-on connection.
3. **Pro-grade recording**: No commercial web DAW has achieved it. No commercial web DAW with take management and comping UI could be confirmed. Causes: browser round-trip latency (best ~14–30ms) and the lack of an accurate latency-reporting API.
4. **Japanese market**: BandLab is overwhelmingly recommended as "free entry-level DTM." The prevailing narrative is that browser DAWs are "for beginners and sketching," with serious production assumed to migrate to Cubase/Studio One/Logic. Japanese UI and learning materials are largely a blank space.

### 1.4 Market Gaps (Unmet Needs Inferred from the Evidence)

1. **Absence of an intermediate-to-pro web DAW** — All players converge on beginners/education/sketching.
2. **Plugin ecosystem** — WAM 2.0 implementations exist only at Amped and in academia. A web plugin marketplace where developers can earn revenue remains unexplored.
3. **Offline/local-first** — The deaths of Endlesss/WavTool proved the value of a "web DAW you can own."
4. **Low-latency recording technology** — No player has directed WASM + AudioWorklet + multithreading toward recording quality (calibration, compensation, comping).
5. **Sustainable business model** — Completely free (Audiotool) has questionable sustainability; subscription fatigue is also evident.
6. **Post-generative-AI editing demand** — The seat for a web DAW that neutrally bridges generative AI and serious editing is empty.
7. **Japanese-language localization** — The education market, a good fit with GIGA School (Chromebooks), is untouched.

---

## 2. Open-Source Building Blocks

### 2.1 Web DAWs Proper (Verified, High Confidence)

| Project | Overview | License | Suitability Assessment |
| --- | --- | --- | --- |
| **openDAW** (Audiotool founder André Michelle) | Next-generation web DAW in TypeScript. Education and privacy focused | **Dual AGPL v3 + commercial** | Prototype stage. Audio engine not yet WASM-ified (TS runs on the AudioWorklet); WASM engine planned for 2026 Q2, 1.0 for Q3. Too early to adopt as a foundation; **design reference + tracking target** |
| **GridSound** | Browser DAW built on Web Audio. Actively maintained (v1.58.5 in June 2026) | **AGPL-3.0**, and "half open-source" (backend not public) | Reusing code triggers AGPL obligations. Value only as a reference implementation to read |
| **Signal** | Web MIDI editor | — | Reference for piano-roll UI |
| **WAM-studio** | Reference DAW for WAM 2.0 (academic) | OSS | Reference for a WAM host implementation |

**Caution**: The major OSS web DAWs that are reuse candidates are uniformly AGPL-family. Incorporating them into a proprietary product requires either a commercial license (openDAW) or source disclosure. **Building the engine in-house is the realistic path**.

### 2.2 Plugin Standard: Web Audio Modules 2.0 (Verified, High Confidence)

- The Web Audio API has **no** high-level plugin abstraction equivalent to VST/AU/AAX/LV2 (this is the root cause of "there is no VST on the web").
- **WAM 2.0** (started 2015, v2.0 in 2021) is the "web VST" standard that fills this gap. Supports DSP + UI components, parameter automation, MIDI, and state save/load.
- Host integration: fetch metadata JSON → dynamic import of an ES Module → connect as a standard AudioNode (DSP is a WamProcessor on the AudioWorklet).
- Ecosystem: many plugins/hosts, 2 DAWs (WAM-studio, commercial Amped Studio), Sequencer.party, etc. However, the scale is a niche of a few dozen OSS plugins, and it is not a W3C standard.
- **C/C++/Faust/Csound can be compiled to WASM and packaged as WAMs**. The Faust online IDE has WAM 2.0 export (wam2-ts / wam2-poly-ts, with polyphonic MIDI support).

### 2.3 Libraries and DSP Assets

| Asset | Purpose | Status |
| --- | --- | --- |
| **Tone.js** | Transport (sample-accurate scheduling), synths, Sampler, effects | Active (v15.5.26 on 2026-07-01, 14.7k stars). However, its abstractions are in places too high-level for a serious DAW engine; suited to the UI layer/prototyping |
| **ringbuf.js** (Paul Adenot) | Wait-free SPSC ring buffer over SAB | Implementation by a co-editor of the W3C Web Audio spec. A core engine component |
| **Faust** | DSP language → WASM/WAM | Mature. Effective for mass-producing effects/synths |
| **Csound-WASM** | Same as above | Mature |
| **RNBO** (Cycling '74) | Max patches → Web | Commercial but proven (Ableton Learning Synths) |
| Rust crates (fundsp, oxisynth, dasp, etc.) | Rust DSP → WASM | Compatible with the path of compiling this repository's existing dawcore (Rust engine) for the wasm32 target |
| **Magenta.js** | In-browser MIDI generation (MusicVAE, etc.) | Effectively in maintenance mode, but one of the few MIDI-generation assets that runs in the browser |
| **demucs-rs / demucs-web / demucs-onnx** | In-browser stem separation (WASM/WebGPU, fully client-side) | Practical as of 2026. Model ~172MB |
| **Transformers.js v3 / onnxruntime-web** | WebGPU inference (musicgen-small, etc.) | Generating short samples is realistic |

---

## 3. Maturity of Web Platform Technologies

### 3.1 Audio Engine (Verified, High Confidence)

The Chrome team's official design patterns are well established:

- **Real-time budget**: The render quantum is **fixed at 128 frames**, **about 3ms** per callback at 44.1kHz. Exceeding it produces audible glitches. (Chrome's `renderSizeHint` is at the origin-trial stage.)
- **AudioWorklet + WASM**: Brings in C/C++/Rust assets + eliminates JS JIT/GC overhead.
- **Standard shape for heavyweight engines**: AudioWorklet + SharedArrayBuffer + Atomics + dedicated Worker. MessagePort is unsuited to real-time audio due to allocation and latency. The AudioWorklet acts as an "audio sink" while DSP executes on the Worker side (Audiotool runs this in production).
- **Precondition**: SAB requires cross-origin isolation via COOP/COEP (constraints on deployment, embedding, and loading third-party assets).

### 3.2 Measured Latency Figures

| Environment | Round-trip latency |
| --- | --- |
| Chrome default | ~67ms |
| Firefox default | ~55ms |
| Chrome tuned (latencyHint:0, EC/NS/AGC off) | **~19ms** |
| Firefox, same tuning | **~14ms** |
| Native ASIO/CoreAudio | < 10ms (single-digit ms) |

- A tuned browser carries a **+10–15ms handicap versus native**. Playing soft synths is within reach; through-monitoring plus effects is difficult.
- `outputLatency` / `MediaTrackSettings.latency` are known to return unreliable values → **loopback calibration is the practical solution** (Soundtrap is virtually the only example offering a calibration feature).
- Mandatory constraints when recording: `echoCancellation/noiseSuppression/autoGainControl: false` (if unspecified, call-oriented processing destroys music recordings). Chrome/Safari have a history of bugs where the constraints do not take effect. iOS Safari requires explicitly specifying 44.1kHz.
- For high-quality recording, **direct PCM capture in an AudioWorklet** is recommended over MediaRecorder.

### 3.3 Storage

- **OPFS SyncAccessHandle (Worker-only)**: ~90ms to write 100MB (≈1.1GB/s), about 9x faster than IndexedDB. Ideal for sequential writes of recording streams. Supported in all major browsers.
- **Quota**: Chrome is 60% of the disk. Firefox is 10GiB (expandable with persist). **Safari deletes everything after 7 days of non-use (ITP)** → cloud sync is mandatory on Safari.
- **File System Access API (reading/writing real folders) is Chromium-only**. Firefox/Safari need fallbacks.

### 3.4 Other I/O

- **Web MIDI**: Chrome/Edge/Firefox yes. **Safari unsupported in all versions** (impossible on iOS since all browsers are forced onto WebKit). USB MIDI measured at ~1ms; BLE MIDI adds +10–30ms jitter.
- **WebCodecs AudioEncoder**: The primary export path is Opus with a WAV fallback. AAC is Safari/Chrome-only (not on Linux); Firefox cannot encode AAC.
- **WebGPU**: Real-time (per 128 samples) GPU DSP is **unrealistic** due to dispatch latency. Real-time DSP is WASM (+SIMD) or nothing. **Offline processing (stem separation, analysis, mastering) is within practical range on WebGPU**.

### 3.5 Collaborative Editing and Local-First

- **The big three CRDTs**: Yjs (largest ecosystem, pure JS) / Automerge 3.0 (full-history DAG, 10–100x memory improvement) / **Loro 1.x (benchmark leader, MovableList/Movable Tree built in — ideal for clip drag-and-drop and track hierarchies)**.
- **The Figma approach** (server-authoritative + per-property LWW, not a CRDT) fits the DAW model of "clip = object, parameter = property" extremely well.
- "Don't put waveform data in the CRDT" is the industry-wide conclusion → separate it via content-addressed (SHA-256) references, syncing blobs with OPFS + object storage.
- The DAW-specific requirement "undo applies only to your own operations" is supported out of the box by the Yjs/Loro UndoManager.
- Sync infrastructure: PartyKit (acquired by Cloudflare, Durable Objects), Liveblocks, Jazz (built-in blob sync via FileStream), PowerSync/ElectricSQL.

---

## 4. AI Music Production Trends (2024–2026)

### 4.1 Market Structure

- **Stem separation and AI mastering are "table stakes."** Logic 11 (Stem Splitter/Mastering Assistant), FL 2025, Ableton 12.3 (locally executed stem separation), and BandLab (all free) already ship them as standard. Having them earns no points; lacking them loses points.
- **Tracklib survey (November 2025)**: Producer AI usage is about 25–32%. Breakdown: **stem separation 73.9%, mastering/EQ 45.5%**, full-song generation a **mere 3%**. **Over 80% oppose AI-generated songs**.
- **Suno Studio** (WavTool acquisition → launched September 2025, $30/mo): a "Generative Audio Workstation." A generation-first browser DAW. Also drew the harsh review "a generator wearing a DAW's skin," and is far from a replacement for existing DAW users. No official public API.
- **Legal**: UMG×Udio settlement (2025/10, walled-garden model with no downloads of generated songs), Warner×Suno settlement (2025/11), **Sony unsettled with a summary judgment hearing scheduled for July 2026**. Partnering with unlicensed-training players is a brand risk.
- **Established "commercially safe" examples**: Stable Audio 2.5 (trained on fully licensed AudioSparx data, API available), Lyria RealTime (Gemini API), Magenta RealTime 2 (open weights; the 230M Small runs even on a MacBook Air).

### 4.2 AI That Is Valued vs. Gimmicks

- **Valued**: stem separation, mastering assistants (as a starting point), Logic Session Players-style "accompaniment generation that returns editable material," text-to-sample (short material), audio restoration.
- **Resented**: one-shot full-song generation, "composing" by prompt alone, models with unknown training sources, chatbots without audio analysis (the tepid reception of FL Gopher).

### 4.3 Realistic AI Differentiation Positions (Research Agents' Assessment)

1. **"Local AI with a privacy guarantee"** (strongest) — Run stem separation and similar fully client-side via WebGPU/WASM. "AI that never uploads your audio" speaks directly to the trust of professionals worried about leaks of unreleased songs. Also consistent with the current dynamics of FL (cloud-required) vs. Ableton (selling local execution).
2. **Assist-focused, generation-independent** — Separation (used by 74%) + API mastering (LANDR/Music.ai have opened APIs) + accompaniment generation that "returns editable MIDI." Browser-complete MIDI generation has had no definitive successor since Magenta.js — a **blank space**.
3. **If adding generation, restrict to commercially safe text-to-sample** — Use the Stable Audio 2.5 API and declare "generated material derives 100% from licensed data."
4. **Real-time interactive generation (mid-term)** — A "jam partner" via Lyria RealTime / Magenta RT 2 is not seriously shipped by any DAW.

---

## 5. Integrated Conclusions

### 5.1 Established Elements of the Technology Stack (High Confidence)

```
UI (TypeScript / any framework)
  │  commands/snapshots
  ▼
Project model: server-authoritative LWW or CRDT (Loro) + content-addressed blobs
  │  lock-free SPSC ring (ringbuf.js style, SharedArrayBuffer + Atomics)
  ▼
Audio engine: WASM (Rust/C++) — dedicated Worker + AudioWorklet (sink)
  │  Plugins: WAM 2.0 host (WamProcessor inside the AudioWorklet)
  ▼
Storage: OPFS SyncAccessHandle (Worker) + cloud sync / File System Access (Chromium)
AI: WebGPU/WASM offline inference (stem separation, etc.) — real-time DSP is WASM only
Deployment: COOP/COEP (cross-origin isolation) required, PWA
```

- This repository's **Rust dawcore (lock-free design, offline-tested) fits this architecture almost as-is when compiled for wasm32** (the main work is replacing cpal → an AudioWorklet bridge and the ringbuf crate → an SAB ring).
- Cross-browser disparities are large (Safari: no Web MIDI, 7-day deletion, WebCodecs limits). A staged strategy of **full features on Chromium, degraded operation elsewhere** is realistic.

### 5.2 Differentiation Candidates (For Discussion)

| # | Candidate | Underlying gap | Risk |
| --- | --- | --- | --- |
| A | **A local-first "web DAW you can own"**: offline PWA, OPFS + real-folder saving, open project format, works survive service shutdown by design | The deaths of Endlesss/WavTool, no offline besides Amped, subscription fatigue | Complexity of sync infrastructure. Tension with monetization via cloud lock-in |
| B | **Local AI with a privacy guarantee**: in-browser stem separation (WebGPU), assistance that returns editable MIDI, commercially safe text-to-sample | AI differentiation positions 1–3. "AI that doesn't upload" is paradoxically novel for a web DAW | Distribution of model size (~170MB), environments without WebGPU |
| C | **Serious recording/editing for intermediates to pros**: loopback calibration, latency compensation, take management/comping, precision mixing | Market gaps #1 and #4. Unachieved by commercial web DAWs | Browser latency ceiling (+10–15ms). Hardware-adjacent verification costs |
| D | **"Figma for music" = real-time co-editing + WAM plugin ecosystem**: Loro/LWW sync, cursor sharing, developer marketplace | Market gap #2, no serious collaboration besides Soundtrap, only one company implements WAM | Ramp-up until network effects kick in, marketplace operating costs |
| E | (Auxiliary axis) **Japanese-language and education (GIGA School/Chromebook)** | Market gap #7 | Education sales channel is a separate business |

These are not mutually exclusive, but deciding on "the first one" determines requirement priorities and architectural trade-offs (e.g., which of local-first vs. real-time collaboration is the default).

---

## 6. Open Research Questions

- Whether openDAW's WASM engine (planned 2026 Q2) and 1.0 (Q3) ship on schedule — if they do, it could become a foundation candidate including a commercial license, so keep tracking.
- Details of Soundtrap's/BandLab's sync protocols (not public).
- Progress of the Chrome `renderSizeHint` (variable render quantum) origin trial.
- The outcome of the Sony v. Suno summary judgment (hearing scheduled July 2026) — it would change the legal premises for AI features.

---

## Key Sources (Excerpt)

- Chrome: Audio Worklet Design Pattern — developer.chrome.com/blog/audio-worklet-design-pattern/
- ringbuf.js (Paul Adenot) — github.com/padenot/ringbuf.js
- WAM 2.0 paper (Buffa et al., WWW '22) — dl.acm.org/doi/fullHtml/10.1145/3487553.3524225
- openDAW — github.com/andremichelle/openDAW / GridSound — github.com/gridsound/daw
- W3C/SMPTE Media Production Workshop (Soundtrap's latency talk) — w3.org/2021/03/media-production-workshop/
- Measured browser latency — jefftk.com/p/browser-audio-latency
- OPFS performance — rxdb.info/rx-storage-opfs.html / renderlog.in
- CRDT benchmarks — github.com/dmonad/crdt-benchmarks / loro.dev/docs/performance
- Figma's multiplayer — figma.com/blog/how-figmas-multiplayer-technology-works/
- Endlesss shutdown post-mortem — cdm.link/endlesss-discontinued/
- Suno×WavTool — suno.com/blog/suno-acquires-wavtool / techcrunch.com (2025-06-26)
- Tracklib producer survey (2025-11) — musicbusinessworldwide.com
- demucs-rs (in-browser stem separation) — github.com/nikhilunni/demucs-rs
- Amped Studio PWA/WAM — ampedstudio.com
- See citations in the body of each section for detailed sources.
