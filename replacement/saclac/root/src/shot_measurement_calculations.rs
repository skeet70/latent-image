#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::{adjust::{metering::{self, DiagnosticTrace, MeteringTables}, rate_policy, timing::LevelScale}, capture_requests, exposure::ports::{CaptureProfiles, Lens, Placement, Properties, Property}, lens_checks};
use xm1_saclac_imaging::{capture::gains::{GainClass, GainSources}, driver::Isp};
use xm1_saclac_interface::{area_measurement::CaptureSelection, calc::AreaGeometry, Calc, Calc1, Completion, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::{controller::{ProfileGeometry, Tables}, profile::ProfileHandle};
use xm1_saclac_profile_records::{field::Index, stock::record::{group_00, group_01, group_05}, Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{calculations_work::Processing, secondary_boundary::MeasurementCalc};
use crate::{calculation_services::Calculation, capture_gains::Sources, geometry::GeometryBindings, profile_calibration::ProfileCalibration, services::Services, shot_services::Libraries, stock::geometry::Memory as GeometryMemory};

// Borrow each owner only for one query: metering alternates its profile and table ports.
struct MeteringView<'a, M, R, E, P, I, T> {
    services: &'a RefCell<Services<M, R, E, P>>,
    geometry: &'a RefCell<ProfileGeometry<GeometryMemory<M>, GeometryBindings<M, P, I>, T>>,
}
macro_rules! metering_port {
    ($contract:path { $($methods:tt)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables>
            $contract for MeteringView<'_, M, R, E, P, I, T> { $($methods)* }
    };
}
metering_port!(Properties {
    fn property(&mut self, property: Property) -> u32 { self.services.borrow_mut().property(property) }
});
metering_port!(Lens {
    fn level(&mut self, lens: u32, reference: u32) -> i16 { self.services.borrow_mut().level(lens, reference) }
    fn base_level(&mut self, lens: u32) -> i16 { self.services.borrow_mut().base_level(lens) }
    fn reference_offset(&mut self, reference: u32) -> i16 { self.services.borrow_mut().reference_offset(reference) }
    fn calibration_index(&mut self, lens: u32) -> u32 { self.services.borrow_mut().calibration_index(lens) }
});
metering_port!(CaptureProfiles {
    fn level_offset(&mut self, profile: u32) -> i16 { ProfileCalibration(&mut self.geometry.borrow_mut()).level_offset(profile) }
    fn class_offset(&mut self, class: u8) -> i16 { ProfileCalibration(&mut self.geometry.borrow_mut()).class_offset(class) }
    fn offset_class(&mut self, profile: u32) -> u8 { ProfileCalibration(&mut self.geometry.borrow_mut()).offset_class(profile) }
    fn sensitivity_class(&mut self, profile: u32) -> u8 { ProfileCalibration(&mut self.geometry.borrow_mut()).sensitivity_class(profile) }
    fn sensitivity_class_for(&mut self, value: u32) -> u8 { ProfileCalibration(&mut self.geometry.borrow_mut()).sensitivity_class_for(value) }
    fn configured_level(&mut self) -> u16 { ProfileCalibration(&mut self.geometry.borrow_mut()).configured_level() }
    fn configured_profile(&mut self, value: i16) -> u32 { ProfileCalibration(&mut self.geometry.borrow_mut()).configured_profile(value) }
    fn record_profile(&mut self) -> u32 { ProfileCalibration(&mut self.geometry.borrow_mut()).record_profile() }
    fn default_dimensions(&mut self) -> [u16; 3] { ProfileCalibration(&mut self.geometry.borrow_mut()).default_dimensions() }
    fn grid_steps(&mut self, layout: u8) -> [u8; 2] { ProfileCalibration(&mut self.geometry.borrow_mut()).grid_steps(layout) }
});
metering_port!(MeteringTables {
    fn channel_weights(&mut self) -> [u16; 3] { Sources(&self.services.borrow().machine).class_gains(GainClass(5), 0) }
    fn feature_bytes(&mut self, profile: u32) -> [u8; 2] { self.geometry.borrow_mut().feature_scales(ProfileHandle(profile)) }
});
metering_port!(LevelScale {
    fn linear(&mut self, level: i16) -> u32 { self.services.borrow_mut().linear(level) }
    fn log2_difference(&mut self, value: u32, reference: u32) -> i16 { self.services.borrow_mut().log2_difference(value, reference) }
});
struct Trace<M>(M);
impl<M: Machine> DiagnosticTrace for Trace<M> {
    fn trace(&mut self, point: u32) { self.0.enter(0x72872c, &[point as usize]); }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q: Calc + Calc1>
    MeasurementCalc for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn capture_selection(&mut self, record: RecordHandle) -> CaptureSelection {
        let (lens, reference, offset, level, alternate) = {
            let services = self.services.borrow();
            let records = &services.records;
            let first = Index::new(0).expect("first measurement slot");
            let lens = records.read_element(record, &group_05::AT_0170, first).expect("measurement lens");
            let reference = records.read(record, &group_01::AT_00BC).expect("measurement reference");
            let offset = records.read(record, &group_01::AT_00A4).expect("measurement offset");
            let level = [&group_05::AT_0178, &group_05::AT_017A, &group_05::AT_017C, &group_05::AT_0186]
                .into_iter().fold(0i16, |sum, field| sum.wrapping_add(records.read_element(record, field, first).expect("measurement level")));
            let alternate = records.read(record, &group_00::AT_0041).expect("measurement alternate") != 0;
            (lens, reference, offset, level, alternate)
        };
        let mut ports = MeteringView { services: self.services, geometry: self.geometry };
        let offset = offset.wrapping_sub(ports.reference_offset(reference));
        let result = metering::lens_level(&mut ports, lens, reference, offset, i32::from(level), alternate);
        CaptureSelection { selection: result.lens, first: result.level, second: result.unclipped as u16, auxiliary: result.profile }
    }
    fn statistics_key(&mut self, block: &Processing) -> [i16; 2] {
        let machine = self.services.borrow().machine.clone();
        let read_plane = |plane: u32| core::array::from_fn(|index| machine.read_u32(xm1_shot_sequence::stock::calculations::STATISTICS + plane * 0x400 + 4 * index as u32));
        let sums = metering::CellSums { red: read_plane(0), green: read_plane(1), blue: read_plane(2), second_green: read_plane(3) };
        let parameters = crate::shot_capture_calculations::processing(block);
        let inputs = metering::MeteringInputs {
            exposure: parameters.extent[0], gain: parameters.extent[1], level: parameters.level, offset: parameters.reference,
            profile: parameters.profile, lens: parameters.lens, reference: parameters.lens_reference,
        };
        let mut ports = MeteringView { services: self.services, geometry: self.geometry };
        let mut tables = MeteringView { services: self.services, geometry: self.geometry };
        let result = metering::metered_levels(&mut ports, &mut tables, &mut Trace(machine), &sums, &inputs);
        [result.mean, result.weighted]
    }
    fn scale_step(&mut self, index: u16) -> i16 {
        match u8::try_from(index) {
            Ok(index) => rate_policy::primary_scale(&mut *self.services.borrow_mut(), index),
            Err(_) => 0,
        }
    }
    fn capture_offset(&mut self, record: RecordHandle) -> u32 { capture_requests::capture_offset(&mut *self.services.borrow_mut(), record) }
    fn curve_within(&mut self, distance: u32, selection: u32, drive: u32, limit: i32) -> bool {
        lens_checks::high_limit_within(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            distance, selection, drive, limit,
        )
    }
    fn request_evaluation(&mut self, record: RecordHandle, block: &Processing, frames: [u32; 2], geometry: &AreaGeometry, completions: [Completion; 2]) {
        assert_eq!(completions, [Completion::Calculation94, Completion::Calculation95]);
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        capture_requests::request_area_evaluation(
            &mut Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() },
            &mut peers, record, &crate::shot_capture_calculations::processing(block), frames, geometry,
            self.callbacks.capture_calculations.area_evaluation,
        ).expect("area evaluation enqueue");
    }
}
