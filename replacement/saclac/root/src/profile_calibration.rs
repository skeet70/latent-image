#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::rate_policy::ProfileCurves, exposure::ports::{CaptureProfiles, Properties, Property}};
use xm1_saclac_imaging::{capture::gains::{GainClass, GainSources}, driver::Isp};
use xm1_saclac_interface::ImageProcessor;
use xm1_saclac_profile_geometry::{controller::{ProfileGeometry, Tables}, profile::ProfileHandle, services::SelectionRequest};
use xm1_saclac_shared::stock::Machine;
use crate::{capture_gains::Sources, geometry::GeometryBindings, stock::geometry::Memory};

pub(crate) struct ProfileCalibration<'a, M, P, I, T>(pub(crate) &'a mut ProfileGeometry<Memory<M>, GeometryBindings<M, P, I>, T>);

impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> Properties for ProfileCalibration<'_, M, P, I, T> {
    fn property(&mut self, property: Property) -> u32 { u32::from(self.0.ports().stages.machine.enter(0x69_f8fc, &[property.key() as usize]) as u8) }
}
impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> CaptureProfiles for ProfileCalibration<'_, M, P, I, T> {
    fn level_offset(&mut self, profile: u32) -> i16 { self.0.profiles().aspect_value(ProfileHandle(profile)) }
    fn class_offset(&mut self, class: u8) -> i16 { self.0.profiles().tables().aspect_value(if class > 1 { 0 } else { class }) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.0.profiles().aspect_index(ProfileHandle(profile)) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.0.profiles().threshold_index(ProfileHandle(profile)) }
    fn sensitivity_class_for(&mut self, value: u32) -> u8 { self.0.profiles().threshold_index_for(value) }
    fn configured_level(&mut self) -> u16 { self.0.selection_values(SelectionRequest { level: true, ..SelectionRequest::default() }).level.expect("requested level") }
    fn configured_profile(&mut self, _: i16) -> u32 { 1 } // 0xeea094 returns one on both branches.
    fn record_profile(&mut self) -> u32 { 1 } // 0xef6790.
    fn default_dimensions(&mut self) -> [u16; 3] { Sources(&self.0.ports().stages.machine).class_gains(GainClass(5), 0) }
    fn grid_steps(&mut self, layout: u8) -> [u8; 2] { self.0.level_pair(0, u32::from(layout)).0 }
}
impl<M: Machine + Clone, P: ImageProcessor, I: Isp, T: Tables> ProfileCurves for ProfileCalibration<'_, M, P, I, T> {
    fn profile_curve(&mut self, profile: i32, class: u8) -> u32 { self.0.profile_curve(profile, class) as u32 }
}

impl<M: Machine + Clone, R: xm1_saclac_profile_records::Memory, E: xm1_saclac_profile_records::StockEntries, P> ProfileCurves for crate::services::Services<M, R, E, P> {
    fn profile_curve(&mut self, profile: i32, class: u8) -> u32 {
        xm1_saclac_profile_geometry::controller::refresh::profile_curve_value(
            |index| self.machine.read_u16(0xef_50c4 + 2 * index as u32) as i16,
            |offset| xm1_saclac_config::adjustment::half(&self.stock, offset), profile, class,
        ) as u32
    }
}
