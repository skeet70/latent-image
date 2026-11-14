#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::Machine};
use xm1_shot_sequence::{boundary::Frame, statistics_dump::DumpBoundary, stock::statistics_dump::DUMP};
use crate::shot_services::Libraries;

const SOURCE: u32 = 0x5ffd_b9ec;
const PREPARE_HEADER: u32 = 0x00fa_89fc;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> DumpBoundary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn statistics_source_present(&mut self) -> bool {
        self.services.borrow().stock.byte(AdjustmentKey(0x5256)) != 0
    }
    fn fill_dump_header(&mut self, frame: Frame) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(PREPARE_HEADER, &[frame.0.wrapping_add(DUMP) as usize]);
    }
    fn fill_frame(&mut self, frame: Frame, offset: u32, length: u32, byte: u8) {
        fill(&mut self.services.borrow().machine.clone(), frame.0.wrapping_add(offset), length, byte);
    }
    fn write_frame_word(&mut self, frame: Frame, offset: u32, value: u32) {
        self.services.borrow().machine.clone().write_u32(frame.0.wrapping_add(offset), value);
    }
    fn copy_statistics(&mut self, frame: Frame, offset: u32, length: u32) {
        copy(&mut self.services.borrow().machine.clone(), frame.0.wrapping_add(offset), SOURCE, length);
    }
}

fn fill(machine: &mut impl Machine, mut destination: u32, mut length: u32, byte: u8) {
    while length != 0 && destination & 3 != 0 {
        machine.write_u8(destination, byte);
        destination = destination.wrapping_add(1);
        length -= 1;
    }
    let word = u32::from(byte) * 0x0101_0101;
    while length >= 4 {
        machine.write_u32(destination, word);
        destination = destination.wrapping_add(4);
        length -= 4;
    }
    for offset in 0..length { machine.write_u8(destination.wrapping_add(offset), byte); }
}

fn copy(machine: &mut impl Machine, mut destination: u32, mut source: u32, mut length: u32) {
    // Different alignments require byte access; do not issue an unaligned native word load/store.
    while length != 0 && (destination | source) & 3 != 0 {
        machine.write_u8(destination, machine.read_u8(source));
        destination = destination.wrapping_add(1);
        source = source.wrapping_add(1);
        length -= 1;
    }
    while length >= 4 {
        machine.write_u32(destination, machine.read_u32(source));
        destination = destination.wrapping_add(4);
        source = source.wrapping_add(4);
        length -= 4;
    }
    for offset in 0..length { machine.write_u8(destination.wrapping_add(offset), machine.read_u8(source.wrapping_add(offset))); }
}
