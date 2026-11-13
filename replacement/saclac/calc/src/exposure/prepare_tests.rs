use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::{group_14, group_27};

use crate::exposure::context::{Binding, ExposureContext};
use crate::exposure::fakes::{handle, World};
use crate::exposure::prepare::prepare;
use crate::policy::ExposureMode;
use crate::stock::exposure_limits::{parameter, ParameterField, PARAMETER_SETS};
use crate::stock::record::{FOCUS_RESULT_BYTES, SUBJECT_LIST_BYTES};
use crate::stock::tables::PROGRAM_PROFILES;
use crate::stock::{dump, property};

const FRAME: SharedAddress = SharedAddress(0x4000_0000);
const LENS_REFERENCE: u32 = 0x55;

/// A world whose program spans four steps, whose scene admits the adjustment path and whose
/// adjustment window is (-200, 300).
fn world() -> World {
    let mut world = World::default();
    for (index, profile) in PROGRAM_PROFILES.iter().enumerate() {
        world.profile_levels.insert(*profile, 100 * index as i16);
        world.sensitivity_classes.insert(*profile, index as u8);
    }
    world.property(parameter(PARAMETER_SETS[0], false, ParameterField::Upper), 30);
    world.near_steps.insert(0, 1);
    world.far_steps.insert(0, 1);
    world.property_word(property::PROGRAM_LEVEL_LIMIT, 1000);
    world.property_word(property::TARGET_LEVEL, 500);
    world.property(property::ADJUSTMENT_FLAGS, 1);
    world.configured_level = 500;
    world.property(property::START_MARGIN, 5);
    world.property(property::STEP_LIMIT, 10);
    world.property(property::UPPER_TOLERANCE, 2);
    world.property(property::LOWER_TOLERANCE, 3);
    world.property(property::FIRST_STEP, 255);
    world.property(property::scene_mask(0), 1);
    world.property_word(property::WEIGHT_LIMIT, 1000);
    world.property(property::window(property::WindowSource::Default).0, 50);
    world.property_word(property::level_step(3), 300);
    world.set(&group_00::AT_0045, 6);
    world
}

/// A record focus result taken at `reference` whose level is 50, with two subjects.
fn record_focus_result(world: &mut World, reference: u32) {
    let mut focus = [0; FOCUS_RESULT_BYTES];
    focus[0..4].copy_from_slice(&reference.to_le_bytes());
    focus[4..8].copy_from_slice(&1u32.to_le_bytes());
    for (index, weight) in [1u16, 2].into_iter().enumerate() {
        focus[0x3c + 2 * index..0x3e + 2 * index].copy_from_slice(&weight.to_le_bytes());
    }
    for (index, level) in [100u16, 20, 0, 30].into_iter().enumerate() {
        focus[0x54 + 2 * index..0x56 + 2 * index].copy_from_slice(&level.to_le_bytes());
    }
    world.set_shared(&group_14::FOCUS_RESULT, focus);
    let mut subjects = [0; SUBJECT_LIST_BYTES];
    subjects[0] = 2;
    world.set_shared(&group_27::AT_026C, subjects);
}

fn binding(mode: u32) -> Binding {
    Binding {
        mask: 2,
        mode,
        frame: Some(FRAME),
        lens_reference: LENS_REFERENCE,
        record: Some(handle()),
        snapshot_record: Some(handle()),
        ..Binding::default()
    }
}

/// A context left by an earlier exposure.
fn earlier() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.adjustment.code = 5;
    context.adjustment.state = 2;
    context.saved.start = 77;
    context.program[0].position = 9;
    context
}

#[test]
fn a_secondary_exposure_mode_is_bound_without_preparing_anything() {
    let mut world = world();
    let mut context = earlier();
    let binding = Binding { exposure_mode: ExposureMode(2), ..binding(0) };
    prepare(&mut world, &mut context, binding);
    assert_eq!(context.binding, binding);
    assert_eq!((context.adjustment.code, context.adjustment.state), (5, 2));
    assert!(world.stores.is_empty());
}

#[test]
fn a_primary_exposure_tags_the_frame_and_replaces_the_earlier_results() {
    let mut world = world();
    let mut context = earlier();
    prepare(&mut world, &mut context, binding(0));
    assert_eq!(world.u32_at(dump::section_tag(FRAME)), dump::SECTION_TAG);
    assert_eq!(world.u32_at(dump::end_tag(FRAME)), dump::END_TAG);
    assert_eq!(world.u32_at(dump::export(FRAME)), 0);
    assert!(!context.outcome.adjusting && !context.outcome.focus_reused);
    assert_eq!(context.adjustment, Default::default());
    assert_eq!((context.selection.count, context.evaluation.tolerance_above), (4, 20));
    assert_eq!(context.saved.start, 77);
}

#[test]
fn a_mode_one_exposure_without_mask_bit_one_reuses_the_focus_result_and_exports_it() {
    let mut world = world();
    let mut context = earlier();
    prepare(&mut world, &mut context, Binding { mask: 0, ..binding(1) });
    assert!(context.outcome.focus_reused);
    assert_eq!(world.u32_at(dump::export(FRAME)), 1);
}

#[test]
fn a_snapshot_at_the_bound_lens_reference_inside_the_window_takes_the_adjustment_path() {
    let mut world = world();
    record_focus_result(&mut world, LENS_REFERENCE);
    let mut context = earlier();
    prepare(&mut world, &mut context, binding(0));
    assert!(context.outcome.adjusting);
    assert_eq!((context.adjustment.state, context.selection.count, context.selection.any_range), (0, 0, true));
    assert_eq!(context.program[0].position, 9);
}

#[test]
fn a_snapshot_at_another_lens_reference_takes_the_ordinary_evaluation() {
    let mut world = world();
    record_focus_result(&mut world, LENS_REFERENCE + 1);
    let mut context = earlier();
    prepare(&mut world, &mut context, binding(0));
    assert!(!context.outcome.adjusting);
    assert_eq!((context.selection.count, context.selection.any_range), (4, false));
}
