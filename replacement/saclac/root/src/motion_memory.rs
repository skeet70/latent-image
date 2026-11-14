#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::motion::readings::{AreaList, FocusRequest, ListedArea, SelectionLists, AREA_POSITIONS};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::subjects::Candidates;

/// A borrowed view of the preparation's candidates and saved lens-control reading.
pub(crate) struct Selection<'a> {
    pub(crate) candidates: &'a Candidates,
    pub(crate) reference: u32,
}

impl SelectionLists for Selection<'_> {
    fn subjects(&mut self) -> u8 { self.candidates.lists[0].count as u8 }
    fn single_subject(&mut self) -> u8 { self.candidates.lists[1].entries[0].detail[0x26] }
    fn areas(&mut self) -> AreaList {
        let source = &self.candidates.lists[1];
        AreaList {
            count: source.count,
            areas: core::array::from_fn(|index| {
                let subject = &source.entries[index];
                let word = |offset| u32::from_le_bytes(subject.detail[offset..offset + 4].try_into().unwrap());
                let half = |offset| u16::from_le_bytes(subject.detail[offset..offset + 2].try_into().unwrap());
                let mut rest = [0; 0x44];
                rest[..0x38].copy_from_slice(&subject.detail[0x24..0x5c]);
                rest[0x38..0x3c].copy_from_slice(&subject.id.0.to_le_bytes());
                rest[0x3c..].copy_from_slice(&subject.detail[0x5c..]);
                //Native areas store top/left,width/height,centre-top/centre-left.
                ListedArea {
                    x: word(4), y: word(0), height: word(0xc), width: word(8), centre_x: word(0x14), centre_y: word(0x10),
                    at_18: word(0x18), at_1c: word(0x1c), at_20: half(0x20), at_22: half(0x22), rest,
                }
            }),
        }
    }
    fn area_positions(&mut self) -> [i16; AREA_POSITIONS] {
        //The last five summary words are the ten halfwords at native context +0x1cb4.
        core::array::from_fn(|index| (self.candidates.summary.0[10 + index / 2] >> (16 * (index % 2))) as i16)
    }
    fn reference(&mut self) -> u32 { self.reference }
    fn area_words(&mut self) -> [u32; 2] { [u32::from(self.candidates.found), self.candidates.source] }
}

///Native fa3768 dereferences input word4 as a36-byte focus request.
pub(crate) fn focus_request(machine: &impl Machine, at: u32) -> FocusRequest {
    let word = |offset| machine.read_u32(at + offset);
    FocusRequest {
        at_00: word(0), at_04: word(4), position: machine.read_u16(at + 8) as i16, at_0a: machine.read_u16(at + 0xa) as i16,
        at_0c: word(0xc), at_10: word(0x10), side: word(0x14),
        positions: core::array::from_fn(|index| machine.read_u16(at + 0x18 + 2 * index as u32) as i16), at_20: word(0x20),
    }
}
