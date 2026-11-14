#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{field::Index, stock::{record as r, shared as s}, Memory};
use xm1_shot_sequence::boundary::{ExposureOffset, ExposureRecord, ExposureSlot, MeteringBlock, METERING_SOURCE_BYTES};
use crate::shot_services::Libraries;

impl<M, R: Memory, E, P, I, T, A, Q> ExposureRecord for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn store_exposure(&mut self, slot: ExposureSlot, value: i32) {
        let mut services = self.services.borrow_mut();
        macro_rules! store { ($field:ident) => {{ let _ = services.records.write(self.record, &r::group_01::$field, value as _); }}; }
        match slot.0 {
            0 => store!(AT_0090_2), 1 => store!(AT_0092), 2 => store!(AT_0094),
            3 => store!(AT_0096), 4 => store!(AT_0098), 5 => store!(AT_009A),
            6 => store!(AT_009C), 7 => store!(AT_009E), 8 => store!(AT_00A0),
            9 => store!(AT_00A2), 10 => store!(AT_00A4), 11 => store!(AT_00A6),
            12 => store!(AT_00A8), 13 => store!(AT_00AA), 14 => store!(AT_00AC),
            15 => store!(AT_00AE), 16 => store!(AT_00B0), 17 => store!(AT_00B2),
            18 => store!(AT_00B4), 19 => store!(AT_00B6), 20 => store!(AT_00B8),
            21 => store!(AT_00BA), 22 => store!(AT_00BC), 23 => store!(AT_00C0),
            24 => store!(AT_00C4), _ => panic!("shot exposure slot outside the record table"),
        }
    }
    fn store_offset(&mut self, offset: ExposureOffset, value: i16) {
        let field = match offset { ExposureOffset::First => &r::group_30::AT_17F0, ExposureOffset::Second => &r::group_30::AT_17F2, ExposureOffset::Third => &r::group_30::AT_17F4 };
        let _ = self.services.borrow_mut().records.write(self.record, field, value);
    }
    fn store_area_exposure(&mut self, index: usize, value: i16) {
        let index = Index::new(index).expect("shot exposure area");
        let _ = self.services.borrow_mut().records.write_element(self.record, &r::group_30::AT_1864, index, value);
    }
    fn metering(&mut self) -> MeteringBlock {
        MeteringBlock(self.services.borrow().records.read_shared(self.record, &s::group_18::AT_00E0).unwrap_or([0; 64]))
    }
    fn metering_source(&mut self) -> [u8; METERING_SOURCE_BYTES] {
        self.services.borrow().records.read(self.record, &r::group_01::AT_0090_56).unwrap_or([0; METERING_SOURCE_BYTES])
    }
    fn store_metering(&mut self, block: &MeteringBlock) {
        let _ = self.services.borrow_mut().records.write_shared(self.record, &s::group_18::AT_00E0, block.0);
    }
}

impl<M: xm1_saclac_shared::stock::Machine + Clone, R, E, P: xm1_saclac_interface::ImageProcessor, I: xm1_saclac_imaging::driver::Isp, T: xm1_saclac_profile_geometry::controller::Tables, A, Q>
    xm1_shot_sequence::boundary::Recorder for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn recording(&mut self, mode: xm1_shot_sequence::boundary::RecorderMode) -> bool {
        self.geometry.borrow_mut().record_mode_available(record_channel(mode))
    }
    fn start_recording(&mut self, mode: xm1_shot_sequence::boundary::RecorderMode) {
        let _ = self.geometry.borrow_mut().begin_record_mode(record_channel(mode));
    }
    fn record_evaluations(&mut self, count: u8) {
        let _ = self.geometry.borrow_mut().finish_record_mode(u16::from(count));
    }
}

fn record_channel(mode: xm1_shot_sequence::boundary::RecorderMode) -> xm1_saclac_profile_geometry::record_mode::RecordChannel {
    match mode {
        xm1_shot_sequence::boundary::RecorderMode::Exposure => xm1_saclac_profile_geometry::record_mode::RecordChannel::SHORT,
    }
}

impl<M: xm1_saclac_shared::stock::Machine + Clone, R, E, P: xm1_saclac_interface::ImageProcessor, I: xm1_saclac_imaging::driver::Isp, T: xm1_saclac_profile_geometry::controller::Tables, A, Q>
    xm1_shot_sequence::pipeline_work::PipelineProfiles for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn requires_adjustment(&mut self, profile: u32) -> bool {
        self.geometry.borrow().profiles().requires_adjustment(xm1_saclac_profile_geometry::profile::ProfileHandle(profile))
    }
    fn record_mode_available(&mut self, mode: u32) -> bool {
        self.geometry.borrow_mut().record_mode_available(xm1_saclac_profile_geometry::record_mode::RecordChannel(mode))
    }
    fn begin_record_mode(&mut self, mode: u32) {
        let _ = self.geometry.borrow_mut().begin_record_mode(xm1_saclac_profile_geometry::record_mode::RecordChannel(mode));
    }
    fn finish_record_mode(&mut self, threshold: u8) {
        let _ = self.geometry.borrow_mut().finish_record_mode(u16::from(threshold));
    }
    fn configure_extended_operations(&mut self) {
        self.geometry.borrow_mut().configure_extended_operations(xm1_saclac_profile_geometry::services::ExtendedOperations::Primary);
    }
    fn configure_secondary_operations(&mut self) {
        self.geometry.borrow_mut().configure_extended_operations(xm1_saclac_profile_geometry::services::ExtendedOperations::Secondary);
    }
}

impl<M: xm1_saclac_shared::stock::Machine + Clone, R: Memory, E: xm1_saclac_profile_records::StockEntries, P, I, T, A, Q>
    xm1_shot_sequence::calculation_work::CalculationProfiles for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn profile(&mut self, record: xm1_saclac_interface::RecordHandle, value: xm1_shot_sequence::calculation_work::CalculationProfile) -> u32 {
        use xm1_shot_sequence::calculation_work::CalculationProfile as V;
        use xm1_saclac_profile_records::SharedHeaderByte;
        let mut services = self.services.borrow_mut();
        macro_rules! field { ($group:ident::$name:ident) => { services.field(record, &r::$group::$name) as u32 }; }
        match value {
            V::LensReference => field!(group_01::AT_00BC),
            V::SelectionKind => u32::from(xm1_saclac_calc::profile::selection_code(services.entries.measurement_selection(record))),
            V::RowLimit => field!(group_00::AT_003D),
            V::RowOption => u32::from(services.field(record, &r::group_00::INTERVAL_TRACKING).0),
            V::AdjustmentKind => field!(group_00::AT_0051),
            V::AdjustmentOption => u32::from(services.entries.header_byte(record, SharedHeaderByte::At63)),
            V::ResumeLead => field!(group_00::AT_001E), V::ResumeOffset => field!(group_00::AT_001C),
            V::At0024 => field!(group_00::AT_0024), V::At0026 => field!(group_00::AT_0026),
            V::At0032 => field!(group_00::AT_0032), V::At0033 => field!(group_00::AT_0033),
            V::At0034 => field!(group_00::AT_0034), V::At003c => field!(group_00::AT_003C),
            V::At003e => field!(group_00::AT_003E), V::At0045 => field!(group_00::AT_0045),
            V::At004b => field!(group_00::AT_004B), V::At005d => field!(group_00::AT_005D),
            V::At0066 => field!(group_00::AT_0066),
            V::HeaderAt31 => u32::from(services.entries.header_byte(record, SharedHeaderByte::At31)),
        }
    }
    fn store_resume_offset(&mut self, record: xm1_saclac_interface::RecordHandle, offset: u16) {
        let _ = self.services.borrow_mut().records.write_shared(record, &s::group_41::AT_0972, offset as i16);
    }
    fn reuse_control(&mut self) -> u32 {
        use xm1_saclac_shared::settings::Adjustments;
        u32::from(self.services.borrow().stock.byte(xm1_saclac_shared::settings::AdjustmentKey(u32::from(xm1_shot_sequence::stock::calculation::REUSE_CONTROL_PROPERTY))))
    }
    fn client_pair(&mut self, at: xm1_shot_sequence::boundary::ClientRef) -> [u32; 2] {
        let services = self.services.borrow();
        [services.machine.read_u32(at.0), services.machine.read_u32(at.0.wrapping_add(4))]
    }
}
