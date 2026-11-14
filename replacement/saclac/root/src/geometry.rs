#![forbid(unsafe_code)]

use xm1_saclac_config::{adjustment, channel, compiled, face, pipeline, profile, scan, stock as config, table};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{ImageProcessor, stock::StockCallbackWord};
use xm1_saclac_profile_geometry::{memory::Location, ports::*, profile::{PipelineGroup, QuadGroup}};
use xm1_saclac_shared::{settings::{Adjustments, AdjustmentKey}, stock::{Machine, Stock}};
use crate::stages::StageDriver;
use crate::stock::{geometry as boundary, stages::StageMemory};

pub struct GeometryBindings<M, P, I> {
    pub stages: StageDriver<I, StageMemory, M>,
    pub peers: P,
}

impl<M: Machine + Clone, P, I> GeometryBindings<M, P, I> {
    fn adjustments(&self) -> Stock<M> { Stock::new(self.stages.machine.clone()) }
    fn block(&self, at: Location) -> config::StockStageBlock<StageMemory> { config::StockStageBlock { address: boundary::address(at), memory: StageMemory } }
}

impl<M: Machine + Clone, P, I> Configuration for GeometryBindings<M, P, I> {
    fn byte(&mut self, at: ConfigOffset) -> u8 { self.adjustments().byte(AdjustmentKey(at.0)) }
    fn halfword(&mut self, at: ConfigOffset) -> u16 { adjustment::half(&self.adjustments(), at.0) }
    fn signed_halfword(&mut self, at: ConfigOffset) -> i16 { adjustment::signed_half(&self.adjustments(), at.0) }
    fn collapse_index(&mut self, value: u32) -> u32 { table::column(&self.adjustments(), table::Step(value)) }
    fn transform(&mut self, value: u32, scale: u32) -> u16 { xm1_saclac_config::dump::DumpDerivations::transformed_value(&mut config::StockDumpDerivations, table::Step(value), table::Row(scale)) }
    fn split_index(&mut self) -> u8 { table::bank_size(&self.adjustments()) as u8 }
}

impl<M: Machine + Clone, P, I: Isp> Pipeline for GeometryBindings<M, P, I> {
    fn stop(&mut self, channel: PipelineChannel, stages: Stages) -> bool { boundary::stop(&self.stages.machine, channel, stages) }
    fn release(&mut self, channel: PipelineChannel, stages: Stages) { boundary::release(&self.stages.machine, channel, stages) }
    fn settle(&mut self) { boundary::settle(&self.stages.machine) }
    fn release_capture_hardware(&mut self) { compiled::release_capture(&mut boundary::Hardware(&self.stages.machine)) }
    fn release_configuration_hardware(&mut self, holder: ConfigurationHolder) { compiled::release_configuration(&mut boundary::Hardware(&self.stages.machine), holder.0) }
    fn enable(&mut self, channel: PipelineChannel, stages: Stages, payloads: Location) -> bool {
        let block = self.block(payloads);
        pipeline::enable_configured_stages(&mut self.stages, pipeline::Client(channel.0), pipeline::Flags(stages.0), &block) == 0
    }
    fn disable(&mut self, channel: PipelineChannel, stages: Stages) -> bool {
        pipeline::disable_stages(&mut self.stages, pipeline::Client(channel.0), pipeline::Flags(stages.0)) == 0
    }
    fn apply(&mut self, channel: PipelineChannel, stages: Stages, payloads: Location) -> bool {
        let block = self.block(payloads);
        pipeline::apply_parameters(&mut self.stages, pipeline::Client(channel.0), pipeline::Flags(stages.0), &block) == 0
    }
    fn configure(&mut self, channel: PipelineChannel, stages: Stages, records: Location) -> bool {
        let block = self.block(records);
        pipeline::configure_geometry(&mut self.stages, pipeline::Client(channel.0), pipeline::Flags(stages.0), &block) == 0
    }
    fn restore_stages(&mut self, _: Location, stages: Stages) {
        compiled::restore(scan::AppliedParts(stages.0), &mut self.adjustments(), &mut boundary::Hardware(&self.stages.machine));
    }
    fn point_statistics(&mut self, channel: PipelineChannel, stages: Stages, destinations: &StatisticsDestinations) { boundary::point_statistics(&self.stages.machine, channel, stages, destinations) }
    fn collect_statistics(&mut self, channel: PipelineChannel, stages: Stages, into: Location) { boundary::collect_statistics(&self.stages.machine, channel, stages, into) }
}

impl<M: Machine + Clone, P, I: Isp> StagePayloads for GeometryBindings<M, P, I> {
    fn mode_payload(&mut self, request: &ModeRequest, into: Location) -> Stages { boundary::mode_payload(&self.stages.machine, request, into) }
    fn tertiary_payload(&mut self, group: PipelineGroup, index_scale: u8, index: u8, into: Location) -> Stages { boundary::tertiary_payload(&self.stages.machine, group, index_scale, index, into) }
    fn set_format(&mut self, payload: &xm1_saclac_interface::pipeline::FormatPayload) -> bool { boundary::format(&mut self.stages, payload) }
    fn quad_selection_offset(&mut self, group: PipelineGroup) -> Option<ConfigOffset> { profile::mode_key(group.0).map(ConfigOffset) }
    fn quad_gains(&mut self, group: QuadGroup, limit: i16, base_only: bool) -> [u16; 4] { profile::adjusted_values(&self.adjustments(), profile::ParameterSet::new(group.0), i32::from(limit), base_only).values }
    fn quad_area_scale(&mut self, area: u32) -> u32 { profile::reciprocal_scale(area) }
    fn quad_area_limit(&mut self) -> u16 { profile::output_limit(&self.adjustments()) }
    fn primary_payload(&mut self, group: PipelineGroup, window: Option<PrimaryWindow>) -> Option<PrimaryPayload> { boundary::primary_payload(&self.stages.machine, &self.adjustments(), group, window) }
    fn secondary_payload(&mut self, group: PipelineGroup, window: Option<SecondaryWindow>) -> Option<SecondaryPayload> { boundary::secondary_payload(&self.stages.machine, group, window) }
    fn has_windowed_payload(&mut self, group: PipelineGroup) -> bool { profile::is_kind_five(group.0) }
}

impl<M: Machine + Clone, P, I> ScanConfiguration for GeometryBindings<M, P, I> {
    fn set(&mut self, at: Location, setting: ScanSetting) { boundary::set_scan(&self.stages.machine, at, setting) }
    fn clear_scan(&mut self, at: Location) { scan::clear(&mut boundary::scan_block(at)); }
    fn default_scan(&mut self, at: Location) { boundary::scan_block(at).set_defaults(&mut config::StockScanLibrary) }
    fn channel_code(&mut self, channel: u32, codes: &'static [u16; 10]) -> u16 { boundary::channel_code(&self.stages.machine, channel, codes) }
    fn export_scan(&mut self, at: Location, part: ScanExport, out: StockData) { boundary::export_scan(&self.stages.machine, at, part, out) }
    fn compile_scan_configuration(&mut self, output_kind: u8) { boundary::compile_scan(output_kind) }
    fn apply_selection(&mut self, selection: Location, configuration: Location) { boundary::apply_selection(&self.stages.machine, selection, configuration) }
    fn attach_scan_configuration(&mut self, configuration: Location, stages: Stages) {
        compiled::attach(scan::AppliedParts(stages.0), xm1_saclac_shared::stock::StockConfiguration::from_pipeline(boundary::compiled(configuration).library()), &mut self.adjustments());
    }
    fn rate_code(&mut self, offset_sum: u32) -> u32 { let mut lens = self.adjustments(); u32::from(xm1_saclac_shared::lens::zone(offset_sum as i32, &mut lens).get()) }
    fn limit_tenths(&mut self, limit: i16) -> u16 { boundary::converted_limit(limit) }
    fn output_level(&mut self) -> u32 { boundary::output_level(&self.stages.machine) }
    fn channel_count(&mut self) -> u8 { channel::count(&self.adjustments()) as u8 }
}

impl<M: Machine + Clone, P: ImageProcessor, I> GeometryService for GeometryBindings<M, P, I> {
    fn start_geometry(&mut self, start: &xm1_saclac_profile_geometry::geometry::GeometryStart, callback: StockCode) -> bool {
        face::submit_triplet(&mut self.peers, [start.mode, start.selection, start.kind], StockCallbackWord(callback.0)).is_ok()
    }
    fn stop_geometry(&mut self) -> bool { face::FaceTracking::initialize(&mut config::StockFaceTracking).is_ok() }
    fn query_areas(&mut self, code: u32, query: &xm1_saclac_profile_geometry::geometry::AreaQuery) -> AreaAnswer { boundary::query_areas(&self.stages.machine, code, query) }
    fn notify_geometry(&mut self, callback: StockCode, status: i32) { self.stages.machine.enter(callback.0, &[status as usize]); }
}
