#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the calculation absorb: the image-4 setters the record stores go through,
//! in the order 0xf1dd84, 0xf18b94 and 0xf1e468 call them, and the metering words it leaves.

use crate::absorb::{AreaDetail, AreaStore, AuxiliaryOutcome, AuxiliaryReference, MeteringMinimum};
use crate::boundary::MeteringBlock;
use xm1_saclac_interface::calculation::{CellSummary, FocusSummary};

/// An image-4 record setter called with the record, the area index where it has one, and a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordStore {
    pub setter: u32,
    pub value: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Source { Byte(usize), Half(usize), UnsignedHalf(usize), Word(usize) }

impl Source {
    fn read(self, bytes: &[u8]) -> u32 {
        let at = |offset: usize, width: usize| bytes.get(offset..offset + width).unwrap_or(&[0; 4][..width]);
        match self {
            Self::Byte(offset) => u32::from(at(offset, 1)[0]),
            Self::Half(offset) => {
                let half = at(offset, 2);
                i16::from_le_bytes([half[0], half[1]]) as i32 as u32
            }
            Self::UnsignedHalf(offset) => {
                let half = at(offset, 2);
                u32::from(u16::from_le_bytes([half[0], half[1]]))
            }
            Self::Word(offset) => {
                let word = at(offset, 4);
                u32::from_le_bytes([word[0], word[1], word[2], word[3]])
            }
        }
    }
}

/// Area fields taken from the area's result (0xf1de6c..0xf1e08c).
const AREA_FIELDS: [(u32, Source); 19] = [
    (0x7529e8, Source::Half(0x0)),
    (0x752a98, Source::Word(0x4)),
    (0x752bf8, Source::Half(0xa)),
    (0x752ca8, Source::Half(0xc)),
    (0x752d58, Source::Half(0xe)),
    (0x752b48, Source::Half(0x8)),
    (0x753648, Source::Word(0x28)),
    (0x7532d8, Source::Byte(0x2c)),
    (0x753178, Source::Half(0x1a)),
    (0x753388, Source::Word(0x1c)),
    (0x753438, Source::Word(0x20)),
    (0x7534e8, Source::Word(0x24)),
    (0x753228, Source::Half(0x18)),
    (0x752f68, Source::Half(0x10)),
    (0x753018, Source::Half(0x12)),
    (0x7530c8, Source::Half(0x14)),
    (0x753598, Source::Half(0x16)),
    (0x753f38, Source::Half(0x4c)),
    (0x753fe8, Source::Half(0x4e)),
];
const AREA_WEIGHT: u32 = 0x7544b8;
const AREA_MARKER: u32 = 0x754568;

pub fn area_stores(store: &AreaStore) -> [RecordStore; 21] {
    let bytes = store.area.bytes();
    core::array::from_fn(|position| match position {
        19 => RecordStore { setter: AREA_WEIGHT, value: store.weight },
        20 => RecordStore { setter: AREA_MARKER, value: store.marker },
        _ => {
            let (setter, source) = AREA_FIELDS[position];
            RecordStore { setter, value: source.read(bytes) }
        }
    })
}

/// 0xf1e100..0xf1e27c; the grid form leaves out the extended flag (0xf18b94).
pub fn detail_stores(detail: &AreaDetail) -> [Option<RecordStore>; 10] {
    let store = |setter, value| Some(RecordStore { setter, value });
    [
        store(0x7536f8, u32::from(detail.selection_class)),
        store(0x7537a8, detail.plan_half as i32 as u32),
        store(0x753858, u32::from(detail.plan_bit_two)),
        store(0x753908, u32::from(detail.plan_bit_five)),
        store(0x7539b8, detail.distance),
        store(0x753a68, u32::from(detail.row_class)),
        store(0x753b18, 0),
        store(0x753bc8, 0),
        detail.extended.and_then(|extended| store(0x7541f8, u32::from(extended))),
        store(0x7542a8, detail.row_word),
    ]
}

pub fn auxiliary_stores(auxiliary: &AuxiliaryReference) -> [RecordStore; 2] {
    [RecordStore { setter: 0x752a98, value: auxiliary.target }, RecordStore { setter: 0x752b48, value: auxiliary.normalized as i32 as u32 }]
}

/// Cell fields (0xf1e908..0xf1e9e0), then the weight.
const CELL_FIELDS: [(u32, Source); 13] = [
    (0x756fb8, Source::Half(0x0)),
    (0x757004, Source::Word(0x4)),
    (0x757050, Source::Half(0xa)),
    (0x7570a0, Source::Half(0xc)),
    (0x7570f0, Source::Half(0x8)),
    (0x757140, Source::Half(0x10)),
    (0x757190, Source::Half(0x16)),
    (0x7571e0, Source::Half(0x1a)),
    (0x75722c, Source::Word(0x1c)),
    (0x757274, Source::Word(0x34)),
    (0x757360, Source::UnsignedHalf(0x40)),
    (0x7573ac, Source::Byte(0x42)),
    (0x7572c0, Source::Half(0x3c)),
];
const CELL_WEIGHT: u32 = 0x7573f4;

pub fn cell_stores(cell: &CellSummary, weight: u32) -> [RecordStore; 14] {
    core::array::from_fn(|position| match CELL_FIELDS.get(position) {
        Some(&(setter, source)) => RecordStore { setter, value: source.read(cell.bytes()) },
        None => RecordStore { setter: CELL_WEIGHT, value: weight },
    })
}

/// 0xf1ea18..0xf1ea84; the sixth value is replaced by its level.
const FOCUS_SUMMARY_SETTERS: [u32; 8] = [0x757534, 0x757b98, 0x757be4, 0x757c34, 0x757cd4, 0x757e10, 0x757ea0, 0x757fc8];

pub fn focus_summary_stores(summary: &FocusSummary, level: u32) -> [RecordStore; 8] {
    core::array::from_fn(|position| {
        let value = if position == FocusSummary::CLASSIFIED { level } else { summary.0[position] };
        RecordStore { setter: FOCUS_SUMMARY_SETTERS[position], value }
    })
}

const METERING_SINGLE_PASS: usize = 0x38;
const METERING_MINIMUM: usize = 0x3c;

/// The two words 0xf1ead0..0xf1eb70 writes into record slot 18.
pub fn set_metering_minimum(block: &mut MeteringBlock, minimum: MeteringMinimum) {
    block.0[METERING_SINGLE_PASS..METERING_SINGLE_PASS + 4].copy_from_slice(&u32::from(minimum.single_pass).to_le_bytes());
    block.0[METERING_MINIMUM..METERING_MINIMUM + 4].copy_from_slice(&minimum.minimum.to_le_bytes());
}

/// The calculation block's auxiliary word (+0x65f0).
pub const fn auxiliary_word(outcome: AuxiliaryOutcome) -> u32 {
    match outcome {
        AuxiliaryOutcome::Unchecked => 0,
        AuxiliaryOutcome::Kept => 1,
        AuxiliaryOutcome::Replaced => 2,
    }
}

pub const fn auxiliary_outcome(word: u32) -> AuxiliaryOutcome {
    match word {
        0 => AuxiliaryOutcome::Unchecked,
        1 => AuxiliaryOutcome::Kept,
        _ => AuxiliaryOutcome::Replaced,
    }
}
