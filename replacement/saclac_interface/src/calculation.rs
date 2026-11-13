#![forbid(unsafe_code)]

//! Memory the calc task reads through the pointers of a calc request, and the result a calc
//! 0x3c02 request has it write.

/// The reference block a calc 0x3c02 request points at (X-M1 1.50 id5 0xf601fc posts its address
/// in the second payload word). 0xf1fc5c fills it from its template and shifts it to a lens
/// reference; the lens return adjusts it at 0xf2f0f4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceRequest([u8; ReferenceRequest::BYTES]);

/// Bound-record values the lens return copies into a reference block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReferenceLimits {
    pub first: i16,
    pub second: i16,
    pub option: u32,
}

impl ReferenceRequest {
    pub const BYTES: usize = 300;
    /// 0xf1fc5c reads the reference here and stores the new one here and at `REFERENCE_COPY`.
    const REFERENCE: usize = 0x100;
    const REFERENCE_COPY: usize = 0x8;
    const POSITION: usize = 0x38;
    const AREA_POSITIONS: usize = 0x104;
    const AREAS: usize = 9;
    /// Halfwords at 0xf2f258 and 0xf2f250, word at 0xf2f24c.
    const FIRST_LIMIT: usize = 0x80;
    const SECOND_LIMIT: usize = 0x88;
    const OPTION: usize = 0x8c;

    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self { Self(bytes) }

    pub const fn bytes(&self) -> &[u8; Self::BYTES] { &self.0 }

    pub fn reference(&self) -> u32 { self.word(Self::REFERENCE) }

    pub fn reference_copy(&self) -> u32 { self.word(Self::REFERENCE_COPY) }

    pub fn position(&self) -> i16 { self.half(Self::POSITION) }

    pub fn area_positions(&self) -> [i16; Self::AREAS] {
        let mut positions = [0; Self::AREAS];
        for (index, position) in positions.iter_mut().enumerate() {
            *position = self.half(Self::AREA_POSITIONS + 2 * index);
        }
        positions
    }

    /// Moves the position and the nine area positions by `delta`, wrapping as halfwords
    /// (0xf2f1b0..0xf2f248).
    pub fn shift_positions(&mut self, delta: i16) {
        self.put_half(Self::POSITION, self.position().wrapping_add(delta));
        for index in 0..Self::AREAS {
            let offset = Self::AREA_POSITIONS + 2 * index;
            self.put_half(offset, self.half(offset).wrapping_add(delta));
        }
    }

    pub fn limits(&self) -> ReferenceLimits {
        ReferenceLimits { first: self.half(Self::FIRST_LIMIT), second: self.half(Self::SECOND_LIMIT), option: self.word(Self::OPTION) }
    }

    pub fn set_limits(&mut self, limits: ReferenceLimits) {
        self.put_half(Self::FIRST_LIMIT, limits.first);
        self.put_half(Self::SECOND_LIMIT, limits.second);
        self.put_word(Self::OPTION, limits.option);
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes([self.0[offset], self.0[offset + 1], self.0[offset + 2], self.0[offset + 3]])
    }

    fn half(&self, offset: usize) -> i16 { i16::from_le_bytes([self.0[offset], self.0[offset + 1]]) }

    fn put_word(&mut self, offset: usize, value: u32) { self.0[offset..offset + 4].copy_from_slice(&value.to_le_bytes()); }

    fn put_half(&mut self, offset: usize, value: i16) { self.0[offset..offset + 2].copy_from_slice(&value.to_le_bytes()); }
}

/// A cell of a calc 0x3c02 result's 3x3 grid.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridCell {
    pub column: u8,
    pub row: u8,
}

impl GridCell {
    pub const fn new(column: u8, row: u8) -> Option<Self> {
        if column < 3 && row < 3 { Some(Self { column, row }) } else { None }
    }
}

/// One of the three grid passes a calc 0x3c02 result reports beside its plain areas.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GridPass(u8);

impl GridPass {
    pub const ALL: [Self; 3] = [Self(0), Self(1), Self(2)];

    pub const fn new(pass: u32) -> Option<Self> {
        if pass < 3 { Some(Self(pass as u8)) } else { None }
    }

    pub const fn index(self) -> u8 { self.0 }
}

/// The area set of a calc 0x3c02 result an area, cell or row belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultPass {
    Plain,
    Grid(GridPass),
}

/// The area index within a cell (0..3) or a tracked kind's triple.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaIndex(u8);

impl AreaIndex {
    pub const COUNT: u8 = 3;

    pub const fn new(index: u8) -> Option<Self> {
        if index < Self::COUNT { Some(Self(index)) } else { None }
    }

    pub const fn get(self) -> u8 { self.0 }
}

/// A tracked subject kind, below 6, whose own area triple a result carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrackedKind(u8);

impl TrackedKind {
    pub const fn new(kind: u8) -> Option<Self> {
        if kind < 6 { Some(Self(kind)) } else { None }
    }

    pub const fn get(self) -> u8 { self.0 }
}

/// Where an area of a calc 0x3c02 result sits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AreaAt {
    Cell { pass: ResultPass, cell: GridCell, index: AreaIndex },
    /// A tracked kind's area, carried per pass.
    Tracked { pass: ResultPass, kind: TrackedKind, index: AreaIndex },
}

/// One area of a calc 0x3c02 result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaResult([u8; AreaResult::BYTES]);

impl Default for AreaResult {
    fn default() -> Self { Self([0; Self::BYTES]) }
}

impl AreaResult {
    pub const BYTES: usize = 84;
    const REFERENCE: usize = 4;
    const POSITION: usize = 8;

    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self { Self(bytes) }

    pub const fn bytes(&self) -> &[u8; Self::BYTES] { &self.0 }

    /// The area's lens reference; the shot's grid minimum takes the smallest (0xf1bb44).
    pub fn reference(&self) -> u32 {
        let at = Self::REFERENCE;
        u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]])
    }

    pub fn position(&self) -> i16 { i16::from_le_bytes([self.0[Self::POSITION], self.0[Self::POSITION + 1]]) }

    /// Replaces the reference and position, as an auxiliary reference does (0xf1d5d4).
    pub fn set_reference(&mut self, reference: u32, position: i16) {
        self.0[Self::REFERENCE..Self::REFERENCE + 4].copy_from_slice(&reference.to_le_bytes());
        self.0[Self::POSITION..Self::POSITION + 2].copy_from_slice(&position.to_le_bytes());
    }
}

/// A calc 0x3c02 result's summary of one grid cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CellSummary([u8; CellSummary::BYTES]);

impl Default for CellSummary {
    fn default() -> Self { Self([0; Self::BYTES]) }
}

impl CellSummary {
    pub const BYTES: usize = 84;

    pub const fn from_bytes(bytes: [u8; Self::BYTES]) -> Self { Self(bytes) }

    pub const fn bytes(&self) -> &[u8; Self::BYTES] { &self.0 }
}

/// The row data the distance evaluation writes for one area of a calc 0x3c02 result.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaRow(pub [u8; AreaRow::BYTES]);

impl AreaRow {
    pub const BYTES: usize = 16;

    pub const fn flags(&self) -> u8 { self.0[0] }

    pub const fn distance(&self) -> u32 { u32::from_le_bytes([self.0[4], self.0[5], self.0[6], self.0[7]]) }
}

/// The eight values of a calc 0x3c02 result the shot stores into the bound record's focus
/// summary, in the order it stores them; the sixth is classified before it is stored.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusSummary(pub [u32; 8]);

impl FocusSummary {
    pub const CLASSIFIED: usize = 5;

    pub const fn classified(&self) -> i16 { self.0[Self::CLASSIFIED] as i16 }
}
