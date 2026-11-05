//! Populated record areas for a FujiMod session: the record-area blocks of a
//! committed NAND image, programmed into the managed pool where the image
//! places them.
//!
//! Image 4 manages physical blocks `[0x380, 0x400)` as one pool. Every page of
//! a pool block carries a location tag naming the block's logical index, and
//! the map builder (`0x6a9354` -> `0x6fb2a0`) places logical blocks by tag, not
//! by position. The record service's five areas own two logical blocks each:
//! the erase dispatch `0x6ae068` sends kind k to the region whose descriptor
//! words at `0x7c8c6c` give start blocks `0x3d4`/`0x3d5` (kind 0), `0x3d2`/`0x3d3`
//! (1), `0x3d6`/`0x3d7` (2), `0x3d8`/`0x3d9` (3) and `0x3e8`/`0x3e9` (4), each
//! less `0x380`.

use crate::adjustment;
use crate::nand_image::{CommittedImage, PAGE_BYTES, PAGES_PER_BLOCK, SPARE_BYTES};
use crate::run::NodeName;
use fujimod_emulator::session::Session;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

pub const BLOCK_BYTES: usize = PAGE_BYTES * PAGES_PER_BLOCK as usize;
const POOL: Range<u32> = 0x380..0x400;
/// Bit 12 of a location tag.
const TAG_MARKER: u16 = 0x1000;
/// Taking the decoder's location count to be the map's 0x6c entries.
const LAST_LOGICAL: u16 = 0x6b;

/// A record area by its record-service kind (the first argument of `0x6ac328`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecordArea {
    Kind0,
    Kind1,
    Kind2,
    Kind3,
    Kind4,
}

impl RecordArea {
    pub const ALL: [RecordArea; 5] = [Self::Kind0, Self::Kind1, Self::Kind2, Self::Kind3, Self::Kind4];

    pub fn logical_blocks(self) -> [LogicalBlock; 2] {
        let first = match self {
            Self::Kind0 => 0x54,
            Self::Kind1 => 0x52,
            Self::Kind2 => 0x56,
            Self::Kind3 => 0x58,
            Self::Kind4 => 0x68,
        };
        [LogicalBlock(first), LogicalBlock(first + 1)]
    }

    fn of(logical: LogicalBlock) -> Option<Self> {
        Self::ALL.into_iter().find(|area| area.logical_blocks().contains(&logical))
    }

    fn name(self) -> &'static str {
        match self {
            Self::Kind0 => "kind-0",
            Self::Kind1 => "kind-1",
            Self::Kind2 => "kind-2",
            Self::Kind3 => "kind-3",
            Self::Kind4 => "kind-4",
        }
    }
}

/// A logical block index relative to the pool's first block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LogicalBlock(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhysicalBlock(pub u32);

impl PhysicalBlock {
    fn first_row(self) -> u32 {
        self.0 * PAGES_PER_BLOCK
    }
}

/// A 16-bit location tag: the logical index shifted left by one, the marker
/// bit and a low bit that makes the population count even. Composed spares
/// hold it big-endian twice, then `0xff`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocationTag(u16);

impl LocationTag {
    pub fn of(logical: LogicalBlock) -> Self {
        let value = TAG_MARKER | (logical.0 << 1);
        Self(if value.count_ones() % 2 == 1 { value | 1 } else { value })
    }

    fn parity_holds(self) -> bool {
        self.0.count_ones() % 2 == 0
    }

    fn index(self) -> Option<LogicalBlock> {
        let index = (self.0 & !TAG_MARKER) >> 1;
        (index <= LAST_LOGICAL).then_some(LogicalBlock(index))
    }

    pub fn spare(self) -> Vec<u8> {
        let mut spare = vec![0xff; SPARE_BYTES];
        let [high, low] = self.0.to_be_bytes();
        spare[..4].copy_from_slice(&[high, low, high, low]);
        spare
    }
}

/// The logical block `nand_decode_location_tags` `0x6eecf4` reads from a
/// spare's two tags; parity is `0x6ee5d0` with nibble table `0x812ba4`.
pub fn decode_spare(spare: &[u8]) -> Option<LogicalBlock> {
    let first = LocationTag(u16::from_be_bytes([spare[0], spare[1]]));
    let second = LocationTag(u16::from_be_bytes([spare[2], spare[3]]));
    if first != second && second.parity_holds() {
        if let Some(logical) = second.index() {
            return if !first.parity_holds() || first.index().is_none() { Some(logical) } else { None };
        }
    }
    if first.parity_holds() { first.index() } else { None }
}

/// Why an image is refused.
#[derive(Debug, PartialEq, Eq)]
pub enum Refusal {
    Image(String),
    Partial(PhysicalBlock),
    /// A record block whose pages differ from its page 0 spare, or whose spare is not the composed form.
    UnevenSpare(PhysicalBlock),
    InAdjustment(PhysicalBlock),
    Duplicate(LogicalBlock),
    NoRecordBlock,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Image(error) => f.write_str(error),
            Self::Partial(block) => write!(f, "record block {:#x} is only partly committed", block.0),
            Self::UnevenSpare(block) => write!(f, "record block {:#x} does not carry one composed tag on every page", block.0),
            Self::InAdjustment(block) => write!(f, "record block {:#x} lies in the adjustment blocks", block.0),
            Self::Duplicate(logical) => write!(f, "logical record block {:#x} appears twice", logical.0),
            Self::NoRecordBlock => f.write_str("the NAND image commits no record-area block"),
        }
    }
}

impl From<Refusal> for String {
    fn from(refusal: Refusal) -> Self {
        refusal.to_string()
    }
}

struct Placed {
    area: RecordArea,
    physical: PhysicalBlock,
    pages: Vec<Vec<u8>>,
}

pub struct RecordMedium {
    blocks: BTreeMap<LogicalBlock, Placed>,
    ignored: Vec<PhysicalBlock>,
}

impl RecordMedium {
    /// Selects every committed pool block whose page tags name a record-area
    /// block. Such a block must be complete, carry its page 0 spare on every
    /// page in the composed form and lie outside the adjustment blocks; a
    /// logical block may appear once. Other committed pool blocks are ignored.
    pub fn parse(gzip: &[u8]) -> Result<Self, Refusal> {
        let image = CommittedImage::parse(gzip).map_err(Refusal::Image)?;
        let mut blocks = BTreeMap::new();
        let mut ignored = Vec::new();
        for block in image.committed_blocks().into_iter().filter(|block| POOL.contains(block)) {
            let physical = PhysicalBlock(block);
            let rows = physical.first_row()..physical.first_row() + PAGES_PER_BLOCK;
            let pages: Vec<Option<&(Vec<u8>, Vec<u8>)>> = rows.map(|row| image.page(row)).collect();
            let Some((logical, area)) = pages
                .iter()
                .flatten()
                .find_map(|(_, spare)| decode_spare(spare).and_then(|logical| Some((logical, RecordArea::of(logical)?))))
            else {
                ignored.push(physical);
                continue;
            };
            let pages: Vec<&(Vec<u8>, Vec<u8>)> = pages.into_iter().collect::<Option<_>>().ok_or(Refusal::Partial(physical))?;
            let composed = LocationTag::of(logical).spare();
            if pages.iter().any(|(_, spare)| *spare != composed) {
                return Err(Refusal::UnevenSpare(physical));
            }
            if adjustment::BLOCKS.contains(&block) {
                return Err(Refusal::InAdjustment(physical));
            }
            let placed = Placed { area, physical, pages: pages.into_iter().map(|(data, _)| data.clone()).collect() };
            if blocks.insert(logical, placed).is_some() {
                return Err(Refusal::Duplicate(logical));
            }
        }
        if blocks.is_empty() {
            return Err(Refusal::NoRecordBlock);
        }
        Ok(Self { blocks, ignored })
    }

    /// Every NAND row this medium programs, with its data and spare, in programming order.
    pub fn rows(&self) -> impl Iterator<Item = (u32, &[u8], Vec<u8>)> {
        self.blocks.iter().flat_map(|(logical, placed)| {
            let spare = LocationTag::of(*logical).spare();
            (placed.physical.first_row()..).zip(&placed.pages).map(move |(row, data)| (row, data.as_slice(), spare.clone()))
        })
    }

    /// Erases each record block in the unexecuted session's NAND `medium` and programs its rows.
    pub fn apply(&self, session: &mut Session, medium: &NodeName) -> Result<(), String> {
        let mut nand = session.nand_medium(&medium.0)?;
        for placed in self.blocks.values() {
            nand.erase(placed.physical.first_row())?;
        }
        for (row, data, spare) in self.rows() {
            nand.program(row, data, &spare)?;
        }
        session.retain_nand(&medium.0, nand)
    }

    pub fn summary(&self) -> Value {
        let blocks: Vec<Value> = self
            .blocks
            .iter()
            .map(|(logical, placed)| {
                json!({
                    "area": placed.area.name(),
                    "logical": format!("{:#x}", logical.0),
                    "physical": format!("{:#x}", placed.physical.0),
                    "data_sha256": format!("{:x}", Sha256::digest(placed.pages.concat())),
                })
            })
            .collect();
        let ignored: Vec<String> = self.ignored.iter().map(|block| format!("{:#x}", block.0)).collect();
        json!({"blocks": blocks, "ignored_pool_blocks": ignored})
    }
}

/// Why a composition is refused.
#[derive(Debug, PartialEq, Eq)]
pub enum CompositionRefusal {
    NotRecordArea(LogicalBlock),
    NotFreePoolBlock(PhysicalBlock),
    WrongLength(LogicalBlock),
    PlacedTwice(PhysicalBlock),
}

impl fmt::Display for CompositionRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotRecordArea(logical) => write!(f, "logical block {:#x} is not a record-area block", logical.0),
            Self::NotFreePoolBlock(block) => write!(f, "physical block {:#x} is outside the pool or in the adjustment blocks", block.0),
            Self::WrongLength(logical) => write!(f, "record block {:#x} data is not {BLOCK_BYTES} bytes", logical.0),
            Self::PlacedTwice(block) => write!(f, "physical block {:#x} is placed twice", block.0),
        }
    }
}

impl From<CompositionRefusal> for String {
    fn from(refusal: CompositionRefusal) -> Self {
        refusal.to_string()
    }
}

/// The committed pages of record blocks: each `(logical, physical, data)`
/// places one 128 KiB block.
pub fn compose_pages(
    blocks: &[(LogicalBlock, PhysicalBlock, Vec<u8>)],
) -> Result<BTreeMap<u32, (Vec<u8>, Vec<u8>)>, CompositionRefusal> {
    let mut pages = BTreeMap::new();
    for (logical, physical, data) in blocks {
        if RecordArea::of(*logical).is_none() {
            return Err(CompositionRefusal::NotRecordArea(*logical));
        }
        if !POOL.contains(&physical.0) || adjustment::BLOCKS.contains(&physical.0) {
            return Err(CompositionRefusal::NotFreePoolBlock(*physical));
        }
        if data.len() != BLOCK_BYTES {
            return Err(CompositionRefusal::WrongLength(*logical));
        }
        let spare = LocationTag::of(*logical).spare();
        for (row, page) in (physical.first_row()..).zip(data.chunks(PAGE_BYTES)) {
            if pages.insert(row, (page.to_vec(), spare.clone())).is_some() {
                return Err(CompositionRefusal::PlacedTwice(*physical));
            }
        }
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(fill: u8) -> Vec<u8> {
        (0..BLOCK_BYTES).map(|index| fill ^ (index as u8)).collect()
    }

    fn image(pages: &BTreeMap<u32, (Vec<u8>, Vec<u8>)>) -> Vec<u8> {
        CommittedImage::compose(&[0u8; 16], pages).unwrap()
    }

    fn refusal(gzip: &[u8]) -> Refusal {
        match RecordMedium::parse(gzip) {
            Ok(_) => panic!("the image was admitted"),
            Err(refusal) => refusal,
        }
    }

    fn spare_of(tags: [u16; 2]) -> Vec<u8> {
        let mut spare = vec![0xff; SPARE_BYTES];
        spare[..2].copy_from_slice(&tags[0].to_be_bytes());
        spare[2..4].copy_from_slice(&tags[1].to_be_bytes());
        spare
    }

    #[test]
    fn tags_match_the_retained_engine_pool_blocks() {
        // Page tags of f7-1.nand blocks 0x380, 0x381 and 0x383 (logical 0x58, 0x29, 0x2b).
        assert_eq!(LocationTag::of(LogicalBlock(0x58)).spare()[..4], [0x10, 0xb0, 0x10, 0xb0]);
        assert_eq!(LocationTag::of(LogicalBlock(0x29)).spare()[..4], [0x10, 0x52, 0x10, 0x52]);
        assert_eq!(LocationTag::of(LogicalBlock(0x2b)).spare()[..4], [0x10, 0x57, 0x10, 0x57]);
        for logical in 0..=LAST_LOGICAL {
            assert_eq!(decode_spare(&LocationTag::of(LogicalBlock(logical)).spare()), Some(LogicalBlock(logical)));
        }
    }

    #[test]
    fn decoding_follows_the_stock_choice_between_the_two_tags() {
        let good = LocationTag::of(LogicalBlock(0x58)).0;
        let other = LocationTag::of(LogicalBlock(0x59)).0;
        let odd = 0x1053;
        assert_eq!(decode_spare(&spare_of([odd, good])), Some(LogicalBlock(0x58)));
        assert_eq!(decode_spare(&spare_of([good, odd])), Some(LogicalBlock(0x58)));
        assert_eq!(decode_spare(&spare_of([good, other])), None);
        assert_eq!(decode_spare(&spare_of([odd, odd])), None);
        assert_eq!(decode_spare(&spare_of([LocationTag::of(LogicalBlock(0x6c)).0; 2])), None);
    }

    #[test]
    fn selects_record_blocks_and_ignores_other_pool_blocks() {
        let mut pages = compose_pages(&[
            (LogicalBlock(0x56), PhysicalBlock(0x3ac), block(1)),
            (LogicalBlock(0x57), PhysicalBlock(0x3b2), block(2)),
            (LogicalBlock(0x58), PhysicalBlock(0x380), block(3)),
        ])
        .unwrap();
        let adjustment = LocationTag::of(LogicalBlock(0x29)).spare();
        for row in 0x381 * PAGES_PER_BLOCK..0x382 * PAGES_PER_BLOCK {
            pages.insert(row, (vec![0; PAGE_BYTES], adjustment.clone()));
        }
        let medium = RecordMedium::parse(&image(&pages)).unwrap();
        let summary = medium.summary();
        assert_eq!(summary["ignored_pool_blocks"], json!(["0x381"]));
        assert_eq!(summary["blocks"][0]["logical"], "0x56");
        assert_eq!(summary["blocks"][0]["physical"], "0x3ac");
        assert_eq!(summary["blocks"][1]["area"], "kind-2");
        assert_eq!(summary["blocks"][1]["data_sha256"], format!("{:x}", Sha256::digest(block(2))));
        assert_eq!(summary["blocks"][2]["physical"], "0x380");
    }

    #[test]
    fn programs_each_record_page_with_its_tag_and_nothing_else() {
        let pages = compose_pages(&[(LogicalBlock(0x58), PhysicalBlock(0x3c0), block(3))]).unwrap();
        let medium = RecordMedium::parse(&image(&pages)).unwrap();
        let rows: Vec<(u32, Vec<u8>, Vec<u8>)> = medium.rows().map(|(row, data, spare)| (row, data.to_vec(), spare)).collect();
        let expected: Vec<(u32, Vec<u8>, Vec<u8>)> =
            pages.into_iter().map(|(row, (data, spare))| (row, data, spare)).collect();
        assert_eq!(rows, expected);
        assert!(rows.iter().all(|(row, _, _)| (0x3c0 * PAGES_PER_BLOCK..0x3c1 * PAGES_PER_BLOCK).contains(row)));
        assert!(rows.iter().all(|(_, _, spare)| decode_spare(spare) == Some(LogicalBlock(0x58))));
    }

    #[test]
    fn refuses_partial_duplicate_uneven_and_adjustment_record_blocks() {
        let one = compose_pages(&[(LogicalBlock(0x58), PhysicalBlock(0x3c0), block(3))]).unwrap();
        let mut partial = one.clone();
        partial.remove(&(0x3c0 * PAGES_PER_BLOCK + 7));
        assert_eq!(refusal(&image(&partial)), Refusal::Partial(PhysicalBlock(0x3c0)));

        let mut duplicate = one.clone();
        duplicate.extend(compose_pages(&[(LogicalBlock(0x58), PhysicalBlock(0x3c1), block(4))]).unwrap());
        assert_eq!(refusal(&image(&duplicate)), Refusal::Duplicate(LogicalBlock(0x58)));

        let mut mixed = one.clone();
        mixed.get_mut(&(0x3c0 * PAGES_PER_BLOCK + 9)).unwrap().1 = LocationTag::of(LogicalBlock(0x59)).spare();
        assert_eq!(refusal(&image(&mixed)), Refusal::UnevenSpare(PhysicalBlock(0x3c0)));

        let mut marked = one.clone();
        marked.get_mut(&(0x3c0 * PAGES_PER_BLOCK)).unwrap().1[5] = 0xe0;
        assert_eq!(refusal(&image(&marked)), Refusal::UnevenSpare(PhysicalBlock(0x3c0)));

        let mut adjustment = BTreeMap::new();
        let spare = LocationTag::of(LogicalBlock(0x58)).spare();
        for row in 0x390 * PAGES_PER_BLOCK..0x391 * PAGES_PER_BLOCK {
            adjustment.insert(row, (vec![0; PAGE_BYTES], spare.clone()));
        }
        assert_eq!(refusal(&image(&adjustment)), Refusal::InAdjustment(PhysicalBlock(0x390)));

        assert_eq!(refusal(&image(&BTreeMap::new())), Refusal::NoRecordBlock);
    }

    #[test]
    fn composition_refuses_blocks_outside_the_record_areas_and_the_free_pool() {
        let refused = |blocks: &[(LogicalBlock, PhysicalBlock, Vec<u8>)]| compose_pages(blocks).unwrap_err();
        assert_eq!(refused(&[(LogicalBlock(0x29), PhysicalBlock(0x3c0), block(0))]), CompositionRefusal::NotRecordArea(LogicalBlock(0x29)));
        assert_eq!(refused(&[(LogicalBlock(0x58), PhysicalBlock(0x390), block(0))]), CompositionRefusal::NotFreePoolBlock(PhysicalBlock(0x390)));
        assert_eq!(refused(&[(LogicalBlock(0x58), PhysicalBlock(0x37f), block(0))]), CompositionRefusal::NotFreePoolBlock(PhysicalBlock(0x37f)));
        assert_eq!(refused(&[(LogicalBlock(0x58), PhysicalBlock(0x3c0), vec![0; 16])]), CompositionRefusal::WrongLength(LogicalBlock(0x58)));
        assert_eq!(
            refused(&[(LogicalBlock(0x58), PhysicalBlock(0x3c0), block(0)), (LogicalBlock(0x59), PhysicalBlock(0x3c0), block(1))]),
            CompositionRefusal::PlacedTwice(PhysicalBlock(0x3c0))
        );
    }
}
