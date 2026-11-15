#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::focus_profile::LensResources, capture_requests::{self, LensCalibration}, exposure::ports::Placement};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::{CaptureMeasurement, CaptureProcessing}, Calc, Calc1, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::calculations_work::{CalculationsLibrary, Processing, SecondJobInput};
use crate::{calculation_services::Calculation, shot_services::Libraries};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    LensResources for Calculation<'_, M, R, E, P, I, T, A> {
    fn resource(&mut self) -> Option<u32> { crate::focus_area::Resources(&self.services.machine, &self.services.stock).resource() }
    fn reference(&mut self) -> u32 { crate::focus_area::Resources(&self.services.machine, &self.services.stock).reference() }
    fn flags(&mut self) -> u32 { crate::focus_area::Resources(&self.services.machine, &self.services.stock).flags() }
    fn channel_bias(&mut self, resource: u32) -> i32 { crate::focus_area::Resources(&self.services.machine, &self.services.stock).channel_bias(resource) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    LensCalibration for Calculation<'_, M, R, E, P, I, T, A> {
    fn calibration(&mut self, lens: u32, index: u32) -> i16 {
        use xm1_saclac_config::table::{self, Row, Step};
        use xm1_vendor_protocol::metadata_math::{transform_mapped_value, CalibrationValue};
        transform_mapped_value(CalibrationValue(table::value(&self.services.stock, Step(lens), Row(index))))
    }
}

pub(crate) fn processing(block: &Processing) -> CaptureProcessing {
    let parameters = xm1_saclac_imaging::stock::statistics::parameters(&block.0);
    CaptureProcessing {
        extent: [parameters.extent.width, parameters.extent.height],
        grid: [parameters.grid.width, parameters.grid.height],
        geometry: block.0[0x10..0x28].try_into().expect("processing geometry"),
        level: parameters.level, reference: parameters.reference as u16,
        decade: parameters.decade as u16, companion: parameters.companion,
        setting: parameters.setting, profile: parameters.profile.0,
        lens: parameters.lens, lens_reference: parameters.lens_reference,
        offset_mode: block.offset_mode(),
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
    CalculationsLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn request_first(&mut self, record: RecordHandle, block: &Processing) {
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        capture_requests::request_first(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut peers, record, &processing(block), self.callbacks.capture_calculations.first,
        ).expect("capture first calculation enqueue");
    }
    fn request_second(&mut self, input: &SecondJobInput, block: &Processing) {
        // f51fec rereads74f564; the incoming block's profile-kind word at+2c is unused.
        let measured = input.measurement;
        let measurement = CaptureMeasurement {
            first: measured.first, second: measured.second, base: input.base,
            third: measured.third, fourth: measured.fourth, slot: input.slot,
            fifth: measured.fifth, sixth: measured.sixth, paired: input.paired,
            count: measured.count, block: measured.block, seventh: measured.seventh,
        };
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        capture_requests::request_second(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut peers, input.record.unwrap_or(xm1_saclac_calc::stock::record::UNBOUND), &processing(block), &measurement,
            input.detail, self.callbacks.capture_calculations.second,
        ).expect("capture second calculation enqueue");
    }
    fn request_third(&mut self, record: RecordHandle, block: &Processing, detail: u32) {
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        capture_requests::request_third(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut peers, record, &processing(block), detail, self.callbacks.capture_calculations.third,
        ).expect("capture third calculation enqueue");
    }
    fn request_fourth(&mut self, record: RecordHandle, block: &Processing, detail: u32) {
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        capture_requests::request_fourth(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut peers, record, &processing(block), detail, self.callbacks.capture_calculations.fourth,
        ).expect("capture fourth calculation enqueue");
    }
}
