use xm1_saclac_interface::exposure::CalculationInput;

use crate::adjust::exposure_limits::NO_INDEX;
use crate::adjust::profile_mode::{FlagInputs, LimitFlags, Positions, Readiness, Spread};
use crate::exposure::context::{EvaluationArea, Levels, EVALUATION_AREAS};
use crate::exposure::fakes::World;
use crate::exposure::flags::*;
use crate::exposure::ports::Property;
use crate::stock::{exposure as wire, property};

fn world() -> World {
    let mut world = World::default();
    for limit in [0, u16::MAX] {
        world.near_steps.insert(limit, 2);
        world.far_steps.insert(limit, 9);
    }
    world.lens_base_levels.insert(2, 200);
    world.device_ready = true;
    world
}

fn input(scene: u32) -> CalculationInput {
    CalculationInput { selection_kind: scene, record_20: i16::MAX, ..CalculationInput::default() }
}

fn query() -> FlagQuery {
    FlagQuery { index: NO_INDEX, readiness: Readiness::Ready, positions: Positions { position: 199, spread: Spread::default() }, ..FlagQuery::default() }
}

#[test]
fn selection_spread_uses_the_supplied_position_and_ignores_the_selection_style() {
    let mut world = world();
    world.property(Property(0x657e), 5);
    let input = CalculationInput { selection_style: 7, ..input(1) };
    let query = FlagQuery { positions: Positions { position: 200, spread: Spread::Configured { from: 100, to: 300 } }, ..query() };
    let (spread, limits) = selection_spread(&mut world, &input, &Levels { measured: 0, ..Levels::default() }, &query);
    assert!(spread);
    assert_eq!(limits.first, 200);
    assert!(!selection_spread(&mut world, &input, &Levels { measured: 200, ..Levels::default() }, &FlagQuery { positions: Positions { position: 199, ..query.positions }, ..query }).0);
}

#[test]
fn class_five_rebases_each_present_area_even_when_the_shared_path_exempts_the_scene() {
    let mut world = world();
    world.property_word(property::level_step(3), 250);
    let input = CalculationInput { record_32: 5, selection_style: 2, ..input(45) };
    let levels = Levels { unmeasured: true, start: 100, ..Levels::default() };
    let mut rows = [AreaFlagRow { flags: LimitFlags(0xaa), retained: 71 }, AreaFlagRow { flags: LimitFlags(0xbb), retained: 83 }];
    area_flags(&mut world, &input, &levels, &query(), Some(&[false, true]), &mut rows);
    assert_eq!(rows, [AreaFlagRow { flags: LimitFlags(0), retained: 71 }, AreaFlagRow { flags: LimitFlags(2), retained: 83 }]);
    area_flags(&mut world, &input, &levels, &query(), None, &mut rows);
    assert_eq!(rows, [AreaFlagRow { flags: LimitFlags(4), retained: 71 }; 2]);
}

#[test]
fn a_nonzero_limit_offset_changes_the_limit_without_rebasing_the_position() {
    let mut world = world();
    world.property_word(Property(0x68da), 30);
    let input = CalculationInput { selection_style: 2, ..input(1) };
    let levels = Levels { unmeasured: true, start: 100, ..Levels::default() };
    let query = FlagQuery { positions: Positions { position: 190, ..query().positions }, ..query() };
    let mut rows = [AreaFlagRow { flags: LimitFlags(0), retained: () }];
    area_flags(&mut world, &input, &levels, &FlagQuery { limit_offset: 10, ..query }, None, &mut rows);
    assert_eq!(rows[0].flags, LimitFlags(2));
    area_flags(&mut world, &input, &levels, &FlagQuery { limit_offset: 25, ..query }, None, &mut rows);
    assert_eq!(rows[0].flags, LimitFlags(1));
}

#[test]
fn adjustment_levels_wrap_as_signed_halves_and_out_of_range_steps_use_step_three() {
    let mut world = world();
    world.property_word(property::level_step(3), 25);
    let input = AdjustmentFlagInput {
        target: 100,
        reference: 30,
        step: 256,
        flags: FlagInputs { scene: 1, index: NO_INDEX, readiness: Readiness::Ready, ..FlagInputs::default() },
    };
    assert_eq!(adjusted_flags(&mut world, &input, 120), AdjustedFlags { level: 165, change: 65, flags: LimitFlags(1) });
    let result = adjusted_flags(&mut world, &AdjustmentFlagInput { target: i16::MAX, reference: -1, ..input }, 0);
    assert_eq!((result.level, result.change), (-32793i32 as i16, -24));
}

#[test]
fn statistics_areas_keep_full_width_words_and_separate_the_unsigned_halves() {
    let mut areas = [EvaluationArea::default(); EVALUATION_AREAS];
    areas[7] = EvaluationArea { profile: 0x89ab_cdef, next_profile: 0x7654_3210, lens_step: 0xfedc_ba98, converted: 0x8001, level: 0xfffe, extended: true };
    let encoded = wire::evaluation_areas(&areas);
    assert_eq!(&encoded[140..], &[0xef, 0xcd, 0xab, 0x89, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 1, 0x80, 0xfe, 0xff, 1, 0, 0, 0]);
}

#[test]
fn the_last_present_area_selects_the_level_and_style_two_always_removes_record_offsets() {
    let mut world = world();
    let input = CalculationInput { record_32: 5, selection_style: 2, record_20: 20, record_level: 30, ..input(1) };
    let query = FlagQuery { positions: Positions { position: 200, ..query().positions }, ..query() };
    let mut rows = [AreaFlagRow { flags: LimitFlags(0xff), retained: 71 }, AreaFlagRow { flags: LimitFlags(0xff), retained: 83 }];
    assert_eq!(evaluate_selection(&mut world, &input, &Levels::default(), &query, 300, Some(&[false, true]), &mut rows), SelectionLevels { selected: 300, reference: 250, baseline: 150 });
    let query = FlagQuery { positions: Positions { position: 199, ..query.positions }, ..query };
    assert_eq!(evaluate_selection(&mut world, &input, &Levels::default(), &query, 300, Some(&[true, true]), &mut rows), SelectionLevels { selected: 199, reference: 149, baseline: 149 });
}

#[test]
fn selection_style_not_record_5d_decides_whether_a_below_selection_keeps_record_offsets() {
    let mut world = world();
    for record_5d in [0, 2] {
        for (selection_style, expected) in [(0, 199), (1, 199), (2, 149)] {
            let input = CalculationInput { record_32: 5, selection_style, record_5d, record_20: 20, record_level: 30, ..input(1) };
            let mut rows = [AreaFlagRow { flags: LimitFlags(0), retained: () }];
            let result = evaluate_selection(&mut world, &input, &Levels::default(), &query(), 300, Some(&[true]), &mut rows);
            assert_eq!(result, SelectionLevels { selected: 199, reference: expected, baseline: expected }, "style {selection_style}, record_5d {record_5d}");
        }
    }
}

#[test]
fn explicit_wrapper_readiness_keeps_the_selection_overrides() {
    let mut world = world();
    for device_ready in [false, true] {
        world.device_ready = device_ready;
        for (wrapper, readiness, flags, selected) in [(1, Readiness::Ready, 2, 300), (2, Readiness::NotReady, 0, 200)] {
            let query = FlagQuery { readiness, positions: Positions { position: 200, ..query().positions }, ..query() };
            let mut rows = [AreaFlagRow { flags: LimitFlags(0), retained: () }];
            let input = CalculationInput { selection_style: 2, ..input(1) };
            let result = evaluate_selection(&mut world, &input, &Levels::default(), &query, 300, None, &mut rows);
            assert_eq!((rows[0].flags, result.selected), (LimitFlags(flags), selected), "wrapper {wrapper}, device ready {device_ready}");
            for suppressed in [CalculationInput { measurement_selection: 1, ..input }, CalculationInput { selection_auxiliary: 2, ..input }] {
                area_flags(&mut world, &suppressed, &Levels::default(), &query, None, &mut rows);
                assert_eq!(rows[0].flags, LimitFlags(0));
            }
            let styled = CalculationInput { selection_style: 7, measurement_selection: 1, selection_auxiliary: 2, ..input };
            area_flags(&mut world, &styled, &Levels::default(), &query, None, &mut rows);
            assert_eq!(rows[0].flags, LimitFlags(0x42));
            world.property(Property(0xa8), 8);
            area_flags(&mut world, &styled, &Levels::default(), &query, None, &mut rows);
            assert_eq!(rows[0].flags, LimitFlags(0));
            world.property(Property(0xa8), 0);
        }
    }
}

#[test]
fn the_adjustment_contexts_decoded_levels_and_flags_drive_the_returned_level() {
    let mut world = world();
    world.property_word(property::level_step(3), 25);
    let mut bytes = [0; wire::ADJUSTMENT_FLAG_BYTES];
    bytes[0xac..0xb0].copy_from_slice(&1u32.to_le_bytes());
    bytes[0xcc..0xce].copy_from_slice(&100i16.to_le_bytes());
    bytes[0xfc..0xfe].copy_from_slice(&30i16.to_le_bytes());
    bytes[0xf4..0xf8].copy_from_slice(&256u32.to_le_bytes());
    bytes[0xf0..0xf2].copy_from_slice(&i16::MAX.to_le_bytes());
    let input = wire::adjustment_flags(&bytes);
    assert_eq!(adjusted_flags(&mut world, &input, 120), AdjustedFlags { level: 165, change: 65, flags: LimitFlags(1) });
}
