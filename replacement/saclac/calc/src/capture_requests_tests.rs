use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_interface::calc::{AreaGeometry, Calc1Payload, CalcPayload, CaptureMeasurement, CaptureProcessing, SourceStatistics, AcquisitionGrid};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_04, group_05, group_07, group_08};
use xm1_saclac_profile_records::stock::shared::group_27;
use xm1_saclac_profile_records::{Bytes, MeasurementSelection, SharedHeaderByte, WindowStatus};

use crate::adjust::focus_profile::LensResources;
use crate::capture_requests::*;
use crate::exposure::fakes::{handle, Headers, World};
use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties, Property};
use crate::profile::Profile;
use crate::requests::Peers;
use crate::stock::capture::FIRST_ELEMENT;

const CALLBACK: StockCallbackWord = StockCallbackWord(0x00f2_e6b4);

#[derive(Default)]
struct Capture {
    world: World,
    lens_word: u32,
    calibrations: BTreeMap<(u32, u32), i16>,
}

impl ProfileAccess for Capture {
    type Records = Bytes;
    type Entries = Headers;
    fn profile(&mut self) -> Profile<'_, Bytes, Headers> { self.world.profile() }
}

impl Properties for Capture {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl Lens for Capture {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl CaptureProfiles for Capture {
    fn level_offset(&mut self, profile: u32) -> i16 { self.world.level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { self.world.class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.world.offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.world.sensitivity_class(profile) }
    fn configured_level(&mut self) -> u16 { self.world.configured_level() }
    fn configured_profile(&mut self, level: i16) -> u32 { self.world.configured_profile(level) }
    fn record_profile(&mut self) -> u32 { self.world.record_profile() }
}

impl LensResources for Capture {
    fn resource(&mut self) -> Option<u32> { None }
    fn reference(&mut self) -> u32 { 0 }
    fn flags(&mut self) -> u32 { self.lens_word }
    fn channel_bias(&mut self, _resource: u32) -> i32 { 0 }
}

impl LensCalibration for Capture {
    fn calibration(&mut self, lens: u32, index: u32) -> i16 { self.calibrations.get(&(lens, index)).copied().unwrap_or(0) }
}

struct Posts(Vec<CalcPayload>);

impl Peers for Posts {
    type Error = ();
    fn calc(&mut self, payload: &CalcPayload) -> Result<(), ()> {
        self.0.push(*payload);
        Ok(())
    }
    fn calc1(&mut self, _payload: &Calc1Payload) -> Result<(), ()> { Err(()) }
    fn store_acquisition_grid(&mut self, _grid: &AcquisitionGrid) {}
}

fn processing() -> CaptureProcessing {
    CaptureProcessing { extent: [64, 48], level: 300, reference: 410, decade: 7, setting: 9, profile: 12, lens: 3, lens_reference: 900, offset_mode: 0, ..CaptureProcessing::default() }
}

fn only(posts: Posts) -> CalcPayload {
    assert_eq!(posts.0.len(), 1);
    posts.0[0]
}

#[test]
fn the_first_request_carries_the_record_value_and_the_processing() {
    let mut capture = Capture::default();
    capture.world.set(&group_00::AT_0022, -40);
    let mut posts = Posts(Vec::new());
    request_first(&mut capture, &mut posts, handle(), &processing(), CALLBACK).unwrap();
    let CalcPayload::CaptureFirst { request, callback } = only(posts) else { panic!("not 0x3c0f") };
    assert_eq!((request.at_0022, request.processing, callback), (-40, processing(), CALLBACK));
}

#[test]
fn the_third_request_carries_the_scene_and_the_calibration_of_the_record_lens_and_lens_zero() {
    let mut capture = Capture::default();
    capture.world.header(SharedHeaderByte::At31, 5);
    capture.world.set_shared(&group_27::AT_0740, 2);
    capture.world.set(&group_05::AT_0170.at(FIRST_ELEMENT), 6);
    capture.calibrations.insert((6, 0), 1200);
    capture.calibrations.insert((0, 0), -300);
    let mut posts = Posts(Vec::new());
    request_third(&mut capture, &mut posts, handle(), &processing(), 0xd1, CALLBACK).unwrap();
    let CalcPayload::CaptureThird { request, .. } = only(posts) else { panic!("not 0x3c0e") };
    assert_eq!((request.scene, request.scene_flags, request.calibration, request.detail), (5, 2, [1200, -300], 0xd1));
}

#[test]
fn the_capture_offset_adds_the_configured_adjustment_only_under_scene_flags_two() {
    let mut capture = Capture::default();
    capture.world.records.write(handle(), &group_04::AT_0160_4, 1000).unwrap();
    capture.world.property(crate::stock::capture::CAPTURE_OFFSET_ADJUSTMENT, 25);
    assert_eq!(capture_offset(&mut capture, handle()), 1000);
    capture.world.set_shared(&group_27::AT_0740, 2);
    assert_eq!(capture_offset(&mut capture, handle()), 1025);
    capture.world.set(&group_00::AT_0069, 1);
    capture.world.records.write(handle(), &group_00::AT_002C, 77).unwrap();
    assert_eq!(capture_offset(&mut capture, handle()), 102);
}

#[test]
fn the_fourth_request_is_a_source_reading_over_the_capture_statistics() {
    let mut capture = Capture::default();
    capture.world.records.write(handle(), &group_04::AT_0160_4, 500).unwrap();
    capture.world.set(&group_00::AT_001C, 11);
    capture.world.set(&group_01::AT_00A0, -12);
    capture.world.select(MeasurementSelection::Record);
    let processing = CaptureProcessing { offset_mode: 2, ..processing() };
    let mut posts = Posts(Vec::new());
    request_fourth(&mut capture, &mut posts, handle(), &processing, 0xd4, CALLBACK).unwrap();
    let CalcPayload::Source { request, .. } = only(posts) else { panic!("not 0x3c11") };
    assert_eq!(request.statistics, SourceStatistics::Capture);
    assert_eq!((request.profile, request.decade, request.profile_halfword, request.level), ([500, 9], 7, 11, (-12i16) as u16));
    assert_eq!((request.reference, request.size, request.exposure, request.offset_mode), (410, [64, 48], 900, 2));
    assert_eq!((request.mode, request.selection, request.extension), (12, 1, 0xd4));
}

#[test]
fn a_refinement_record_measures_person_scenes_as_portrait_unless_configured() {
    let mut world = World::default();
    assert_eq!(refined_scene(&mut world, 19, 0), RefinedSceneClass { scene: 19, class: 0 });
    assert_eq!(refined_scene(&mut world, 19, 1), RefinedSceneClass { scene: 1, class: 18 });
    assert_eq!(refined_scene(&mut world, 18, 1), RefinedSceneClass { scene: 18, class: 18 });
    assert_eq!(refined_scene(&mut world, 4, 1), RefinedSceneClass { scene: 1, class: 0 });
    assert_eq!(refined_scene(&mut world, 41, 1), RefinedSceneClass { scene: 1, class: 40 });
    assert_eq!(refined_scene(&mut world, 48, 1), RefinedSceneClass { scene: 0, class: 0 });
    assert_eq!(refined_scene(&mut world, 15, 1), RefinedSceneClass { scene: 1, class: 0 });
    world.property(Property(0x7951), 1);
    world.property(Property(0x7954), 1);
    assert_eq!(refined_scene(&mut world, 15, 1), RefinedSceneClass { scene: 12, class: 0 });
    assert_eq!(refined_scene(&mut world, 20, 1), RefinedSceneClass { scene: 18, class: 18 });
}

#[test]
fn the_second_request_refines_the_scene_and_gates_the_offset_fields() {
    let mut capture = Capture::default();
    capture.world.header(SharedHeaderByte::At31, 26);
    capture.world.header(SharedHeaderByte::At63, 1);
    for (array, value) in [(&group_05::AT_0178, 1), (&group_05::AT_017A, 2), (&group_05::AT_017C, 3), (&group_05::AT_0186, 4)] {
        capture.world.set(&array.at(FIRST_ELEMENT), value);
    }
    capture.world.set(&group_01::AT_00A4, 10);
    capture.world.set(&group_01::AT_00BC, 77);
    capture.world.reference_offsets.insert(77, 5);
    capture.world.set(&group_08::AT_01F2.at(FIRST_ELEMENT), 100);
    capture.world.set(&group_07::AT_01B0.at(FIRST_ELEMENT), 0xb0);
    capture.world.set(&group_07::AT_01B4.at(FIRST_ELEMENT), 0xb4);
    capture.world.set(&group_07::AT_01EC.at(FIRST_ELEMENT), 0xec);
    capture.world.set_shared(&group_27::WINDOW_STATUS, WindowStatus(4));
    let measurement = CaptureMeasurement { base: 3, count: 2, ..CaptureMeasurement::default() };

    let mut posts = Posts(Vec::new());
    request_second(&mut capture, &mut posts, handle(), &processing(), &measurement, 0xd2, CALLBACK).unwrap();
    let CalcPayload::CaptureSecond { request, .. } = only(posts) else { panic!("not 0x3c10") };
    assert_eq!((request.scene, request.scene_class, request.header_63), (1, 25, 1));
    assert_eq!((request.level_total, request.lens_level), (20, 105));
    assert_eq!((request.offsets, request.at_01ec), ([0, 0], 0));
    assert!(request.window_measured);
    assert_eq!((request.measurement, request.detail), (measurement, 0xd2));

    let offset_processing = CaptureProcessing { offset_mode: 2, ..processing() };
    capture.world.property(crate::stock::capture::KEEPS_OFFSET_FIELD, 1);
    let mut posts = Posts(Vec::new());
    request_second(&mut capture, &mut posts, handle(), &offset_processing, &measurement, 0, CALLBACK).unwrap();
    let CalcPayload::CaptureSecond { request, .. } = only(posts) else { panic!("not 0x3c10") };
    assert_eq!((request.offsets, request.at_01ec), ([0xb0, 0xb4], 0xec));
}

#[test]
fn the_area_evaluation_takes_the_lens_temperature_and_the_lens_level_at_the_processing_reference() {
    let mut capture = Capture::default();
    capture.world.lens_levels.insert((3, 900), 50);
    capture.world.level_offsets.insert(12, -8);
    capture.world.set(&group_05::AT_0174.at(FIRST_ELEMENT), 20);
    capture.world.set(&group_01::AT_00A4, 10);
    capture.world.set(&group_05::AT_0178.at(FIRST_ELEMENT), 1);
    let geometry = AreaGeometry { count: 1, ..AreaGeometry::default() };
    let callbacks = [StockCallbackWord(0x00f2_acd0), StockCallbackWord(0x00f2_ac98)];
    let mut temperatures = Vec::new();
    for word in [0x0000_00f0, 0x0000_0040, 0x8000_0010, 0x0000_0012] {
        capture.lens_word = word;
        let mut posts = Posts(Vec::new());
        request_area_evaluation(&mut capture, &mut posts, handle(), &processing(), [0xa0, 0xa1], &geometry, callbacks).unwrap();
        let CalcPayload::AreaEvaluation { request, callbacks: posted } = only(posts) else { panic!("not 0x3c0d") };
        assert_eq!((request.lens_level, request.level_offset, request.level_total), (50 + 300 - 20, -8, 1));
        assert_eq!((request.frames, request.geometry, posted), ([0xa0, 0xa1], geometry, callbacks));
        temperatures.push(request.lens_temperature);
    }
    assert_eq!(temperatures, [-5, 55, 25, 18]);
}
