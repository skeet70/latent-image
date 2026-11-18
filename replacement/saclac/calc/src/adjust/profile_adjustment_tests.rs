use xm1_saclac_profile_records::{MeasurementSelection, WindowStatus};

use crate::adjust::profile_adjustment::*;
use crate::exposure::fakes::World;
use crate::exposure::ports::Property;
use crate::stock::profile_adjustment as table;

impl StepRatios for World {
    fn step_ratio(&mut self, code: u16, selector: u8) -> u32 { u32::from(code) + 100 * u32::from(selector) }
}

fn profile(id: u32) -> AdjustmentProfile { AdjustmentProfile::new(id).unwrap() }

fn choice() -> ProfileChoice {
    ProfileChoice {
        selection: MeasurementSelection::Unselected,
        result: 0,
        window_status: WindowStatus(3),
        mode: 0,
        flagged: false,
        banded: false,
        percent: 0,
        held: false,
        held_variant: 0,
    }
}

#[test]
fn a_selected_measurement_in_mode_46_chooses_from_the_wide_column() {
    let mut world = World::default();
    let choice = ProfileChoice { selection: MeasurementSelection::Record, result: 5, window_status: WindowStatus(4), mode: 46, ..choice() };
    assert_eq!(choose(&mut world, &choice), AdjustmentProfile::new(0x2d));
    let choice = ProfileChoice { mode: 0, ..choice };
    assert_eq!(choose(&mut world, &choice), AdjustmentProfile::new(0x29));
}

#[test]
fn other_window_statuses_follow_the_alternate_choice_setting() {
    let mut world = World::default();
    let choice = ProfileChoice { result: 20, window_status: WindowStatus(0), ..choice() };
    assert_eq!(choose(&mut world, &choice), AdjustmentProfile::new(0x22));
    world.property(Property(0x6c08), 1);
    assert_eq!(choose(&mut world, &choice), AdjustmentProfile::new(0x23));
}

#[test]
fn window_status_two_chooses_no_banded_profile_above_thirty_percent() {
    let mut world = World::default();
    let choice = ProfileChoice { banded: true, window_status: WindowStatus(2), percent: 30, ..choice() };
    assert_eq!(choose(&mut world, &choice), AdjustmentProfile::new(0xc2));
    assert_eq!(choose(&mut world, &ProfileChoice { percent: 31, ..choice }), None);
    assert_eq!(choose(&mut world, &ProfileChoice { percent: 31, result: 13, ..choice }), AdjustmentProfile::new(0xac));
}

#[test]
fn profile_ids_past_the_table_are_refused() {
    assert!(AdjustmentProfile::new(212).is_some());
    assert!(AdjustmentProfile::new(213).is_none());
}

#[test]
fn every_selected_measurement_chooses_a_profile_of_the_table() {
    let mut world = World::default();
    for alternate in [0, 1] {
        world.property(Property(0x6c08), alternate);
        for (status, result, mode, banded) in (0..6).flat_map(|status| (0..40).flat_map(move |result| [0, 46].into_iter().flat_map(move |mode| [false, true].map(|banded| (status, result, mode, banded))))) {
            let selected = SelectedChoice { result, window_status: WindowStatus(status), mode, banded };
            let chosen = choose_selected(&mut world, &selected);
            assert_eq!(AdjustmentProfile::new(chosen.id()), Some(chosen));
            let choice = ProfileChoice { selection: MeasurementSelection::Record, result, window_status: WindowStatus(status), mode, banded, ..choice() };
            assert_eq!(choose(&mut world, &choice), Some(chosen));
        }
    }
}

#[test]
fn the_selection_percent_reads_the_first_selection_of_the_configured_range_set() {
    let mut world = World::default();
    let record = profile(0).record();
    world.property(Property(record.range_key), 1);
    let set = &table::RANGE_SET_TABLE[usize::from(table::RANGE_CLASS_SETS[1][record.policy as usize])];
    world.property(set.selections[0], 4);
    assert_eq!(selection_percent(&mut world, profile(0)), 200);
    world.property(set.selections[0], 2);
    assert_eq!(selection_percent(&mut world, profile(0)), 100);
    world.property(Property(record.range_key), 0);
    assert_eq!(selection_percent(&mut world, profile(0)), 0);
}

#[test]
fn a_fixed_profile_reads_its_own_range_set() {
    let mut world = World::default();
    let fixed = profile(155);
    assert!(fixed.record().fixed_range_set);
    let set = &table::RANGE_SET_TABLE[fixed.record().range_key as usize];
    world.property(set.selections[0], 4);
    assert_eq!(selection_percent(&mut world, fixed), 200);
}

#[test]
fn lens_limits_are_read_in_tenths_unless_unset() {
    let mut world = World::default();
    world.property_word(Property(0x6c01), 12);
    world.property_word(Property(0x6c03), 0xffff);
    let limits = lens_limits(&mut world, profile(0), 3);
    assert_eq!(limits, LensLimits { near: 121, far: 0xffff + 1000 });
}

#[test]
fn a_primary_resolution_takes_the_first_ranges_capture_profile_and_keeps_the_level() {
    let mut world = World::default();
    let first = range(&mut world, profile(0), 0);
    world.property(first.option, 9);
    let inputs = ResolutionInputs { lens: 0, reference: 0, adjustment: 0, mode: 0, forced: true, primary: true, alternate: false, request: 0 };
    let mut level = 40;
    let mut split = Split { held: 1, rest: 2 };
    let resolution = resolve(&mut world, profile(0), &inputs, &mut level, &mut split);
    assert_eq!(resolution, Resolution { capture_profile: 20, code: 0, selector: 0 });
    assert_eq!((level, split), (40, Split { held: 1, rest: 2 }));
}

#[test]
fn halfword_offsets_follow_the_row_and_column_bands() {
    assert_eq!(halfword_offset(2, 1, 1), 448);
    assert_eq!(halfword_offset(2, 4, 1), 364);
    assert_eq!(halfword_offset(7, 1, 0), 392);
    assert_eq!(halfword_offset(7, 3, 0), 344);
}
