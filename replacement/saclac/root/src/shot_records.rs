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
