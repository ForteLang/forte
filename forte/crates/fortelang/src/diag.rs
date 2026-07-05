//! Diagnostics: every error carries a source position and a music-vocabulary
//! message (SRS-LANG-008). Codes follow the SDD scheme `E-<AREA>-<NNN>`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pos {
    pub line: u32, // 1-based
    pub col: u32,  // 1-based
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Clone, Debug)]
pub struct Diag {
    pub code: &'static str,
    pub pos: Pos,
    pub message: String,
}

impl Diag {
    pub fn new(code: &'static str, pos: Pos, message: impl Into<String>) -> Self {
        Diag { code, pos, message: message.into() }
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]: {}", self.pos, self.code, self.message)
    }
}
