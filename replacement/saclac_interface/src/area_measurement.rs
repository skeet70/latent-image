#![forbid(unsafe_code)]

//! The capture's secondary area measurement: imgdc 0x3f46 measures the record's areas
//! (completions 0x5c, then 0x5b), calc 0x3c0d evaluates the measurement (0x94, then 0x95) and
//! imgdc 0x3f47 transfers the evaluation (0x5d).

/// An area of one of the record's two area lists, as the record layer stores it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaEntry(pub [u8; AreaEntry::BYTES]);

impl Default for AreaEntry {
    fn default() -> Self { Self::EMPTY }
}

impl AreaEntry {
    pub const BYTES: usize = 104;
    pub const EMPTY: Self = Self([0; Self::BYTES]);
    const RECTANGLE: usize = 0;
    const CENTRE: usize = 0x10;
    const SCALE: usize = 0x22;

    /// Left, top, height and width.
    pub fn rectangle(&self) -> [u32; 4] { core::array::from_fn(|index| self.word(Self::RECTANGLE + 4 * index)) }

    pub fn set_rectangle(&mut self, rectangle: [u32; 4]) {
        for (index, value) in rectangle.into_iter().enumerate() { self.put_word(Self::RECTANGLE + 4 * index, value); }
    }

    /// Left, top.
    pub fn centre(&self) -> [u32; 2] { [self.word(Self::CENTRE), self.word(Self::CENTRE + 4)] }

    pub fn set_centre(&mut self, centre: [u32; 2]) {
        self.put_word(Self::CENTRE, centre[0]);
        self.put_word(Self::CENTRE + 4, centre[1]);
    }

    /// Follows the width when the area is clipped.
    pub fn scale(&self) -> u16 { u16::from_le_bytes([self.0[Self::SCALE], self.0[Self::SCALE + 1]]) }

    pub fn set_scale(&mut self, scale: u16) { self.0[Self::SCALE..Self::SCALE + 2].copy_from_slice(&scale.to_le_bytes()); }

    fn word(&self, offset: usize) -> u32 { u32::from_le_bytes([self.0[offset], self.0[offset + 1], self.0[offset + 2], self.0[offset + 3]]) }

    fn put_word(&mut self, offset: usize, value: u32) { self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
}

/// The areas imgdc reads through `MeasurementRequest::areas` after the request is posted.
pub type MeasuredAreas = [AreaEntry; 3];

/// The record's capture selection the calc library answers for the bound record (0xf4c958).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureSelection {
    pub selection: u32,
    /// Compared with the scale steps that size the measurement band.
    pub first: i16,
    pub second: u16,
    pub auxiliary: u32,
}

/// Imgdc 0x3f46: one measurement of the record's areas. The request also carries the address of
/// the `MeasuredAreas` it measures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementRequest {
    pub subject_source: u32,
    pub subjects_locked: u32,
    pub layout: [u16; 2],
    pub secondary_mode: u32,
    pub secondary_option: u32,
    pub secondary_latch: u32,
    pub profile_class: u16,
    pub capture_offset: u32,
    pub frame_words: [u32; 2],
    pub selection: CaptureSelection,
    pub slot_offset: i16,
    pub slot_rate: u32,
    pub subject_area: u32,
    /// Whether each of the record's area lists holds an area.
    pub lists: [bool; 2],
    /// The detail word of each list, zero for an empty list.
    pub list_details: [u32; 2],
    pub area_count: u32,
    /// Rows the measurement band moves towards row 0; negative moves it the other way.
    pub shift: i16,
}

/// The source the 0x5c completion names for the statistics of one measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MeasurementReady {
    pub source: u8,
}

/// Calc 0x3c0d's first result (completion 0x94), which the secondary job keeps whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Evaluation(pub [u8; Evaluation::BYTES]);

impl Default for Evaluation {
    fn default() -> Self { Self([0; Self::BYTES]) }
}

/// Which of the record's area lists an evaluation refers to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluatedList { First, Second }

impl Evaluation {
    pub const BYTES: usize = 84;
    const MEASURED: usize = 0;
    const PRIMARY: usize = 0x4;
    const TRANSFER: usize = 0x8;
    const BASE: usize = 0xc;
    const REFINED: usize = 0x10;
    const THIRD: usize = 0x14;
    const FIFTH: usize = 0x16;
    const SIXTH: usize = 0x18;
    const FOURTH: usize = 0x1a;
    const COUNT: usize = 0x1c;
    const BLOCK: usize = 0x20;
    pub const BLOCK_BYTES: usize = 24;
    const AREA_MODE: usize = 0x38;
    const LIST: usize = 0x3c;
    const LIST_POSITION: usize = 0x40;
    const PRIMARY_LIMIT: usize = 0x48;
    const SEVENTH: usize = 0x4c;
    const SECOND_LIMIT: usize = 0x4e;
    const STORES_LIMIT: usize = 0x50;

    /// Nonzero enables the secondary job; also stored into the bound record.
    pub fn measured(&self) -> u32 { self.word(Self::MEASURED) }
    pub fn set_measured(&mut self, value: u32) { self.put_word(Self::MEASURED, value); }
    /// Read by the primary job.
    pub fn primary(&self) -> u32 { self.word(Self::PRIMARY) }
    pub fn set_primary(&mut self, value: u32) { self.put_word(Self::PRIMARY, value); }
    /// Nonzero asks for imgdc 0x3f47, which carries it.
    pub fn transfer(&self) -> u32 { self.word(Self::TRANSFER) }
    pub fn set_transfer(&mut self, value: u32) { self.put_word(Self::TRANSFER, value); }
    pub fn base(&self) -> i16 { self.half(Self::BASE) }
    pub fn set_base(&mut self, value: i16) { self.put_half(Self::BASE, value); }
    pub fn refined(&self) -> bool { self.word(Self::REFINED) != 0 }
    pub fn third(&self) -> i16 { self.half(Self::THIRD) }
    pub fn set_third(&mut self, value: i16) { self.put_half(Self::THIRD, value); }
    pub fn fourth(&self) -> i16 { self.half(Self::FOURTH) }
    pub fn set_fourth(&mut self, value: i16) { self.put_half(Self::FOURTH, value); }
    pub fn fifth(&self) -> i16 { self.half(Self::FIFTH) }
    pub fn set_fifth(&mut self, value: i16) { self.put_half(Self::FIFTH, value); }
    pub fn sixth(&self) -> i16 { self.half(Self::SIXTH) }
    pub fn count(&self) -> u8 { self.0[Self::COUNT] }
    pub fn block(&self) -> [u8; Self::BLOCK_BYTES] { core::array::from_fn(|index| self.0[Self::BLOCK + index]) }
    pub fn set_block(&mut self, block: [u8; Self::BLOCK_BYTES]) { self.0[Self::BLOCK..Self::BLOCK + Self::BLOCK_BYTES].copy_from_slice(&block); }
    /// 1 asks for the subject state 2 and for the first item to follow the evaluated area.
    pub fn area_mode(&self) -> u32 { self.word(Self::AREA_MODE) }
    pub fn list(&self) -> Option<EvaluatedList> {
        match self.word(Self::LIST) {
            1 => Some(EvaluatedList::First),
            2 => Some(EvaluatedList::Second),
            _ => None,
        }
    }
    /// The evaluated area's 1-based position in its list; 0 names none.
    pub fn list_position(&self) -> u8 { self.0[Self::LIST_POSITION] }
    /// Read by the primary job.
    pub fn primary_limit(&self) -> u32 { self.word(Self::PRIMARY_LIMIT) }
    pub fn set_primary_limit(&mut self, value: u32) { self.put_word(Self::PRIMARY_LIMIT, value); }
    pub fn seventh(&self) -> i16 { self.half(Self::SEVENTH) }
    pub fn set_seventh(&mut self, value: i16) { self.put_half(Self::SEVENTH, value); }
    /// The first slot's second limit, when `stores_limit`.
    pub fn second_limit(&self) -> i16 { self.half(Self::SECOND_LIMIT) }
    pub fn stores_limit(&self) -> bool { self.word(Self::STORES_LIMIT) == 1 }

    fn word(&self, offset: usize) -> u32 { u32::from_le_bytes([self.0[offset], self.0[offset + 1], self.0[offset + 2], self.0[offset + 3]]) }
    fn put_word(&mut self, offset: usize, value: u32) { self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }
    fn half(&self, offset: usize) -> i16 { i16::from_le_bytes([self.0[offset], self.0[offset + 1]]) }
    fn put_half(&mut self, offset: usize, value: i16) { self.0[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
}

/// What the secondary job takes from calc 0x3c0d's second result (completion 0x95).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Refinement {
    pub fifth: i16,
    pub block: [u8; Evaluation::BLOCK_BYTES],
    /// Stored into the bound record's first slot.
    pub slot_word: u32,
    pub slot_offset: i16,
}

/// Imgdc 0x3f47: the evaluation's transfer word (completion 0x5d).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferRequest {
    pub transfer: u32,
}
