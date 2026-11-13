use std::vec::Vec;

use xm1_saclac_profile_records::stock::record::{group_00, group_05};
use xm1_saclac_profile_records::stock::shared::group_41;
use xm1_saclac_profile_records::{Bytes, Index, IntervalTracking, MeasurementSelection, SharedHeaderByte};

use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{ProfileAccess, Properties, Property};
use crate::policy::{ExposureMode, SelectionKind};
use crate::profile::Profile;
use crate::record_policy::*;
use crate::stock::property::{level_step, ADJUSTMENT_FLAGS};
use crate::stock::record_policy as stock;

struct Bench {
    world: World,
    applied: Vec<(u32, i16, bool)>,
}

impl Properties for Bench {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl ProfileAccess for Bench {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl ChannelMapping for Bench {
    fn channel(&mut self, pair: [u32; 2], bank: u32) -> u32 { 10 * (pair[0] + pair[1]) + bank }
    fn mapped_value(&mut self, channel: u32, _bank: u32) -> u32 { channel / 10 }
}

impl AdjustmentPosition for Bench {
    fn apply(&mut self, selector: u32, position: i16, unmeasured: bool) { self.applied.push((selector, position, unmeasured)); }
}

fn bench() -> Bench { Bench { world: World::default(), applied: Vec::new() } }

#[test]
fn a_record_without_header_byte_0x63_set_to_1_selects_no_exposure_mode() {
    let mut bench = bench();
    for header in [0, 2] {
        bench.world.header(SharedHeaderByte::At63, header);
        assert_eq!(exposure_mode(&mut bench, handle(), 5, false), ExposureMode(0));
    }
}

#[test]
fn half_press_modes_5_and_6_select_by_the_record_measurement_selection() {
    let mut bench = bench();
    bench.world.header(SharedHeaderByte::At63, 1);
    let modes = |bench: &mut Bench| [(5, true), (5, false), (6, false), (4, false)].map(|(mode, refinement)| exposure_mode(bench, handle(), mode, refinement).0);
    assert_eq!(modes(&mut bench), [1, 2, 3, 0]);
    bench.world.select(MeasurementSelection::Record);
    assert_eq!(modes(&mut bench), [4, 4, 5, 0]);
}

fn allowed_query() -> RowDataQuery {
    RowDataQuery { selection: MeasurementSelection::Unselected, kind: SelectionKind(3), evaluation: 1, layout: 2, tracking: IntervalTracking(1) }
}

#[test]
fn row_data_needs_the_setting_tracking_and_evaluation_and_refuses_listed_kinds_and_layouts() {
    let mut bench = bench();
    bench.world.property(stock::ROW_DATA, 2);
    assert!(row_data_allowed(&mut bench, &allowed_query()));
    let refused = [
        RowDataQuery { selection: MeasurementSelection::Record, ..allowed_query() },
        RowDataQuery { kind: SelectionKind(24), ..allowed_query() },
        RowDataQuery { kind: SelectionKind(55), ..allowed_query() },
        RowDataQuery { evaluation: 2, ..allowed_query() },
        RowDataQuery { layout: 4, ..allowed_query() },
        RowDataQuery { layout: 5, ..allowed_query() },
        RowDataQuery { tracking: IntervalTracking(0), ..allowed_query() },
    ];
    for query in refused {
        assert!(!row_data_allowed(&mut bench, &query), "{query:?}");
    }
    assert!(row_data_allowed(&mut bench, &RowDataQuery { kind: SelectionKind(46), selection: MeasurementSelection::Other(2), layout: 6, ..allowed_query() }));
    bench.world.property(ADJUSTMENT_FLAGS, 1);
    assert!(!row_data_allowed(&mut bench, &allowed_query()));
    bench.world.property(ADJUSTMENT_FLAGS, 0);
    bench.world.property(stock::ROW_DATA, 4);
    assert!(!row_data_allowed(&mut bench, &allowed_query()));
}

#[test]
fn the_adjustment_status_is_accepted_for_an_enabled_overridden_or_accepted_kind() {
    assert_eq!(adjustment_status(0, SelectionKind(2), 0).code(), 3);
    assert_eq!(adjustment_status(1, SelectionKind(2), 0).code(), 6);
    assert_eq!(adjustment_status(2, SelectionKind(2), 0).code(), 3);
    assert_eq!(adjustment_status(0, SelectionKind(2), 1).code(), 6);
    assert_eq!(adjustment_status(0, SelectionKind(52), 0), AdjustmentStatus::Accepted);
}

fn stepped() -> Bench {
    let mut bench = bench();
    for step in 2..=10u8 {
        bench.world.property_word(level_step(step), 100 * u16::from(step));
    }
    bench
}

#[test]
fn class_limits_take_the_low_nibble_as_upper_and_the_next_bits_as_lower() {
    let mut bench = stepped();
    bench.world.property(stock::FIXED_CLASS_LIMITS, 0x3c);
    assert_eq!(fixed_class_limits(&mut bench), ClassLimits { upper: 0xc, lower: 3 });
    for (band, value) in (1..=10u8).zip(0x10..) {
        bench.world.property(stock::band_class_limits(band), value);
    }
    let lower = |bench: &mut Bench, level| level_class_limits(bench, level).lower;
    assert_eq!([lower(&mut bench, 50), lower(&mut bench, 250), lower(&mut bench, 950), lower(&mut bench, 1200), lower(&mut bench, 0)], [1; 5]);
    let upper = |bench: &mut Bench, level| level_class_limits(bench, level).upper;
    assert_eq!([upper(&mut bench, 50), upper(&mut bench, 250), upper(&mut bench, 950), upper(&mut bench, 1200), upper(&mut bench, 0)], [0, 1, 8, 9, 1]);
}

#[test]
fn a_level_below_step_two_is_raised_to_it() {
    let mut bench = stepped();
    assert_eq!([floor_level(&mut bench, -50), floor_level(&mut bench, 199), floor_level(&mut bench, 200), floor_level(&mut bench, 450)], [200, 200, 200, 450]);
}

fn lens_entry(bench: &mut Bench, entry: usize, pair: [u32; 2]) {
    let mut bytes = [0; 8];
    bytes[..4].copy_from_slice(&pair[0].to_le_bytes());
    bytes[4..].copy_from_slice(&pair[1].to_le_bytes());
    bench.world.set_shared(&group_41::AT_0974.at(Index::new(entry).unwrap()), bytes);
}

#[test]
fn a_channel_changes_when_its_mapped_value_differs_from_the_previous_channel_value() {
    let mut bench = bench();
    lens_entry(&mut bench, 2, [3, 4]);
    assert!(!channel_changed(&mut bench, handle(), 1, 75));
    assert!(channel_changed(&mut bench, handle(), 1, 85));
}

#[test]
fn the_reference_comparison_maps_the_reference_channel_in_its_own_bank() {
    let mut bench = bench();
    lens_entry(&mut bench, 2, [3, 4]);
    lens_entry(&mut bench, 4, [9, 71]);
    assert_eq!(compare_reference(&mut bench, handle(), 2), ReferenceComparison { channel: 72, changed: false });
    lens_entry(&mut bench, 4, [9, 81]);
    assert_eq!(compare_reference(&mut bench, handle(), 2), ReferenceComparison { channel: 72, changed: true });
}

fn position_element(selector: u32, terms: [u16; 4], enabled: [u32; 2]) -> [u8; 168] {
    let mut element = [0; 168];
    element[4..8].copy_from_slice(&selector.to_le_bytes());
    for (at, term) in [0xc, 0xe, 0x10, 0x1a].into_iter().zip(terms) {
        element[at..at + 2].copy_from_slice(&term.to_le_bytes());
    }
    element[0x44..0x48].copy_from_slice(&enabled[0].to_le_bytes());
    element[0x48..0x4c].copy_from_slice(&enabled[1].to_le_bytes());
    element
}

#[test]
fn the_mode_position_goes_to_image_4_only_when_both_enable_words_are_set() {
    let mut bench = bench();
    bench.world.set(&group_05::AT_016C_168.at(stock::FIRST_POSITION), position_element(7, [1, 2, 3, 4], [1, 0]));
    apply_mode_position(&mut bench, handle());
    assert!(bench.applied.is_empty());
    bench.world.set(&group_05::AT_016C_168.at(stock::FIRST_POSITION), position_element(7, [1, 2, 3, 0xfff0], [1, 9]));
    bench.world.set(&group_00::AT_0022, 0);
    apply_mode_position(&mut bench, handle());
    assert_eq!(bench.applied, [(7, -10, true)]);
}

#[test]
fn record_kind_5_takes_the_second_mode_position_and_a_measured_level_clears_the_unmeasured_flag() {
    let mut bench = bench();
    bench.world.set(&group_05::AT_016C_168.at(stock::FIRST_POSITION), position_element(7, [1, 0, 0, 0], [1, 1]));
    bench.world.set(&group_05::AT_016C_168.at(stock::SECOND_POSITION), position_element(8, [0, 0, 5, 0], [1, 1]));
    bench.world.set(&group_00::AT_0032, 5);
    bench.world.set(&group_00::AT_0022, 120);
    apply_mode_position(&mut bench, handle());
    assert_eq!(bench.applied, [(8, 5, false)]);
}
