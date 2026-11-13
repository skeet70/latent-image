use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::context_trace::*;
use crate::exposure::context::{ExposureContext, ExposureStep, Subject};
use crate::exposure::fakes::{self, World};
use crate::selection::RowPhase;

const FRAME: SharedAddress = SharedAddress(0x4000_0000);
const STATISTICS_BLOCK: SharedAddress = SharedAddress(0x4100_0000);
const REGIONS: SharedAddress = SharedAddress(0x4200_0000);

impl ChannelPixels for World {
    fn channel_pixels(&mut self, pixels: u32) -> [u32; 4] { [pixels / 4, pixels / 2, pixels / 4, 0] }
}

fn frame_word(world: &World, offset: u32) -> u32 { world.u32_at(SharedAddress(FRAME.0 + offset)) }

fn put_word(world: &mut World, at: u32, value: u32) {
    world.put_u16(SharedAddress(at), value as u16);
    world.put_u16(SharedAddress(at + 2), (value >> 16) as u16);
}

fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.frame = Some(FRAME);
    context.binding.record = Some(fakes::handle());
    context
}

#[test]
fn a_context_without_frame_stores_nothing() {
    let mut world = World::default();
    let mut context = context();
    context.binding.frame = None;
    trace(&mut world, &context);
    assert!(world.stores.is_empty() && world.fills.is_empty());
}

#[test]
fn program_steps_are_traced_up_to_the_span_and_cleared_after() {
    let mut world = World::default();
    world.lens_base_levels.insert(3, -20);
    world.level_offsets.insert(0x12, 9);
    put_word(&mut world, FRAME.0 + 0x4a78 + 0x48, 0xdead);
    let mut context = context();
    context.levels.target = 640;
    context.span.last = 2;
    context.program[0] = ExposureStep { lens_step: 3, level: -5, converted: 0xfff0, offset: 7, profile: 0x11, next_profile: 0x12, ..ExposureStep::default() };
    context.program[1] = ExposureStep { lens_step: 4, profile: 0x13, ..ExposureStep::default() };
    context.program[2] = ExposureStep { lens_step: 5, ..ExposureStep::default() };
    trace(&mut world, &context);
    let first: std::vec::Vec<u32> = (0..9).map(|word| frame_word(&world, 0x4a78 + 4 * word)).collect();
    assert_eq!(first, [3, -20i32 as u32, -5i32 as u32, -16i32 as u32, 640, 7, 9, 0x11, 0x12]);
    assert_eq!((frame_word(&world, 0x4a9c), frame_word(&world, 0x4a9c + 0x1c)), (4, 0x13));
    assert_eq!(frame_word(&world, 0x4a78 + 0x48), 0);
}

#[test]
fn the_request_measured_pairs_pack_first_halfword_high() {
    let mut world = World::default();
    let mut context = context();
    context.evaluation.measured = [[0x1234, 0x5678], [1, 2], [0x0abc, 0x0def]];
    trace(&mut world, &context);
    assert_eq!((frame_word(&world, 0x4bbc), frame_word(&world, 0x4bc0)), (0x1234_5678, 0x0abc_0def));
}

#[test]
fn an_invalid_snapshot_traces_the_empty_focus_result() {
    let mut world = World::default();
    put_word(&mut world, FRAME.0 + 0xde54 + 0xfc, 0xdead);
    put_word(&mut world, FRAME.0 + 0x35dc, 0xdead);
    let mut context = context();
    context.snapshot.focus.reference = 9;
    trace(&mut world, &context);
    let focus: std::vec::Vec<u32> = (0..12).map(|word| frame_word(&world, 0x35dc + 4 * word)).collect();
    assert_eq!(focus, [0, 58, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(frame_word(&world, 0xde54 + 0xfc), 0);
}

#[test]
fn a_valid_snapshot_traces_its_focus_result_and_the_work_table() {
    let mut world = World::default();
    world.header(SharedHeaderByte::At31, 5);
    world.set(&group_00::AT_002E, 0x22);
    world.calibration_indices.insert(7, 70);
    world.lens_base_levels.insert(70, -3);
    world.put_u16(SharedAddress(0x5ffc_0140), 0xfffe);
    world.put_u16(SharedAddress(0x5ffc_01be), 12);
    let mut context = context();
    context.snapshot.valid = 1;
    context.snapshot.offset = -40;
    context.snapshot.focus.reference = 9;
    context.snapshot.focus.lens = 7;
    context.snapshot.focus.terms = [1, 2, 3, 4, 5, 0xffff];
    trace(&mut world, &context);
    let focus: std::vec::Vec<u32> = (0..12).map(|word| frame_word(&world, 0x35dc + 4 * word)).collect();
    assert_eq!(focus, [9, 5, 0x22, 460, 70, -3i32 as u32, 1, 2, 3, 4, u32::MAX, 5]);
    assert_eq!((frame_word(&world, 0xde54), frame_word(&world, 0xde54 + 0xfc)), (-2i32 as u32, 12));
}

#[test]
fn subjects_are_traced_with_the_snapshot_limits() {
    let mut world = World::default();
    let mut context = context();
    let mut detail = [0; 100];
    detail[..12].copy_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0]);
    context.snapshot.subjects.count = 1;
    context.snapshot.subjects.entries[0] = Subject { id: 0x55, detail };
    context.snapshot.limits = [10, 11, 12, 13, 14, -1];
    context.snapshot.bounds = [-20, 21, 22, 23];
    context.snapshot.limit_word = 0x77;
    trace(&mut world, &context);
    assert_eq!(frame_word(&world, 0x11118), 1);
    let subject: std::vec::Vec<u32> = (0..4).map(|word| frame_word(&world, 0x1111c + 4 * word)).collect();
    assert_eq!(subject, [0x55, 1, 3, 2]);
    let limits: std::vec::Vec<u32> = (0..10).map(|word| frame_word(&world, 0x111bc + 4 * word)).collect();
    assert_eq!(limits, [11, 12, 13, 10, -20i32 as u32, 21, 22, 14, 0xffff, 0x77]);
}

#[test]
fn no_subjects_mark_every_limit_unknown() {
    let mut world = World::default();
    trace(&mut world, &context());
    assert!((0..10).all(|word| frame_word(&world, 0x111bc + 4 * word) == u32::MAX));
}

fn evaluation_context(world: &mut World) -> ExposureContext {
    let mut context = context();
    context.span.first = 2;
    context.span.low = -2;
    context.span.high = 300;
    context.span.start = 40;
    context.span.end = 280;
    context.span.first_step = 15;
    context.span.step = -6;
    context.selection.count = 3;
    context.selection.rows[0].phase = RowPhase::Applied;
    context.selection.rows[1].phase = RowPhase::Applied;
    context.selection.rows[2].phase = RowPhase::Requested;
    context.selection.rows[1].enabled = true;
    context.selection.columns[0].count = 1;
    context.evaluation.column_count = 1;
    context.evaluation.column_regions = Some(REGIONS);
    context.evaluation.statistics = [1, 0xffff, 3, 4];
    put_word(world, REGIONS.0, 4);
    put_word(world, REGIONS.0 + 4, 8);
    let [buffer, tail] = [0x5ffb_6120 + 0x1150, 0x5ffb_7120 + 0x1150];
    put_word(world, buffer + 0xffc, 0xaa);
    put_word(world, tail + 0xc, 0xbb);
    let sums = 0x5ffb_7130 + 0x1150;
    put_word(world, sums, 800);
    put_word(world, sums + 8, 0);
    put_word(world, sums + 12, 1);
    put_word(world, sums + 24, 5);
    context
}

#[test]
fn the_evaluation_path_traces_its_span() {
    let mut world = World::default();
    let context = evaluation_context(&mut world);
    trace(&mut world, &context);
    assert_eq!(frame_word(&world, 0x4a70), 7);
    assert_eq!(frame_word(&world, 0x4a74), (-2i32 << 16) as u32 + 300);
    let span: std::vec::Vec<u32> = (0..6).map(|word| frame_word(&world, 0x4b98 + 4 * word)).collect();
    assert_eq!(span, [2, 3, 40, 280, 15, -6i32 as u32]);
}

#[test]
fn each_enabled_row_fills_the_next_slot_with_its_buffers_and_channel_levels() {
    let mut world = World::default();
    let context = evaluation_context(&mut world);
    put_word(&mut world, FRAME.0 + 0x4bc4 + 0x1ffc, 0xdead);
    trace(&mut world, &context);
    assert_eq!((frame_word(&world, 0x4bc4 + 0xffc), frame_word(&world, 0xcbc4 + 0xc)), (0xaa, 0xbb));
    let levels: std::vec::Vec<u32> = (0..4).map(|word| frame_word(&world, 0xe164 + 4 * word)).collect();
    assert_eq!(levels, [100, 0x1000_0000, 0, u32::MAX]);
    assert_eq!(frame_word(&world, 0x4bc4 + 0x1ffc), 0);
    let statistics: std::vec::Vec<u32> = (0..4).map(|word| frame_word(&world, 0xcc44 + 4 * word)).collect();
    assert_eq!(statistics, [1, u32::MAX, 3, 4]);
}

#[test]
fn without_columns_every_channel_level_is_cleared() {
    let mut world = World::default();
    let mut context = evaluation_context(&mut world);
    context.selection.columns[0].count = 0;
    trace(&mut world, &context);
    assert_eq!(frame_word(&world, 0xe164), 0);
}

#[test]
fn region_sizes_are_kept_by_row_until_the_unused_slots_clear_those_rows() {
    let mut world = World::default();
    let mut context = evaluation_context(&mut world);
    trace(&mut world, &context);
    assert_eq!(frame_word(&world, 0xe024 + 0x28), 0);
    context.selection.rows[0].enabled = true;
    trace(&mut world, &context);
    assert_eq!((frame_word(&world, 0xe024), frame_word(&world, 0xe024 + 0x28)), (4 | 8 << 16, 4 | 8 << 16));
}

#[test]
fn the_adjustment_path_traces_the_statistics_block_first_and_its_code() {
    let mut world = World::default();
    let mut context = evaluation_context(&mut world);
    context.outcome.adjusting = true;
    context.adjustment.state = 1;
    context.adjustment.code = 5;
    context.snapshot.focus.statistics = Some(STATISTICS_BLOCK);
    context.snapshot.subjects.count = 1;
    context.snapshot.table_block[..2].copy_from_slice(&[2, 3]);
    put_word(&mut world, STATISTICS_BLOCK.0 + 0x10, 0x11);
    put_word(&mut world, STATISTICS_BLOCK.0 + 0x1004, 0x22);
    put_word(&mut world, STATISTICS_BLOCK.0 + 0x1010 + 8, 9);
    put_word(&mut world, FRAME.0 + 0x4b98, 0xdead);
    trace(&mut world, &context);
    assert_eq!((frame_word(&world, 0x4a70), frame_word(&world, 0x4b98)), (5, 0));
    assert_eq!((frame_word(&world, 0x4bc4 + 0x10), frame_word(&world, 0xcbc4 + 4)), (0x11, 0x22));
    assert_eq!((frame_word(&world, 0xe024), frame_word(&world, 0xe168)), (2 | 3 << 16, 3));
    assert_eq!((frame_word(&world, 0x5bc4 + 0xffc), frame_word(&world, 0xe164 + 0xa0)), (0xaa, 100));
}

#[test]
fn the_adjustment_path_keeps_the_path_word_until_its_state_is_set() {
    let mut world = World::default();
    put_word(&mut world, FRAME.0 + 0x4a70, 0x33);
    let mut context = context();
    context.outcome.adjusting = true;
    trace(&mut world, &context);
    assert_eq!(frame_word(&world, 0x4a70), 0x33);
}
