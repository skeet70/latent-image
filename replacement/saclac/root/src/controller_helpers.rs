#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::{adjust::{profile_adjustment::halfword_offset, profile_mode::{self, ModeInputs}}, exposure::ports::{Placement, Properties, StockMemory}, scene_recognition::SceneRecognition, stock::{calculation::Cell, property::{ADJUSTMENT_FLAGS, CORRECTION_FLAGS}}};
use xm1_saclac_controllers::layers::{CorrectedWindow, CurveQuery, DiagnosticExports, Field, LensControl, LensCurve, LensProbe, MirrorSources, ProfileRecord, RecordQuery, SharedHelpers, StatisticsSettings, S1Values, WindowInputs, RECORD_QUERY_BYTES};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, stock::record_handle_word, ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{lens::{self, LensBounds, LensNotices}, settings, stock::Machine};
use xm1_shot_sequence::stock::state as shot;
use crate::calculation_services::Calculation;
use crate::services::Services;

/// One helper operation's borrows of the same calculation, recognition and shot backing.
pub struct Helpers<'a, M, R, E, P, I, T, A> {
    pub calculation: Calculation<'a, M, R, E, P, I, T, A>,
    pub recognition: &'a RefCell<SceneRecognition>,
}

pub(crate) fn bounds<M: Machine + Clone, R: Memory, E: StockEntries, P>(
    services: &mut Services<M, R, E, P>, record: RecordHandle, position: i16, mode: u32,
) -> LensBounds {
    let kind = ProfileRecord::read(services, record, Field::SceneKind).wrapping_sub(46);
    let range = if kind <= 10 && (0x503u32 & (1 << kind)) != 0 {
        u32::from(services.field(record, &xm1_saclac_profile_records::stock::record::group_00::AT_0044))
    } else { 0 };
    let mut bounds = [u32::MAX; 3];
    if !LensControl::overridden(services, LensProbe::Second) && !LensControl::overridden(services, LensProbe::First) {
        let channel = services.machine.enter(0x76_d9c8, &[]);
        let reference = LensControl::reference(services);
        if LensCurve::interpolates(services) {
            let (channel, fraction) = LensControl::channel(services);
            let query = CurveQuery { range, reference, mode_index: mode, step: LensControl::step(services) };
            if let Some(value) = LensCurve::interpolated_positions(services, position, channel, fraction, query) { bounds = value; }
        } else {
            let query = CurveQuery { range, reference, mode_index: mode, step: LensControl::step(services) };
            bounds = LensCurve::positions(services, position, channel, query);
        }
    }
    LensBounds(bounds)
}

pub(crate) fn current_bounds<M: Machine + Clone, R: Memory, E: StockEntries, P>(
    services: &mut Services<M, R, E, P>, record: RecordHandle,
) -> Option<LensBounds> {
    let position = LensControl::position(services);
    let mode = if services.machine.enter(0x76_da38, &[]) != 0 { 1 } else { LensControl::mode_index(services) };
    if !services.stock.notifier_enabled() { return None; }
    Some(bounds(services, record, position as i16, mode))
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    SharedHelpers for Helpers<'_, M, R, E, P, I, T, A> {
    fn update_selection_range(&mut self, record: RecordHandle) {
        let services = &mut *self.calculation.services;
        if let Some(bounds) = current_bounds(services, record) {
            lens::notify_bounds(0, bounds, &mut services.stock);
        }
    }

    fn clear_calculations(&mut self) {
        self.recognition.borrow_mut().reset(self.calculation.services);
    }

    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32 {
        let machine = &self.calculation.services.machine;
        let rate = machine.read_u32(shot::SELECTION_MODE.get());
        let at = shot::SAVED_SELECTION.get().wrapping_add(halfword_offset(7, rate, 0));
        let cell = Cell { column: machine.read_u8(at), row: machine.read_u8(at + 1) };
        let lens_reference = machine.enter(0x76_d9c8, &[]);
        profile_mode::mode_step(&mut self.calculation, &ModeInputs { record, rate, cell, lens_reference })
    }

    fn convert_resource(&mut self, record: RecordHandle) {
        let services = &mut *self.calculation.services;
        let mode = ProfileRecord::read(services, record, Field::FormatMode);
        let ratio = ProfileRecord::read(services, record, Field::FormatRatio);
        let _ = MirrorSources::capture_window(services, mode, ratio);
    }
}

fn debug_flags(area: &impl settings::Adjustments) -> u8 { settings::Adjustments::byte(area, settings::AdjustmentKey(0x24c)) }

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    StatisticsSettings for Helpers<'_, M, R, E, P, I, T, A> {
    fn parameter_feature(&mut self) -> bool { self.calculation.services.machine.enter(0x6a_7e68, &[]) == 1 }
    fn set_parameter_feature(&mut self, on: bool) { self.calculation.services.machine.enter(0x6a_7de0, &[usize::from(on)]); }
    fn diagnostic_exports(&mut self) -> DiagnosticExports {
        let services = &mut *self.calculation.services;
        DiagnosticExports {
            row_data: services.property(CORRECTION_FLAGS) & 1 != 0 && services.property(ADJUSTMENT_FLAGS) & 0x80 != 0,
            debug: debug_flags(&services.stock) != 0 && services.property(ADJUSTMENT_FLAGS) & 0x80 != 0,
        }
    }
    fn export_row_data(&mut self, buffer: u32) {
        self.calculation.services.machine.enter(0xee_92e4, &[6, buffer.wrapping_add(0x35c8) as usize, 0xe0e4, 0]);
    }
    fn export_debug(&mut self, buffer: u32) {
        let services = &mut *self.calculation.services;
        if debug_flags(&services.stock) & 1 != 0 {
            services.machine.enter(0x74_213c, &[buffer.wrapping_add(0x6cd60) as usize, 0x5ffc_6354, 0xd230]);
        }
        if debug_flags(&services.stock) & 2 != 0 {
            services.write_u32(SharedAddress(buffer.wrapping_add(0x79f90)), 0x5354_4346);
            let count = services.machine.read_u32(0x16c_2228);
            services.write_u32(SharedAddress(buffer.wrapping_add(0x79f94)), count);
            let source = services.machine.read_u32(0xf4_9358);
            services.machine.enter(0x74_213c, &[buffer.wrapping_add(0x79f98) as usize, source as usize, 0x7efc]);
            services.machine.enter(0x74_213c, &[buffer.wrapping_add(0x81e94) as usize, source.wrapping_add(0x7efc) as usize, 0xbc]);
            services.write_u32(SharedAddress(buffer.wrapping_add(0x81f50)), 0x4c41_4346);
            if debug_flags(&services.stock) & 2 != 0 {
                let next = buffer.wrapping_add(0x79f98).wrapping_add(services.machine.read_u32(0x16c_2228).wrapping_mul(0x6c));
                services.fill(SharedAddress(next), 0x6c, 0);
            }
        }
        self.recognition.borrow().store_dump_section(services, SharedAddress(buffer));
        for (bit, offset, bytes) in [(1, 0x6cd60, 0xd230), (2, 0x79f90, 0x7fc4), (4, 0x1e00, 0x17c8)] {
            if debug_flags(&services.stock) & bit != 0 {
                services.machine.enter(0xee_92e4, &[1, buffer.wrapping_add(offset) as usize, bytes, 0]);
            }
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    S1Values for Helpers<'_, M, R, E, P, I, T, A> {
    fn descriptor(&mut self, record: RecordHandle) -> u32 {
        xm1_saclac_calc::capture_requests::capture_offset(self.calculation.services, record)
    }
    fn lens_value(&mut self) -> u32 { LensControl::value(self.calculation.services) }
    fn refresh(&mut self, record: RecordHandle) -> u32 {
        self.calculation.services.shared(record, &xm1_saclac_profile_records::stock::shared::group_26::AT_023C)
    }
    fn curve(&mut self, inputs: [u32; 11]) -> u32 {
        u32::from(self.calculation.geometry.configured_operation(&crate::controller_geometry::operation_conditions(inputs)))
    }
    fn stop_curve(&mut self, stop_mode: u32) -> u32 {
        u32::from(xm1_saclac_profile_geometry::profile::is_paired_mode(stop_mode))
    }
    fn unselected_view(&mut self, record: RecordHandle) -> u32 {
        xm1_saclac_calc::lens_checks::level_rise(&mut self.calculation, record)
    }
    fn span(&mut self, record: RecordHandle) -> u32 {
        u32::from(self.calculation.geometry.configured_operation_available(record_handle_word(record)))
    }
    fn unselected(&mut self, record: RecordHandle) -> u32 {
        let stock = xm1_saclac_shared::stock::Stock::new(self.calculation.services.machine.clone());
        u32::from(settings::unselected_mode(record, &stock, self.calculation.services))
    }
    fn corrected_window(&mut self, input: &WindowInputs) -> CorrectedWindow {
        crate::controller_geometry::corrected_window(self.calculation.geometry, input)
    }
    fn release_steps(&mut self, position: i16, first: u32, second: u32) -> u16 {
        let machine = &self.calculation.services.machine;
        let reference = machine.enter(0x76_f340, &[]);
        machine.enter(0x74_9be4, &[position as i32 as usize, first as usize, 0, reference as usize, second as usize]) as u16
    }
    fn trace_snapshot(&mut self) { self.calculation.services.machine.enter(0x72_872c, &[0x0700_0000]); }
    fn release_channel(&mut self) { self.calculation.services.machine.enter(0x74_2a6c, &[5, 0]); }
    fn searched_lens_pair(&mut self, record: RecordHandle) -> u32 { SharedHelpers::searched_lens_pair(self, record) }
    fn extended_results(&mut self) -> bool { settings::Adjustments::byte(&self.calculation.services.stock, settings::AdjustmentKey(0x1307)) != 0 }
    fn query_records(&mut self, query: &RecordQuery, out: &mut [u8; RECORD_QUERY_BYTES]) {
        let source = xm1_saclac_profile_geometry::geometry::AreaSource {
            area: [query.descriptor, query.pair[0], query.pair[1]],
            ratio: [query.ratio as u16, (query.ratio >> 16) as u16],
        };
        if let Ok((areas, _)) = self.calculation.geometry.geometry_areas(xm1_saclac_profile_geometry::geometry::AreaKind(query.bank), xm1_saclac_profile_geometry::profile::ProfileHandle(query.mode), &source) {
            areas.write_into(out);
        }
    }
    fn ticks(&mut self) -> u32 { self.calculation.services.machine.enter(0x74_3dc8, &[]) }
    fn admits_selection(&mut self, requested: u32, offered: u32) -> bool {
        xm1_saclac_controllers::layers::Geometry::admits(self.calculation.services, requested, offered)
    }
}
