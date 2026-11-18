#![forbid(unsafe_code)]

use xm1_saclac_calc::geometry::{ClipRule, ClipSettings};
use xm1_saclac_shared::{settings::{self, AdjustmentKey, Adjustments, AspectPair, CaptureMode, WindowScale}, stock::Machine};
use xm1_shot_sequence::{capture_stock, dump_boundary::DumpShared, secondary_boundary::{MeasurementSetting, MeasurementSettings}};
use crate::shot_services::Libraries;

impl<M: Machine, R, E, P, I, T, A, Q> MeasurementSettings for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn measurement_setting(&mut self, setting: MeasurementSetting) -> u32 {
        let services = self.services.borrow();
        let key = AdjustmentKey(u32::from(capture_stock::measurement_setting_id(setting)));
        match setting {
            MeasurementSetting::MeasuredDefault => u32::from(services.stock.byte(key))
                | (u32::from(services.stock.byte(AdjustmentKey(key.0 + 1))) << 8)
                | (u32::from(services.stock.byte(AdjustmentKey(key.0 + 2))) << 16),
            MeasurementSetting::TransferDefault | MeasurementSetting::PrimaryLimitDefault => u32::from(settings::halfword(&services.stock, key)),
            _ => u32::from(services.stock.byte(key)),
        }
    }
}
impl<M: Machine, R, E, P, I, T, A, Q> ClipSettings for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn clip_rule(&mut self) -> ClipRule {
        //f4d1f4/244:0 overlaps,1 tests visible share,every larger byte tests the centre.
        match self.services.borrow().stock.byte(AdjustmentKey(0x131c)) { 0 => ClipRule::Overlap, 1 => ClipRule::VisibleShare, _ => ClipRule::CentreInside }
    }
    fn minimum_visible_percent(&mut self) -> u32 { u32::from(self.services.borrow().stock.byte(AdjustmentKey(0x131d))) }
}
impl<M: Machine, R, E, P, I, T, A, Q> DumpShared for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn capture_window(&mut self, mode: u32, layout: [u16; 2]) -> Option<[u32; 2]> {
        let mode = CaptureMode::new(mode)?;
        match settings::capture_window(mode, AspectPair(layout), &mut self.services.borrow_mut().stock).ok()? {
            WindowScale::Unscaled => None,
            WindowScale::Scaled(window) => Some([window.alternate, window.value]),
        }
    }
}
