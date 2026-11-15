#![forbid(unsafe_code)]

use xm1_power_lifecycle::{record_address_space::GuestAddress, record_image_command_resources};
use xm1_saclac_calc::exposure::ports::{Properties, Property};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::Machine};
use xm1_shot_sequence::{boundary::Frame, capture_boundary::{CaptureAdjustment, CaptureDiagnostics, CapturePlatform, CaptureTrace, DistanceSplit, FrameOffset}};
use crate::shot_services::Libraries;

struct Configuration<'a, S>(&'a S);
impl<S: Adjustments> Properties for Configuration<'_, S> {
    fn property(&mut self, property: Property) -> u32 {
        u32::from(self.0.byte(AdjustmentKey(property.key())))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> CaptureDiagnostics for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn trace(&mut self, trace: CaptureTrace) {
        self.services.borrow().machine.enter(0x72_872c, &[xm1_shot_sequence::capture_stock::trace_word(trace) as usize]);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: xm1_saclac_interface::ImageProcessor, I: xm1_saclac_imaging::driver::Isp, T: xm1_saclac_profile_geometry::controller::Tables, A, Q>
    CapturePlatform for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn adjustment(&mut self, adjustment: CaptureAdjustment) -> u8 {
        self.services.borrow().stock.byte(AdjustmentKey(u32::from(xm1_shot_sequence::capture_stock::adjustment_id(adjustment))))
    }

    fn prepare_setting(&mut self) {
        use xm1_saclac_profile_geometry::stock::records::Parameters;
        let machine = self.services.borrow().machine.clone();
        let descriptor = Parameters::from_bytes(core::array::from_fn(|index| machine.read_u8(0x016f_84f4 + index as u32)));
        let mut geometry = self.geometry.borrow_mut();
        let _ = geometry.apply_parameters(&descriptor);
        let _ = geometry.start();
    }

    fn refresh_setting(&mut self) { let _ = self.geometry.borrow_mut().stop(); }

    fn monitor_device_commands(&mut self, monitored: bool) {
        use xm1_saclac_shared::stock::DeviceCommandMonitoring;
        self.services.borrow_mut().stock.monitor_device_commands(if monitored { DeviceCommandMonitoring::Enabled } else { DeviceCommandMonitoring::Disabled });
    }

    fn device_status_bit(&mut self) -> bool { self.geometry.borrow_mut().device_status() }

    fn slot_rate(&mut self, active: u32, profile: u32) -> u32 {
        let services = self.services.borrow();
        let active = active != 0;
        let aspect = if active { 0 } else { services.machine.read_u8(0x00ef_6df2u32.wrapping_add(profile.wrapping_mul(20))) };
        u32::from(xm1_saclac_calc::adjust::focus_profile::context_category(active, aspect))
    }

    fn split_distance(&mut self, distance: i16) -> DistanceSplit {
        crate::calibration_inputs::split_distance(&self.services.borrow().machine, distance)
    }

    fn device_value(&mut self) -> u32 { self.services.borrow().machine.read_u32(0x0091_4dec) }

    fn export_record_snapshot(&mut self, frame: Frame) {
        let output = (frame.0 != 0).then_some(xm1_saclac_profile_geometry::ports::DumpBuffer(frame.0));
        self.geometry.borrow_mut().export_record_snapshot(output);
    }

    fn export_pipeline(&mut self, frame: Frame) {
        record_image_command_resources::helper_103b054(&mut crate::calibration_inputs::runtime(&self.services.borrow().machine), GuestAddress::new(frame.0))
            .expect("capture pipeline export");
    }

    fn prepare_dump_area(&mut self, frame: Frame, offset: FrameOffset) {
        let services = self.services.borrow();
        let mut machine = services.machine.clone();
        for (index, value) in xm1_saclac_calc::adjust::rate_policy::configuration_words(&mut Configuration(&services.stock)) {
            machine.write_u32(frame.0.wrapping_add(offset.0).wrapping_add((index as u32 + 1) * 4), value);
        }
    }
}
