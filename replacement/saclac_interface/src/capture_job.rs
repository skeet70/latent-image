#![forbid(unsafe_code)]

//! The capture's primary job. Imgdc 0x3f49 captures the frame and reports its start (0x62), the
//! end of the exposure (0x5e), each output (0x5f, the last 0x60), the readout (0x61) and the
//! areas it delivers (0x64); statistics 0x4005 measures the frame (0x7d); imgdc 0x3f4e shows it.

/// The capture start (0x62).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureStart {
    /// The profile the delivered areas are laid out in.
    pub profile: u32,
    /// What the default calibration takes besides the record.
    pub curve_pair: [u16; 2],
    pub curve_words: [u32; 2],
}

impl CaptureStart {
    /// Passed on as the statistics' flag.
    pub fn flagged(&self) -> bool { self.curve_words[1] != 0 }
}

/// The end of the exposure (0x5e): the readout pair and the word the client keeps after it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureEnd {
    pub readout: [u32; 2],
    pub detail: u32,
}

/// One output of the capture (0x5f, or 0x60 for the last).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureOutput(pub [u8; CaptureOutput::BYTES]);

impl Default for CaptureOutput {
    fn default() -> Self { Self([0; Self::BYTES]) }
}

impl CaptureOutput {
    pub const BYTES: usize = 88;

    fn half(&self, offset: usize) -> u16 { u16::from_le_bytes([self.0[offset], self.0[offset + 1]]) }

    fn word(&self, offset: usize) -> u32 { u32::from_le_bytes([self.0[offset], self.0[offset + 1], self.0[offset + 2], self.0[offset + 3]]) }

    /// What the source descriptor keeps.
    pub fn source(&self) -> (u16, u32) { (self.half(0x0c), self.word(0x10)) }
    pub fn second_limit(&self) -> i16 { self.half(0x14) as i16 }
    pub fn trim(&self) -> i16 { self.half(0x16) as i16 }
    pub fn slot_halves(&self) -> [i16; 2] { [self.half(0x18) as i16, self.half(0x1a) as i16] }
    /// The profile whose output count `delivered` is compared with.
    pub fn profile(&self) -> u32 { self.word(0x1c) }
    pub fn reference(&self) -> u32 { self.word(0x20) }
    pub fn distance(&self) -> u32 { self.word(0x24) }
    pub fn scale_index(&self) -> i16 { self.half(0x2a) as i16 }
    /// The two words the frame rate is scaled by, in the order the scaling takes them.
    pub fn scale_words(&self) -> [u32; 2] { [self.word(0x30), self.word(0x2c)] }
    pub fn slot_words(&self) -> [u32; 2] { [self.word(0x34), self.word(0x38)] }
    /// How many outputs the capture makes; the last arrives as 0x60.
    pub fn outputs(&self) -> u8 { self.0[0x3c] }
    /// How many outputs the capture has made with this one.
    pub fn delivered(&self) -> u8 { self.0[0x3e] }
    /// What the client's selection keeps of the output.
    pub fn selection(&self) -> OutputSelection {
        OutputSelection {
            marker: self.half(0x28),
            word: self.word(0x40),
            pair: self.half(0x44),
            byte: self.0[0x46],
            words: [self.word(0x48), self.word(0x4c), self.word(0x50)],
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OutputSelection {
    pub marker: u16,
    pub word: u32,
    /// Kept twice.
    pub pair: u16,
    pub byte: u8,
    pub words: [u32; 3],
}

/// The areas imgdc delivers (0x64).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaDelivery {
    /// Without areas the job clears the client's area lists.
    pub found: bool,
}

/// One displayed window of the capture; the request carries the half height twice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureWindow {
    pub left: u16,
    pub width: u16,
    pub half_height: u16,
}

/// The lens point the capture moves to after the frame and the slot values that go with it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TransferPoint {
    pub reference: u32,
    pub second_limit: i16,
    pub trim: i16,
    pub offset: i16,
    pub residual: i16,
    pub step: i16,
}

/// The secondary job's evaluation the capture carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SecondaryValues {
    pub measured: u32,
    pub primary: u32,
    pub primary_limit: u32,
}

/// Imgdc 0x3f49: one capture of the bound record. Field names after `at_` are the record fields
/// of the profile-record layer they come from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureRequest {
    /// Words of the command's source descriptor.
    pub source: u32,
    pub source_words: [u32; 3],
    pub source_option: u32,
    pub source_byte: u8,
    pub windows: [CaptureWindow; 2],
    pub transfer: TransferPoint,
    pub frame_words: [u32; 2],
    pub layout: [u16; 2],
    pub subject_source: u32,
    pub resource_kind: u32,
    pub subjects_locked: u32,
    pub slot_reference: u32,
    pub profile_selector: u32,
    pub secondary: SecondaryValues,
    pub options: u32,
    pub mode: u8,
    pub profile_class: u16,
    pub slot_offset: i16,
    pub slot_second_limit: i16,
    pub slot_trim: i16,
    pub slot_residual: i16,
    pub slot_step: i16,
    pub at_0184: i16,
    pub at_0186: i16,
    pub at_017a: i16,
    pub secondary_mode: u8,
    pub secondary_latch: u8,
    pub secondary_blocked: u8,
    pub secondary_enabled: bool,
    /// The capture controller's hold and pending secondary job when the job was prepared.
    pub held: bool,
    pub secondary_pending: bool,
    pub at_0036: u8,
    pub at_0039: u8,
    pub at_003e: u8,
    pub at_0042: u8,
    pub at_004d: u8,
    pub at_0053: u8,
    pub at_006c: u8,
    pub at_0188: u8,
    pub at_0198: u8,
    pub at_0208: u8,
    pub paired: bool,
    /// The selection's requested slot and its byte at +4.
    pub requested_slot: u8,
    pub selection_byte: u8,
    /// The record's second reference limit is unset.
    pub second_limit_unset: bool,
    pub slot_positions: u8,
    pub subject_area: u8,
}

/// Imgdc 0x3f4e: the display of the measured frame.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayRequest {
    pub setting_word: u32,
    pub setting_pair: [u32; 2],
    pub extent: [u32; 2],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsFrame {
    pub record_word: u32,
    pub frame_words: [u32; 2],
    pub layout: [u16; 2],
    pub flagged: bool,
    pub at_0198: u8,
    pub selector: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsProfile {
    /// Record fields 0x35, 0x36, 0x38, slot 0x208, slot 0x188, 0x39, 0x3a and 0x3b.
    pub record_words: [u32; 8],
    pub calibration_word: u32,
    pub calibration: [u16; 3],
    pub trim: i16,
    pub second_limit: i16,
    pub option_word: u32,
    pub option: u8,
    pub at_003e: u8,
    pub record_byte: u8,
}

/// What a capture's statistics 0x4005 request measures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureStatistics {
    pub frame: StatisticsFrame,
    /// Left, top, height and width.
    pub window: [u32; 4],
    pub lens: [u32; 3],
    pub profile: StatisticsProfile,
}
