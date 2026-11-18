#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::{exposure_limits::{CellLevels, ChannelLevels}, lens::{BiasTable, LensConfiguration, LensControl, LensSteps, Step, StepRequest}, profile_mode::DeviceReadiness}, exposure::ports::{self, CaptureProfiles, ImageReport, Lens, Placement, ProfileAccess, ProfileLevels, Properties, Property, StockMemory}, profile::Profile, scene_levels::FeatureClasses, stock::calculation::Cell};
use xm1_saclac_calc::adjust::{motion::{drive::{DriveKind, DriveLimits, DriveMode, DriveSpeed}, plan_ports::{PlanServices, PlanSpeedRequest, PositionRemap}}, timing::LevelScale};
use xm1_saclac_config::{channel, table};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, ImageProcessor};
use xm1_saclac_profile_geometry::{controller::{ProfileGeometry, Tables}, profile::ProfileHandle, services::LevelPair};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::calc::Tables as Calibration;
use xm1_saclac_shared::{settings::Channel, stock::{Argument::{Buffer, Word}, Machine}};
use crate::{geometry::GeometryBindings, lens_calibration::Log, profile_calibration::ProfileCalibration, services::Services, stock::{acquisition::Aligned, geometry::Memory as GeometryMemory}};

/// One operation's borrows of the shared services and geometry; exposure state stays with its caller.
pub struct Calculation<'a, M, R, E, P, I, T, A> {
    pub services: &'a mut Services<M, R, E, P>,
    pub geometry: &'a mut ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>,
    pub placement: &'a mut A,
}
impl<M: Machine, R, E, P, I, T, A> Calculation<'_, M, R, E, P, I, T, A> {
    fn calibration(&self) -> Calibration<'_, M> { Calibration(&self.services.machine) }
    fn profiles(&mut self) -> ProfileCalibration<'_, M, P, I, T> { ProfileCalibration(self.geometry) }
}

macro_rules! implement {
    ($contract:path { $($methods:tt)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
            $contract for Calculation<'_, M, R, E, P, I, T, A> { $($methods)* }
    };
}
implement!(ProfileAccess {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { self.services.profile() }
});
implement!(Properties { fn property(&mut self, property: Property) -> u32 { Properties::property(self.services, property) } });
implement!(xm1_saclac_calc::scene_recognition::ports::Overlay {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) {
        crate::round_scene::overlay_text(&self.services.machine, text, column, row, foreground, background);
    }
});
implement!(xm1_saclac_calc::adjust::motion::ports::AreaCurves {
    fn with_profile_factors(&mut self, points: [u16; 3], deltas: [u8; 2], selector: u8) -> [u16; 3] {
        self.geometry.apply_profile_factors(points, u32::from(deltas[0]), u32::from(deltas[1]), u32::from(selector))
    }
});
implement!(Lens {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { Lens::level(self.services, lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { Lens::base_level(self.services, lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { Lens::reference_offset(self.services, reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { Lens::calibration_index(self.services, lens) }
});
implement!(LensConfiguration {
    fn channel_value(&mut self, channel: i32) -> u32 { self.calibration().channel_value(channel) }
    fn channel_curve(&mut self, channel: i32) -> u32 { self.calibration().channel_curve(channel) }
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16 { self.calibration().bias(resource, table) }
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16 { self.calibration().biased_position(resource, table) }
    fn resource_count(&mut self) -> u8 { self.calibration().resource_count() }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 { self.calibration().selection_word(channel, flags, offset) }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 { self.calibration().remap(word, channel, mode) }
    fn position_limits(&mut self, key: u32) -> [i16; 2] { self.calibration().position_limits(key) }
    fn mapped_value(&mut self, key: u32) -> u32 { self.calibration().mapped_value(key) }
});
implement!(LensControl {
    fn position(&mut self) -> u16 { self.calibration().position() }
    fn travel_margin(&mut self) -> u16 { self.calibration().travel_margin() }
});
implement!(LensSteps {
    fn step(&mut self, request: StepRequest) -> Step { self.calibration().step(request) }
    fn travel_distance(&mut self, resource: u32, index: u32, travel: i16) -> u16 { self.calibration().travel_distance(resource, index, travel) }
});
implement!(CaptureProfiles {
    fn level_offset(&mut self, profile: u32) -> i16 { self.profiles().level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { self.profiles().class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { self.profiles().offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { self.profiles().sensitivity_class(profile) }
    fn sensitivity_class_for(&mut self, value: u32) -> u8 { self.profiles().sensitivity_class_for(value) }
    fn configured_level(&mut self) -> u16 { self.profiles().configured_level() }
    fn configured_profile(&mut self, level: i16) -> u32 { self.profiles().configured_profile(level) }
    fn record_profile(&mut self) -> u32 { self.profiles().record_profile() }
    fn default_dimensions(&mut self) -> [u16; 3] { self.profiles().default_dimensions() }
    fn grid_steps(&mut self, layout: u8) -> [u8; 2] { self.profiles().grid_steps(layout) }
});
implement!(ProfileLevels {
    fn profile_level(&mut self, profile: u32) -> i16 {
        let mut output = Aligned([0; 2]);
        self.services.machine.enter_with(0x100_6000, &[Word(profile), Word(0), Buffer(0)], &mut output.0);
        i16::from_le_bytes(output.0)
    }
    fn converted_level(&mut self, profile: u32, level: i16) -> u16 { self.services.machine.enter(0x100_5df8, &[profile as usize, level as i32 as usize]) as u16 }
});
implement!(ChannelLevels {
    fn channel_level(&mut self, channel: u32, offset: bool) -> i16 { channel::level(&self.services.stock, &mut Log(&self.services.machine), Channel(channel as i32), 0, offset) }
    fn compensated_level(&mut self, step: u32, reference: u32) -> i16 { table::compensated_word(&self.services.stock, table::Step(step), table::Row(reference)) }
    fn steps(&mut self) -> u32 { table::bank_size(&self.services.stock) }
});
implement!(FeatureClasses { fn feature_class(&mut self, profile: u32) -> u8 { self.geometry.profiles().entry(ProfileHandle(profile)).feature } });
implement!(CellLevels {
    fn level_class(&mut self, cell: Cell) -> u32 { LevelPair([cell.column, cell.row]).encoded_class() }
    fn row_level(&mut self, cell: Cell) -> i16 { self.geometry.record_decade(LevelPair([cell.column, cell.row])) }
});
implement!(StockMemory {
    fn read_u8(&mut self, at: SharedAddress) -> u8 { StockMemory::read_u8(self.services, at) }
    fn read_u16(&mut self, at: SharedAddress) -> u16 { StockMemory::read_u16(self.services, at) }
    fn read_u32(&mut self, at: SharedAddress) -> u32 { StockMemory::read_u32(self.services, at) }
    fn write_u8(&mut self, at: SharedAddress, value: u8) { StockMemory::write_u8(self.services, at, value); }
    fn write_u16(&mut self, at: SharedAddress, value: u16) { StockMemory::write_u16(self.services, at, value); }
    fn write_u32(&mut self, at: SharedAddress, value: u32) { StockMemory::write_u32(self.services, at, value); }
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8) { StockMemory::fill(self.services, at, bytes, value); }
});
implement!(Placement { fn address(&mut self, block: ports::InPlace) -> SharedAddress { self.placement.address(block) } });
implement!(ImageReport {
    fn second_report_word(&mut self) -> u32 { self.services.second_report_word() }
    fn fourth_report_word(&mut self) -> u32 { self.services.fourth_report_word() }
});
implement!(DeviceReadiness { fn device_ready(&mut self) -> bool { self.services.machine.enter(0x74_081c, &[]) != 0 } });
implement!(LevelScale {
    fn linear(&mut self, level: i16) -> u32 { LevelScale::linear(self.services, level) }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 { LevelScale::log2_difference(self.services, value, reference) }
});
implement!(xm1_saclac_calc::lens_checks::LensControl {
    fn held_reference(&mut self) -> u32 { xm1_saclac_calc::lens_checks::LensControl::held_reference(self.services) }
});
implement!(DriveLimits {
    fn first_ramp_speed(&mut self, kind: DriveKind, mode: DriveMode) -> u32 { self.calibration().first_ramp_speed(kind, mode) }
    fn first_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32 { self.calibration().first_ramp_acceleration(kind, mode) }
    fn second_ramp_acceleration(&mut self, kind: DriveKind, mode: DriveMode) -> u32 { self.calibration().second_ramp_acceleration(kind, mode) }
    fn slowest_ramped_speed(&mut self) -> u32 { self.calibration().slowest_ramped_speed() }
});
implement!(DriveSpeed {
    fn top_speed(&mut self, kind: DriveKind, mode: DriveMode) -> u32 {
        use xm1_saclac_config::limits;
        limits::limit(&self.services.stock, limits::Limit::OptionalShiftedNextHalf,
            if kind.0 == 0 { limits::Kind::First } else { limits::Kind::Second },
            if mode.direction == 1 { limits::Side::One } else { limits::Side::Other }, mode.step)
    }
});
implement!(PlanServices {
    fn plan_speed(&mut self, request: PlanSpeedRequest) -> Step { self.services.plan_speed(request) }
    fn remap_position(&mut self, request: PositionRemap) -> i16 { self.services.remap_position(request) }
    fn minimum_speed(&mut self) -> u32 { self.services.minimum_speed() }
    fn current_side(&mut self) -> u32 { self.services.current_side() }
    fn checks_drive_speed(&mut self) -> bool { self.services.checks_drive_speed() }
    fn load_register(&mut self, register: u8) -> u8 { self.services.load_register(register) }
    fn snapshot_mode(&mut self) -> u32 { self.services.snapshot_mode() }
    fn cached_focus(&mut self) -> Option<[u32; 4]> { self.services.cached_focus() }
    fn release_aux_text(&mut self) { self.services.release_aux_text(); }
});
implement!(xm1_saclac_calc::gains::CaptureGains {
    fn gains(&mut self, request: &xm1_saclac_calc::gains::GainRequest) -> xm1_saclac_calc::gains::SelectedGains {
        xm1_saclac_calc::gains::CaptureGains::gains(self.services, request)
    }
    fn blend_weight(&mut self, selector: &xm1_saclac_calc::gains::BlendSelector) -> u8 {
        xm1_saclac_calc::gains::CaptureGains::blend_weight(self.services, selector)
    }
});
implement!(xm1_saclac_calc::diagnostics::CacheMaintenance {
    fn invalidate(&mut self, at: SharedAddress, bytes: usize) {
        self.services.machine.enter(0x74_0e74, &[at.0 as usize, bytes]);
    }
});
implement!(xm1_saclac_calc::adjust::context_trace::ChannelPixels {
    fn channel_pixels(&mut self, pixels: u32) -> [u32; 4] {
        let mut output = Aligned([0; 20]);
        self.services.machine.enter_with(0xef_56cc, &[Buffer(0), Word(pixels)], &mut output.0);
        core::array::from_fn(|index| u32::from_le_bytes(output.0[index * 4..index * 4 + 4].try_into().expect("channel pixels")))
    }
});
implement!(xm1_saclac_calc::adjust::profile_adjustment::LensSteps {
    fn near_step(&mut self, reference: u32, limit: u16, which: xm1_saclac_calc::adjust::profile_adjustment::StepTable) -> u32 {
        xm1_saclac_calc::adjust::profile_adjustment::LensSteps::near_step(self.services, reference, limit, which)
    }
    fn far_step(&mut self, reference: u32, limit: u16, which: xm1_saclac_calc::adjust::profile_adjustment::StepTable) -> u32 {
        xm1_saclac_calc::adjust::profile_adjustment::LensSteps::far_step(self.services, reference, limit, which)
    }
    fn lens_step(&mut self, step: u32) -> u32 { xm1_saclac_calc::adjust::profile_adjustment::LensSteps::lens_step(self.services, step) }
});
