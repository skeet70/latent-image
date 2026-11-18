#![forbid(unsafe_code)]

use xm1_saclac_config::{limits::{self, Kind, Limit, Side, WordQuery}, numeric, stock::StockDumpBuffer, table::{self, Row, Step}};
use xm1_saclac_shared::settings::Channel;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::{stock::{record as r, shared as s}, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{Frame, LensPoint}, finish_work::{AreaPositions, CorrectionRecord}, pipeline_work::{AreaBlock, AreaSlot, OutcomeHistory, OutcomeValue, PipelineLens, PipelineRecords, StatisticsRecordValues}, stock::{finish, pipeline}};
use crate::{calibration_inputs::{self, Native}, shot_services::Libraries};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> PipelineRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn stamp_frame(&mut self, frame: Frame, lens_reference: u32) {
        let services = self.services.borrow();
        StockDumpBuffer { address: frame.0, memory: Native(&services.machine, &services.stock) }.export_configuration(
            &services.stock, &mut Native(&services.machine, &services.stock), &mut Native(&services.machine, &services.stock), Channel(lens_reference as i32),
        );
    }
    fn resource(&mut self, record: RecordHandle) -> u32 {
        u32::from(self.services.borrow_mut().entries.header_byte(record, SharedHeaderByte::At47))
    }
    fn resource_bank(&mut self, record: RecordHandle) -> u32 {
        u32::from(self.services.borrow_mut().entries.header_byte(record, SharedHeaderByte::At46))
    }
    fn outcome_history(&mut self, record: RecordHandle) -> OutcomeHistory {
        OutcomeHistory(self.services.borrow().shared_bytes(record, &s::group_33::AT_07A0))
    }
    fn store_outcome_history(&mut self, record: RecordHandle, history: &OutcomeHistory) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_33::AT_07A0, history.0);
    }
    fn store_outcome(&mut self, record: RecordHandle, value: OutcomeValue) {
        let mut services = self.services.borrow_mut();
        macro_rules! store { ($field:ident, $value:expr) => {{ let _ = services.records.write(record, &r::group_02::$field, $value); }}; }
        match value {
            OutcomeValue::Status(value) => store!(AT_00C8_4, value),
            OutcomeValue::PositionCode(value) => store!(AT_00CC, u32::from(value)),
            OutcomeValue::Point(point) => store!(AT_00D4, pipeline::point_bytes(&point)),
            OutcomeValue::AdjustedPoint(point) => store!(AT_00E4, pipeline::point_bytes(&point)),
            OutcomeValue::Cell(column, row) => {
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&column.to_le_bytes());
                bytes[4..].copy_from_slice(&row.to_le_bytes());
                store!(AT_00F4, bytes);
            }
            OutcomeValue::Offset(value) => store!(AT_00FC, value as u16),
            OutcomeValue::Flag(value) => store!(AT_0100, value),
            OutcomeValue::Source(value) => store!(AT_00D0, value),
        }
    }
    fn store_outcome_correction(&mut self, record: RecordHandle, correction: &CorrectionRecord) {
        let mut bytes = [0; pipeline::CORRECTION_BYTES];
        bytes.copy_from_slice(&correction.bytes[..pipeline::CORRECTION_BYTES]);
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_19::AT_0120, bytes);
    }
    fn store_area_block(&mut self, record: RecordHandle, slot: AreaSlot, areas: &AreaBlock) {
        let mut services = self.services.borrow_mut();
        match slot {
            AreaSlot::First => { let _ = services.records.write(record, &r::group_02::AT_0104, areas.count); }
            AreaSlot::Second => { let _ = services.records.write(record, &r::group_02::AT_0108, areas.source); }
            AreaSlot::Third => {
                let bytes = pipeline::area_block(areas);
                let mut cells = [0; 40];
                cells.copy_from_slice(&bytes[24..]);
                let _ = services.records.write(record, &r::group_02::AT_011C, cells);
            }
        }
    }
    fn store_outcome_positions(&mut self, record: RecordHandle, positions: &AreaPositions) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_34::AT_0814, finish::area_positions_bytes(positions));
    }
    fn statistics_values(&mut self, record: RecordHandle) -> StatisticsRecordValues {
        let mut services = self.services.borrow_mut();
        StatisticsRecordValues {
            selection: u32::from(services.field(record, &r::group_00::AT_004C)),
            layout: services.shared(record, &s::group_27::AT_0740),
            flags: [services.entries.header_byte(record, SharedHeaderByte::At63), services.field(record, &r::group_00::INTERVAL_TRACKING).0, services.field(record, &r::group_00::AT_006D), services.field(record, &r::group_00::AT_0058)],
            area: services.field(record, &r::group_13::AT_20B8),
            extent: services.field(record, &r::group_13::AT_20B4),
            sample_kind: services.field(record, &r::group_00::AT_0059),
        }
    }
    fn lens_value(&mut self, value: PipelineLens) -> u32 {
        let services = self.services.borrow();
        match value {
            PipelineLens::Drive => calibration_inputs::lens_position_flags(&services.machine),
            PipelineLens::Position => calibration_inputs::lens_position(&services.machine),
            PipelineLens::Direction => calibration_inputs::lens_direction(&services.machine),
        }
    }
    fn position_code(&mut self, point: &LensPoint, drive: u32) -> u16 {
        self.services.borrow().machine.enter(0x749be4, &[point.position as i32 as u32 as usize, point.reference.0 as usize, point.state as usize, drive as usize, point.kind as usize]) as u16
    }
    fn tone_level(&mut self, level: i16, reference: u32, state: u32, geometry: u32, mode: u32) -> u32 {
        let services = self.services.borrow();
        limits::channel_step_word(&services.stock, &mut Native(&services.machine, &services.stock), WordQuery {
            level, channel: Channel(reference as i32), step: Step(state), export_first: geometry,
            export_second: 0, direction_mode: mode,
        }) as u32
    }
    fn level_bounds(&mut self, state: u32, reference: u32) -> (u32, u32) {
        let services = self.services.borrow();
        (table::rounded_word(&services.stock, Step(state), Row(reference)) as i32 as u32,
            table::compensated_word(&services.stock, Step(state), Row(reference)) as i32 as u32)
    }
    fn level_value(&mut self, level: u32) -> u32 {
        numeric::round_tenths(level as i32) as u32
    }
    fn channel_threshold(&mut self, reference: u32, state: u32, scaled: u32) -> u32 {
        self.services.borrow().machine.enter(0x749254, &[reference as usize, state as usize, scaled as usize])
    }
    fn correction_base(&mut self, tolerance_kind: u32) -> u16 {
        // Capture +0xf0 comes from 0x76f360, whose result is 0 or 1.
        let kind = match tolerance_kind { 0 => Kind::First, 1 => Kind::Second, _ => panic!("pipeline drive kind outside its producer domain") };
        limits::limit(&self.services.borrow().stock, Limit::Half, kind, Side::Other, 8) as u16
    }
}

