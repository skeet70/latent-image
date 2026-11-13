use xm1_saclac_interface::exposure::CalculationInput;
use xm1_saclac_profile_records::stock::record::{group_00, group_01};
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::exposure::calculation::{build_input, resume_input, PlanInput};
use crate::exposure::context::{ExposureContext, Levels};
use crate::exposure::fakes::{handle, World};

const RECORD_REFERENCE: u32 = 0x77;

/// A record whose base levels are 100 and 20 and whose lens reference offset is 30.
fn world() -> World {
    let mut world = World::default();
    world.set(&group_01::AT_00BC, RECORD_REFERENCE);
    world.reference_offsets.insert(RECORD_REFERENCE, 30);
    world.set(&group_01::AT_00B8, 100);
    world.set(&group_01::AT_00BA, 20);
    world
}

fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.binding.snapshot_record = Some(handle());
    context.binding.lens_reference = 0x55;
    context.binding.option = 1;
    context.binding.refinement_pass = true;
    context.outcome.first_treatment = true;
    context.levels = Levels { target: -5, start: 300, measured: 400, curve: 0x6431, unmeasured: true, policy: 6 };
    context
}

fn built(world: &mut World, context: &mut ExposureContext) -> CalculationInput {
    build_input(world, context, [7, 8], PlanInput { kind: 2, exposure: [11, 12, 13, 14, 15] }, true);
    context.calculation
}

#[test]
fn an_input_carries_the_exposure_levels_the_binding_and_the_plan() {
    let input = built(&mut world(), &mut context());
    assert_eq!((input.selection, input.lens_reference, input.option, input.area), (true, 0x55, 1, [7, 8]));
    assert_eq!((input.target_word, input.target_half), (0x80, 0));
    assert_eq!((input.target_level, input.start_level, input.measured_level, input.level_profile, input.unmeasured, input.level_policy), (-5, 300, 400, 0x6431, true, 6));
    assert!(input.refinement_pass && input.first_treatment);
    assert_eq!((input.plan_kind, input.plan_exposure), (2, [11, 12, 13, 14, 15]));
    assert_eq!(input.record_lens_reference, RECORD_REFERENCE);
}

#[test]
fn the_metered_level_follows_the_record_style_and_is_relative_to_the_lens() {
    let mut world = world();
    world.set(&group_01::AT_0092, 200);
    world.set(&group_01::AT_0094, 400);
    world.set(&group_01::AT_00A8, 240);
    for (style, level) in [(1u8, 400i16), (2, 200), (0, 240), (3, 240)] {
        world.set(&group_00::AT_0043, style);
        let mut context = context();
        let input = built(&mut world, &mut context);
        assert_eq!((context.metered_level, input.metering_level), (level - 30, level as u16 + 90), "style {style}");
    }
}

#[test]
fn the_grid_is_three_by_three_unless_the_record_selects_its_measurement_or_the_exposure_is_live() {
    let mut world = world();
    let input = built(&mut world, &mut context());
    assert_eq!((input.grid, input.grid_steps), ([3, 3], [0, 0]));
    let mut live = context();
    live.binding.live = true;
    let input = built(&mut world, &mut live);
    assert_eq!((input.grid, input.grid_steps), ([1, 1], [0, 0]));
    world.select(MeasurementSelection::Record);
    world.set(&group_00::AT_003D, 4);
    world.grid_steps.insert(4, [0, 3]);
    let input = built(&mut world, &mut context());
    assert_eq!((input.grid, input.grid_steps, input.grid_layout), ([1, 4], [0, 3], 4));
}

#[test]
fn the_levels_stand_on_the_record_base_and_the_fifth_is_unset_while_its_flag_is_clear() {
    let mut world = world();
    for (field, value) in [(&group_01::AT_00AE, 1i16), (&group_01::AT_00AC, 2), (&group_01::AT_00A4, 3), (&group_01::AT_00A6, 4), (&group_01::AT_00AA, 5), (&group_01::AT_00B0, 6), (&group_01::AT_00B2, 7)] {
        world.set(field, value);
    }
    let input = built(&mut world, &mut context());
    assert_eq!(input.levels, [91, 92, 93, 94, 0x7fff, 92, 91, 96, 97]);
    world.set(&group_01::AT_00C4, 1);
    assert_eq!(built(&mut world, &mut context()).levels[4], 95);
}

#[test]
fn the_last_level_is_the_alternative_only_while_the_scene_adjustment_is_enabled() {
    let mut world = world();
    world.set(&group_01::AT_00A4, 3);
    world.set(&group_01::AT_00B2, 7);
    assert_eq!(built(&mut world, &mut context()).levels[8], 97);
    world.header(SharedHeaderByte::At46, 1);
    let mut unrefined = context();
    unrefined.binding.refinement_pass = false;
    assert_eq!(built(&mut world, &mut unrefined).levels[8], 93);
}

#[test]
fn a_resumed_input_moves_its_levels_to_the_new_lens_reference() {
    let mut world = world();
    world.reference_offsets.insert(0x99, 10);
    let saved = CalculationInput { metering_level: 100, levels: [1, 2, 3, 4, 0x7fff, 6, 7, 8, 9], record_lens_reference: RECORD_REFERENCE, lens_reference: RECORD_REFERENCE, option: 5, ..CalculationInput::default() };
    let mut context = context();
    resume_input(&mut world, &mut context, &saved, 0x99);
    let input = context.calculation;
    assert_eq!(input.metering_level, 120);
    assert_eq!(input.levels, [21, 22, 23, 24, 0x7fffi16.wrapping_add(20), 26, 27, 28, 29]);
    assert_eq!((input.lens_reference, input.record_lens_reference, input.option), (0x99, 0x99, 5));
}
