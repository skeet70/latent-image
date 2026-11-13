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
use crate::stock::layout::{changes, controller as field, diagnostics, dump, parameters, policy, requests, work, work::records};
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
}

impl Configuration for Fake {
    fn byte(&mut self, offset: ConfigOffset) -> u8 { self.config.iter().find(|(at, _)| *at == offset.0).map_or(0, |(_, value)| *value) }
    fn halfword(&mut self, offset: ConfigOffset) -> u16 {
        self.halfwords.iter().find(|(at, _)| *at == offset.0).map_or(0, |(_, value)| *value)
    }
    fn signed_halfword(&mut self, _: ConfigOffset) -> i16 { 0 }
    fn collapse_index(&mut self, value: u32) -> u32 { value }
    fn transform(&mut self, value: u32, scale: u32) -> u16 { (value * 10 + scale) as u16 }
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
}

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
    fn set(&mut self, _: ScanSetting) {}
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
}

impl StatisticsMemory for Fake {
    fn statistics_word(&mut self, buffer: StatisticsBuffer, offset: u32) -> u32 { buffer.0 + offset }
}

impl FrameDumps for Fake {
    fn register(&mut self, register: Register) -> u32 { if register.0 == 0xfff8_802c { self.status } else { register.0 & 0xff } }
    fn write_dump(&mut self, buffer: DumpBuffer, offset: u32, bytes: &[u8]) { self.dumped.push((buffer.0, offset, bytes.to_vec())) }
    fn dump_policy(&mut self, _: DumpBuffer, dump: PolicyDump, _: Location) { self.policy_dumps.push(dump) }
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
    class: GeometryClass,
    slot: ScanSlot,
    policy: u32,
}

impl ProfileTables for Tables {
    fn entry(&self, _: ProfileHandle) -> ProfileEntry {
        ProfileEntry { class: ClassIndex(0), kind: ProfileKind(self.kind), flags: 0, threshold_index: 3, feature: self.feature, aspect_index: 0 }
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
    let tables = Tables { feature, kind, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 1 };
    ProfileGeometry::new(Bytes::default(), Fake::default(), Profiles::new(tables))
}

/// A layer whose profile class scales by one and whose window is `window`.
fn framed(window: Rectangle, class: GeometryClass, slot: ScanSlot) -> Layer {
    let tables = Tables { feature: 0, kind: 0, class: GeometryClass { first_scale: 1, second_scale: 1, ..class }, slot, policy: 1 };
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
    let tables = Tables { feature: 0, kind: 0, class: GeometryClass::default(), slot: ScanSlot::default(), policy: 2 };
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
    assert_eq!((half(0x290), half(0x296), half(0x298)), (0xd10e, 0xd1ce, 0xd12e));
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
