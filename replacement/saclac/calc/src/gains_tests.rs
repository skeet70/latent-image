use std::vec::Vec;

use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_07};
use xm1_saclac_profile_records::{Bytes, SharedHeaderByte};

use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{ProfileAccess, Properties, Property};
use crate::gains::*;
use crate::profile::Profile;
use crate::stock::capture::{refined_scene, RefinedScene, FIRST_ELEMENT};
use crate::stock::gains::{request, write_result, REQUEST_BYTES, RESULT_BYTES};

#[derive(Default)]
struct Selection {
    world: World,
    requests: Vec<GainRequest>,
    selectors: Vec<BlendSelector>,
    selected: SelectedGains,
    weight: u8,
}

impl ProfileAccess for Selection {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl Properties for Selection {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl CaptureGains for Selection {
    fn gains(&mut self, request: &GainRequest) -> SelectedGains {
        self.requests.push(*request);
        self.selected
    }
    fn blend_weight(&mut self, selector: &BlendSelector) -> u8 {
        self.selectors.push(*selector);
        self.weight
    }
}

const SELECTED: SelectedGains = SelectedGains { gains: [410, 420, 430], class: GainClass(18) };

fn seed(variant_set: u32) -> CalibrationSeed { CalibrationSeed { record: handle(), custom: [1, 2, 3], column: 7, level: -9, variant_set } }

fn record_settings(selection: &mut Selection) {
    selection.world.header(SharedHeaderByte::At31, 5);
    selection.world.header(SharedHeaderByte::At46, 1);
    selection.world.set(&group_00::AT_0048, 3);
    selection.world.set(&group_00::AT_002A, 0x1234);
    selection.world.set(&group_00::AT_000E, [0x10, 0, 0x20, 0, 0x30, 1]);
    selection.world.set(&group_07::AT_01B0.at(FIRST_ELEMENT), 2);
    selection.world.set(&group_01::AT_0090_2, -20);
}

#[test]
fn the_default_calibration_selects_the_records_settings_with_the_seeds_values() {
    let mut selection = Selection { selected: SELECTED, ..Selection::default() };
    record_settings(&mut selection);
    assert_eq!(default_calibration(&mut selection, &seed(3)), SELECTED);
    let expected = GainRequest {
        mode: 5,
        variant: 3,
        custom_source: 0x1234,
        bank: 1,
        fixed: [0x10, 0x20, 0x130],
        preset: 2,
        temperature: (-20i16) as u16,
        column: 7,
        level: -9,
        custom: [1, 2, 3],
        variant_set: 3,
        forced: false,
        variant_subset: 0,
    };
    assert_eq!(selection.requests, [expected]);
}

#[test]
fn without_a_variant_set_the_default_calibration_takes_no_preset() {
    let mut selection = Selection::default();
    record_settings(&mut selection);
    default_calibration(&mut selection, &seed(0));
    assert_eq!((selection.requests[0].preset, selection.requests[0].variant_set), (0, 0));
}

#[test]
fn reference_gains_select_the_request_at_column_500_and_level_600() {
    let mut selection = Selection { selected: SELECTED, ..Selection::default() };
    let asked = GainRequest { mode: 9, column: 1, level: 2, custom: [4, 5, 6], forced: true, ..GainRequest::default() };
    assert_eq!(reference_gains(&mut selection, &asked), SELECTED);
    assert_eq!(selection.requests, [GainRequest { column: 500, level: 600, ..asked }]);
}

#[test]
fn the_focus_blend_reads_a_refined_scene_when_header_63_is_one() {
    let mut selection = Selection { weight: 77, ..Selection::default() };
    let query = BlendQuery { scene: 19, column: 2, sensitivity: 4, header_63: 1, variant: 3 };
    assert_eq!(focus_blend(&mut selection, &query), 77);
    selection.world.property(refined_scene(RefinedScene::Beach), 1);
    focus_blend(&mut selection, &query);
    focus_blend(&mut selection, &BlendQuery { header_63: 0, ..query });
    let selector = |mode, preset| BlendSelector { mode, variant: 3, column: 2, sensitivity: 4, preset };
    assert_eq!(selection.selectors, [selector(1, 1), selector(18, 1), selector(19, 0)]);
}

#[test]
fn a_scene_beyond_a_byte_keeps_its_value() {
    let mut selection = Selection::default();
    focus_blend(&mut selection, &BlendQuery { scene: 0x113, header_63: 1, ..BlendQuery::default() });
    assert_eq!(selection.selectors[0].mode, 0x113);
}

#[test]
fn a_stock_request_is_read_from_the_offsets_the_gain_selection_reads() {
    let mut bytes = [0xeeu8; REQUEST_BYTES];
    let mut put = |at: usize, value: &[u8]| bytes[at..at + value.len()].copy_from_slice(value);
    put(0x00, &5u32.to_le_bytes());
    put(0x04, &6u32.to_le_bytes());
    put(0x08, &7u32.to_le_bytes());
    put(0x0c, &8u32.to_le_bytes());
    put(0x18, &[1, 0, 2, 0, 3, 0]);
    put(0x20, &4u32.to_le_bytes());
    put(0x28, &300u16.to_le_bytes());
    put(0x2a, &(-2i16).to_le_bytes());
    put(0x2c, &(-3i16).to_le_bytes());
    put(0x2e, &[9, 0, 10, 0, 11, 0]);
    put(0x34, &2u32.to_le_bytes());
    put(0x38, &0x100u32.to_le_bytes());
    put(0x3c, &1u32.to_le_bytes());
    let expected = GainRequest {
        mode: 5,
        variant: 6,
        custom_source: 7,
        bank: 8,
        fixed: [1, 2, 3],
        preset: 4,
        temperature: 300,
        column: -2,
        level: -3,
        custom: [9, 10, 11],
        variant_set: 2,
        forced: true,
        variant_subset: 1,
    };
    assert_eq!(request(&bytes), expected);
}

#[test]
fn a_stock_result_takes_the_gains_and_the_class_around_two_kept_bytes() {
    let mut result = [0xeeu8; RESULT_BYTES];
    write_result(&mut result, &SELECTED);
    assert_eq!(result, [0x9a, 1, 0xa4, 1, 0xae, 1, 0xee, 0xee, 18, 0, 0, 0]);
}
