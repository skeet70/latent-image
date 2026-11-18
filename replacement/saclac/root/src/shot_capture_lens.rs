#![forbid(unsafe_code)]

use xm1_saclac_config::table::{self, Row, Step};
use xm1_saclac_shared::stock::Machine;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_shot_sequence::{finish_work::{FinishLayers, FinishLensValue}, secondary_boundary::MeasurementLens};
use xm1_shot_sequence::capture_boundary::{LensGeometry, RemapRequest};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R, E, P, I, T, A, Q> LensGeometry for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn reference_offset(&mut self, reference: u32) -> i16 { table::relative_word(&self.services.borrow().stock, Row(reference)) }
    fn rounded(&mut self, value: u32) -> i16 { table::primary_rounded_word(&self.services.borrow().stock, Step(value)) }
    fn convert(&mut self, base: u32, value: u32, lens: u32) -> u32 { table::nearest_step(&self.services.borrow().stock, Row(base), Step(value), Row(lens)).0 }
    fn mapped(&mut self, value: u32, selector: u32) -> u32 { u32::from(table::value(&self.services.borrow().stock, Step(value), Row(selector))) }
    fn compensated(&mut self, reference: u32, lens: u32) -> i16 { table::compensated_word(&self.services.borrow().stock, Step(reference), Row(lens)) }
    fn remap(&mut self, request: RemapRequest) -> i32 {
        let remap = u32::from(request.remap);
        xm1_power_lifecycle::record_profile_mapping::helper_74947c(
            &mut crate::calibration_inputs::runtime(&self.services.borrow().machine), request.position,
            request.reference, request.state, request.target, remap, remap, request.kind,
        ).expect("capture relative remap") as i32
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> MeasurementLens for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn lens(&mut self, value: FinishLensValue) -> u32 { FinishLayers::lens(self, value) }
    fn clamp_step(&mut self, position: i16, kind: u32, direction: u32) -> i16 { FinishLayers::clamp_step(self, position, kind, direction) }
    fn reference_value(&mut self, index: i32) -> u32 { xm1_saclac_config::channel::packed(&self.services.borrow().stock, xm1_saclac_shared::settings::Channel(index)) }
}
