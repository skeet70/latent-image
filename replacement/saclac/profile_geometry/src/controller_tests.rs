use crate::controller::lifecycle::LifecycleRefusal;
use crate::controller::{Lifecycle, Operation, OperationState, ProfileGeometry, Refusal};
use crate::memory::{Bytes, Location, Memory};
use crate::numeric::FractionTables;
use crate::ports::*;
use crate::profile::*;
use crate::controller::changes::{Stage, Subscription};
use crate::stock::layout::{changes, controller as field, diagnostics, dump, parameters, requests, work};

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
}

#[derive(Default)]
struct Fake {
    calls: Vec<Call>,
    refuse_stop: bool,
    config: Vec<(u32, u8)>,
}

impl Configuration for Fake {
    fn byte(&mut self, offset: ConfigOffset) -> u8 { self.config.iter().find(|(at, _)| *at == offset.0).map_or(0, |(_, value)| *value) }
    fn halfword(&mut self, _: ConfigOffset) -> u16 { 0 }
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
    fn quad_gains(&mut self, _: QuadGroup, _: i16) -> [u16; 4] { [11, 22, 33, 44] }
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
    fn relative_rectangle(&mut self, _: ProfileHandle, _: bool) -> Rectangle { Rectangle::default() }
    fn encode_thresholds(&mut self, _: i16, _: i16) -> u32 { 0 }
    fn converted_decade(&mut self, _: u32) -> i32 { 0 }
}

struct Tables {
    feature: u8,
    kind: u8,
}

impl ProfileTables for Tables {
    fn entry(&self, _: ProfileHandle) -> ProfileEntry {
        ProfileEntry { class: ClassIndex(0), kind: ProfileKind(self.kind), flags: 0, threshold_index: 3, feature: self.feature, aspect_index: 0 }
    }
    fn class(&self, _: ClassIndex) -> GeometryClass { GeometryClass::default() }
    fn threshold(&self, index: u8) -> u16 { index as u16 * 5 }
    fn default_pair(&self) -> ScalePair { ScalePair { first: 2, second: 2 } }
    fn expansion(&self) -> ExpansionRatio { ExpansionRatio { numerator: 1, divisor: 1 } }
    fn curve_knot(&self, index: usize) -> i16 { 1000 + 50 * index as i16 }
    fn feature_scale_offsets(&self, _: u32) -> [u32; 2] { [0, 0] }
    fn row(&self, index: u32) -> ProfileRow {
        ProfileRow { rectangle_row: 16, pipeline_group: PipelineGroup(index), policy: 1, quad_group: QuadGroup(1) }
    }
    fn aspect_value(&self, _: u8) -> i16 { 0 }
    fn rectangle_configuration(&self, _: RectangleKind, _: u32) -> [u32; 4] { [0; 4] }
    fn scan_slot(&self, _: u32) -> ScanSlot { ScanSlot::default() }
    fn record_code(&self, index: u32) -> [u16; 2] { [index as u16; 2] }
}

impl FractionTables for Tables {
    fn fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 256.0).log2()) as u8 }
    fn wide_fraction(&self, index: u8) -> u8 { (100.0 * (1.0 + index as f64 / 100.0).log2()) as u8 }
}

type Layer = ProfileGeometry<Bytes, Fake, Tables>;

fn layer(feature: u8, kind: u8) -> Layer { ProfileGeometry::new(Bytes::default(), Fake::default(), Profiles::new(Tables { feature, kind })) }

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
