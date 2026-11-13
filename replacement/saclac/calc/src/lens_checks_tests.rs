use std::vec::Vec;

use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};
use xm1_saclac_profile_records::Bytes;

use crate::adjust::motion::drive::DriveKind;
use crate::adjust::motion::ports::{LensCalibration, LensLimits, LensTables, LimitQuery};
use crate::adjust::timing::LevelScale;
use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{Lens, ProfileAccess, Properties, Property};
use crate::lens_checks::*;
use crate::profile::Profile;
use crate::scene_recognition::ports::LensServices;
use crate::stock::lens_checks::{OVERRIDDEN_ZONE, ZONE_OVERRIDE};
use crate::stock::record::FOCUS_RESULT_BYTES;

#[derive(Default)]
struct Bench {
    world: World,
    held_reference: u32,
    limits: LensLimits,
    limit_queries: Vec<LimitQuery>,
}

impl ProfileAccess for Bench {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl Lens for Bench {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl LensControl for Bench {
    fn held_reference(&mut self) -> u32 { self.held_reference }
}

/// The linear value of a level is the level plus 1000.
impl LevelScale for Bench {
    fn linear(&mut self, level: i16) -> u32 { (i32::from(level) + 1000) as u32 }
    fn log2_difference(&mut self, _value: u32, _reference: u32) -> i16 { 0 }
}

impl LensCalibration for Bench {
    fn steps(&mut self) -> u32 { 0 }
    fn lowest_step(&mut self, _lens: u32) -> u32 { 0 }
    fn highest_step(&mut self, _lens: u32) -> u32 { 0 }
    fn depth_positions(&mut self, _lens: u32, _step: u32, _depth: u16) -> i16 { 0 }
    fn table_entry(&mut self, _lens: u32, _table: Property, _entries: u32) -> u32 { 0 }
    fn lens_state(&mut self) -> u32 { 0 }
    fn drive_kind(&mut self) -> DriveKind { DriveKind::default() }
    fn lens_limits(&mut self, query: &LimitQuery) -> LensLimits {
        self.limit_queries.push(*query);
        self.limits
    }
}

#[test]
fn the_high_limit_with_margins_is_within_a_limit_it_does_not_pass() {
    let mut bench = Bench { limits: LensLimits { low: -30, high: 9 }, ..Bench::default() };
    assert!(high_limit_within(&mut bench, 4, 6, 0x30, 9));
    assert!(!high_limit_within(&mut bench, 4, 6, 0x30, 8));
    assert_eq!(bench.limit_queries[0], LimitQuery { channel: 0, lens: 4, step: 6, reference: 0x30, with_margins: true, lens_state: 0 });
    bench.limits.high = -7;
    assert!(high_limit_within(&mut bench, 4, 6, 0x30, -7));
    assert!(!high_limit_within(&mut bench, 4, 6, 0x30, -8));
}

const FOCUS_REFERENCE: u32 = 0x40;
const FOCUS_LENS: u32 = 3;
const CURRENT_LENS: u32 = 5;
const HELD_REFERENCE: u32 = 0x77;

/// A focus result whose level is 500 - (40 + 10) - 20 - 30 + 60 = 460; its fifth term is not part
/// of it.
fn focus_result(valid: u32) -> [u8; FOCUS_RESULT_BYTES] {
    let mut bytes = [0; FOCUS_RESULT_BYTES];
    bytes[0..4].copy_from_slice(&FOCUS_REFERENCE.to_le_bytes());
    bytes[4..8].copy_from_slice(&valid.to_le_bytes());
    bytes[8..12].copy_from_slice(&FOCUS_LENS.to_le_bytes());
    for (index, term) in [500u16, 10, 20, 30, 999, 40].into_iter().enumerate() {
        bytes[0x0c + 2 * index..0x0e + 2 * index].copy_from_slice(&term.to_le_bytes());
    }
    bytes
}

/// A record whose current level is `offset` + 120 - 5 - 6 - 9.
fn risen(offset: i16, valid: u32) -> Bench {
    let mut bench = Bench { held_reference: HELD_REFERENCE, ..Bench::default() };
    bench.world.set_shared(&group_14::FOCUS_RESULT, focus_result(valid));
    bench.world.lens_levels.insert((FOCUS_LENS, FOCUS_REFERENCE), 60);
    bench.world.lens_levels.insert((CURRENT_LENS, HELD_REFERENCE), 120);
    bench.world.set_shared(&group_26::AT_0200, CURRENT_LENS);
    bench.world.set_shared(&group_26::AT_0206, offset);
    bench.world.set_shared(&group_26::AT_0208, 5);
    bench.world.set_shared(&group_26::AT_020C, 6);
    bench.world.set_shared(&group_26::AT_0216, 9);
    bench
}

#[test]
fn a_level_above_the_focus_results_rises_by_the_linear_difference() {
    assert_eq!(level_rise(&mut risen(400, 1), handle()), 1040);
}

#[test]
fn a_lowered_focus_result_counts_one_hundred_lower() {
    let mut bench = risen(400, 1);
    bench.world.set_shared(&group_27::AT_072C, 1);
    assert_eq!(level_rise(&mut bench, handle()), 1140);
}

#[test]
fn a_level_not_above_the_focus_results_has_not_risen() {
    assert_eq!(level_rise(&mut risen(360, 1), handle()), NO_RISE);
    assert_eq!(level_rise(&mut risen(340, 1), handle()), NO_RISE);
}

#[test]
fn an_invalid_focus_result_has_no_rise() {
    assert_eq!(level_rise(&mut risen(400, 0), handle()), NO_RISE);
}

const LENS: u32 = 6;
const STEP: u32 = 2;
const LENS_REFERENCE: u32 = 0x51;
const SIDE: u32 = 3;

/// The lens targets 100 and its far table position is 260, so a sixteenth of the span is 10.
#[derive(Default)]
struct Span {
    world: World,
    response: i32,
    side_shift: i16,
    position_queries: Vec<[u32; 4]>,
    side_queries: Vec<(i16, u32, u32)>,
}

impl Properties for Span {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl LensTables for Span {
    fn channel_positions(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> [i16; 2] {
        self.position_queries.push([lens, step, reference, mode]);
        [100, 260]
    }
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16 {
        self.side_queries.push((value, from, to));
        value + self.side_shift
    }
    fn table_offset(&mut self, _lens: u32, _step: u32, _delta: i16) -> i32 { 0 }
    fn channel_start(&mut self, _lens: u32, _channel: u8, _reference: u32) -> i16 { 0 }
    fn period_position(&mut self, _period: u16, _lens: u32, _step: u32, _reference: u32) -> i32 { 0 }
}

/// Every lens's response is `response`.
impl LensServices for Span {
    fn lens_reference(&mut self) -> u32 { LENS_REFERENCE }
    fn primary_tenths(&mut self, _lens: u32) -> u32 { 0 }
    fn response(&mut self, _value: u16, _lens: u32, _step: u32, _reference: u32) -> i32 { self.response }
    fn converted_difference(&mut self, _first: u32, _second: u32) -> i16 { 0 }
    fn level_class(&mut self, _lens: u32) -> u32 { 0 }
    fn class_level(&mut self, _lens: u32, _setting: u32) -> i32 { 0 }
}

fn zone(span: &mut Span, position: i16) -> PointZone { point_zone(span, &LensPoint { position, side: SIDE }, LENS, STEP) }

#[test]
fn a_point_is_placed_against_the_far_position_the_span_and_the_sixteenth_nearest_the_response() {
    let mut span = Span { response: 47, ..Span::default() };
    let zones = [261, 260, 250, 151, 150].map(|position| zone(&mut span, position));
    assert_eq!(zones, [PointZone::BeyondFar, PointZone::BeyondSpan, PointZone::BeyondResponse, PointZone::BeyondResponse, PointZone::Within]);
    assert_eq!(zones.map(PointZone::code), [1, 2, 3, 3, 4]);
    assert!(span.position_queries.iter().all(|&query| query == [LENS, STEP, LENS_REFERENCE, 0]));
    assert_eq!(span.side_queries[0], (261, SIDE, 1));
}

#[test]
fn of_two_sixteenths_equally_near_the_response_the_lower_is_taken() {
    let mut span = Span { response: 45, ..Span::default() };
    assert_eq!(zone(&mut span, 145), PointZone::BeyondResponse);
    span.response = 46;
    assert_eq!(zone(&mut span, 145), PointZone::Within);
}

#[test]
fn a_point_is_placed_on_the_side_it_is_moved_to() {
    let mut span = Span { side_shift: 20, ..Span::default() };
    assert_eq!(zone(&mut span, 245), PointZone::BeyondFar);
}

#[test]
fn the_override_names_the_zone_without_reading_the_lens() {
    let mut span = Span::default();
    span.world.property(ZONE_OVERRIDE, 1);
    let zones = [0, 1, 7].map(|configured| {
        span.world.property(OVERRIDDEN_ZONE, configured);
        zone(&mut span, 0)
    });
    assert_eq!(zones, [PointZone::Within, PointZone::BeyondSpan, PointZone::BeyondFar]);
    assert!(span.position_queries.is_empty() && span.side_queries.is_empty());
}
