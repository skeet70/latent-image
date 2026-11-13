#![forbid(unsafe_code)]

//! Memory the calc task reads through the pointers of a calc request.

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
