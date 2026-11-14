use std::vec::Vec;

use super::arguments::{auto as auto_arguments, mvrec as mvrec_arguments, raid0 as raid0_arguments, GuestMemory};
use crate::driver::{refuse_parameter, Bank, Client, Event, Fault, Operation, Refusal, Register, SessionId, Unit};
use crate::fake::{Effect, FakeIsp};
use crate::{auto, mvrec, raid0};

/// Little-endian guest memory from `base`.
struct Memory {
    base: u32,
    words: Vec<u32>,
}

impl Memory {
    fn new(base: u32, words: &[u32]) -> Self { Self { base, words: words.to_vec() } }

    fn index(&self, address: u32) -> usize { ((address - self.base) / 4) as usize }
}

impl GuestMemory for Memory {
    fn byte(&self, address: u32) -> u8 { (self.words[self.index(address)] >> (8 * (address % 4))) as u8 }

    fn halfword(&self, address: u32) -> u16 { (self.words[self.index(address)] >> (8 * (address % 4))) as u16 }

    fn word(&self, address: u32) -> u32 { self.words[self.index(address)] }

    fn words(&self, address: u32, count: usize) -> &[u32] { &self.words[self.index(address)..][..count] }

    fn table<const N: usize>(&self, address: u32) -> &[u32; N] { self.words(address, N).try_into().unwrap() }
}

/// Halfwords packed two per word, the first in the low half.
fn halves(values: &[u16]) -> Vec<u32> {
    values.chunks(2).map(|pair| u32::from(pair[0]) | u32::from(*pair.get(1).unwrap_or(&0)) << 16).collect()
}

const BLOCK: u32 = 0x1000;

fn parameter(unit: Unit, operation: u32) -> Event {
    Event::Refused { client: Client::REC, unit, operation: Operation(operation), fault: Fault::Parameter }
}

#[test]
fn a_window_block_programs_the_registers_its_stock_entry_writes() {
    let memory = Memory::new(BLOCK, &[0x0456_0123, 0x0100_3412, 0x3000_0200]);
    let mut isp = FakeIsp::new();
    raid0::set_window(&mut isp, Client::REC, raid0_arguments::window(&memory, BLOCK)).unwrap();
    let r = Register::Raid0;
    assert_eq!(isp.writes(), [(r(0x64), 0x0123_0456), (r(0x68), 0x0012_0034), (r(0x6c), 0x0100_0200), (r(0x70), 0x3000)]);
}

#[test]
fn a_block_layout_keeps_its_limits_out_of_the_registers() {
    let mut words = vec![0x0001_2340];
    words.extend(halves(&[0x100, 1, 2, 3, 4, 0x3fff, 0x3fff, 0x3fff, 0x3fff]));
    let memory = Memory::new(BLOCK, &words);
    let mut isp = FakeIsp::new();
    raid0::set_block(&mut isp, Client::REC, raid0_arguments::block(&memory, BLOCK)).unwrap();
    let r = Register::Raid0;
    assert_eq!(isp.writes(), [(r(0x48), 0x1_2340), (r(0x4c), 0x100), (r(0x50), 0x0002_0001), (r(0x54), 0x0004_0003)]);
}

#[test]
fn the_point_list_is_read_through_the_block_and_shifted() {
    let list = BLOCK + 0x10;
    let memory = Memory::new(BLOCK, &[list, 0x0001_0002, 0x0000_0002, 0, 0x0010_0020, 0x0030_c040]);
    let (points, shift) = raid0_arguments::points(&memory, BLOCK).unwrap();
    assert_eq!(points, [0x0010_0020, 0x0030_c040]);
    let mut isp = FakeIsp::new();
    raid0::set_points(&mut isp, Client::REC, points, shift).unwrap();
    let r = Register::Raid0;
    assert_eq!(isp.writes()[..2], [(r(0x100), 0x0012_0021), (r(0x104), 0x0032_c041)]);
}

#[test]
fn an_overlong_list_is_refused_without_reading_it() {
    let memory = Memory::new(BLOCK, &[0xdead_0000, 0x0000_0401, 0]);
    let fault = raid0_arguments::points(&memory, BLOCK).unwrap_err();
    let mut isp = FakeIsp::new();
    assert_eq!(refuse_parameter(&mut isp, Client::REC, None, fault), Refusal::Parameter);
    assert_eq!(isp.logs(), [parameter(Unit::Raid0, 0x16)]);
    let memory = Memory::new(BLOCK, &[0xdead_0000, 0x0000_0011]);
    assert!(raid0_arguments::pairs(&memory, BLOCK).is_err());
}

#[test]
fn a_switch_word_other_than_zero_or_one_is_a_parameter_fault() {
    assert_eq!(raid0_arguments::gate(1), Ok(true));
    assert_eq!(raid0_arguments::output(0), Ok(false));
    let fault = raid0_arguments::gate(2).unwrap_err();
    let mut isp = FakeIsp::new();
    assert_eq!(refuse_parameter(&mut isp, Client::REC, None, fault), Refusal::Parameter);
    assert_eq!(isp.take(), [Effect::Note(Unit::Raid0, Operation(0x14)), Effect::Log(parameter(Unit::Raid0, 0x14))]);
    assert_eq!(raid0_arguments::gates([1, 2]).map_err(|fault| fault.operation()), Err(Operation(8)));
    assert_eq!(raid0_arguments::path([4, 1]).map_err(|fault| fault.operation()), Err(Operation(9)));
    assert_eq!(raid0_arguments::path([3, 1]), Ok(raid0::Path { source: raid0::PathSource::Fourth, enabled: true }));
    assert_eq!(raid0_arguments::mode(3, 5).map_err(|fault| fault.operation()), Err(Operation(0x21)));
    assert_eq!(raid0_arguments::stage_mode(raid0::Stage::Second, 3).map_err(|fault| fault.operation()), Err(Operation(5)));
    assert_eq!(raid0_arguments::stage_mode(raid0::Stage::Fourth, 2), Ok(raid0::StageMode::Alternate));
}

#[test]
fn a_division_mode_word_above_three_is_refused() {
    let memory = Memory::new(BLOCK, &[4, 3]);
    assert_eq!(raid0_arguments::division(&memory, BLOCK).map_err(|fault| fault.operation()), Err(Operation(0x12)));
    let memory = Memory::new(BLOCK, &[3, 0xf]);
    assert_eq!(raid0_arguments::division(&memory, BLOCK), Ok(raid0::Division { mode: raid0::DivisionMode::Fourth, count: 15 }));
}

#[test]
fn the_second_window_and_extent_share_one_block() {
    let memory = Memory::new(BLOCK, &halves(&[0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x0201, 0x1fff]));
    let mut isp = FakeIsp::new();
    raid0::set_second_window(&mut isp, Client::REC, raid0_arguments::second_window(&memory, BLOCK)).unwrap();
    raid0::set_second_extent(&mut isp, Client::REC, raid0_arguments::second_extent(&memory, BLOCK)).unwrap();
    let r = Register::Raid0;
    assert_eq!(isp.writes(), [
        (r(0x78), 0x0010_0020), (r(0x84), 0x0102_1fff),
        (r(0x7c), 0x0030_0040), (r(0x80), 0x0050_0060), (r(0x84), 0x0102_1fff),
    ]);
}

#[test]
fn table_arguments_point_at_their_tables() {
    let mut words = vec![BLOCK + 8, BLOCK + 8 + 14 * 4];
    words.extend(1..=28);
    let memory = Memory::new(BLOCK, &words);
    let [first, second] = raid0_arguments::rotated(&memory, BLOCK);
    assert_eq!((first[0], first[13], second[0], second[13]), (1, 14, 15, 28));
}

#[test]
fn transform_blocks() {
    let memory = Memory::new(BLOCK, &[1, 0x0020_0010]);
    assert_eq!(mvrec_arguments::geometry(&memory, BLOCK), Ok(mvrec::Geometry { enabled: true, first: 0x10, second: 0x20 }));
    let memory = Memory::new(BLOCK, &[2, 0]);
    assert_eq!(mvrec_arguments::geometry(&memory, BLOCK).map_err(|fault| (fault.unit(), fault.operation())), Err((Unit::Mvrec, Operation(7))));
    let memory = Memory::new(BLOCK, &[0x0002_0001, 3]);
    assert_eq!(mvrec_arguments::offset(&memory, BLOCK, mvrec::Offset::First), [1, 2]);
    assert_eq!(mvrec_arguments::offset(&memory, BLOCK, mvrec::Offset::Second), [0x0002_0001, 3]);
    let memory = Memory::new(BLOCK, &[0x0200_0100, 0x0000_2004]);
    assert_eq!(mvrec_arguments::window(&memory, BLOCK), mvrec::Window { position: [0x100, 0x200], repeat: 4, count: 0x20 });
}

#[test]
fn a_readback_enable_word_other_than_one_disables_and_a_wide_mode_is_refused() {
    let memory = Memory::new(BLOCK, &[2, 0x40, 3, 4]);
    let readback = mvrec_arguments::readback(&memory, BLOCK).unwrap();
    assert_eq!(readback, mvrec::Readback { enabled: false, region: 0x40, source: mvrec::ReadbackSource::Fourth, mode: mvrec::ReadbackMode::Fifth });
    let mut isp = FakeIsp::new();
    mvrec::set_readback(&mut isp, Client::REC, Bank::Second, readback).unwrap();
    assert_eq!(isp.writes(), [(Register::Mvrec(Bank::Second, 0xb8), 0x0040_0034)]);
    let memory = Memory::new(BLOCK, &[1, 0, 0, 5]);
    assert_eq!(mvrec_arguments::readback(&memory, BLOCK).map_err(|fault| fault.operation()), Err(Operation(0x15)));
}

#[test]
fn an_auto_refusal_checks_the_session_first() {
    let memory = Memory::new(BLOCK, &[0x0302, 1, 2, 3, 1]);
    let fault = auto_arguments::selectors(&memory, BLOCK).unwrap_err();
    let mut isp = FakeIsp::new();
    assert_eq!(refuse_parameter(&mut isp, Client::REC, Some(SessionId(7)), fault), Refusal::Parameter);
    let resource = Event::Refused { client: Client::REC, unit: Unit::Auto, operation: Operation(6), fault: Fault::ResourceId };
    assert_eq!(isp.logs(), [resource, parameter(Unit::Auto, 6)]);
}

#[test]
fn selector_fields_and_source() {
    let memory = Memory::new(BLOCK, &[0x0302, 0xff, 0x6, 2, 9]);
    let selectors = auto_arguments::selectors(&memory, BLOCK).unwrap();
    assert_eq!(selectors, auto::Selectors { fields: [2, 3, 0xff, 6], source: auto::Source::Third, source_enabled: true });
}

#[test]
fn grid_and_bank_blocks() {
    let memory = Memory::new(BLOCK, &halves(&[0x10, 0x0004, 0x20, 0x30, 0x0008, 0x40, 0x50, 0x60]));
    let span = |start, blocks, size| auto::Span { start, blocks, size };
    assert_eq!(auto_arguments::grid(&memory, BLOCK), auto::Grid { spans: [span(0x10, 4, 0x20), span(0x30, 8, 0x40)], gaps: [0x50, 0x60] });
    let words: Vec<u32> = (0..20).map(|index| (index + 1) << 16 | index).collect();
    let memory = Memory::new(BLOCK, &words);
    let bank = auto_arguments::bank(&memory, BLOCK);
    let segment = |position, length| auto::Segment { position, length };
    assert_eq!(bank[0], [segment(0, 1), segment(10, 11)]);
    assert_eq!(bank[9], [segment(9, 10), segment(19, 20)]);
    let memory = Memory::new(BLOCK, &halves(&[0x100, 0x0003, 0x20, 0x40, 0x50]));
    assert_eq!(auto_arguments::second_window(&memory, BLOCK), auto::Window::Second(span(0x100, 3, 0x20), segment(0x40, 0x50)));
    let memory = Memory::new(BLOCK, &halves(&[0x40, 0x50, 0x100, 0x0003, 0x20]));
    assert_eq!(auto_arguments::third_window(&memory, BLOCK), auto::Window::Third(segment(0x40, 0x50), span(0x100, 3, 0x20)));
}
