use std::vec::Vec;

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::exposure::{Correction, CORRECTION_HALVES};

use crate::diagnostics::*;
use crate::exposure::fakes::World;
use crate::exposure::ports::{Properties, Property, StockMemory};
use crate::library_state;
use crate::scene_recognition::ports::Overlay;
use crate::stock::property::{ADJUSTMENT_FLAGS, CORRECTION_RATE};

const FRAME: SharedAddress = SharedAddress(0x4000_0000);
const JOINT_TABLES: u32 = 0x016c_73f0;
const SAMPLE_HISTORY: u32 = 0x016d_47a0;

#[derive(Default)]
struct Bench {
    world: World,
    invalidated: Vec<(u32, usize)>,
    texts: Vec<(Vec<u8>, u8, u8, u8, u8)>,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl StockMemory for Bench {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { self.world.read_u8(at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { self.world.read_u16(at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { self.world.read_u32(at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { self.world.write_u8(at, value) }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { self.world.write_u16(at, value) }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { self.world.write_u32(at, value) }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { self.world.fill(at, bytes, value) }
}

impl CacheMaintenance for Bench {
    fn invalidate(&mut self, at: SharedAddress, bytes: usize) { self.invalidated.push((at.0, bytes)); }
}

impl Overlay for Bench {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) { self.texts.push((text.to_vec(), column, row, foreground, background)); }
}

fn dumping() -> Bench {
    let mut bench = Bench::default();
    bench.world.property(ADJUSTMENT_FLAGS, 0x80);
    bench
}

fn frame_word(bench: &Bench, offset: u32) -> u32 { bench.world.u32_at(SharedAddress(FRAME.0 + offset)) }

fn put(bench: &mut Bench, at: u32, bytes: &[u8]) {
    for (index, byte) in bytes.iter().enumerate() {
        bench.world.put_u8(SharedAddress(at + index as u32), *byte);
    }
}

#[test]
fn the_joint_tables_are_exported_field_by_field_as_words_between_their_marks() {
    let mut bench = Bench::default();
    put(&mut bench, JOINT_TABLES, &[0x81]);
    put(&mut bench, JOINT_TABLES + 0x12c, &[1, 2, 0x34, 0x12, 3, 0xff, 0x80, 0x55, 0xfe, 0xff, 0x0a, 0x80]);
    put(&mut bench, JOINT_TABLES + 0x12c + 24 * 255 + 22, &[0x99, 0x88]);
    put(&mut bench, JOINT_TABLES + 0x192c, &[7, 8, 9]);
    put(&mut bench, JOINT_TABLES + 0x1c2c + 299, &[0xe0]);
    put(&mut bench, JOINT_TABLES + 0x1d58 + 24, &0x1234_5678u32.to_le_bytes());
    put(&mut bench, JOINT_TABLES + 0x1d74 + 0x12c + 6, &[0x7f]);
    put(&mut bench, JOINT_TABLES + 0x1d74 + 0x192c + 3 * 255 + 2, &[0x42]);
    put(&mut bench, JOINT_TABLES + 0x1d74 + 0x1c2c, &[0x11]);
    put(&mut bench, JOINT_TABLES + 0x1d74 + 0x1d58, &[0x21, 0x43]);
    export_joint_tables(&mut bench, FRAME);

    let out = |offset: u32| frame_word(&bench, 0xa_7a20 + offset);
    assert_eq!((out(0), out(4), out(0x9afc)), (0x5354_4f4a, 0x81, 0x4c41_4f4a));
    assert_eq!([0x4b4, 0x4b8, 0x4bc, 0x4c0, 0x4c4, 0x4c8, 0x4cc, 0x4d0].map(out), [1, 2, 0x1234, 3, u32::MAX, 0xffff_ff80, 0xffff_fffe, 0x800a]);
    assert_eq!(out(0x4b4 + 56 * 255 + 52), 0x8899);
    assert_eq!([0x3cb4, 0x3cb8, 0x3cbc].map(out), [7, 8, 9]);
    assert_eq!((out(0x48b4 + 4 * 299), out(0x4d64 + 24)), (0xe0, 0x1234_5678));
    assert_eq!(out(0x5230 + 20), 0x7f, "the second block's first mapping, its second signed byte");
    assert_eq!(out(0x8a30 + 12 * 255 + 8), 0x42);
    assert_eq!((out(0x9630), out(0x9ae0)), (0x11, 0x4321));
    assert_eq!(bench.world.fills, [(FRAME.0 + 0xa_7a20, 0x9b00, 0)], "the export area is cleared first");
    assert_eq!(bench.world.stores.len(), 2 + 2 * 4959, "every word of the area is written once after the clear");
}

#[test]
fn clearing_the_selection_needs_a_frame_and_the_dump_enabled() {
    let mut bench = Bench::default();
    clear_selection(&mut bench, Some(FRAME), true);
    let mut bench_without_frame = dumping();
    clear_selection(&mut bench_without_frame, None, true);
    assert!(bench.world.fills.is_empty() && bench_without_frame.world.fills.is_empty());
}

#[test]
fn a_primary_clear_zeroes_the_primary_sections_and_another_clear_the_secondary_ones() {
    let mut bench = dumping();
    clear_selection(&mut bench, Some(FRAME), true);
    let fills = |bench: &Bench| bench.world.fills.iter().map(|&(at, bytes, _)| (at - FRAME.0, bytes)).collect::<Vec<_>>();
    assert_eq!(fills(&bench), [(0x1_0768, 0x9b0), (0xfd78, 0x3c), (0x1_15f8, 0x1c), (0x1_1614, 0x4c), (0xfd40, 0x28)]);
    let mut bench = dumping();
    clear_selection(&mut bench, Some(FRAME), false);
    assert_eq!(fills(&bench), [(0xdfa8, 0x7c), (0xdf54, 0x3c), (0xf824, 0x1c), (0xf840, 0x4c), (0xe664, 0x28), (0xe68c, 0x28), (0xe6b4, 0x28), (0xe6dc, 0x28)]);
}

fn correction() -> Correction {
    let mut halves: [u16; CORRECTION_HALVES] = core::array::from_fn(|half| half as u16);
    halves[2] = 100;
    halves[3] = 0xfff6;
    halves[53] = 0xfff0;
    halves[54] = 0xfff0;
    halves[18] = 0x5678;
    halves[19] = 0x1234;
    halves[29] = 10;
    halves[33] = 4;
    Correction(halves)
}

fn exported(bench: &mut Bench, export: CorrectionExport) { export_correction(bench, Some(FRAME), &correction(), &export) }

const PLAIN: CorrectionExport = CorrectionExport { first_treatment: false, subjects: 3, clear_samples: true };

#[test]
fn the_correction_is_exported_relative_to_500_with_its_base_level_kept() {
    let mut bench = Bench::default();
    bench.world.property(Property(0x6585), 0xfffe);
    bench.world.property(Property(0x659d), 0xf0);
    bench.world.property(Property(0x659a), 0x123);
    exported(&mut bench, PLAIN);
    let word = |offset| frame_word(&bench, offset);
    assert_eq!(word(0xfd78), 100);
    assert_eq!([word(0xfd7c), word(0xfda8), word(0xfdac), word(0xfdb0)], [390, 414, 416, 417]);
    assert_eq!([word(0x1_1604), word(0x1_1608), word(0x1_1614)], [0xfff0, 0xffff_fff0, 0x1234_5678]);
    assert_eq!([word(0x1_1638), word(0xf894), word(0xf890)], [u32::MAX - 1, 0xffff_fff0, 0x123]);
    assert_eq!([word(0x1_1654), word(0x1_1658), word(0x1_165c)], [410, 404, (-6i32) as u32]);
    assert_eq!([0, 1, 2, 3, 9].map(|index| word(0xfd40 + 4 * index)), [438, 439, 440, 0, 0]);
}

#[test]
fn the_rate_is_exported_only_for_a_first_treatment_with_property_0x657d_set() {
    let mut bench = Bench::default();
    bench.world.property(Property(0x6584), 5);
    bench.world.property(Property(0x746b), 6);
    exported(&mut bench, CorrectionExport { first_treatment: true, ..PLAIN });
    assert_eq!((frame_word(&bench, 0x1_1648), frame_word(&bench, 0x1_1634)), (5, u32::MAX));
    bench.world.property(CORRECTION_RATE, 1);
    exported(&mut bench, PLAIN);
    assert_eq!((frame_word(&bench, 0x1_1648), frame_word(&bench, 0x1_1634)), (5, u32::MAX));
    exported(&mut bench, CorrectionExport { first_treatment: true, ..PLAIN });
    assert_eq!((frame_word(&bench, 0x1_1648), frame_word(&bench, 0x1_1634)), (6, 22));
}

#[test]
fn at_most_ten_subject_levels_are_exported() {
    let mut bench = Bench::default();
    exported(&mut bench, CorrectionExport { subjects: 12, ..PLAIN });
    assert_eq!([0, 9].map(|index| frame_word(&bench, 0xfd40 + 4 * index)), [438, 447]);
    assert_eq!(frame_word(&bench, 0xfd68), 0, "the word after the tenth level stays unwritten");
}

#[test]
fn the_correction_is_exported_without_the_dump_enabled_but_the_samples_are_not() {
    let mut bench = Bench::default();
    exported(&mut bench, CorrectionExport { clear_samples: false, ..PLAIN });
    assert_eq!(frame_word(&bench, 0xfd78), 100);
    assert!(bench.invalidated.is_empty() && bench.world.fills.is_empty());
    let mut bench = dumping();
    export_correction(&mut bench, None, &correction(), &PLAIN);
    assert!(bench.world.stores.is_empty() && bench.world.fills.is_empty());
}

#[test]
fn clearing_samples_zeroes_both_sample_areas() {
    let mut bench = dumping();
    exported(&mut bench, PLAIN);
    assert_eq!(bench.world.fills, [(FRAME.0 + 0xfdb8, 0x9b0, 0), (FRAME.0 + 0x1_0768, 0x9b0, 0)]);
    assert!(bench.invalidated.is_empty());
}

fn history(bench: &mut Bench, mark: u8, count: u8) {
    put(bench, SAMPLE_HISTORY, &[mark, count]);
    for slot in 0..20u32 {
        put(bench, SAMPLE_HISTORY + 2 + 2 * slot, &(slot as u16 + 1).to_le_bytes());
        put(bench, SAMPLE_HISTORY + 42 + 2 * slot, &(100 + slot as u16).to_le_bytes());
        put(bench, SAMPLE_HISTORY + 82 + 2 * slot, &(0xffff - slot as u16).to_le_bytes());
        put(bench, SAMPLE_HISTORY + 0x7c + 76 * slot, &(0x8000 + slot as u16).to_le_bytes());
        put(bench, SAMPLE_HISTORY + 0x7c + 76 * slot + 72, &(0x1000 + slot).to_le_bytes());
        put(bench, SAMPLE_HISTORY + 0x66c + 76 * slot + 18, &(0xff00 + slot as u16).to_le_bytes());
    }
}

const SAMPLED: CorrectionExport = CorrectionExport { clear_samples: false, ..PLAIN };

#[test]
fn each_sample_slot_is_exported_to_both_areas_after_the_history_is_invalidated() {
    let mut bench = dumping();
    bench.world.property(Property(0x6556), 33);
    history(&mut bench, 9, 2);
    exported(&mut bench, SAMPLED);
    assert_eq!(bench.invalidated, [(SAMPLE_HISTORY, 0xc5c)]);
    let record = |bench: &Bench, area: u32, slot: u32, offset: u32| frame_word(bench, area + 0x7c * slot + offset);
    assert_eq!(frame_word(&bench, 0xfdb4), 9);
    assert_eq!([0, 4, 0x60, 8, 0x78].map(|offset| record(&bench, 0xfdb8, 1, offset)), [601, 502, 33, 0xffff_8001, 0x1001]);
    assert_eq!([0, 4, 0x2c].map(|offset| record(&bench, 0x1_0768, 1, offset)), [498, 502, 0xff01]);
    assert_eq!(record(&bench, 0xfdb8, 2, 4), 0, "slots past the count stay unwritten");
}

#[test]
fn at_most_twenty_sample_slots_are_exported() {
    let mut bench = dumping();
    history(&mut bench, 0, 25);
    exported(&mut bench, SAMPLED);
    assert_eq!(frame_word(&bench, 0x1_0768 + 0x7c * 19 + 4), 520);
    assert_eq!(frame_word(&bench, 0x1_0768 + 0x7c * 20 + 4), 0);
}

#[test]
fn an_empty_history_exports_only_its_mark() {
    let mut bench = dumping();
    history(&mut bench, 7, 0);
    exported(&mut bench, SAMPLED);
    assert_eq!((frame_word(&bench, 0xfdb4), frame_word(&bench, 0xfdb8 + 4)), (7, 0));
}

#[test]
fn the_rate_status_is_drawn_in_the_first_column_with_the_second_blanked() {
    let mut bench = Bench::default();
    draw_rate_status(&mut bench, 123, RateStatus::from_code(1), false);
    let expected: [(&[u8], u8, u8, u8, u8); 4] = [(b"12.3  ", 2, 5, 1, 10), (b"ACT   ", 2, 6, 1, 10), (b"----- ", 8, 5, 1, 7), (b"----- ", 8, 6, 1, 7)];
    assert_eq!(bench.texts, expected.map(|(text, column, row, foreground, background)| (text.to_vec(), column, row, foreground, background)));
}

#[test]
fn the_rate_status_in_the_second_column_takes_its_status_colour() {
    let mut bench = Bench::default();
    draw_rate_status(&mut bench, 5, RateStatus::from_code(0), true);
    draw_rate_status(&mut bench, 40, RateStatus::from_code(3), true);
    draw_rate_status(&mut bench, 99_990, RateStatus::from_code(2), true);
    let expected: [(&[u8], u8); 6] = [(b"0.5   ", 7), (b"STIL  ", 7), (b"4.0   ", 4), (b"NO F  ", 4), (b"9999.0", 4), (b"FAIL  ", 4)];
    assert_eq!(bench.texts, expected.map(|(text, colour)| (text.to_vec(), 8, if text[0].is_ascii_digit() { 5 } else { 6 }, 1, colour)));
}

#[test]
fn a_rate_longer_than_six_characters_is_drawn_whole() {
    let mut bench = Bench::default();
    draw_rate_status(&mut bench, u32::MAX, RateStatus::Still, true);
    assert_eq!(bench.texts[0].0, b"429496729.5");
}

#[test]
fn starting_arms_calc_and_marks_the_first_joint_extras_unset_and_stopping_disarms_calc() {
    let mut bench = Bench::default();
    library_state::start(&mut bench);
    assert_eq!(bench.world.u32_at(SharedAddress(0x016c_2370)), 1);
    assert_eq!(bench.world.fills, [(JOINT_TABLES + 0x1c2c, 300, 0xe0)]);
    library_state::stop(&mut bench);
    assert_eq!(bench.world.u32_at(SharedAddress(0x016c_2370)), 0);
}
