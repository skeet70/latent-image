use std::vec::Vec;

use xm1_saclac_profile_records::stock::shared::{group_14, group_26, group_27};
use xm1_saclac_profile_records::Bytes;

use crate::adjust::motion::drive::DriveKind;
use crate::adjust::motion::ports::{LensCalibration, LensLimits, LimitQuery};
use crate::adjust::timing::LevelScale;
use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{Lens, ProfileAccess, Property};
use crate::lens_checks::*;
use crate::profile::Profile;
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
