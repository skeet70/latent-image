use std::collections::BTreeMap;
use std::vec::Vec;

use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::configuration::{Configuration, ControllerFlag, FrameInterval};
use crate::adjust::drive_step::*;
use crate::adjust::focus_profile::LensResources;
use crate::adjust::lens::{LensSteps, Step, StepRequest};
use crate::adjust::motion::drive::{DriveKind, DriveMode};
use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::{Lens, ProfileAccess, Properties, Property};
use crate::profile::Profile;

const DRIVE: DriveKind = DriveKind(4);
const CHANNEL: u32 = 2;
const TIMING: u32 = 0x30;

/// The lens table answers a step of half the base at a hundred times the base, and a distance of
/// twice the travel.
struct Steps {
    world: World,
    resource: Option<u32>,
    intervals: BTreeMap<u32, u32>,
    requests: Vec<StepRequest>,
    travels: Vec<(u32, u32, i16)>,
}

impl Properties for Steps {
    fn property(&mut self, property: Property) -> u32 { Properties::property(&mut self.world, property) }
}

impl ProfileAccess for Steps {
    type Records = <World as ProfileAccess>::Records;
    type Entries = <World as ProfileAccess>::Entries;
    fn profile(&mut self) -> Profile<'_, Self::Records, Self::Entries> { self.world.profile() }
}

impl Lens for Steps {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.world.level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.world.base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.world.reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.world.calibration_index(lens) }
}

impl LensResources for Steps {
    fn resource(&mut self) -> Option<u32> { self.resource }
    fn reference(&mut self) -> u32 { 0 }
    fn flags(&mut self) -> u32 { 0 }
    fn channel_bias(&mut self, _resource: u32) -> i32 { 0 }
}

impl LensSteps for Steps {
    fn step(&mut self, request: StepRequest) -> Step {
        self.requests.push(request);
        Step { size: request.base / 2, rate: u32::from(request.base) * 100 }
    }
    fn travel_distance(&mut self, resource: u32, index: u32, travel: i16) -> u16 {
        self.travels.push((resource, index, travel));
        (travel as u16).wrapping_mul(2)
    }
}

impl FrameInterval for Steps {
    fn frame_interval(&mut self, timing: u32) -> Option<u32> { self.intervals.get(&timing).copied() }
}

fn steps() -> Steps {
    let mut world = World::default();
    world.calibration_indices.insert(CHANNEL, 11);
    for (id, value) in [(0x87df, 1), (0x87e1, 2), (0x87eb, 3), (0x87ed, 4), (0x87e5, 5), (0x87e7, 6), (0x8882, 7), (0x8884, 8), (0x8883, 9)] {
        world.property(Property(id), value);
    }
    Steps { world, resource: Some(6), intervals: [(TIMING, 40_000)].into(), requests: Vec::new(), travels: Vec::new() }
}

fn query(kind: StepKind, contrast: u32) -> StepQuery { StepQuery { kind, channel: CHANNEL, contrast, timing: TIMING, drive_kind: DRIVE } }

fn base_of(steps: &mut Steps, configuration: &Configuration, kind: StepKind, contrast: u32) -> u16 {
    step(steps, configuration, handle(), query(kind, contrast));
    steps.requests.last().unwrap().base
}

const PHASES: [StepKind; 6] = [StepKind::Direction, StepKind::Scan, StepKind::Return, StepKind::ContinuousStart, StepKind::ContinuousPeak, StepKind::ContinuousTrack];

#[test]
fn each_search_phase_takes_ten_times_its_configured_step_and_the_record_selected_one_where_it_has_one() {
    let mut steps = steps();
    let configuration = Configuration::default();
    assert_eq!(PHASES.map(|kind| base_of(&mut steps, &configuration, kind, 0)), [10, 30, 50, 70, 80, 90]);
    steps.world.select(MeasurementSelection::Record);
    assert_eq!(PHASES.map(|kind| base_of(&mut steps, &configuration, kind, 0)), [20, 40, 60, 70, 80, 90]);
}

#[test]
fn the_scan_and_the_continuous_peak_search_take_the_low_contrast_step_below_the_probe_contrast() {
    let mut steps = steps();
    let configuration = Configuration::default();
    steps.world.property(Property(0x8814), 50);
    steps.world.property(Property(0x87f1), 12);
    steps.world.property(Property(0x87f2), 13);
    assert_eq!(PHASES.map(|kind| base_of(&mut steps, &configuration, kind, 499)), [10, 120, 50, 70, 120, 90]);
    assert_eq!(base_of(&mut steps, &configuration, StepKind::Scan, 500), 30);
    steps.world.select(MeasurementSelection::Record);
    assert_eq!(base_of(&mut steps, &configuration, StepKind::Scan, 499), 120);
}

#[test]
fn the_lens_table_is_asked_for_the_lens_resource_and_index_at_the_shifted_frame_interval() {
    let mut steps = steps();
    steps.world.property(Property(0x87da), 2);
    let answer = step(&mut steps, &Configuration::default(), handle(), query(StepKind::Scan, 0));
    assert_eq!(answer, Step { size: 15, rate: 3000 });
    let request = StepRequest { resource: 6, index: 11, base: 30, period: 40_000 / 4 / 100, kind: DRIVE, mode: DriveMode { direction: 0, step: 9 } };
    assert_eq!(steps.requests, [request]);
}

#[test]
fn the_fine_step_flag_asks_for_the_fine_drive_mode() {
    let mut steps = steps();
    let mut configuration = Configuration::default();
    configuration.set_controller_flag(ControllerFlag::First, true);
    step(&mut steps, &configuration, handle(), query(StepKind::Direction, 0));
    assert_eq!(steps.requests[0].mode, DriveMode { direction: 1, step: 10 });
}

#[test]
fn without_a_resource_a_frame_interval_or_with_the_interval_shifted_out_the_table_reads_zero() {
    let mut steps = steps();
    steps.resource = None;
    steps.world.property(Property(0x87da), 32);
    step(&mut steps, &Configuration::default(), handle(), query(StepKind::Direction, 0));
    steps.world.property(Property(0x87da), 0);
    steps.intervals.clear();
    step(&mut steps, &Configuration::default(), handle(), query(StepKind::Direction, 0));
    assert_eq!(steps.requests.iter().map(|request| (request.resource, request.period)).collect::<Vec<_>>(), [(0, 0), (0, 0)]);
}

#[test]
fn a_monitored_step_grows_by_the_mark_in_thousandths_and_takes_the_resources_scale_in_hundredths() {
    let mut world = World::default();
    world.property(Property(0x8942 + 3), 50);
    world.property(Property(0x8969 + 3), 200);
    assert_eq!(monitored_step(&mut world, 3, Monitor { moved: true, mark: -500 }, 40), 30);
    assert_eq!(monitored_step(&mut world, 3, Monitor { moved: false, mark: 500 }, 40), 120);
    world.property(Property(0x8942 + 3), 10);
    assert_eq!(monitored_step(&mut world, 3, Monitor { moved: true, mark: 0 }, -15), -1);
}

fn scan_query(monitor: Option<Monitor>, contrast: u32) -> ScanStepQuery {
    ScanStepQuery { monitor, index: 11, resource: 6, interval: 40_000, contrast, drive_kind: DRIVE }
}

#[test]
fn the_scan_step_reports_its_base_the_table_step_and_the_travel_per_frame() {
    let mut steps = steps();
    let scan = scan_step(&mut steps, &Configuration::default(), handle(), scan_query(None, 0));
    let tenths = 40_000 * 3000 / 100_000;
    assert_eq!(scan, ScanStep { base: 30, step: Step { size: 15, rate: 3000 }, travel: (tenths / 10) as u16, distance: 2 * tenths as u16 });
    assert_eq!(steps.requests[0], StepRequest { resource: 6, index: 11, base: 30, period: 400, kind: DRIVE, mode: DriveMode { direction: 0, step: 9 } });
    assert_eq!(steps.travels, [(6, 11, tenths as i16)]);
}

#[test]
fn a_monitored_scan_scales_the_return_step_the_table_gives() {
    let mut steps = steps();
    steps.world.property(Property(0x8969 + 6), 200);
    let scan = scan_step(&mut steps, &Configuration::default(), handle(), scan_query(Some(Monitor { moved: false, mark: 0 }), 0));
    assert_eq!((scan.base, scan.step), (50, Step { size: 50, rate: 5000 }));
}

#[test]
fn a_low_contrast_scan_step_takes_the_record_selected_low_step_for_a_selected_measurement() {
    let mut steps = steps();
    steps.world.property(Property(0x8814), 50);
    steps.world.property(Property(0x87f1), 12);
    steps.world.property(Property(0x87f2), 13);
    assert_eq!(scan_step(&mut steps, &Configuration::default(), handle(), scan_query(None, 499)).base, 120);
    steps.world.select(MeasurementSelection::Record);
    assert_eq!(scan_step(&mut steps, &Configuration::default(), handle(), scan_query(None, 499)).base, 130);
}
