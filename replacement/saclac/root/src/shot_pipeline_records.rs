#![forbid(unsafe_code)]

use xm1_saclac_config::{dump::{self, DumpDerivations}, limits::{self, Kind, Limit, Side, WordQuery}, numeric, ports::{Derivations, ExportQuery}, stock::{GuestMemory, StockDumpBuffer}, table::{self, Row, Step}};
use xm1_calibration_core::{Calibration, ChannelIndex, CurveIndex, configuration::{ConfigAddress, Configuration, ConfigurationError, ConfigurationMemory}};
use xm1_power_lifecycle::{record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, GuestDomain, MemoryError, RecordContext}, record_profile_globals, record_profile_mapping};
use xm1_vendor_protocol::{metadata_calibration::MetadataCalibration, metadata_math::{CalibrationValue, transform_mapped_value}};
use xm1_saclac_shared::{settings::Channel, stock::Argument::{Buffer, Word}};
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::{stock::{record as r, shared as s}, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{Frame, LensPoint}, finish_work::{AreaPositions, CorrectionRecord}, pipeline_work::{AreaBlock, AreaSlot, OutcomeHistory, OutcomeValue, PipelineLens, PipelineRecords, StatisticsRecordValues}, stock::{finish, pipeline}};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> PipelineRecords for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn stamp_frame(&mut self, frame: Frame, lens_reference: u32) {
        if frame.0 == 0 { return; }
        let services = self.services.borrow();
        let dump = dump::configuration_dump(&services.stock, &mut Native(&services.machine, &services.stock), &mut Native(&services.machine, &services.stock), Channel(lens_reference as i32));
        StockDumpBuffer { address: frame.0, memory: Native(&services.machine, &services.stock) }.write_configuration(&dump);
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
            PipelineLens::Drive => record_profile_globals::helper_76f340(&mut Native(&services.machine, &services.stock).record()).expect("lens runtime read"),
            PipelineLens::Position => record_profile_globals::helper_76da84(&mut Native(&services.machine, &services.stock).record()).expect("lens position read"),
            PipelineLens::Direction => record_profile_globals::helper_76dad4(&mut Native(&services.machine, &services.stock).record()).expect("lens direction read"),
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

struct Native<'a, M>(&'a M, &'a xm1_saclac_shared::stock::Stock<M>);

impl<M: Machine + Clone> GuestMemory for Native<'_, M> {
    fn byte(&self, at: u32) -> u8 { self.0.read_u8(at) }
    fn halfword(&self, at: u32) -> u16 { self.0.read_u16(at) }
    fn word(&self, at: u32) -> u32 { self.0.read_u32(at) }
    fn set_byte(&self, at: u32, value: u8) { self.0.clone().write_u8(at, value); }
    fn set_halfword(&self, at: u32, value: u16) {
        let mut machine = self.0.clone();
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() { machine.write_u8(at.wrapping_add(offset as u32), byte); }
    }
    fn set_word(&self, at: u32, value: u32) { self.0.clone().write_u32(at, value); }
    fn fill(&self, at: u32, value: u8, count: u32) { self.0.enter(0x7221c8, &[at as usize, usize::from(value), count as usize]); }
}

impl<M: Machine + Clone> Derivations for Native<'_, M> {
    fn export_word(&mut self, channel: Channel, query: ExportQuery) -> i16 {
        let mut word = crate::stock::acquisition::Aligned([0; 2]);
        self.0.enter_with(0x7498e0, &[Word(channel.0 as u32), Word(query.selector), Word(query.first), Word(query.second), Buffer(0), Word(0)], &mut word);
        i16::from_le_bytes(word.0)
    }
    fn adjust_direction(&mut self, word: i16, direction: u32, mode: u32) -> i16 {
        record_profile_mapping::helper_747aac(&mut self.record(), word as i32 as u32, direction, mode).expect("lens direction configuration") as i16
    }
    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16 { self.0.enter(0x749254, &[channel.0 as u32 as usize, first as usize, scale as usize]) as u16 }
    fn channel_scale(&mut self, channel: Channel) -> u16 {
        self.calibration().scaled_channel_curve(ChannelIndex(channel.0)).expect("channel calibration").0
    }
    fn table_scale(&mut self, step: u32, channel: Channel) -> u16 {
        self.metadata().scaled_mapped_word(CurveIndex(step), ChannelIndex(channel.0)).expect("mapped calibration").0
    }
    fn curve_position(&mut self, level: i16, channel: Channel) -> u32 { self.0.enter(0x746120, &[level as i32 as u32 as usize, channel.0 as u32 as usize]) }
    fn curve_value(&mut self, position: u32, channel: Channel) -> i16 { self.0.enter(0x7461a8, &[position as usize, channel.0 as u32 as usize]) as i16 }
    fn channel_word(&mut self, channel: Channel, selector: u32, table: u32) -> i16 { self.0.enter(0x747408, &[channel.0 as u32 as usize, selector as usize, table as usize]) as i16 }
    fn reference_word(&mut self, mode: u32, channel: Channel) -> i16 {
        let mut word = crate::stock::acquisition::Aligned([0; 2]);
        self.0.enter_with(0x747584, &[Word(0), Word(mode), Word(channel.0 as u32), Word(0), Word(0), Word(0), Buffer(0)], &mut word);
        i16::from_le_bytes(word.0)
    }
    fn apply_reference(&mut self, word: i16, channel: Channel) { self.0.enter(0x749be4, &[word as i32 as u32 as usize, channel.0 as u32 as usize, 0, u32::MAX as usize, 1]); }
    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32 { self.0.enter(0x7462e8, &[selector as usize, first as usize, second as usize]) }
    fn lens_limits(&mut self, table: u32, words: [u32; 11]) -> u32 {
        let mut arguments = [0; 12];
        arguments[0] = table as usize;
        for (argument, word) in arguments[1..].iter_mut().zip(words) { *argument = word as usize; }
        self.0.enter(0x74b05c, &arguments)
    }
}

impl<M: Machine + Clone> DumpDerivations for Native<'_, M> {
    fn transformed_value(&mut self, step: Step, row: Row) -> u16 {
        transform_mapped_value(CalibrationValue(table::value(self.1, step, row))) as u16
    }
    fn transformed_word(&mut self, step: Step, row: Row) -> u16 {
        self.calibration().transform_mapped_word(CurveIndex(step.0), ChannelIndex(row.0 as i32)).expect("mapped calibration").0
    }
    fn lens_position_flags(&mut self) -> u32 {
        record_profile_globals::helper_76f340(&mut self.record()).expect("lens runtime read")
    }
    fn selection_word(&mut self, channel: Channel, mode: u32, flags: u32, word: i16) -> u32 {
        self.0.enter(0x748398, &[channel.0 as u32 as usize, mode as usize, flags as usize, word as i32 as u32 as usize])
    }
}

// ConfigurationMemory addresses are offsets into the canonical adjustment area.
struct Area<'a, A>(&'a A);
impl<A: xm1_saclac_shared::settings::Adjustments> ConfigurationMemory for Area<'_, A> {
    type Error = core::convert::Infallible;
    fn read_byte(&mut self, at: ConfigAddress) -> Result<u8, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::byte(self.0, at.0)) }
    fn read_half(&mut self, at: ConfigAddress) -> Result<u16, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::half(self.0, at.0)) }
    fn read_word(&mut self, at: ConfigAddress) -> Result<u32, ConfigurationError<Self::Error>> { Ok(xm1_saclac_config::adjustment::word(self.0, at.0)) }
}

impl<M: Machine + Clone> Native<'_, M> {
    fn calibration(&self) -> Calibration<'_, Area<'_, xm1_saclac_shared::stock::Stock<M>>> {
        Calibration::new(Configuration::new(Area(self.1), ConfigAddress(0)))
    }
    fn metadata(&self) -> MetadataCalibration<'_, Area<'_, xm1_saclac_shared::stock::Stock<M>>> {
        MetadataCalibration::new(Configuration::new(Area(self.1), ConfigAddress(0)))
    }
    fn record(&self) -> RecordContext<RecordMemory<'_, M>, ()> { RecordContext::new(RecordMemory(self.0), ()) }
}

struct RecordMemory<'a, M>(&'a M);
impl<M: Machine + Clone> GuestAddressSpace for RecordMemory<'_, M> {
    fn domain(&self) -> GuestDomain { GuestDomain::A }
    fn read_u8(&mut self, at: GuestAddress) -> Result<u8, MemoryError> { Ok(self.0.read_u8(at.get())) }
    fn read_u16(&mut self, at: GuestAddress) -> Result<u16, MemoryError> { Ok(self.0.read_u16(at.get())) }
    fn read_u32(&mut self, at: GuestAddress) -> Result<u32, MemoryError> { Ok(self.0.read_u32(at.get())) }
    fn write_u8(&mut self, at: GuestAddress, value: u8) -> Result<(), MemoryError> { self.0.clone().write_u8(at.get(), value); Ok(()) }
    fn write_u16(&mut self, at: GuestAddress, value: u16) -> Result<(), MemoryError> {
        let mut machine = self.0.clone();
        for (offset, byte) in value.to_le_bytes().into_iter().enumerate() { machine.write_u8(at.get().wrapping_add(offset as u32), byte); }
        Ok(())
    }
    fn write_u32(&mut self, at: GuestAddress, value: u32) -> Result<(), MemoryError> { self.0.clone().write_u32(at.get(), value); Ok(()) }
    fn copy_bytes(&mut self, destination: GuestAddress, source: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        let mut machine = self.0.clone();
        for offset in 0..length.0 { machine.write_u8(destination.checked_add(offset)?.get(), machine.read_u8(source.checked_add(offset)?.get())); }
        Ok(())
    }
}
