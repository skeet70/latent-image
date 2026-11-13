#![forbid(unsafe_code)]

//! What the capture's block readout (completion 0x61) and its four calc jobs (completions 0x99,
//! 0x97, 0x98 and 0x9a) deliver to the shot.

/// The readout fields the capture's jobs read from completion 0x61.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadoutPayload {
    pub source: u8,
    /// Meaningful when `mode` is 3.
    pub variant: u8,
    pub mode: u32,
    pub value: u32,
}

pub const FIRST_RESULT_BYTES: usize = 12;
pub const SECOND_RESULT_BYTES: usize = 48;

/// Calc 0x3c0f's result (completion 0x99).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FirstResult(pub [u8; FIRST_RESULT_BYTES]);

impl FirstResult {
    pub fn last_word(&self) -> u32 { u32::from_le_bytes([self.0[8], self.0[9], self.0[10], self.0[11]]) }
}

/// Calc 0x3c10's result (completion 0x97).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SecondResult(pub [u8; SECOND_RESULT_BYTES]);

impl Default for SecondResult {
    fn default() -> Self { Self([0; SECOND_RESULT_BYTES]) }
}

/// Calc 0x3c0e's result (completion 0x98).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ThirdResult(pub u32);

/// Calc 0x3c11's result (completion 0x9a).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FourthResult {
    pub first: u8,
    pub second: u16,
    pub third: u16,
}
