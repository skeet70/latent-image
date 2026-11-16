#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::motion::ports::LensTables;
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{Calc, Calc1, ImageProcessor, ImgDc};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{Lens, LensPoint, LensReading}, finish_work::{FinishLayers, FinishLensValue}, stock};
use crate::{round_ports::RoundPorts, shot_services::Libraries};

impl<M: Machine + Clone + crate::shot_native::AtomicLensChange, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics,
    I: Isp, T: Tables, A, Q> Lens for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn read(&mut self, reading: LensReading) -> u32 {
        match reading {
            LensReading::State => {
                let services = self.services.borrow();
                xm1_power_lifecycle::record_profile_mapping::helper_76dbb4(
                    &mut crate::calibration_inputs::runtime(&services.machine),
                ).expect("lens normalized state")
            }
            LensReading::Position => FinishLayers::lens(self, FinishLensValue::Position),
            LensReading::Kind => FinishLayers::lens(self, FinishLensValue::Direction),
            LensReading::Control => FinishLayers::lens(self, FinishLensValue::Drive),
            LensReading::Reference => FinishLayers::lens(self, FinishLensValue::Distance),
        }
    }
    fn take_change(&mut self) -> u32 { self.services.borrow().machine.take_lens_change().0 }
    fn designated_point(&mut self) -> Option<LensPoint> {
        stock::designated_point(&self.services.borrow_mut().entries.lens_designation(self.record))
    }
    fn target_distance(&mut self, point: LensPoint, control: u32) -> u32 {
        self.services.borrow().machine.enter(0x74_9be4, &[point.position as i32 as usize, point.reference.0 as usize, point.state as usize, control as usize, point.kind as usize])
    }
    fn target_step(&mut self, point: LensPoint) -> u16 {
        let mut services = self.services.borrow_mut();
        let mut geometry = self.geometry.borrow_mut();
        let mut ports = RoundPorts::new(&mut services, &mut geometry, self.recognition, self.record);
        let selected = xm1_saclac_calc::scene_recognition::frame::target(&mut ports, point.reference.0, point.state);
        LensTables::side_adjusted(&mut ports, selected, 1, point.kind) as u16
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    xm1_shot_sequence::boundary::ExposureServices for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn lens_offset(&mut self, reference: xm1_shot_sequence::boundary::LensReference) -> u16 {
        xm1_saclac_calc::exposure::ports::Lens::reference_offset(&mut *self.services.borrow_mut(), reference.0) as u16
    }
    fn calibration_offset(&mut self) -> u16 {
        xm1_saclac_calc::exposure::settings::level_step(&mut *self.services.borrow_mut(), 3) as u16
    }
    fn exports_last_correction(&mut self) -> bool {
        xm1_saclac_calc::exposure::ports::Properties::property(&mut *self.services.borrow_mut(), xm1_saclac_calc::stock::property::CORRECTION_FLAGS) & 2 != 0
    }
}
