use std::{cell::RefCell, collections::HashMap, rc::Rc};
use xm1_saclac_shared::settings::Channel;
use crate::adjustment::{AdjustmentKey, Adjustments};
use crate::dump::*;
use crate::fake::Area;
use crate::ports::{Derivations, ExportQuery};
use crate::stock::{GuestMemory, StockDumpBuffer};
use crate::table::{Row, Step};

struct Scales;
impl Derivations for Scales {
    fn export_word(&mut self, _: Channel, _: ExportQuery) -> i16 { panic!("export word") }
    fn adjust_direction(&mut self, _: i16, _: u32, _: u32) -> i16 { panic!("adjust direction") }
    fn scaled_limit(&mut self, _: Channel, _: u32, _: u32) -> u16 { panic!("scaled limit") }
    fn channel_scale(&mut self, _: Channel) -> u16 { 77 }
    fn table_scale(&mut self, step: u32, _: Channel) -> u16 { 500 + step as u16 }
    fn curve_position(&mut self, _: i16, _: Channel) -> u32 { panic!("curve position") }
    fn curve_value(&mut self, _: u32, _: Channel) -> i16 { panic!("curve value") }
    fn channel_word(&mut self, _: Channel, _: u32, _: u32) -> i16 { panic!("channel word") }
    fn reference_word(&mut self, _: u32, _: Channel) -> i16 { panic!("reference word") }
    fn apply_reference(&mut self, _: i16, _: Channel) { panic!("apply reference") }
    fn lens_table(&mut self, _: u32, _: u32, _: u32) -> u32 { panic!("lens table") }
    fn lens_limits(&mut self, _: u32, _: [u32; 11]) -> u32 { panic!("lens limits") }
}

#[derive(Clone, Default)]
struct Buffer(Rc<RefCell<HashMap<u32, u8>>>);
impl GuestMemory for Buffer {
    fn byte(&self, at: u32) -> u8 { self.0.borrow().get(&at).copied().unwrap_or(0) }
    fn halfword(&self, at: u32) -> u16 { u16::from_le_bytes([GuestMemory::byte(self, at), GuestMemory::byte(self, at + 1)]) }
    fn word(&self, at: u32) -> u32 { u32::from_le_bytes(core::array::from_fn(|index| GuestMemory::byte(self, at + index as u32))) }
    fn set_byte(&self, at: u32, value: u8) { self.0.borrow_mut().insert(at, value); }
    fn set_halfword(&self, at: u32, value: u16) { for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.set_byte(at + index as u32, byte); } }
    fn set_word(&self, at: u32, value: u32) { for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.set_byte(at + index as u32, byte); } }
    fn fill(&self, _: u32, _: u8, _: u32) { panic!("fill") }
}
impl Adjustments for Buffer {
    fn byte(&self, key: AdjustmentKey) -> u8 { GuestMemory::byte(self, key.0) }
    fn lens_adjustment(&self) -> u16 { panic!("lens adjustment") }
    fn admits_unselected_mode(&self) -> bool { panic!("mode") }
}

#[derive(Default)]
struct Sources { mutate: Option<(Buffer, u32)> }
impl DumpDerivations for Sources {
    fn transformed_value(&mut self, step: Step, _: Row) -> u16 { 100 + step.0 as u16 }
    fn transformed_word(&mut self, step: Step, _: Row) -> u16 { 200 + step.0 as u16 }
    fn lens_position_flags(&mut self) -> u32 { 0x40 }
    fn selection_word(&mut self, _: Channel, mode: u32, _: u32, word: i16) -> u32 {
        if let Some((buffer, section)) = self.mutate.take() { buffer.set_word(section + 0x118, 0x100); buffer.set_word(section + 0x11c, 0x8001); }
        0x8000_0000 | mode << 16 | u32::from(word as u16)
    }
}

const BANK_SIZE: u32 = 0x3e0f82;
const FRAME: u32 = 0x0200_0000;
const SECTION: u32 = FRAME + 0xb174c;
fn area() -> Area {
    let area = Area::default().with(BANK_SIZE, 3).with_half(0x3e0319, 0xfffe).with_half(0x3e03e2, 7);
    let area = (1..).zip([0x3e04a9, 0x3e0572, 0x3e063b, 0x3e0704, 0x3e07f7, 0x3e08c0]).fold(area, |area, (index, at)| area.with_half(at + 2, 10 * index));
    [0x3e07cd, 0x3e07e2, 0x3e0989].into_iter().fold(area, |area, at| area.with_half(at + 2, 0x7fff))
}
fn export(area: &impl Adjustments, sources: &mut Sources, channel: Channel) -> Buffer {
    let memory = Buffer::default();
    StockDumpBuffer { address: FRAME, memory: memory.clone() }.export_configuration(area, &mut Scales, sources, channel);
    memory
}

#[test]
fn step_columns_terminate_without_touching_the_next_cell() {
    let memory = Buffer::default();
    memory.set_halfword(SECTION + 12, 0x9876);
    StockDumpBuffer { address: FRAME, memory: memory.clone() }.export_configuration(&area(), &mut Scales, &mut Sources::default(), Channel(1));
    assert_eq!(memory.word(SECTION), 0x53544c45);
    assert_eq!(memory.word(SECTION + 0x2a4), 0x4c414c45);
    assert_eq!([memory.halfword(SECTION + 8), memory.halfword(SECTION + 72), memory.halfword(SECTION + 144), memory.halfword(SECTION + 208), memory.halfword(SECTION + 372)], [0, 0, 102, 202, 502]);
    for at in [10, 74, 146, 210, 374] { assert_eq!(memory.halfword(SECTION + at), 0xfdfd); }
    assert_eq!(memory.halfword(SECTION + 12), 0x9876);
    assert_eq!(memory.word(SECTION + 0x16c), 77);
}

#[test]
fn selected_words_export_the_signed_base_and_wrapped_sums() {
    let memory = export(&area(), &mut Sources::default(), Channel(1));
    assert_eq!(memory.word(SECTION + 0x118), 0xffff_fffe);
    assert_eq!([memory.word(SECTION + 0x278), memory.word(SECTION + 0x27c), memory.word(SECTION + 0x280), memory.word(SECTION + 0x284), memory.word(SECTION + 0x288), memory.word(SECTION + 0x28c), memory.word(SECTION + 0x298), memory.word(SECTION + 0x29c)], [0x8002fffe, 0x80010007, 0x80020008, 0x80020012, 0x8002001c, 0x80020026, 0x80020030, 0x8002003a]);
    assert_eq!([memory.word(SECTION + 0x130), memory.word(SECTION + 0x134), memory.word(SECTION + 0x140)], [0x7fff; 3]);
    for at in [0x290, 0x294, 0x2a0] { assert_eq!(memory.word(SECTION + at), 0x80027ffd); }
}

#[test]
fn channels_above_nine_publish_zero_reference_words_and_selections() {
    let memory = export(&area(), &mut Sources::default(), Channel(10));
    for at in [0x130, 0x134, 0x140, 0x290, 0x294, 0x2a0] { assert_eq!(memory.word(SECTION + at), 0); }
}

#[test]
fn unset_optional_limits_export_all_ones_halfwords() {
    let memory = export(&Area::default(), &mut Sources::default(), Channel(1));
    for index in 0..30 { assert_eq!(memory.word(SECTION + 0x1b0 + 4 * index), 0xffff); }
    for index in 30..50 { assert_eq!(memory.word(SECTION + 0x1b0 + 4 * index), 0); }
}

#[test]
fn signed_record_values_shifted_floors_and_late_words_reach_the_frame() {
    let area = area().with_half(0x3f3d14 + 22, 0xfffe).with_half(0x3e15a6, 9).with_half(0x3e15a8, 13).with(0x3e162f, 3)
        .with_half(0x3e0b42, 0x101).with_half(0x3e0c0b, 0x203).with_half(0x3e0cd4, 0x405).with_half(0x3e0d9d, 0x807);
    let memory = export(&area, &mut Sources::default(), Channel(1));
    assert_eq!([memory.word(SECTION + 0x84), memory.word(SECTION + 0x88)], [0xffff_fffe; 2]);
    assert_eq!([memory.word(SECTION + 0x14c), memory.word(SECTION + 0x150)], [72, 104]);
    assert_eq!([memory.word(SECTION + 0x15c), memory.word(SECTION + 0x160), memory.word(SECTION + 0x164), memory.word(SECTION + 0x168)], [0x101, 0x203, 0x405, 0x807]);
}

#[test]
fn a_frame_alias_changes_the_next_adjustment_read() {
    let memory = Buffer::default();
    memory.set_halfword(0x3e0240, 0x1234);
    memory.set_halfword(0x3e0243, 0x5678);
    let frame = 0x3e0240 - 0xb174c - 0x10c;
    StockDumpBuffer { address: frame, memory: memory.clone() }.export_configuration(&memory, &mut Scales, &mut Sources::default(), Channel(10));
    assert_eq!(memory.word(0x3e0240), 0x1234);
    assert_eq!(memory.word(0x3e0244), 0x5600);
}

#[test]
fn a_step_publication_changes_the_live_bank_limit() {
    let memory = Buffer::default();
    memory.set_byte(BANK_SIZE, 3);
    let section = BANK_SIZE - 6;
    memory.set_halfword(section + 10, 0x9876);
    StockDumpBuffer { address: section - 0xb174c, memory: memory.clone() }.export_configuration(&memory, &mut Scales, &mut Sources::default(), Channel(10));
    assert_eq!(memory.halfword(section + 8), 0xfdfd);
    assert_eq!(memory.halfword(section + 10), 0x9876);
}

#[test]
fn selection_derivation_effects_reach_the_next_destination_read() {
    let memory = Buffer::default();
    let mut sources = Sources { mutate: Some((memory.clone(), SECTION)) };
    StockDumpBuffer { address: FRAME, memory: memory.clone() }.export_configuration(&area(), &mut Scales, &mut sources, Channel(1));
    assert_eq!(memory.word(SECTION + 0x278), 0x8002fffe);
    assert_eq!(memory.word(SECTION + 0x27c), 0x80018001);
    assert_eq!(memory.word(SECTION + 0x280), 0x8002010a);
    assert_eq!(memory.word(SECTION + 0x290), 0x800280ff);
}

#[test]
fn a_null_frame_accesses_neither_adjustments_nor_destination() {
    struct Absent;
    impl Adjustments for Absent {
        fn byte(&self, _: AdjustmentKey) -> u8 { panic!("absent adjustments"); }
        fn lens_adjustment(&self) -> u16 { panic!("absent lens adjustment"); }
        fn admits_unselected_mode(&self) -> bool { panic!("absent mode"); }
    }
    let memory = Buffer::default();
    StockDumpBuffer { address: 0, memory: memory.clone() }.export_configuration(&Absent, &mut Scales, &mut Sources::default(), Channel(1));
    assert!(memory.0.borrow().is_empty());
}
