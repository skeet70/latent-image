use std::cell::RefCell;
use std::rc::Rc;

use xm1_saclac_interface::pipeline::FormatPayload;

use crate::controller::lifecycle::LifecycleRefusal;
use crate::controller::{Lifecycle, Operation, OperationState, ProfileGeometry, Refusal};
use crate::memory::{Bytes, Location, Memory};
use crate::numeric::FractionTables;
use crate::ports::*;
use crate::profile::*;
use crate::controller::changes::{Stage, Subscription};
use crate::controller::queries::*;
use crate::controller::windows::GridRequest;
use crate::stock::layout::{callbacks, changes, controller as field, diagnostics, display, dump, parameters, policy, requests, work, work::records};
use crate::controller::scan::ProcessingRequest;
use crate::services::{DumpKind, ExtendedOperations, LevelPair, OperationConditions, SelectionRequest, SelectionValues, WindowPower};
use crate::record_mode::RecordChannel;
use crate::stock::layout::record_mode;
use crate::controller::statistics::{RecordHalf, StatisticsSummary, Summary, SummaryAreas, SummaryRequest};
use crate::stock::statistics::{write_summary, COPY_OFFSET, HALF_BYTES, RECORD_BYTES};
use crate::stock::Parameters;
use crate::geometry::*;
use crate::stock::areas::TransitionAreas;
use crate::stock::layout::geometry as service;

#[derive(Clone, Debug, Eq, PartialEq)]
enum Call {
    Stop(u32, u32),
    Release(u32, u32),
    Settle,
    CaptureReleased,
    Disable(u32, u32),
    Attach(HandlerUnit),
    Detach(HandlerUnit, u32),
    Notified(Operation, u32),
    Reserve(u32, u32, u32),
    Register(u32, u32, u32),
    ReleaseRegion(u32),
    Apply(u32, u32),
    Configure(u32, u32),
    Format(u16, u16),
    GeometryStart(GeometryStart),
    GeometryNotified(u32, i32),
    AreaQuery(u32, AreaQuery),
    Point(u32, u32, StatisticsDestinations),
    Collect(u32, u32, Location),
}

/// Calls to the ports `Call` does not record.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Out {
    ClearScan(Location),
    DefaultScan(Location),
    ChannelCode(u32),
    Export(Location, ScanExport, u32),
    Route(u32, u32),
    Wait(u32),
    Task(u32, u32),
    Display(&'static str),
    FileName(Location),
    Copy(Location, Location),
    Append(Location, &'static [u8]),
    Open(Location),
    Write(u32, u32, u32),
    Close(u32),
    Finish(u32),
    Numbers(u32),
}

/// Stock memory the fake storage service's completions write while the layer waits.
#[derive(Clone, Default)]
struct SharedBytes(Rc<RefCell<Bytes>>);

impl Memory for SharedBytes {
    fn read_u8(&self, at: Location) -> u8 { self.0.borrow().read_u8(at) }
    fn read_u16(&self, at: Location) -> u16 { self.0.borrow().read_u16(at) }
    fn read_u32(&self, at: Location) -> u32 { self.0.borrow().read_u32(at) }
    fn write_u8(&mut self, at: Location, value: u8) { self.0.borrow_mut().write_u8(at, value) }
    fn write_u16(&mut self, at: Location, value: u16) { self.0.borrow_mut().write_u16(at, value) }
    fn write_u32(&mut self, at: Location, value: u32) { self.0.borrow_mut().write_u32(at, value) }
}

#[derive(Default)]
struct Fake {
    calls: Vec<Call>,
    refuse_stop: bool,
    config: Vec<(u32, u8)>,
    halfwords: Vec<(u32, u16)>,
    window: Rectangle,
    status: u32,
    dumped: Vec<(u32, u32, Vec<u8>)>,
    policy_dumps: Vec<PolicyDump>,
    settings: Vec<(Location, ScanSetting)>,
    out: Vec<Out>,
    display_refuses: Option<&'static str>,
    files: Option<Rc<RefCell<Bytes>>>,
    /// Each file request's completion status in turn; zero once used up.
    file_statuses: Vec<u32>,
    completion: Option<(FileCompletion, u32, u32)>,
    /// Collections so far; each writes its count into the top bits of every result halfword.
    collections: u16,
}

impl Configuration for Fake {
    fn byte(&mut self, offset: ConfigOffset) -> u8 { self.config.iter().find(|(at, _)| *at == offset.0).map_or(0, |(_, value)| *value) }
    fn halfword(&mut self, offset: ConfigOffset) -> u16 {
        self.halfwords.iter().find(|(at, _)| *at == offset.0).map_or(0, |(_, value)| *value)
    }
    fn signed_halfword(&mut self, _: ConfigOffset) -> i16 { 0 }
    fn collapse_index(&mut self, value: u32) -> u32 { value }
    fn transform(&mut self, value: u32, scale: u32) -> u16 { (value * 10 + scale) as u16 }
    fn split_index(&mut self) -> u8 { 4 }
}

impl Pipeline for Fake {
    fn stop(&mut self, channel: PipelineChannel, stages: Stages) -> bool {
        self.calls.push(Call::Stop(channel.0, stages.0));
        !self.refuse_stop
    }
    fn release(&mut self, channel: PipelineChannel, stages: Stages) { self.calls.push(Call::Release(channel.0, stages.0)) }
    fn settle(&mut self) { self.calls.push(Call::Settle) }
    fn release_capture_hardware(&mut self) { self.calls.push(Call::CaptureReleased) }
    fn release_configuration_hardware(&mut self) {}
    fn enable(&mut self, _: PipelineChannel, _: Stages, _: Location) -> bool { true }
    fn disable(&mut self, channel: PipelineChannel, stages: Stages) -> bool {
        self.calls.push(Call::Disable(channel.0, stages.0));
        true
    }
    fn apply(&mut self, channel: PipelineChannel, stages: Stages, _: Location) -> bool {
        self.calls.push(Call::Apply(channel.0, stages.0));
        true
    }
    fn configure(&mut self, channel: PipelineChannel, stages: Stages, _: Location) -> bool {
        self.calls.push(Call::Configure(channel.0, stages.0));
        true
    }
    fn restore_stages(&mut self, _: Location, _: Stages) {}
    fn point_statistics(&mut self, channel: PipelineChannel, stages: Stages, destinations: &StatisticsDestinations) {
        self.calls.push(Call::Point(channel.0, stages.0, *destinations));
    }
    fn collect_statistics(&mut self, channel: PipelineChannel, stages: Stages, into: Location) {
        self.calls.push(Call::Collect(channel.0, stages.0, into));
        self.collections += 1;
        if let Some(memory) = &self.files {
            let mut memory = SharedBytes(memory.clone());
            for offset in (0..RESULT_BYTES).step_by(2) {
                memory.write_u16(into.plus(offset), self.collections << 12 | offset);
            }
        }
    }
}

const RESULT_BYTES: u16 = 0x190;

impl StagePayloads for Fake {
    fn mode_payload(&mut self, _: &ModeRequest, _: Location) -> Stages { Stages(0) }
    fn tertiary_payload(&mut self, _: PipelineGroup, _: u8, _: u8, _: Location) -> Stages { Stages(0) }
    fn set_format(&mut self, payload: &FormatPayload) -> bool {
        self.calls.push(Call::Format(payload.first_scale, payload.second_scale));
        true
    }
    fn quad_selection_offset(&mut self, _: PipelineGroup) -> Option<ConfigOffset> { Some(ConfigOffset(0x1de5)) }
    fn quad_gains(&mut self, _: QuadGroup, _: i16, _: bool) -> [u16; 4] { [11, 22, 33, 44] }
    fn quad_area_scale(&mut self, _: u32) -> u32 { 0 }
    fn quad_area_limit(&mut self) -> u16 { 0 }
    fn primary_payload(&mut self, _: PipelineGroup, _: Option<PrimaryWindow>) -> Option<PrimaryPayload> { None }
    fn secondary_payload(&mut self, _: PipelineGroup, _: Option<SecondaryWindow>) -> Option<SecondaryPayload> { None }
    fn has_windowed_payload(&mut self, _: PipelineGroup) -> bool { false }
}

impl ScanConfiguration for Fake {
    fn set(&mut self, scan: Location, setting: ScanSetting) { self.settings.push((scan, setting)) }
    fn clear_scan(&mut self, scan: Location) { self.out.push(Out::ClearScan(scan)) }
    fn default_scan(&mut self, scan: Location) { self.out.push(Out::DefaultScan(scan)) }
    fn channel_code(&mut self, channel: u32, codes: &'static [u16; 10]) -> u16 {
        self.out.push(Out::ChannelCode(channel));
        codes[channel.min(9) as usize] + 100
    }
    fn export_scan(&mut self, scan: Location, part: ScanExport, out: StockData) { self.out.push(Out::Export(scan, part, out.0)) }
    fn compile_scan_configuration(&mut self, _: u8) {}
    fn apply_selection(&mut self, _: Location, _: Location) {}
    fn attach_scan_configuration(&mut self, _: Location, _: Stages) {}
    fn rate_code(&mut self, _: u32) -> u32 { 0 }
    fn limit_tenths(&mut self, _: i16) -> u16 { 0 }
    fn output_level(&mut self) -> u32 { 0 }
    fn channel_count(&mut self) -> u8 { 1 }
}

impl RadialGeometry for Fake {
    fn radial_model(&mut self, _: &[u32; 31]) -> RadialModel { RadialModel::default() }
    fn fit_compact(&mut self, _: (u32, u32), _: &RadialModel, _: &mut RadialBuffer) -> bool { false }
    fn radial_bound(&mut self, _: (u32, u32)) -> u16 { 0 }
    fn radial_centre(&mut self, _: &mut RadialModel) -> u32 { 0 }
    fn radial_extent(&mut self, _: (u32, u32), _: u32, _: u16, _: u16) -> u16 { 0 }
    fn radial_table(&mut self, _: (u32, u32), _: [u32; 2], _: &RadialModel, _: u16, _: [u16; 2], _: u16) -> u16 { 0 }
}

impl Handlers for Fake {
    fn attach(&mut self, unit: HandlerUnit) -> Result<u32, u32> {
        self.calls.push(Call::Attach(unit));
        Ok(0x55)
    }
    fn detach(&mut self, unit: HandlerUnit, handle: u32) -> u32 {
        self.calls.push(Call::Detach(unit, handle));
        0
    }
    fn notify_stopped(&mut self, operation: Operation, callback: StockCode) { self.calls.push(Call::Notified(operation, callback.0)) }
}

impl DumpRegions for Fake {
    fn reserve_region(&mut self, region: u32, address: u32, bytes: u32) { self.calls.push(Call::Reserve(region, address, bytes)) }
    fn register_region(&mut self, region: u32, address: u32, bytes: u32) { self.calls.push(Call::Register(region, address, bytes)) }
    fn release_region(&mut self, region: u32) { self.calls.push(Call::ReleaseRegion(region)) }
}

fn label(text: &[u8], into: &mut [u8]) -> usize {
    into[..text.len()].copy_from_slice(text);
    text.len()
}

impl DiagnosticLabels for Fake {
    fn profile_label(&mut self, _: ProfileHandle, into: &mut [u8]) -> usize { label(b"REC1", into) }
    fn feature_label(&mut self, feature: u8, into: &mut [u8]) -> usize {
        label(if feature == 3 { b"_16MIX" } else { b"_2MIX" }, into)
    }
    fn fixed_label(&mut self, into: &mut [u8]) -> usize { label(b"FIXED", into) }
    fn rate_separator(&mut self, into: &mut [u8]) -> usize { label(b".", into) }
    fn rate_suffix(&mut self, into: &mut [u8]) -> usize { label(b"FPS/", into) }
}

impl ConfiguredRate for Fake {
    fn configured_rate(&mut self) -> u16 { 2997 }
}

impl Retained for Fake {
    fn limit_triplet(&mut self, triplet: [u16; 3]) -> [u16; 3] { triplet }
    fn expanded_rectangle(&mut self, _: ProfileHandle) -> Rectangle { Rectangle::default() }
    fn configured_rectangle(&mut self, _: ProfileHandle) -> Rectangle { Rectangle::default() }
    fn relative_rectangle(&mut self, _: ProfileHandle, _: bool) -> Rectangle { self.window }
    fn encode_thresholds(&mut self, _: i16, _: i16) -> u32 { 0 }
    fn converted_decade(&mut self, _: u32) -> i32 { 0 }
    fn statistics_offset(&mut self, region: StatisticsRegion) -> u32 { region.0 * 0x1000 }
    fn selection_limit(&mut self) -> u32 { 7 }
    fn profile_factors(&mut self, selection: u32) -> [u16; 2] { [selection as u16, 2 * selection as u16] }
    fn calibration_factors(&mut self, first: u32, second: u32) -> [u16; 3] { [first as u16, second as u16, 0x400] }
    fn configured_decade(&mut self, offset: u8) -> i16 { 10 * offset as i16 }
    fn decode_level(&mut self, encoded: u32) -> u8 { encoded as u8 + 1 }
}

impl StatisticsMemory for Fake {
    fn statistics_word(&mut self, buffer: StatisticsBuffer, offset: u32) -> u32 { buffer.0 + offset }
    fn low_word(&mut self, address: LowAddress) -> u32 { 0x1000 + address.0 as u32 }
}

impl DeviceRegisters for Fake {
    fn register(&mut self, register: Register) -> u32 { if register.0 == 0xfff8_802c { self.status } else { register.0 & 0xff } }
}

impl FrameDumps for Fake {
    fn write_dump(&mut self, buffer: DumpBuffer, offset: u32, bytes: &[u8]) { self.dumped.push((buffer.0, offset, bytes.to_vec())) }
    fn dump_policy(&mut self, _: DumpBuffer, dump: PolicyDump, _: Location) { self.policy_dumps.push(dump) }
}

impl OutputRoutes for Fake {
    fn set_requested_level(&mut self, route: u32, level: u32) -> i32 {
        self.out.push(Out::Route(route, level));
        0
    }
}

impl Interface for Fake {
    fn read_adc_byte(&mut self, channel: u32) -> AdcReading { AdcReading { status: 0, value: channel as u8 * 3 } }
    fn wait_ticks(&mut self, ticks: u32) {
        self.out.push(Out::Wait(ticks));
        if let (Some(memory), Some(completion)) = (&self.files, self.completion.take()) {
            let mut memory = SharedBytes(memory.clone());
            match completion {
                (FileCompletion::Opened, status, handle) => crate::services::file_opened(&mut memory, status, handle),
                (FileCompletion::Done, status, _) => crate::services::file_done(&mut memory, status),
            }
        }
    }
    fn configure_task(&mut self, task: u32, priority: u32) -> i32 {
        self.out.push(Out::Task(task, priority));
        0
    }
}

impl PeakService for Fake {
    fn read_selection(&mut self) {}
    fn notify_return(&mut self) {}
    fn state(&mut self) -> u32 { self.status }
}

impl LensValues for Fake {
    fn lens_value(&mut self, index: u32) -> i16 { 100 - index as i16 }
}

impl ProfileResources for Fake {
    fn resource_byte(&mut self, record: u32) -> u8 { record as u8 }
}

impl Fake {
    fn display(&mut self, call: &'static str) -> bool {
        self.out.push(Out::Display(call));
        self.display_refuses != Some(call)
    }

    fn file_request(&mut self, completion: FileCompletion, request: Out) {
        self.out.push(request);
        let status = if self.file_statuses.is_empty() { 0 } else { self.file_statuses.remove(0) };
        self.completion = Some((completion, status, 0x77));
    }
}

impl DisplayDriver for Fake {
    fn write_selected_quad(&mut self, _: u32, _: Location) -> bool { self.display("quad") }
    fn write_selected_channel(&mut self, _: u32, _: Location) -> bool { self.display("channel") }
    fn apply_scale_layout(&mut self, _: Location) -> bool { self.display("layout") }
    fn graphic_service(&mut self, _: u32) -> bool { self.display("graphic") }
    fn update_configuration_bits(&mut self, _: u32) -> bool { self.display("bits") }
    fn forward_selection(&mut self, _: Location) -> bool { self.display("forward") }
    fn apply_selected(&mut self, _: u32) -> bool { self.display("selected") }
    fn set_selected_colour(&mut self, _: u32) { self.display("colour"); }
    fn forward_selected_value(&mut self, _: u32) -> bool { self.display("value") }
    fn on_selected_event(&mut self, _: StockCode) -> bool { self.display("event") }
}

impl DumpFiles for Fake {
    fn next_file_name(&mut self, name: Location) { self.out.push(Out::FileName(name)) }
    fn copy_text(&mut self, to: Location, from: Location) { self.out.push(Out::Copy(to, from)) }
    fn append_text(&mut self, to: Location, text: &'static [u8]) { self.out.push(Out::Append(to, text)) }
    fn open_file(&mut self, completion: FileCompletion, path: Location) -> u32 {
        self.file_request(completion, Out::Open(path));
        0x99
    }
    fn write_file(&mut self, completion: FileCompletion, handle: u32, length: u32, data: StockData) {
        self.file_request(completion, Out::Write(handle, length, data.0))
    }
    fn close_file(&mut self, completion: FileCompletion, handle: u32) { self.file_request(completion, Out::Close(handle)) }
    fn finish_file(&mut self, completion: FileCompletion, handle: u32) { self.file_request(completion, Out::Finish(handle)) }
    fn read_file_numbers(&mut self, numbers: StockData) { self.out.push(Out::Numbers(numbers.0)) }
}

impl GeometryService for Fake {
    fn start_geometry(&mut self, start: &GeometryStart, _: StockCode) -> bool {
        self.calls.push(Call::GeometryStart(*start));
        !self.refuse_stop
    }
    fn stop_geometry(&mut self) -> bool { true }
    fn query_areas(&mut self, code: u32, query: &AreaQuery) -> AreaAnswer {
        self.calls.push(Call::AreaQuery(code, *query));
        let mut answer = TransitionAreas::EMPTY;
        answer.count = 1;
        answer.areas[0][0x06..0x08].copy_from_slice(&5u16.to_le_bytes());
        answer.areas[0][0x04..0x06].copy_from_slice(&100u16.to_le_bytes());
        AreaAnswer::Areas(answer)
    }
    fn notify_geometry(&mut self, callback: StockCode, status: i32) { self.calls.push(Call::GeometryNotified(callback.0, status)) }
}

struct Tables {
    feature: u8,
    kind: u8,
    flags: u16,
    class: GeometryClass,
    slot: ScanSlot,
    policy: u32,
}

impl ProfileTables for Tables {
    fn entry(&self, _: ProfileHandle) -> ProfileEntry {
        ProfileEntry { class: ClassIndex(0), kind: ProfileKind(self.kind), flags: self.flags, threshold_index: 3, feature: self.feature, aspect_index: 0 }
    }
    fn class(&self, _: ClassIndex) -> GeometryClass { self.class }
    fn threshold(&self, index: u8) -> u16 { index as u16 * 5 }
    fn default_pair(&self) -> ScalePair { ScalePair { first: 2, second: 2 } }
    fn expansion(&self) -> ExpansionRatio { ExpansionRatio { numerator: 1, divisor: 1 } }
    fn curve_knot(&self, index: usize) -> i16 { 1000 + 50 * index as i16 }
    fn feature_scale_offsets(&self, _: u32) -> [u32; 2] { [0, 0] }
    fn row(&self, index: u32) -> ProfileRow {
        ProfileRow { rectangle_row: 16, pipeline_group: PipelineGroup(index), policy: self.policy, quad_group: QuadGroup(1) }
    }
    fn aspect_value(&self, _: u8) -> i16 { 0 }
    fn rectangle_configuration(&self, _: RectangleKind, _: u32) -> [u32; 4] { [0; 4] }
    fn scan_slot(&self, _: u32) -> ScanSlot { self.slot }
    fn record_code(&self, index: u32) -> [u16; 2] { [index as u16; 2] }
}

impl FractionTables for Tables {
    fn fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 256.0).log2()) as u8 }
    fn wide_fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 100.0).log2()) as u8 }
}

type Layer = ProfileGeometry<Bytes, Fake, Tables>;

fn layer(feature: u8, kind: u8) -> Layer {
    let tables = Tables { feature, kind, flags: 0, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 1 };
    ProfileGeometry::new(Bytes::default(), Fake::default(), Profiles::new(tables))
}

/// A layer whose profile class scales by one and whose window is `window`.
fn framed(window: Rectangle, class: GeometryClass, slot: ScanSlot) -> Layer {
    let tables = Tables { feature: 0, kind: 0, flags: 0, class: GeometryClass { first_scale: 1, second_scale: 1, ..class }, slot, policy: 1 };
    ProfileGeometry::new(Bytes::default(), Fake { window, ..Fake::default() }, Profiles::new(tables))
}

fn open(feature: u8, kind: u8) -> Layer {
    let mut layer = layer(feature, kind);
    layer.memory.write_u32(field::LIFECYCLE, Lifecycle::Open.word());
    layer
}

fn description(layer: &Layer) -> Vec<u8> {
    let mut bytes = [0; diagnostics::DESCRIPTION_BYTES];
    layer.memory().read_bytes(diagnostics::DESCRIPTION, &mut bytes);
    bytes.iter().copied().take_while(|&byte| byte != 0).collect()
}

#[test]
fn initialize_closes_the_controller_and_declares_both_dump_regions() {
    let mut layer = layer(0, 0);
    layer.initialize();
    assert_eq!(layer.lifecycle(), Some(Lifecycle::Closed));
    assert_eq!(layer.memory().read_u32(field::PIPELINE_CHANNEL), 6);
    assert_eq!(layer.memory().read_u16(parameters::FIRST_DIVISOR), 3);
    assert_eq!(layer.memory().read_u16(parameters::SECOND_DIVISOR), 4);
    for (at, address) in dump::BUFFERS {
        assert_eq!(layer.memory().read_u32(at), address);
    }
    let [(first, first_at), (second, second_at)] = dump::REGIONS;
    assert_eq!(layer.ports().calls, vec![
        Call::Reserve(first, first_at, dump::REGION_BYTES),
        Call::Reserve(second, second_at, dump::REGION_BYTES),
        Call::Register(first, first_at, dump::REGION_BYTES),
        Call::Register(second, second_at, dump::REGION_BYTES),
    ]);
}

#[test]
fn terminate_releases_the_regions_once_and_only_from_closed() {
    let mut layer = layer(0, 0);
    layer.initialize();
    layer.ports().calls.clear();
    assert_eq!(layer.terminate(), Ok(()));
    assert_eq!(layer.lifecycle(), Some(Lifecycle::Terminated));
    assert_eq!(layer.ports().calls, vec![Call::ReleaseRegion(12), Call::ReleaseRegion(13)]);
    assert_eq!(layer.terminate(), Err(Refusal::AlreadyThere));
    assert_eq!(open(0, 0).terminate(), Err(Refusal::WrongState));
}

#[test]
fn start_attaches_the_first_unit_once() {
    let mut layer = open(0, 0);
    assert_eq!(layer.start(), Ok(()));
    assert_eq!(layer.lifecycle(), Some(Lifecycle::Started));
    assert_eq!(layer.memory().read_u32(field::UNIT_HANDLES[0]), 0x55);
    assert_eq!(layer.start(), Err(LifecycleRefusal::Controller(Refusal::AlreadyThere)));
    assert_eq!(layer.ports().calls, vec![Call::Attach(HandlerUnit::First)]);
    assert_eq!(self::layer(0, 0).start().map_err(LifecycleRefusal::code), Err(0x81));
}

#[test]
fn stop_ends_a_running_primary_operation_and_tells_its_starter() {
    let mut layer = open(0, 0);
    layer.start().unwrap();
    layer.memory.write_u32(field::state(Operation::Primary), OperationState::RUNNING.0);
    layer.memory.write_u32(field::completion(Operation::Primary).unwrap(), 0xf1234);
    layer.memory.write_u32(field::PIPELINE_CHANNEL, 5);
    layer.ports().calls.clear();
    assert_eq!(layer.stop(), Ok(()));
    assert_eq!(layer.lifecycle(), Some(Lifecycle::Open));
    assert!(layer.operation(Operation::Primary).idle());
    assert_eq!(layer.ports().calls, vec![
        Call::Detach(HandlerUnit::First, 0x55),
        Call::Stop(5, 0x80),
        Call::Notified(Operation::Primary, 0xf1234),
        Call::Settle,
        Call::CaptureReleased,
        Call::Release(2, 0x1e),
        Call::Release(2, 0x1),
    ]);
    assert_eq!(layer.stop(), Err(LifecycleRefusal::Controller(Refusal::AlreadyThere)));
}

#[test]
fn stop_keeps_the_controller_started_while_the_pipeline_refuses() {
    let mut layer = open(0, 0);
    layer.start().unwrap();
    layer.memory.write_u32(field::state(Operation::Second), OperationState::RUNNING.0);
    layer.ports().refuse_stop = true;
    assert_eq!(layer.stop(), Err(LifecycleRefusal::Controller(Refusal::PipelineRefused)));
    assert_eq!(layer.lifecycle(), Some(Lifecycle::Started));
    assert_eq!(layer.operation(Operation::Second), OperationState::STOPPING);
}

#[test]
fn description_shows_rate_profile_and_feature_while_enabled() {
    let mut layer = layer(2, 0);
    layer.describe_profile(ProfileHandle(1));
    assert!(description(&layer).is_empty());
    layer.memory.write_u32(diagnostics::ENABLED, 1);
    layer.describe_profile(ProfileHandle(1));
    assert_eq!(description(&layer), b"015.00FPS/REC1_2MIX");
}

#[test]
fn description_is_bounded_to_its_field_and_skips_the_unlabelled_feature() {
    let mut layer = layer(11, 2);
    layer.memory.write_u32(diagnostics::ENABLED, 1);
    layer.describe_profile(ProfileHandle(1));
    assert_eq!(description(&layer), b"029.97FPS/REC1");
    let mut long = self::layer(3, 2);
    long.memory.write_u32(diagnostics::ENABLED, 1);
    long.describe_profile(ProfileHandle(1));
    assert_eq!(description(&long).len(), diagnostics::DESCRIPTION_BYTES - 1);
}

#[test]
fn description_is_suppressed_by_the_configuration() {
    let mut layer = layer(0, 0);
    layer.memory.write_u32(diagnostics::ENABLED, 1);
    layer.ports().config = vec![(0xb6, 1)];
    layer.describe_profile(ProfileHandle(1));
    assert!(description(&layer).is_empty());
}

fn parameters_for(profile: u32) -> crate::stock::Parameters {
    let mut parameters = crate::stock::Parameters::from_bytes([0; crate::stock::Parameters::BYTES]);
    parameters.set_profile(ProfileHandle(profile));
    parameters
}

#[test]
fn a_new_profile_sends_the_format_to_output_and_format_subscribers() {
    let mut layer = layer(0, 0);
    layer.memory.write_u32(requests::subscription(Subscription::Output), 1);
    layer.memory.write_u32(requests::subscription(Subscription::Format), 1);
    assert_eq!(layer.apply_parameters(&parameters_for(1)), Ok(()));
    assert_eq!(layer.ports.calls, vec![Call::Format(1, 1)]);
    layer.ports.calls.clear();
    assert_eq!(layer.apply_parameters(&parameters_for(1)), Ok(()));
    assert!(layer.ports.calls.is_empty());
}

#[test]
fn a_claimed_quad_selection_takes_the_selected_gains_and_applies_them() {
    let mut layer = layer(0, 0);
    layer.ports.config.push((0x1de5, 1));
    layer.memory.write_u32(field::state(Operation::First), OperationState::RUNNING.0);
    let (at, bit) = changes::claim(Stage::QuadSelection);
    layer.memory.set_bits_u8(at, bit);
    assert_eq!(layer.apply_parameters(&parameters_for(1)), Ok(()));
    assert_eq!(work::QUAD_GAINS.map(|at| layer.memory.read_u16(at)), [22, 11, 33, 44]);
    assert_eq!(layer.ports.calls, vec![Call::Apply(1, Stage::QuadSelection.pipeline_bit())]);
}

#[test]
fn a_disabled_quad_selection_clears_its_gains_and_disables_the_stage() {
    let mut layer = layer(0, 0);
    layer.ports.config.push((0x1de5, 1));
    layer.memory.write_u32(field::state(Operation::First), OperationState::RUNNING.0);
    let (at, bit) = changes::claim(Stage::QuadSelection);
    layer.memory.set_bits_u8(at, bit);
    assert_eq!(layer.apply_parameters(&parameters_for(1)), Ok(()));
    layer.ports.calls.clear();
    layer.ports.config.insert(0, (0xb2, 4));
    assert_eq!(layer.apply_parameters(&parameters_for(2)), Ok(()));
    assert_eq!(work::QUAD_GAINS.map(|at| layer.memory.read_u16(at)), [0; 4]);
    assert_eq!(layer.ports.calls, vec![Call::Disable(1, Stage::QuadSelection.pipeline_bit())]);
}

const WINDOW: Rectangle = Rectangle { top: 0, left: 0, width: 640, height: 480 };

fn framed_parameters() -> Parameters {
    let mut parameters = Parameters::from_bytes([0; Parameters::BYTES]);
    parameters.set_format_ratio([3, 4]);
    parameters
}

fn tracking(top: u32, direction: TrackDirection) -> ExtendedRequest {
    let tracking = Tracking { centre: [0, 0], top, left: 150, height: 100, width: 60, direction };
    ExtendedRequest { grid: GridRequest::default(), tracking: Some(tracking) }
}

#[test]
fn a_tracked_window_stacks_down_to_the_crop_and_replaces_the_tracked_record() {
    let mut layer = framed(WINDOW, GeometryClass::default(), ScanSlot::default());
    let extended = tracking(150, TrackDirection::Down);
    let requests = Requests { extended: Some(&extended), ..Requests::default() };
    let selection = Selection { tracked_windows: true, ..Selection::default() };
    let report = layer.profile_geometry(&framed_parameters(), selection, &requests);
    let summary = GridSummary { column_width: 60, row_height: 100, top: 150, left: 150, rows: 3, columns: 1, ..GridSummary::default() };
    assert_eq!(report.tracked_windows, Some(summary));
    assert_eq!((report.window, report.crop), ([640, 480], [360, 480]));
    let sizes = layer.record_sizes(&framed_parameters(), Selection::default(), &requests).tracked_windows;
    assert_eq!(sizes[..4], [Size { width: 60, height: 100 }, Size { width: 60, height: 100 }, Size { width: 60, height: 100 }, Size { width: 1, height: 1 }]);
    let starts: Vec<u16> = (0..3).map(|index| layer.memory().read_u16(records::TRACKED_WINDOWS.plus(4 * index))).collect();
    assert_eq!(starts, [150, 250, 350]);
}

#[test]
fn a_tracked_window_stacking_up_starts_above_it() {
    let mut layer = framed(WINDOW, GeometryClass::default(), ScanSlot::default());
    let extended = tracking(250, TrackDirection::Up);
    let requests = Requests { extended: Some(&extended), ..Requests::default() };
    let selection = Selection { tracked_windows: true, ..Selection::default() };
    let report = layer.profile_geometry(&framed_parameters(), selection, &requests).tracked_windows.unwrap();
    assert_eq!((report.top, report.rows, report.columns), (50, 3, 1));
}

#[test]
fn fixed_windows_follow_the_window_origin_and_round_down_to_the_scale_pair() {
    let window = Rectangle { top: 100, left: 200, width: 600, height: 400 };
    let mut layer = framed(window, GeometryClass::default(), ScanSlot::default());
    let mut fixed = FixedWindows { count: 2, ..FixedWindows::default() };
    fixed.windows[0] = FixedWindow { top: 10, left: 20, width: 33, height: 1 };
    fixed.windows[1] = FixedWindow { top: 0, left: 0, width: 64, height: 65 };
    fixed.windows[2] = FixedWindow { top: 0, left: 0, width: 64, height: 64 };
    let requests = Requests { fixed: Some(&fixed), ..Requests::default() };
    let selection = Selection { fixed_windows: true, ..Selection::default() };
    let sizes = layer.record_sizes(&framed_parameters(), selection, &requests).fixed_windows;
    assert_eq!(sizes[..3], [Size { width: 32, height: 2 }, Size { width: 64, height: 64 }, Size { width: 1, height: 1 }]);
    let at = records::FIXED_WINDOWS;
    assert_eq!((layer.memory().read_u16(at), layer.memory().read_u16(at.plus(0x28))), (110, 220));
}

#[test]
fn only_a_gain_quad_selection_reports_the_adjusted_quad() {
    let mut layer = layer(0, 0);
    layer.ports.config.push((0x1de5, 1));
    assert_eq!(layer.quad_selection_of(ProfileHandle(1)), QuadSelection::Gains);
    assert_eq!(layer.enabled_quad(0, ProfileHandle(1)), [11, 22, 33, 44]);
    layer.ports.config.insert(0, (0x1de5, 2));
    assert_eq!(layer.quad_selection_of(ProfileHandle(1)), QuadSelection::Area);
    assert_eq!(layer.enabled_quad(0, ProfileHandle(1)), [0; 4]);
    assert_eq!(layer.adjusted_quad(0, ProfileHandle(40), true), [0; 4]);
}

#[test]
fn a_scaled_rectangle_keeps_its_centre() {
    let rectangle = Rectangle { top: 10, left: 20, width: 1000, height: 500 };
    assert_eq!(centred_rectangle(rectangle, [5000, 2000]), Rectangle { top: 210, left: 270, width: 500, height: 100 });
}

#[test]
fn the_window_scale_is_the_slot_window_over_the_profile_window_and_selects_the_slot() {
    let class = GeometryClass { limits: [1, 1], ..GeometryClass::default() };
    let slot = ScanSlot { height: 400, width: 600, divisor: 1000, ..ScanSlot::default() };
    let mut layer = framed(Rectangle { top: 0, left: 0, width: 800, height: 400 }, class, slot);
    assert_eq!(layer.window_scale(ProfileHandle(1), [0; 3]), [7500, 15000]);
    assert_eq!(layer.memory().read_u32(policy::SLOT), 9);
}

fn word_at(dumped: &[(u32, u32, Vec<u8>)], offset: u32) -> Option<u32> {
    dumped.iter().find(|(_, at, _)| *at == offset).map(|(_, _, bytes)| u32::from_le_bytes(bytes[..4].try_into().unwrap()))
}

#[test]
fn the_pipeline_dump_marks_the_buffer_and_copies_only_the_enabled_bank() {
    let mut layer = layer(0, 0);
    layer.ports.status = 0x4;
    layer.dump_pipeline(Some(DumpBuffer(0x4000_0000)));
    assert_eq!(layer.ports.policy_dumps, vec![PolicyDump::Pipeline]);
    let dumped = &layer.ports.dumped;
    assert_eq!(dumped.len(), 2 + 9 + 31 + 13);
    assert_eq!(word_at(dumped, 0xb_1520), Some(u32::from_le_bytes(*b"TMTS")));
    assert_eq!(word_at(dumped, 0xb_1528), Some(0x04));
    assert_eq!(word_at(dumped, 0xb_15f4), Some(0xf4));
    assert_eq!(word_at(dumped, 0xb_15f8), None);
}

#[test]
fn the_result_dump_adds_each_word_offset_to_the_first_register_of_a_repeated_block() {
    let mut layer = layer(0, 0);
    layer.dump_result(Some(DumpBuffer(0x4000_0000)));
    let dumped = &layer.ports.dumped;
    assert_eq!(word_at(dumped, 0xd_f284 + 0x374), Some(u32::from_le_bytes(*b"S-R0")));
    assert_eq!(word_at(dumped, 0xd_f284 + 0x37c), Some(0x04));
    assert_eq!(word_at(dumped, 0xd_f284 + 0x39c), Some(0x20 + 4));
    layer.ports.dumped.clear();
    layer.dump_result(None);
    assert!(layer.ports.dumped.is_empty());
}

fn summarizing() -> Layer {
    let tables = Tables { feature: 0, kind: 0, flags: 0, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 2 };
    ProfileGeometry::new(Bytes::default(), Fake::default(), Profiles::new(tables))
}

#[test]
fn a_level_summary_averages_four_block_words_and_is_capped() {
    let mut layer = summarizing();
    layer.ports.halfwords.push((0x258c, 0x3000));
    let request = SummaryRequest { tag: 7, echoed: [1, 2, 3], measured: false, references: [StatisticsBuffer(0); 2] };
    let areas = SummaryAreas { level: [8, 2], ..SummaryAreas::default() };
    let summary = layer.statistics_summary(&parameters_for(1), Some(StatisticsBuffer(0x100)), Some(&areas), &request);
    assert_eq!(summary, Some(StatisticsSummary { half: RecordHalf::First, summary: Some(Summary::Level(0x3000)) }));
    layer.ports.halfwords[0].1 = 0xffff;
    let summary = layer.statistics_summary(&parameters_for(1), Some(StatisticsBuffer(0x100)), Some(&areas), &request);
    assert_eq!(summary.unwrap().summary, Some(Summary::Level(0x3441)));
}

#[test]
fn a_measured_summary_fills_its_record_half_and_the_kept_copy() {
    let mut layer = summarizing();
    let request = SummaryRequest { tag: 7, echoed: [1, 2, 3], measured: true, references: [StatisticsBuffer(0x200), StatisticsBuffer(0)] };
    let areas = SummaryAreas { minimum: [8, 2], level: [0, 0], quadrant: [2, 2] };
    let summary = layer.statistics_summary(&parameters_for(1), Some(StatisticsBuffer(0x100)), Some(&areas), &request).unwrap();
    let mut record = [0xaa; RECORD_BYTES];
    write_summary(&mut record, Some(&summary), true);
    let word = |at: usize| u32::from_le_bytes(record[at..at + 4].try_into().unwrap());
    let half = |at: usize| u16::from_le_bytes([record[at], record[at + 1]]);
    assert_eq!((record[0x284], word(0x288), word(0x2a0), word(0x2a8)), (7, 0, 1, 3));
    assert_eq!((word(0x2bc), word(0x2c0), word(0x2c4)), (0x1_d160, 0, 0x1_d260));
    assert_eq!((half(0x290), half(0x296), half(0x298)), (0x100e, 0x10ce, 0x102e));
    assert_eq!(half(0x2ac), 0xc40);
    assert_eq!(record[COPY_OFFSET..COPY_OFFSET + HALF_BYTES], record[..HALF_BYTES]);
    assert_eq!(record[HALF_BYTES], 0xaa);
}

#[test]
fn a_running_geometry_service_refuses_a_start_and_tells_the_starter() {
    let mut layer = layer(0, 0);
    assert_eq!(layer.start_geometry(0, 1, 52, StockCode(0xf000)), Ok(()));
    assert_eq!(layer.ports.calls, vec![Call::GeometryStart(GeometryStart { mode: 0, selection: 1, kind: 2 })]);
    layer.ports.calls.clear();
    assert_eq!(layer.start_geometry(0, 1, 52, StockCode(0xf000)), Err(Refusal::WrongState));
    assert_eq!(layer.ports.calls, vec![Call::GeometryNotified(0xf000, -14)]);
    assert_eq!(layer.reset_geometry(), Ok(()));
    assert_eq!(layer.memory().read_u32(service::WINDOW_SCALE[0]), 10000);
}

#[test]
fn window_areas_query_the_inner_window_and_rescale_each_area() {
    let mut layer = layer(0, 0);
    let geometry = WindowGeometry {
        window: [100, 400],
        inner: Rectangle { top: 10, left: 20, width: 30, height: 40 },
        outer: Rectangle { top: 0, left: 0, width: 100, height: 200 },
    };
    assert_eq!(layer.window_areas(AreaKind(1), &geometry), Err(Refusal::WrongState));
    layer.memory.write_u32(service::RUNNING, 1);
    assert_eq!(layer.window_areas(AreaKind(3), &geometry), Err(Refusal::BadArgument));
    let mut list = layer.window_areas(AreaKind(1), &geometry).unwrap();
    let query = AreaQuery { window: [10, 20, 40, 30], extent: [200, 100], ..AreaQuery::default() };
    assert_eq!(layer.ports.calls, vec![Call::AreaQuery(1, query)]);
    assert_eq!(list.count, 1);
    assert_eq!((list.present()[0].origins()[0][0], list.present()[0].extent()), (5, 50));
}

#[test]
fn an_inner_rectangle_past_the_outer_one_takes_the_outer_axis() {
    let outer = Rectangle { top: 10, left: 10, width: 100, height: 100 };
    let inner = Rectangle { top: 20, left: 0, width: 300, height: 30 };
    assert_eq!(within(outer, inner), Rectangle { top: 10, left: 0, width: 100, height: 30 });
}

fn filing(statuses: Vec<u32>) -> ProfileGeometry<SharedBytes, Fake, Tables> {
    let memory = SharedBytes::default();
    let fake = Fake { files: Some(memory.0.clone()), file_statuses: statuses, ..Fake::default() };
    let tables = Tables { feature: 0, kind: 0, flags: 0, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 1 };
    ProfileGeometry::new(memory, fake, Profiles::new(tables))
}

#[test]
fn a_dump_names_opens_writes_and_closes_its_file_then_reports_its_numbers() {
    let mut layer = filing(vec![]);
    assert_eq!(layer.write_dump(DumpKind::from_code(3), Some(StockData(0x4000)), 0x200, Some(StockData(0x5000))), Ok(()));
    assert_eq!(layer.ports.out, vec![
        Out::FileName(dump::FILE_NAME),
        Out::Copy(dump::FILE_PATH, dump::FILE_NAME),
        Out::Append(dump::FILE_PATH, b".tma\0"),
        Out::Open(dump::FILE_PATH),
        Out::Wait(3),
        Out::Write(0x77, 0x200, 0x4000),
        Out::Wait(3),
        Out::Close(0x77),
        Out::Wait(3),
        Out::Numbers(0x5000),
    ]);
}

#[test]
fn a_failed_dump_write_finishes_the_file_instead_of_closing_it() {
    let mut layer = filing(vec![0, 9]);
    assert_eq!(layer.write_dump(DumpKind::from_code(1), Some(StockData(0x4000)), 4, None), Ok(()));
    assert_eq!(layer.ports.out[2], Out::Append(dump::FILE_PATH, b".bin\0"));
    assert_eq!(layer.ports.out[7..], [Out::Finish(0x77), Out::Wait(3)]);
}

#[test]
fn a_dump_without_data_is_refused_before_any_request() {
    let mut layer = filing(vec![]);
    assert_eq!(layer.write_dump(DumpKind::Raw, None, 4, None), Err(Refusal::BadArgument));
    assert!(layer.ports.out.is_empty());
}

#[test]
fn the_display_configuration_applies_each_pending_part_and_clears_it() {
    let mut layer = layer(0, 0);
    layer.memory.write_u32(display::PENDING, 0x55);
    assert_eq!(layer.apply_display_configuration(StockCode(0xf00)), Ok(()));
    let calls: Vec<Out> =
        ["quad", "channel", "layout", "graphic", "bits", "forward", "selected", "colour", "value", "event"].into_iter().map(Out::Display).collect();
    assert_eq!(layer.ports.out, calls);
    assert_eq!(layer.memory().read_u32(display::PENDING), 0);
}

#[test]
fn a_refused_display_part_stays_pending_and_stops_the_rest() {
    let mut layer = layer(0, 0);
    layer.memory.write_u32(display::PENDING, 0x55);
    layer.ports.display_refuses = Some("selected");
    assert_eq!(layer.apply_display_configuration(StockCode(0xf00)), Err(Refusal::WrongState));
    assert_eq!(layer.ports.out.last(), Some(&Out::Display("selected")));
    assert_eq!(layer.memory().read_u32(display::PENDING), 0x51);
    layer.memory.write_u32(display::PENDING, 0);
    layer.ports.out.clear();
    assert_eq!(layer.apply_display_configuration(StockCode(0xf00)), Err(Refusal::WrongState));
    assert!(layer.ports.out.is_empty());
}

#[test]
fn the_configured_operation_needs_the_profile_flag_an_enabling_request_and_an_open_mode() {
    let tables = Tables { feature: 0, kind: 0, flags: 1 << 14, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 1 };
    let mut flagged = ProfileGeometry::new(Bytes::default(), Fake::default(), Profiles::new(tables));
    let base = OperationConditions {
        requested: 1,
        alternate: 0,
        held: 0,
        vetoes: [0; 3],
        mode: 0,
        profile: ProfileHandle(0),
        busy: 0,
        stage: 0,
    };
    assert!(flagged.configured_operation(&base));
    assert!(flagged.configured_operation(&OperationConditions { requested: 0, alternate: 1, ..base }));
    assert!(!flagged.configured_operation(&OperationConditions { held: 2, ..base }));
    assert!(!flagged.configured_operation(&OperationConditions { mode: 3, ..base }));
    assert!(!flagged.configured_operation(&OperationConditions { vetoes: [0, 1, 0], ..base }));
    assert!(!flagged.configured_operation(&OperationConditions { stage: 3, ..base }));
    assert!(!layer(0, 0).configured_operation(&base));
}

#[test]
fn profile_factors_scale_the_second_and_third_values_in_twentieth_powers_of_two() {
    let mut layer = layer(0, 0);
    assert_eq!(layer.apply_profile_factors([7, 1000, 2000], 0x8000, 0x4000, 3), [7, 93, 187]);
}

#[test]
fn level_pairs_follow_their_kind() {
    let mut layer = layer(0, 0);
    assert_eq!(layer.level_pair(1, 3), LevelPair([4, 0]));
    assert_eq!(layer.level_pair(2, 4), LevelPair::PRIMARY);
    assert_eq!(layer.level_pair(2, 3), LevelPair::SECONDARY);
    assert_eq!(layer.level_pair(0, 2), LevelPair([0, 3]));
    assert_eq!([LevelPair([0, 3]).class(), LevelPair([4, 0]).class(), LevelPair::PRIMARY.class()], [0, 1, 2]);
    assert_eq!([LevelPair([0, 0]), LevelPair([1, 0]), LevelPair([2, 0])].map(LevelPair::first_complement), [1, 0, 0]);
    assert_eq!(layer.record_decade(LevelPair([2, 5])), 50);
}

#[test]
fn selection_values_answer_only_what_is_asked_and_zero_a_selection_past_the_split() {
    let mut layer = layer(0, 0);
    layer.ports.config.push((0xad, 3));
    let all = SelectionRequest { selection: true, offset: true, level: true };
    assert_eq!(layer.selection_values(all), SelectionValues { selection: Some(3), offset: Some(0), level: Some(93) });
    layer.ports.config[0].1 = 4;
    assert_eq!(layer.selection_values(SelectionRequest { selection: true, ..SelectionRequest::default() }).selection, Some(0));
    assert_eq!(layer.selection_values(SelectionRequest::default()), SelectionValues::default());
}

#[test]
fn service_requests_name_their_route_tasks_and_register() {
    let mut layer = layer(0, 0);
    assert_eq!(layer.set_window_power(WindowPower::Held), 0);
    layer.configure_extended_operations(ExtendedOperations::Primary);
    assert_eq!(layer.ports.out, vec![Out::Route(13, 8), Out::Task(50, 3), Out::Task(43, 4), Out::Task(47, 4), Out::Task(48, 4), Out::Task(44, 5)]);
    assert!(layer.device_status());
    assert_eq!(layer.read_adc_channel5(), AdcReading { status: 0, value: 15 });
    layer.ports.status = 3;
    assert!(layer.peak_active());
    layer.ports.status = 1;
    assert!(!layer.peak_active());
}

#[test]
fn an_exported_request_fills_the_export_scan_object_and_writes_its_four_parts() {
    let mut layer = layer(0, 0);
    let request = ProcessingRequest {
        option: 1,
        output_mode: 0,
        dimensions: [720, 1280],
        format: 13,
        selection: 9,
        code: 0,
        geometry_kind: 4,
        geometry_enabled: 1,
        query_mode: 2,
        record_kinds: [0, 9],
        processing_level: 3,
        profile: ProfileHandle(0),
        record_kind: 0,
        limit: 0x123,
        rate_companion: 9,
        toggle: 2,
        option_flags: 0,
        byte70: 0x70,
        channel: 0,
    };
    layer.export_processing_request(&request, StockData(0x6000));
    let scan = callbacks::EXPORT_SCAN;
    let settings: Vec<ScanSetting> = layer.ports.settings.iter().map(|&(at, setting)| {
        assert_eq!(at, scan);
        setting
    }).collect();
    use ScanSetting::*;
    assert_eq!(settings, vec![
        OutputMode(0), ProcessingOption(5), GeometryKind(5), RecordProcessing(7), Dimensions(1), QueryKind(1), Format(0), Code(0),
        QueryMode(4), PrimaryRecordKind(4), SecondaryRecordKind(2), GeometryEnabled(1), FormatCompanion(0), Limit(0x123),
        RateCompanion(9), Toggle(1), Byte70(0x70), ProcessingLevel(12), ChannelCode(101),
    ]);
    assert_eq!(layer.ports.out, vec![
        Out::ClearScan(scan),
        Out::DefaultScan(scan),
        Out::ChannelCode(1),
        Out::Export(scan, ScanExport::Settings, 0x6000),
        Out::Export(scan, ScanExport::Geometry, 0x6038),
        Out::Export(scan, ScanExport::Values, 0x6064),
        Out::Export(scan, ScanExport::Curve, 0x6076),
    ]);
}

fn recording(channels: u8) -> ProfileGeometry<SharedBytes, Fake, Tables> {
    let mut layer = filing(vec![]);
    layer.ports.config.push((0x98c0, channels));
    layer
}

fn buffer_at(index: u16) -> Location { record_mode::BUFFERS.plus(0x14 * index) }

#[test]
fn record_mode_lays_out_one_buffer_per_enabled_capture_channel() {
    let mut layer = recording(0b1001);
    assert_eq!(layer.initialize_record_mode(), Ok(()));
    assert_eq!(layer.memory().read_u8(record_mode::BUFFER_COUNT), 3);
    assert_eq!(layer.memory().read_u32(buffer_at(2).plus(4)), 0x48a6_1000);
    assert_eq!(layer.memory().read_u32(record_mode::STATE), 1);
    let mut one = recording(0b0010);
    assert_eq!(one.initialize_record_mode(), Ok(()));
    assert_eq!(one.memory().read_u8(record_mode::BUFFER_COUNT), 1);
    assert_eq!(recording(0).initialize_record_mode(), Err(Refusal::BadArgument));
    assert!(one.record_mode_available(RecordChannel::SHORT));
    assert!(!one.record_mode_available(RecordChannel::SINGLE));
    assert!(!one.record_mode_available(RecordChannel(6)));
}

#[test]
fn a_single_record_channel_keeps_the_last_filled_buffer_and_releases_the_rest() {
    let mut layer = recording(0b1001);
    layer.initialize_record_mode().unwrap();
    layer.memory.write_u32(buffer_at(0), 2);
    let filled = buffer_at(1);
    layer.memory.write_u32(filled, 2);
    layer.memory.write_u32(filled.plus(4), 0x1000);
    layer.memory.write_u16(filled.plus(0xc), 4);
    layer.memory.write_u16(filled.plus(0xe), 5);
    layer.memory.write_u16(filled.plus(0x10), 9);
    assert_eq!(layer.begin_record_mode(RecordChannel::SINGLE), Ok(()));
    assert_eq!(layer.memory().read_u32(record_mode::FIRST_FREE), 2);
    assert_eq!(layer.begin_record_mode(RecordChannel::SINGLE), Err(Refusal::RecordState));
    layer.memory.write_u32(buffer_at(2), 7);
    assert_eq!(layer.finish_record_mode(0), Ok(()));
    assert_eq!([0, 1, 2].map(|index| layer.memory().read_u32(buffer_at(index))), [3, 3, 0]);
    let kept = record_mode::RECORDS[0].0;
    assert_eq!(layer.memory().read_u32(kept.plus(4)), 0x1000);
    assert_eq!([0, 8, 0xa, 0x10].map(|offset| layer.memory().read_u16(kept.plus(offset))), [1, 4, 5, 9]);
    assert_eq!(layer.memory().read_u16(record_mode::KEPT), 1);
    assert_eq!(layer.finish_record_mode(0), Err(Refusal::RecordState));
}

#[test]
fn the_short_channel_drops_records_past_the_limit_and_ends_its_area_after_the_last_kept() {
    let mut layer = recording(0b0010);
    layer.initialize_record_mode().unwrap();
    assert_eq!(layer.begin_record_mode(RecordChannel::SHORT), Ok(()));
    let (records, _) = record_mode::RECORDS[1];
    layer.memory.write_u16(record_mode::RECORD_COUNT, 3);
    for (index, enable) in [5u16, 0, 2].into_iter().enumerate() {
        let at = records.plus(0x14 * index as u16);
        layer.memory.write_u16(at, enable);
        layer.memory.write_u32(at.plus(4), 0x2000 * (index as u32 + 1));
        layer.memory.write_u16(at.plus(8), 10);
        layer.memory.write_u16(at.plus(0xa), 3);
    }
    assert_eq!(layer.finish_record_mode(3), Ok(()));
    assert_eq!(layer.memory().read_u16(records), 0);
    assert_eq!(layer.memory().read_u16(record_mode::KEPT.plus(2)), 1);
    assert_eq!(layer.memory().read_u32(record_mode::END_ADDRESS), 0x6000 + 60);
}

#[test]
fn an_export_writes_each_kept_record_to_a_file_and_lists_it_in_the_snapshot() {
    let mut layer = recording(0b1001);
    layer.initialize_record_mode().unwrap();
    let filled = buffer_at(1);
    layer.memory.write_u32(filled, 2);
    layer.memory.write_u32(filled.plus(4), 0x1000);
    layer.memory.write_u16(filled.plus(0xc), 4);
    layer.memory.write_u16(filled.plus(0xe), 5);
    layer.begin_record_mode(RecordChannel::SINGLE).unwrap();
    layer.finish_record_mode(0).unwrap();
    layer.ports.out.clear();
    layer.export_record_snapshot(Some(DumpBuffer(0x4000_0000)));
    assert!(layer.ports.out.contains(&Out::Write(0x77, 40, 0x1000)));
    let numbers = crate::stock::layout::address(record_mode::RECORDS[0].0.plus(0xc)) as u32;
    assert_eq!(layer.ports.out.last(), Some(&Out::Numbers(numbers)));
    let words: Vec<(u32, Vec<u8>)> = layer.ports.dumped.iter().filter(|(_, _, bytes)| bytes.iter().any(|&byte| byte != 0)).map(|(_, offset, bytes)| (offset - 0xa_5fcc, bytes.clone())).collect();
    let word = |value: u32| value.to_le_bytes().to_vec();
    assert_eq!(words, vec![
        (0x0, b"RATS".to_vec()),
        (0xa10, b"RAAL".to_vec()),
        (0x4, word(0b1001)),
        (0x8, word(1 << 16)),
        (0x10, word(1 | 2 << 16)),
        (0x510, word(4 | 5 << 16)),
    ]);
    assert_eq!(layer.memory().read_u32(record_mode::STATE), 1);
}

fn controller(offset: u16) -> Location { Location::new(crate::memory::Block::Controller, offset) }

fn work_at(offset: u16) -> Location { Location::new(crate::memory::Block::Work, offset) }

/// What the fake pipeline's `round`th collection leaves in the result halfword at work `offset`.
const fn collected(round: u16, offset: u16) -> u32 { (round << 12 | (offset - 0x1c8)) as u32 }

/// Destinations in `buffer` for `stages`, each a stage bit with its regions; the fake's region
/// offset is the region times 0x1000.
fn destinations(buffer: u32, stages: &[(usize, &[u32])]) -> StatisticsDestinations {
    let mut destinations = StatisticsDestinations::default();
    for &(bit, regions) in stages {
        for (slot, region) in regions.iter().enumerate() {
            destinations.0[bit][slot] = StockData(buffer + region * 0x1000);
        }
    }
    destinations
}

const CHANNEL_REGIONS: [(usize, &[u32]); 6] = [(0, &[3]), (1, &[7]), (2, &[0]), (3, &[2]), (4, &[11, 12]), (5, &[13, 21, 29])];

fn framing() -> ProfileGeometry<SharedBytes, Fake, Tables> {
    let mut layer = filing(vec![]);
    layer.memory.write_u32(field::PIPELINE_CHANNEL, 6);
    layer
}

#[test]
fn a_frame_records_its_index_and_clears_the_applied_stages() {
    let mut layer = framing();
    layer.memory.write_u32(crate::stock::layout::counters::INDEX, 7);
    for (_, at) in work::APPLIED {
        layer.memory.write_u32(at, 1);
    }
    layer.handle_frame();
    assert_eq!(layer.memory().read_u32(crate::stock::layout::counters::FRAME_INDEX), 7);
    assert_eq!([0x3f4, 0x600, 0x3a4].map(|offset| layer.memory().read_u32(work_at(offset))), [0; 3]);
    assert_eq!(layer.ports.calls, vec![]);
}

#[test]
fn a_channel_frame_moves_the_stages_to_the_other_buffer_and_keeps_the_results_by_bank() {
    let mut layer = framing();
    layer.memory.write_u32(field::state(Operation::Channel), OperationState::RUNNING.0);
    layer.memory.write_u32(controller(0xa98), 1);
    layer.memory.write_u32(controller(0xa9c), 1);
    layer.handle_frame();
    assert_eq!(layer.ports.calls, vec![
        Call::Point(6, 0x3f, destinations(0x5ffb_3770, &CHANNEL_REGIONS)),
        Call::Collect(6, 0x3f, work_at(0x1c8)),
    ]);
    assert_eq!([0xa98, 0xa9c].map(|offset| layer.memory().read_u32(controller(offset))), [2, 2]);
    let first = |offset| layer.memory().read_u32(controller(offset));
    assert_eq!([0x8e4, 0x8e8, 0x8ec, 0x8f0].map(first), [0x2c6, 0x2c0, 0x266, 0x260].map(|at| collected(1, at)));
    assert_eq!([0x904, 0x908, 0x90c, 0x910].map(first), [0x346, 0x340, 0x326, 0x320].map(|at| collected(1, at)));
    assert_eq!([0x914, 0x918, 0x95c, 0x960].map(first), [0x2f6, 0x2ce, 0x31a, 0x2f2].map(|at| collected(1, at)));
    assert_eq!([0x964, 0x968].map(first), [0x296, 0x26e].map(|at| collected(1, at)));
    assert_eq!(first(0x9bc), 0);

    layer.ports.calls.clear();
    layer.handle_frame();
    assert_eq!(layer.ports.calls[0], Call::Point(6, 0x3f, destinations(0x5ffb_0dc0, &CHANNEL_REGIONS)));
    assert_eq!([0xa98, 0xa9c].map(|offset| layer.memory().read_u32(controller(offset))), [1, 1]);
    assert_eq!(layer.memory().read_u32(controller(0x9bc)), collected(2, 0x2c6));
    assert_eq!(layer.memory().read_u32(controller(0x8e4)), collected(1, 0x2c6));
}

#[test]
fn a_stopping_channel_operation_still_alternates_its_banks() {
    let mut layer = framing();
    layer.memory.write_u32(field::state(Operation::Channel), OperationState::STOPPING.0);
    layer.memory.write_u32(controller(0xa98), 2);
    layer.memory.write_u32(controller(0xa9c), 2);
    layer.handle_frame();
    assert_eq!(layer.ports.calls[0], Call::Point(6, 0x3f, destinations(0x5ffb_0dc0, &CHANNEL_REGIONS)));
    assert_eq!(layer.memory().read_u32(controller(0x9bc + 4)), collected(1, 0x2c0));
}

#[test]
fn a_cleared_bank_collects_the_results_without_keeping_them() {
    let mut layer = framing();
    layer.memory.write_u32(field::state(Operation::Channel), OperationState::RUNNING.0);
    layer.handle_frame();
    assert_eq!(layer.ports.calls, vec![
        Call::Point(6, 0x3f, destinations(0x5ffb_0dc0, &CHANNEL_REGIONS)),
        Call::Collect(6, 0x3f, work_at(0x1c8)),
    ]);
    assert_eq!([0x8e4, 0x9bc].map(|offset| layer.memory().read_u32(controller(offset))), [0, 0]);
    assert_eq!([0xa98, 0xa9c].map(|offset| layer.memory().read_u32(controller(offset))), [1, 1]);
}

#[test]
fn an_indexed_frame_marks_the_operation_and_keeps_its_result_pair() {
    let mut layer = framing();
    layer.memory.write_u32(field::state(Operation::Indexed), OperationState::RUNNING.0);
    layer.memory.write_u32(controller(0x774), 1);
    layer.memory.write_u32(controller(0x778), 1);
    layer.handle_frame();
    assert_eq!(layer.ports.calls, vec![
        Call::Point(6, 0x40, destinations(0x5ffb_3770, &[(6, &[37])])),
        Call::Collect(6, 0x40, work_at(0x1c8)),
    ]);
    let word = |offset| layer.memory().read_u32(controller(offset));
    assert_eq!([0x774, 0x778, 0x77c].map(word), [2, 2, 1]);
    assert_eq!([0x760, 0x764, 0x768].map(word), [collected(1, 0x34e), collected(1, 0x34a), 0]);
    layer.handle_frame();
    assert_eq!([0x768, 0x76c].map(|offset| layer.memory().read_u32(controller(offset))), [collected(2, 0x34e), collected(2, 0x34a)]);
}

#[test]
fn a_stopping_indexed_operation_is_left_alone() {
    let mut layer = framing();
    layer.memory.write_u32(field::state(Operation::Indexed), OperationState::STOPPING.0);
    layer.handle_frame();
    assert_eq!(layer.ports.calls, vec![]);
    assert_eq!(layer.memory().read_u32(controller(0x77c)), 0);
}

#[test]
fn a_destination_record_puts_each_stage_where_the_pipeline_reads_it() {
    const INDEXED: (usize, &[u32]) = (6, &[37]);
    let all: Vec<(usize, &[u32])> = CHANNEL_REGIONS.into_iter().chain([INDEXED]).collect();
    let record = crate::stock::statistics::destination_record(&destinations(0, &all));
    assert_eq!(record, [0, 0, 3, 0, 11, 12, 13, 21, 29, 7, 2, 37].map(|region| region * 0x1000));
}
