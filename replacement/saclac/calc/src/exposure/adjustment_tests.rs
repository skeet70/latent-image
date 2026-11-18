use xm1_saclac_interface::calc::CalcPayload;
use xm1_saclac_interface::stock::StockCallbackWord;

use crate::exposure::adjustment::set_up;
use crate::exposure::context::{Adjustment, ExposureContext};
use crate::exposure::evaluation::request_row;
use crate::exposure::fakes::{placed, Posted, World};
use crate::exposure::ports::InPlace;
use crate::selection::{BufferSlot, RowIndex};
use crate::stock::property;
use crate::stock::shared;
use crate::stock::tables::PROGRAM_PROFILES;

fn world() -> World {
    let mut world = World::default();
    for (index, profile) in PROGRAM_PROFILES.iter().enumerate() {
        world.sensitivity_classes.insert(*profile, index as u8);
        world.offset_classes.insert(*profile, index as u8);
    }
    world.property_word(property::PROGRAM_LEVEL_LIMIT, 1000);
    world.property(property::TABLE_CEILING, 100);
    world.property(property::WINDOW_MARGIN, 5);
    world.property_word(property::ADJUSTMENT_RAISE_TARGET, 300);
    world.property(property::ADJUSTMENT_RAISE_LIMIT, 4);
    for (id, bound) in property::ADJUSTMENT_BANDS.into_iter().zip([400, 300, 200, 100]) {
        world.property_word(id, bound);
    }
    for (id, value) in property::ADJUSTMENT_SENSITIVITIES.into_iter().zip([60, 50, 40, 30, 20]) {
        world.property(id, value);
    }
    world
}

fn context(state: u32, level: u16) -> ExposureContext {
    let mut context = ExposureContext::default();
    context.outcome.adjusting = true;
    context.adjustment = Adjustment { level, state, code: 0 };
    context.levels.target = 100;
    context.snapshot.subjects.count = 2;
    for (index, subject) in context.snapshot.subjects.entries.iter_mut().enumerate() {
        subject.detail[4..12].copy_from_slice(&[index as u8 + 1; 8]);
    }
    context
}

fn prepared(world: &mut World, state: u32, level: u16) -> ExposureContext {
    let mut context = context(state, level);
    set_up(world, &mut context);
    context
}

fn step_levels(context: &ExposureContext) -> std::vec::Vec<(i16, i16, u32)> {
    context.program[..usize::from(context.span.last)].iter().map(|step| (step.position, step.level, step.profile)).collect()
}

#[test]
fn state_one_evaluates_the_lower_end_of_the_window_at_offset_four() {
    let context = prepared(&mut world(), 1, 400);
    assert_eq!(step_levels(&context), [(750, 350, 0x1b)]);
    let bank = context.selection;
    assert_eq!((bank.count, bank.filled, bank.any_range, bank.expected.highest), (1, 1, true, 0));
    assert_eq!((bank.candidates[0].value, bank.candidates[0].level, bank.candidates[0].state), (350, 0, 4));
    assert_eq!(context.adjustment.code, 3);
}

#[test]
fn state_two_evaluates_the_upper_end_at_offset_three() {
    let context = prepared(&mut world(), 2, 400);
    assert_eq!(step_levels(&context), [(850, 450, 0x1c)]);
    assert_eq!(context.selection.candidates[0].state, 3);
    assert_eq!(context.adjustment.code, 1);
}

#[test]
fn state_three_evaluates_both_ends_and_chains_their_profiles() {
    let context = prepared(&mut world(), 3, 400);
    assert_eq!(step_levels(&context), [(750, 350, 0x1b), (850, 450, 0x1c)]);
    assert_eq!(context.program[0].next_profile, context.program[1].profile);
    assert_eq!(context.program[1].next_profile, PROGRAM_PROFILES[6]);
    let states: std::vec::Vec<u32> = context.selection.candidates[..2].iter().map(|candidate| candidate.state).collect();
    assert_eq!(states, [4, 3]);
    assert_eq!((context.areas[1].profile, context.areas[1].level), (0x1c, 100));
    assert_eq!(context.adjustment.code, 5);
}

#[test]
fn state_zero_evaluates_nothing() {
    let context = prepared(&mut world(), 0, 400);
    assert_eq!((context.span.last, context.selection.count, context.adjustment.code), (0, 0, 0));
}

#[test]
fn a_step_below_the_raise_target_is_raised_by_at_most_the_limit_and_takes_its_band_profile() {
    let context = prepared(&mut world(), 1, 200);
    let step = context.program[0];
    assert_eq!((step.position, step.offset, step.level, step.profile), (550, 40, 190, 0x19));
    assert_eq!(context.selection.candidates[0].value, 150);
}

#[test]
fn a_window_narrower_than_the_margin_widens_below_the_ceiling_in_state_three() {
    let mut world = world();
    world.property(property::TABLE_CEILING, 97);
    let context = prepared(&mut world, 3, 980);
    assert_eq!(step_levels(&context).iter().map(|step| step.0).collect::<std::vec::Vec<_>>(), [920 + 400, 970 + 400]);
}

#[test]
fn a_window_narrower_than_the_margin_widens_above_the_floor_in_state_three() {
    let mut world = world();
    world.property(property::TABLE_FLOOR, 50);
    let context = prepared(&mut world, 3, 480);
    assert_eq!(step_levels(&context).iter().map(|step| step.0).collect::<std::vec::Vec<_>>(), [500 + 400, 550 + 400]);
}

#[test]
fn rows_take_the_next_buffer_slot_and_columns_the_snapshot_regions_without_a_count() {
    let context = prepared(&mut world(), 3, 400);
    let bank = context.selection;
    assert_eq!(bank.rows[0].slot, BufferSlot::new(1));
    assert_eq!(bank.rows[6].slot, BufferSlot::new(7));
    assert_eq!(bank.rows[7].slot, None);
    assert_eq!((bank.columns[6].count, bank.columns[6].regions[1]), (0, [2; 8]));
    assert_eq!(bank.columns[7], Default::default());
}

#[test]
fn the_row_requests_name_the_shifted_buffers_and_the_fixed_offsets() {
    let mut world = world();
    let mut context = prepared(&mut world, 3, 400);
    let mut posted = Posted::default();
    let callback = StockCallbackWord(0x00f1_ee10);
    request_row(&mut world, &mut posted, &mut context, RowIndex::new(0).unwrap(), callback).unwrap();
    request_row(&mut world, &mut posted, &mut context, RowIndex::new(1).unwrap(), callback).unwrap();
    let requests: std::vec::Vec<_> = posted
        .calc
        .iter()
        .map(|payload| match payload {
            CalcPayload::Evaluation { request, .. } => (request.row_buffer, request.column_count, request.direction, request.floor),
            other => panic!("not an evaluation request: {other:?}"),
        })
        .collect();
    let buffer = |slot| Some(shared::row_buffers(BufferSlot::new(slot).unwrap())[0]);
    assert_eq!(requests, [(buffer(1), 0, 4, -900), (buffer(2), 0, 3, -600)]);
}

#[test]
fn the_correction_names_the_snapshot_subjects_against_the_lens_reference_level() {
    let context = prepared(&mut world(), 1, 400);
    assert_eq!((context.correction.subjects, context.correction.summary), (Some(placed(InPlace::SnapshotSubjects)), Some(placed(InPlace::SnapshotTables))));
    assert_eq!(context.correction.reference, (1000 + 250 - 100) as u16);
}
