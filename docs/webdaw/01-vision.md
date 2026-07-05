# Vision — White-Boxing Music Production and the Fork-Lineage Ecosystem

Working name (provisional): **Forte** — a "compose in code" platform consisting of a language, an engine, and a hub.
(All names are provisional. Language = Forte lang, hub = Forte Hub, engine = forte-core.)

Status: Draft / 2026-07-02 / This document defines the product vision that sits above the requirements specifications (02 onward).

---

## 1. In One Sentence

**Move music production from the world of black-box project files and one-off audio-sample sales
to a world of open development through "code, fork, build, release."**

Do for composition what GitHub did for software development.

## 2. Problems with the Status Quo (Why We Do This)

1. **Black-boxed projects** — DAW project files are opaque binaries, and how a song was made
   is never shared. Learning has degenerated into "transcribing by ear and borderline-illegal sampling."
2. **The poverty of the sample economy** — Splice and the like are structured around "buying and
   selling a single snare," which does not match the cost structure of an era where songs are
   commoditized by subscriptions. Selling audio files piecemeal leaves no **lineage** of reuse.
3. **Invisibility of contribution** — Much of J-POP reuses particular progressions (e.g.,
   "Marunouchi Sadistic") and techniques, yet the person who created the origin receives nothing
   and gets no spotlight. There is no market where individual developers who created core tools,
   progressions, and designs can contribute to major players.
4. **The human role after AI** — In an era when AI generates audio, the value of "the work of
   producing audio" has fallen. The next job for humans is, as Mozart did when writing scores,
   **to craft and share the structure of a piece as a white box**.

## 3. Three Inventions

### Invention 1: White-Boxing Music (Music as Code)

- Songs, projects, tracks, instruments (synths), effects, MIDI sequencers, progressions,
  utility tools — **everything is expressed as code in a programming language (Forte lang)**.
- Code is a **module**, reused via `import`. The concept of a plugin is generalized from
  "a special binary" to "a module you import."
- Abstraction levels above plugins (a song's skeleton, arrangement templates, mix designs)
  are also modularized and importable. Bring software engineering's modularization and
  dependency management into composition.

### Invention 2: Enforced Fork Lineage (Provenance by Construction)

- Both songs and modules are managed as **repositories** (private / public).
- **Public repositories cannot be cloned. Use always goes through a fork.**
  By walking the fork lineage, whoever created the more fundamental module becomes visible,
  creating a structure in which they benefit.
- Playing through an instrument module leaves that instrument's fork information behind.
  A microphone-recorded track carries its recording provenance.
- In the future, a **point system** will be introduced: you can use as much as others have used
  your work. Audio files themselves are **not** bought or sold. Value flows as contribution to
  the lineage.

### Invention 3: Songs as Deterministic Builds (Song as Reproducible Build)

- **Bounce = build.** From the same commit plus the same lock file, bit-identical audio is reproduced.
- Just like GitHub's release feature, a song built from a tag is **released** and can be listened to
  on players within the ecosystem.
- The released audio itself cannot be reused. If you want the content, **fork the repository**.
  (Re-recording released audio with a microphone to make it your own asset is a terms-of-service violation.)

## 4. A New Composing Experience

> Make music in an editor like VSCode. Build it, and (without any GUI operation) you can hear the song.
> Improve the song in code while listening to it.

- The single source of truth for editing is **code**. There is no editing in a GUI.
- However, **read-only visualizations** (piano roll, waveform, and mixer views generated from
  the code) are provided. They correspond to a debugger/profiler in coding.
- Live preview: every save/evaluation triggers an incremental build, and the sound updates while
  preserving the playback position (hot reload).
- The only inputs are **MIDI input** (recording performances as code-level note sequences) and
  **microphone input** (recorded assets with provenance).
  Importing external audio files does not exist in the specification.
  This is a deliberate restriction to structurally exclude bringing in black-box audio.

## 5. Rules of the Ecosystem (Differences from GitHub)

| Item | GitHub | Forte Hub |
| --- | --- | --- |
| Repositories | private/public | Same |
| Reuse | Free to clone | **Public requires fork; clone not allowed** |
| Dependencies | Package registries (npm, etc.) are separate systems | Registry and fork lineage are **one** (dependency = lineage) |
| Releases | Arbitrary binaries | **Deterministically built audio + provenance manifest** |
| License | Free choice | Public defaults to a **lineage-preserving license** (fork required, attribution, per-track provenance) |
| Economics | None (sponsors, etc.) | **Point system** (use as much as you are used) introduced in stages |

- Assets of live vocals and live performances circulate Splice-style, as "publishing your own
  performance for others to use." However, they flow **not as file sales but as fork + provenance + points**.
- As release forms, we are considering **full mix** and **open-stems** (publishing the stems and
  inviting forks that add vocals or live instruments) (§7).

## 6. The Listener's Experience (A New Way of Digging)

- "The version of this song sung by Sakurai," "the Kuwata version," "the rock version where
  Matsumoto added guitar," "the list of songs forked with this song as a reference" — all
  **traversable as a lineage graph**.
- Even without an explicit fork, similarity of progressions and structures becomes analyzable at
  the language level (because it is code, similarity search is meaningful). You can reach
  "this artist composes similarly to this person," "they use the same instrument (instrument module),"
  "people who play solos on this instrument."
- As with revivals 30 years later (Tatsuro Yamashita → city pop), the spotlight lands on
  **effects, progressions, and ways of making** themselves, and the original creators are discovered.
- Long-term vision: the monolith of songwriting rights is dismantled; per-track contributors are
  tracked through forks; developers of utility tools and overall designs are also credited; and the
  contribution shares of listened-to songs are distributed. Bands shift from fixed lineups to a
  fluid form where "a composing user combines other people's performances."

## 7. Unresolved Design Questions (Explicitly Flagged for Study)

1. **Scope of incorporating live vocals/performances** — How far to integrate vocal recording into
   the code world. Current proposal: on open-stems releases, treat the "performance fork"
   (a derivation consisting only of fork + adding recorded tracks) as a first-class citizen, and
   allow a minimal GUI only for recording (transport + take management).
2. **Detecting audio brought in from outside the ecosystem** — Perfect detection is impossible.
   The policy is not "prevention" but a structure where "proof of provenance carries value," plus
   after-the-fact moderation (fingerprint matching).
3. **Design of the point economy** — Do not introduce it initially; start by recording lineage only
   (the economy can be layered on later precisely because the lineage data exists).
4. **Connection to existing rights** — Implement the ideal as a "lineage-preserving license" on top
   of current copyright law. Legal review is required.

## 8. Our (the Operator's) Job

- Develop the **core library** (natively implemented audio engine + DSP, exposed as an API)
- Develop the **language and toolchain** (compiler, package manager, LSP)
- Operate the **ecosystem** (Hub: registry, fork lineage, releases, player, points)
- Give composers "a simpler programming language" and hide the core beneath it.
  (The founder is an ML engineer. Build the core in a native language, expose it as an API, and
  drive it via the DSL on top.)

## 9. Differentiation from Prior Art

"Music in code" exists — Sonic Pi / TidalCycles / Strudel / Glicol / Faust / SuperCollider /
LilyPond, etc. — but all of them are tools for **live coding (performance) or notation**.
Nothing exists that integrates the following:

1. A package manager and **fork lineage** (a registry where dependency = lineage)
2. A **deterministic build → release** pipeline
3. A DAW-quality audio engine (identical engine for offline and real-time)
4. A listening platform (the experience of digging through lineage)

The battleground is not the language itself but the **ecosystem centered on the language**.
Note that as of 2026 the web DAW market has converged on beginners (00-research-report.md),
so the "intermediate-to-advanced users composing in code" segment is a new market that does not
compete with any existing player.
