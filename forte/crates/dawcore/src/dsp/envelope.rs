//! Linear-attack / exponential-decay ADSR envelope generator.

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    sample_rate: f32,
    attack: f32,  // seconds
    decay: f32,
    sustain: f32, // 0..1
    release: f32,
}

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            stage: Stage::Idle,
            level: 0.0,
            sample_rate,
            attack: 0.005,
            decay: 0.2,
            sustain: 0.7,
            release: 0.3,
        }
    }

    pub fn set(&mut self, attack: f32, decay: f32, sustain: f32, release: f32) {
        self.attack = attack.max(0.0005);
        self.decay = decay.max(0.0005);
        self.sustain = sustain.clamp(0.0, 1.0);
        self.release = release.max(0.0005);
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
    }

    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Hard cut: force a ~6 ms release regardless of the configured time.
    /// The sampler's choke — fast enough to BE the cut, slow enough not to
    /// click even on loud low-frequency material. The rest it leaves behind
    /// is the groove.
    pub fn cut(&mut self) {
        if self.stage != Stage::Idle {
            self.release = 0.006;
            self.stage = Stage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// One-pole time constant: reach ~63% of target per `time` seconds.
    #[inline]
    fn rate(&self, time: f32) -> f32 {
        1.0 - crate::dmath::exp(-1.0 / (time * self.sample_rate))
    }

    #[inline]
    #[allow(clippy::should_implement_trait)] // audio-rate tick, not an Iterator
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                // linear attack is snappier and avoids the slow exp tail
                self.level += 1.0 / (self.attack * self.sample_rate);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let r = self.rate(self.decay);
                self.level += (self.sustain - self.level) * r;
                if (self.level - self.sustain).abs() < 1e-4 {
                    self.level = self.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                self.level = self.sustain;
            }
            Stage::Release => {
                let r = self.rate(self.release);
                self.level += (0.0 - self.level) * r;
                if self.level < 1e-4 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}
