use xm1_saclac_config::{compiled as compiled_api, pipeline, scan, stock as config};
use xm1_saclac_config::scan::ScanLibrary;
use xm1_saclac_imaging::{driver::Isp, stock::capture::StockCapture};
use xm1_saclac_profile_geometry::{memory::{Location, Memory as ProfileMemory}, ports::*, profile::PipelineGroup};
use xm1_saclac_profile_geometry::stock::{layout, areas};
use xm1_saclac_shared::{settings::Adjustments, stock::{Argument::{Buffer, Word}, Machine}};
use crate::stages::StageDriver;
use super::stages::StageMemory;
use super::acquisition::Aligned;

pub fn address(at: Location) -> u32 { layout::address(at) as u32 }

pub struct Memory<M>(pub M);
impl<M: Machine> ProfileMemory for Memory<M> {
    fn read_u8(&self, at: Location) -> u8 { (self.0.read_u16(address(at) & !1) >> ((address(at) & 1) * 8)) as u8 }
    fn read_u16(&self, at: Location) -> u16 { self.0.read_u16(address(at)) }
    fn read_u32(&self, at: Location) -> u32 { self.0.read_u32(address(at)) }
    fn write_u8(&mut self, at: Location, value: u8) { self.0.write_u8(address(at), value) }
    fn write_u16(&mut self, at: Location, value: u16) { self.0.write_u16(address(at), value) }
    fn write_u32(&mut self, at: Location, value: u32) { self.0.write_u32(address(at), value) }
}

pub struct Hardware<'a, M>(pub &'a M);
impl<M: Machine> xm1_saclac_config::registers::PipelineHardware for Hardware<'_, M> {
    fn acquire(&mut self, client: u32) -> u32 { self.0.enter(0xda_1b4c, &[client as usize]) }
    fn release(&mut self, client: u32, grant: u32) { self.0.enter(0xda_1d48, &[client as usize, grant as usize]); }
}

pub fn stop(m: &impl Machine, channel: PipelineChannel, stages: Stages) -> bool { m.enter(0xd9_f504, &[channel.0 as usize, stages.0 as usize]) == 0 }
pub fn release(m: &impl Machine, channel: PipelineChannel, stages: Stages) { m.enter(0xd9_ef3c, &[channel.0 as usize, stages.0 as usize]); }
pub fn settle(m: &impl Machine) { m.enter(0xd9_f0a0, &[]); }
pub fn point_statistics(m: &impl Machine, channel: PipelineChannel, stages: Stages, destinations: &StatisticsDestinations) {
    let record = xm1_saclac_profile_geometry::stock::statistics::destination_record(destinations);
    m.enter(0xda_035c, &[channel.0 as usize, stages.0 as usize, record.as_ptr() as usize, 0]);
}
pub fn collect_statistics(m: &impl Machine, channel: PipelineChannel, stages: Stages, into: Location) { m.enter(0xd9_ffe8, &[channel.0 as usize, (stages.0 << 4) as usize, address(into) as usize]); }
pub fn mode_payload(m: &impl Machine, r: &ModeRequest, into: Location) -> Stages {
    Stages(m.enter(0xda_0ab0, &[r.group.0 as usize, r.index_scale as usize, r.index as usize, r.shift as usize, r.mode as usize, address(r.first) as usize, address(r.second) as usize, r.mode_byte as usize, address(into) as usize]))
}
pub fn tertiary_payload(m: &impl Machine, group: PipelineGroup, scale: u8, index: u8, into: Location) -> Stages { Stages(m.enter(0xda_09f4, &[group.0 as usize, scale as usize, index as usize, address(into) as usize])) }

pub fn format<I: Isp, M: Machine>(stages: &mut StageDriver<I, StageMemory, M>, payload: &xm1_saclac_interface::pipeline::FormatPayload) -> bool {
    let bytes = Aligned(xm1_saclac_interface::stock::pipeline::format_block(payload));
    let block = config::StockStageBlock { address: bytes.as_ptr() as usize as u32, memory: StageMemory };
    pipeline::write_region(stages, pipeline::Client(2), pipeline::Flags(4), &block) == 0
}

pub fn primary_payload(m: &impl Machine, area: &impl Adjustments, group: PipelineGroup, window: Option<PrimaryWindow>) -> Option<PrimaryPayload> {
    use xm1_saclac_config::profile::*;
    let window = window.map_or(PointWindow::default(), |w| PointWindow { enabled: true, x: w.rectangle.top, y: w.rectangle.left, width: w.rectangle.height, height: w.rectangle.width, x_origin: w.scale, y_origin: w.offset });
    let mut record = PointRecord { source: PointSource::Buffer, count: 0, shift: [0; 2], mode: PointMode::Off, window };
    // The returned payload outlives this call and names the pipeline's shared point buffer.
    let buffer = unsafe { &mut *(0x14c_59e8usize as *mut [u32; POINT_BUFFER_WORDS]) };
    prepare_points(area, group.0, &mut record, buffer);
    let entries = match record.source { PointSource::Buffer => 0x14c_59e8, PointSource::Adjustment(key) => m.enter(0x6a_0010, &[key as usize]) };
    let option = match record.mode { PointMode::Off => 0, PointMode::First => 1, PointMode::Second => 2 };
    Some(PrimaryPayload { entries, count: record.count, first_offset: record.shift[0] as u16, second_offset: record.shift[1] as u16, option })
}
pub fn secondary_payload(m: &impl Machine, group: PipelineGroup, _: Option<SecondaryWindow>) -> Option<SecondaryPayload> {
    let mut bytes = Aligned([0; 16]);
    let present = m.enter_with(0xda_b900, &[Word(group.0), Buffer(0)], &mut bytes);
    (present != 0).then(|| SecondaryPayload { first: half(&bytes, 0), second: half(&bytes, 2), value: word(&bytes, 4), extra: word(&bytes, 8) })
}

pub fn scan_block(at: Location) -> config::StockScanBlocks { config::StockScanBlocks { configuration: address(at), request: address(at) + 0x1260, memory: config::StockMemory } }
pub fn compiled(at: Location) -> config::StockCompiledScan { config::StockCompiledScan { address: address(at), memory: config::StockMemory } }
pub fn set_scan(m: &impl Machine, at: Location, setting: ScanSetting) {
    use ScanSetting::*;
    let mut block = scan_block(at);
    let (entry, value, leading) = match setting {
        RateCompanion(v) => { block.set_ec(v); return; }
        RecordProcessing(v) => { block.set_140(v); return; }
        RecordProcessingSecond(v) => { block.set_144(v != 0); return; }
        Channel(v) => { block.set_11a(v as u8); return; }
        ChannelCode(v) => { block.set_13c(v); return; }
        Dimensions(v) => { block.set_148(v); return; }
        WhiteBalance(v) => { scan::set_window(&mut block, v); return; }
        RecordProcessingThird(v) => { block.set_1dc(v); return; }
        ProcessingOption(v) => (0xd7_83ec, v, false), OutputMode(v) => (0xd7_910c, v, false), ScanOption(v) => (0xd7_912c, u32::from(v), false),
        GeometryKind(v) => (0xd7_8af0, v, false), Toggle(v) => (0xd7_85e0, v, false), QueryKind(v) => (0xd7_8a6c, v, false), Format(v) => (0xd7_8a80, v, false), Code(v) => (0xd7_8660, v, false), QueryMode(v) => (0xd7_7f7c, v, false),
        PrimaryRecordKind(v) => (0xd7_7fec, v, false), SecondaryRecordKind(v) => (0xd7_805c, v, false), GeometryEnabled(v) => (0xd7_8508, v, false), FormatCompanion(v) => (0xd7_8538, v, false), OffsetSum(v) => (0xd7_8070, v, false), OffsetSumLow(v) => (0xd7_86ac, u32::from(v), true),
        ScanRate(v) => (0xd7_8db8, v, false), Limit(v) => (0xd7_87b8, u32::from(v), true), Fixed(v) => (0xd7_85a8, v, false), LimitTenths(v) => (0xd7_8f80, u32::from(v), false), Rate(v) => (0xd7_8f9c, u32::from(v), false), Byte70(v) => (0xd7_8eec, u32::from(v), false),
        ProcessingLevel(v) => (0xd7_90f8, v, false), AreaPresent(v) => (0xd7_9344, u32::from(v), false), FirstOf(v) => (0xd7_92e4, v, false), Split(v) => (0xd7_9324, v, false), Multiple(v) => (0xd7_9304, u32::from(v), false), OutputLevel(v) => (0xd7_8fb8, v, false),
    };
    if leading { m.enter(entry, &[address(at) as usize, 0, value as usize]); } else { m.enter(entry, &[address(at) as usize, value as usize]); }
}
pub fn channel_code(m: &impl Machine, channel: u32, codes: &[u16; 10]) -> u16 { m.enter(0xd5_dce8, &[channel as usize, codes.as_ptr() as usize]) as u16 }
pub fn export_scan(m: &impl Machine, at: Location, part: ScanExport, out: StockData) {
    let blocks = scan_block(at);
    match part {
        ScanExport::Settings => { config::StockScanLibrary.fill_settings(&blocks, &mut config::StockScanOutput(out.0)); }
        ScanExport::Geometry => { config::StockScanLibrary.fill_geometry(&blocks, &mut config::StockScanOutput(out.0)); }
        ScanExport::Values => { let values = scan::nine_values(&mut config::StockScanLibrary, &blocks); store_halves(m, out.0, &values); }
        ScanExport::Curve => { let values = scan::four_values(&mut config::StockScanLibrary, &blocks); store_halves(m, out.0, &values); }
    }
}
fn store_halves(m: &impl Machine, at: u32, values: &[u16]) { let memory = config::StockMemory; use config::GuestMemory; for (i, value) in values.iter().enumerate() { memory.set_halfword(at + 2 * i as u32, *value); } let _ = m; }
pub fn compile_scan(output_kind: u8) {
    use scan::Part::*;
    let mut block = scan_block(layout::policy::SCAN);
    for part in [Geometry, Second] { scan::apply(part, &mut block, &mut config::StockScanLibrary); }
    compiled_api::CompiledScan::set_output_kind(&mut compiled(layout::policy::CONFIGURATION), output_kind);
    for part in [Window, Fourth, Fifth, Sixth, Eighth, Ninth, Tenth, Eleventh, Twelfth, Pair, Sixteenth, Seventeenth, Eighteenth, Nineteenth] { scan::apply(part, &mut block, &mut config::StockScanLibrary); }
}
pub fn apply_selection(m: &impl Machine, selection: Location, configuration: Location) {
    let source = address(selection);
    let dest = address(configuration);
    m.enter(0xd5_e0f8, &[source as usize, dest as usize]);
    m.enter(0xd5_dfd4, &[(source + 56) as usize, dest as usize]);
    let mut scan = compiled(configuration);
    compiled_api::store_nine(&mut scan, core::array::from_fn(|i| m.read_u16(source + 100 + 2 * i as u32)));
    compiled_api::store_four(&mut scan, core::array::from_fn(|i| m.read_u16(source + 118 + 2 * i as u32)));
}
pub fn converted_limit(limit: i16) -> u16 {
    xm1_saclac_imaging::capture::control::converted_sample_period(&mut StockCapture, xm1_saclac_imaging::capture::ProfileHandle(limit as u16 as u32))
}
pub fn output_level(m: &impl Machine) -> u32 { m.enter(0x100_4618, &[]) }

pub fn query_areas(m: &impl Machine, code: u32, query: &xm1_saclac_profile_geometry::geometry::AreaQuery) -> AreaAnswer {
    let query = areas::query_bytes(query);
    let mut bytes = Aligned([0; 24 + 4 + 10 * areas::TRANSITION_AREA_BYTES]);
    bytes[..24].copy_from_slice(&query);
    let result = m.enter_with(0x106_a2f0, &[Word(code), Buffer(0), Buffer(24)], &mut bytes) as i32;
    if result == -11 { return AreaAnswer::Empty; }
    if result != 0 { return AreaAnswer::Refused; }
    let mut answer = areas::TransitionAreas::EMPTY;
    answer.count = bytes[24];
    for (area, bytes) in answer.areas.iter_mut().zip(bytes[28..].chunks_exact(areas::TRANSITION_AREA_BYTES)) { area.copy_from_slice(bytes); }
    AreaAnswer::Areas(answer)
}

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }

#[no_mangle]
pub static XM1_REQUIRES_LENS_SAFETY_CONTRACT: u32 = 1;

pub extern "C" fn frame_handler() {
    #[cfg(not(target_arch = "arm"))]
    panic!("the stock frame callback requires the ARM kernel backend");
    #[cfg(target_arch = "arm")]
    {
    let kernel = xm1_kernel::Kernel::new(xm1_kernel::SlotTable);
    match kernel.context() {
        xm1_kernel::Context::Task(cx) => frame_with(&kernel, &cx),
        xm1_kernel::Context::Interrupt(cx) => frame_with(&kernel, &cx),
        xm1_kernel::Context::Masked(_) => { xm1_saclac_shared::stock::Guest.enter(super::FAULT, &[]); }
    }
    }
}

#[cfg(target_arch = "arm")]
fn frame_with<C: xm1_saclac_peers::Poster>(kernel: &xm1_kernel::Kernel<xm1_kernel::SlotTable>, cx: &C) {
    use xm1_saclac_profile_geometry::{controller::ProfileGeometry, profile::Profiles, stock::StockTables};
    let peers = xm1_saclac_peers::PeerPorts::new(cx, xm1_saclac_peers::EmulatorOnly::new(&XM1_REQUIRES_LENS_SAFETY_CONTRACT).unwrap());
    let ports = crate::geometry::GeometryBindings {
        stages: StageDriver { isp: xm1_saclac_imaging::stock::StockIsp { exclusion: crate::stages::KernelExclusion(kernel) }, arguments: StageMemory, machine: xm1_saclac_shared::stock::Guest },
        peers,
    };
    ProfileGeometry::new(Memory(xm1_saclac_shared::stock::Guest), ports, Profiles::new(StockTables)).handle_frame();
}
