#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the geometry service's area query and answer and of the caller's area
//! list they become (0xeec1a0).

use crate::geometry::AreaQuery;

pub const AREAS: usize = 10;
pub const TRANSITION_AREA_BYTES: usize = 88;
pub const AREA_BYTES: usize = 104;
/// The count word and ten areas.
pub const AREA_LIST_BYTES: usize = 4 + AREAS * AREA_BYTES;

/// The geometry service's answer to an area query (0x106638c): a count byte, then the areas
/// from +4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransitionAreas {
    pub count: u8,
    pub areas: [[u8; TRANSITION_AREA_BYTES]; AREAS],
}

impl TransitionAreas {
    pub const EMPTY: Self = Self { count: 0, areas: [[0; TRANSITION_AREA_BYTES]; AREAS] };
}

/// One area of the caller's area list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Area(pub [u8; AREA_BYTES]);

impl Area {
    fn word(&self, at: usize) -> u32 { u32::from_le_bytes([self.0[at], self.0[at + 1], self.0[at + 2], self.0[at + 3]]) }

    fn set_word(&mut self, at: usize, value: u32) { self.0[at..at + 4].copy_from_slice(&value.to_le_bytes()) }

    /// The area's two origins, top then left: the area's own and its inner one.
    pub fn origins(&self) -> [[u32; 2]; 2] { [[self.word(0x0), self.word(0x4)], [self.word(0x10), self.word(0x14)]] }

    pub fn set_origins(&mut self, origins: [[u32; 2]; 2]) {
        for (index, at) in [0x0, 0x4, 0x10, 0x14].into_iter().enumerate() {
            self.set_word(at, origins[index / 2][index % 2]);
        }
    }

    /// The extent the pair ratio rescales.
    pub fn extent(&self) -> u16 { u16::from_le_bytes([self.0[0x22], self.0[0x23]]) }

    pub fn set_extent(&mut self, extent: u16) { self.0[0x22..0x24].copy_from_slice(&extent.to_le_bytes()) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaList {
    pub count: u32,
    pub areas: [Area; AREAS],
}

impl AreaList {
    pub fn bytes(&self) -> [u8; AREA_LIST_BYTES] {
        let mut bytes = [0; AREA_LIST_BYTES];
        bytes[..4].copy_from_slice(&self.count.to_le_bytes());
        for (index, area) in self.areas.iter().enumerate() {
            bytes[4 + index * AREA_BYTES..][..AREA_BYTES].copy_from_slice(&area.0);
        }
        bytes
    }

    pub fn present(&mut self) -> &mut [Area] { &mut self.areas[..(self.count as usize).min(AREAS)] }
}

/// Each area field as (area offset, answer offset, bytes); the area's +0x26 byte takes the low
/// byte of the answer's word at +0x54.
const FIELDS: [(usize, usize, usize); 23] = [
    (0x00, 0x06, 2),
    (0x04, 0x08, 2),
    (0x08, 0x0c, 2),
    (0x0c, 0x0a, 2),
    (0x10, 0x00, 2),
    (0x14, 0x02, 2),
    (0x18, 0x10, 1),
    (0x1c, 0x11, 1),
    (0x20, 0x0e, 2),
    (0x22, 0x04, 2),
    (0x24, 0x12, 1),
    (0x25, 0x13, 1),
    (0x26, 0x54, 1),
    (0x28, 0x14, 4),
    (0x2c, 0x18, 4),
    (0x30, 0x1c, 2),
    (0x32, 0x1e, 1),
    (0x34, 0x20, 16),
    (0x44, 0x30, 8),
    (0x4c, 0x38, 16),
    (0x5c, 0x48, 4),
    (0x60, 0x4c, 4),
    (0x64, 0x50, 4),
];

/// 0xeec1a0: at most ten areas, the rest of the list zero. Halfword and byte fields widen into
/// zeroed words; the bytes at +0x27 and +0x33 of a converted area, which stock leaves as they
/// were, are zero.
pub fn area_list(answer: &TransitionAreas) -> AreaList {
    let count = answer.count.min(AREAS as u8) as usize;
    let mut list = AreaList { count: count as u32, areas: [Area([0; AREA_BYTES]); AREAS] };
    for (area, source) in list.areas.iter_mut().zip(&answer.areas).take(count) {
        for (to, from, bytes) in FIELDS {
            area.0[to..to + bytes].copy_from_slice(&source[from..from + bytes]);
        }
    }
    list
}

/// The query's halfwords in their stock order.
pub fn query_bytes(query: &AreaQuery) -> [u8; 24] {
    let halves = [
        query.window[0],
        query.window[1],
        query.window[2],
        query.window[3],
        query.extent[0],
        query.extent[1],
        query.reference[0],
        query.reference[1],
        query.reference[2],
        query.reference[3],
        query.pair[0],
        query.pair[1],
    ];
    let mut bytes = [0; 24];
    for (pair, half) in bytes.chunks_exact_mut(2).zip(halves) {
        pair.copy_from_slice(&half.to_le_bytes());
    }
    bytes
}
