use xm1_saclac_profile_records::MeasurementSelection;

use crate::exposure::context::{CandidateSpan, ExposureContext, StartBound};
use crate::exposure::fakes::World;
use crate::exposure::program::*;
use crate::stock::exposure_limits::{parameter, ParameterField, PARAMETER_SETS};
use crate::stock::tables::PROGRAM_PROFILES;
use crate::stock::{property, shared};

/// Program profile `k` sits at level `100 k` with sensitivity class `k`; the range is 0..300 above
/// a measured 500 with lens 1 at both ends, steps are at most 100, and the start sits 50 above the
/// low end.
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
    world.property(property::START_MARGIN, 5);
    world.property(property::STEP_LIMIT, 10);
    world.property(property::UPPER_TOLERANCE, 2);
    world.property(property::LOWER_TOLERANCE, 2);
    world.property(property::FIRST_STEP, 255);
    world
}

fn context() -> ExposureContext {
    let mut context = ExposureContext::default();
    context.levels.target = 500;
    context.levels.measured = 500;
    context
}

#[test]
fn the_range_is_divided_into_steps_no_larger_than_the_step_limit() {
    let program = build(&mut world(), &context());
    let expected = CandidateSpan { first: 1, last: 4, low: 0, high: 300, start: 50, end: 280, first_step: 50, step: 77, bound: StartBound::Margin };
    assert_eq!(program.span, expected);
    assert_eq!(program.choice, FirstChoice::Unmeasured);
    let steps = &program.steps[..4];
    assert_eq!(steps.iter().map(|step| step.position).collect::<std::vec::Vec<_>>(), [50, 127, 204, 280]);
    assert_eq!(steps.iter().map(|step| step.level).collect::<std::vec::Vec<_>>(), [50, 127, 204, 280]);
    assert_eq!(steps.iter().map(|step| step.profile).collect::<std::vec::Vec<_>>(), [0x16, 0x17, 0x18, 0x18]);
    assert_eq!(steps.iter().map(|step| step.next_profile).collect::<std::vec::Vec<_>>(), [0x17, 0x18, 0x18, 0x18]);
    assert_eq!(steps.iter().map(|step| step.index).collect::<std::vec::Vec<_>>(), [0, 1, 2, 3]);
}

#[test]
fn each_level_is_converted_for_its_profile() {
    let mut world = world();
    world.conversions.insert(0x17, 1000);
    let program = build(&mut world, &context());
    assert_eq!(program.steps[1].converted, 1127);
    assert_eq!(program.steps[0].converted, 50);
}

#[test]
fn a_floor_above_the_margin_starts_the_program_at_the_floor() {
    let mut context = context();
    context.levels.target = 400;
    let program = build(&mut world(), &context);
    assert_eq!((program.span.start, program.span.bound, program.span.first_step), (100, StartBound::Floor, 0));
    assert_eq!((program.span.last, program.span.step, program.span.end), (3, 90, 280));
}

#[test]
fn a_pinned_snapshot_raises_the_start_to_its_offset_below_the_high_end() {
    let mut world = world();
    world.property(property::FIRST_STEP, 1);
    let mut context = context();
    context.snapshot.valid = 1;
    context.snapshot.focus.pinned = 1;
    let program = build(&mut world, &context);
    assert_eq!((program.span.start, program.span.bound, program.span.last), (280, StartBound::Pinned, 1));
}

#[test]
fn a_configured_first_step_inside_the_program_is_taken() {
    let mut world = world();
    world.property(property::FIRST_STEP, 3);
    let program = build(&mut world, &context());
    assert_eq!((program.span.first, program.choice), (3, FirstChoice::Configured));
    world.property(property::FIRST_STEP, 5);
    assert_eq!(build(&mut world, &context()).span.first, 1);
    world.property(property::FIRST_STEP, 0);
    assert_eq!(build(&mut world, &context()).span.first, 1);
}

#[test]
fn the_snapshot_offset_chooses_the_first_step_unless_the_focus_curve_falls_below_its_band() {
    let mut context = context();
    context.snapshot.valid = 1;
    context.snapshot.offset = -400;
    let mut world = world();
    let program = build(&mut world, &context);
    assert_eq!((program.span.first, program.choice), (2, FirstChoice::Curve));
    world.put_u16(shared::focus_curve(17), -1000i16 as u16);
    assert_eq!(build(&mut world, &context).span.first, 1);
}

#[test]
fn a_step_beyond_the_level_limit_moves_the_lens_toward_the_far_step() {
    let mut world = world();
    world.property_word(property::PROGRAM_LEVEL_LIMIT, 60);
    world.lens_levels.insert((2, 0), 30);
    world.lens_levels.insert((3, 0), 60);
    world.far_steps.insert(0, 3);
    let program = build(&mut world, &context());
    assert_eq!((program.span.last, program.span.step, program.span.end), (2, 70, 120));
    assert_eq!((program.steps[1].lens_step, program.steps[1].level, program.steps[1].position), (3, 60, 120));
    assert_eq!(program.steps[0].lens_step, 1);
}

#[test]
fn a_positive_program_offset_below_the_ceiling_appends_an_extended_step_one_offset_beyond_the_last() {
    let mut world = world();
    world.property_word(property::PROGRAM_LEVEL_LIMIT, 200);
    let plain = build(&mut world, &context());
    assert_eq!((plain.span.last, plain.span.end), (3, 200));
    assert!(plain.steps.iter().all(|step| !step.extended));

    world.property_word(property::PROGRAM_OFFSET, 30);
    let program = build(&mut world, &context());
    assert_eq!(program.span.last, 4);
    let [last, extended] = [program.steps[2], program.steps[3]];
    assert!(extended.extended && !last.extended);
    assert_eq!((extended.index, extended.position), (3, last.position + 30));
    assert_eq!((extended.level, extended.profile, extended.lens_step, extended.converted), (last.level, last.profile, last.lens_step, last.converted));
}

#[test]
fn a_positive_program_offset_up_to_the_high_end_appends_no_extended_step() {
    let mut world = world();
    world.property_word(property::PROGRAM_OFFSET, 30);
    let program = build(&mut world, &context());
    assert!(program.steps[..usize::from(program.span.last)].iter().all(|step| !step.extended));
}

#[test]
fn a_selected_measurement_raises_only_its_high_end_by_the_configured_offset() {
    let span = |offset: u16| {
        let mut world = world();
        world.select(MeasurementSelection::Record);
        world.property_word(property::SELECTED_HIGH_OFFSET, offset);
        let program = build(&mut world, &context());
        (program.span.low, program.span.high)
    };
    let (low, high) = span(0);
    assert_eq!(span(100), (low, high.max(low) + 100));
}

#[test]
fn a_step_value_counts_the_converted_level_less_its_offset_plus_its_lens() {
    let mut world = world();
    world.lens_levels.insert((4, 9), 25);
    world.level_offsets.insert(0x18, 5);
    world.property_word(property::EXTENDED_OFFSET, 1000);
    let step = crate::exposure::context::ExposureStep { converted: 300, offset: 20, lens_step: 4, next_profile: 0x18, ..Default::default() };
    assert_eq!(step_value(&mut world, &step, 9), 300);
    assert_eq!(step_value(&mut world, &crate::exposure::context::ExposureStep { extended: true, ..step }, 9), 1300);
}
