#![forbid(unsafe_code)]

use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::ImageProcessor;
use xm1_saclac_profile_geometry::{controller::{queries::{Requests, Selection}, windows::{CropInput, GridRequest}}, geometry::{AreaKind, AreaSource, WindowGeometry}, memory::Memory as GeometryMemory, ports::Retained, profile::{ProfileHandle, Rectangle}, stock::{areas::{AreaList, AREA_BYTES, AREA_LIST_BYTES}, layout::geometry as state, records::Parameters}, controller::Tables};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::ClientRef, primary_boundary::{AreaFrame, AreaMode, Calibration, CaptureProfiles, DisplayArea, DisplayLayout, FactorRequest, GeometryWords, ProfileCalibration}, secondary_boundary::{CropGeometry, CropRequest, GeometryRequest, MeasurementProfiles, ProfileGeometry, ScalePair}};
use crate::shot_services::Libraries;
use xm1_calibration_core::{configuration::CfgKey, profile as calibration};

struct CalibrationTables<'a, M>(&'a M);
impl<M: Machine> calibration::Tables for CalibrationTables<'_, M> {
    fn split_keys(&self, index: calibration::SplitTableIndex) -> [CfgKey; 2] {
        let at = 0xef_5048 + u32::from(index.0) * 4;
        [CfgKey(u32::from(self.0.read_u16(at))), CfgKey(u32::from(self.0.read_u16(at + 2)))]
    }
    fn history_fraction(&self, index: calibration::HistoryIndex) -> u8 { self.0.read_u8(0xee_9dc8u32.wrapping_add(index.0 as i32 as u32)) }
}

fn rectangle_words(rectangle: Rectangle) -> [u32; 4] { [rectangle.top, rectangle.left, rectangle.width, rectangle.height] }

fn rectangle(words: [u32; 4]) -> Rectangle { Rectangle { top: words[0], left: words[1], width: words[2], height: words[3] } }

pub(crate) fn publish_areas(machine: &mut impl Machine, buffer: ClientRef, list: &AreaList) {
    let mut bytes = [0; AREA_LIST_BYTES];
    list.write_into(&mut bytes);
    machine.write_u32(buffer.0, list.count);
    for (offset, byte) in bytes.iter().enumerate().skip(4) {
        let area = (offset - 4) / AREA_BYTES;
        let field = (offset - 4) % AREA_BYTES;
        if area < list.count as usize && matches!(field, 0x27 | 0x33) { continue; }
        machine.write_u8(buffer.0 + offset as u32, *byte);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A, Q> MeasurementProfiles for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn profile_geometry(&mut self, request: &GeometryRequest) -> ProfileGeometry {
        let mut parameters = Parameters::from_bytes([0; Parameters::BYTES]);
        parameters.set_profile(ProfileHandle(request.selector));
        parameters.set_limit(request.limits[0]);
        parameters.set_secondary_limit(request.limits[1]);
        parameters.set_index(request.reference);
        parameters.set_index_scale(request.distance);
        parameters.set_area([request.offset, request.frame_words[0], request.frame_words[1]]);
        parameters.set_format_ratio(request.layout);
        let grid = GridRequest { height_percent: 80, width_percent: 80, bands: 16, ..GridRequest::default() };
        let report = self.geometry.borrow_mut().profile_geometry(&parameters, Selection { compact_grid: true, ..Selection::default() }, &Requests { compact: Some(&grid), ..Requests::default() });
        let grid = report.compact_grid.expect("requested compact grid");
        ProfileGeometry { frame: report.window, cell: [grid.column_width, grid.row_height], extent: [grid.top, grid.left], gap: [grid.row_gap, grid.column_gap], factors: [grid.rows, grid.columns] }
    }

    fn crop_geometry(&mut self, request: &CropRequest) -> CropGeometry {
        let geometry = self.geometry.borrow_mut().crop_geometry(&CropInput { profile: ProfileHandle(request.selector), area: [request.offset, request.frame_words[0], request.frame_words[1]], divisors: request.layout, relative: request.full });
        CropGeometry { origin: [geometry.window.top, geometry.window.left], frame: [geometry.window.width, geometry.window.height], window: rectangle_words(geometry.crop) }
    }

    fn flagged_scale_pair(&mut self, selector: u32) -> ScalePair {
        let pair = self.geometry.borrow().profiles().flagged_scale_pair(ProfileHandle(selector));
        ScalePair { first: pair.first, second: pair.second }
    }

    fn profile_extent(&mut self, selector: u32, inset: bool) -> [u32; 4] {
        rectangle_words(self.geometry.borrow_mut().ports().relative_rectangle(ProfileHandle(selector), inset))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A, Q> CaptureProfiles for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn area_selector(&mut self, profile: u32, _: u8) -> u32 { profile } // ef6700 returns one; ef617c keeps the handle.

    fn display_layout(&mut self, selector: u32, _: u8) -> DisplayLayout {
        let mut geometry = self.geometry.borrow_mut();
        let profile = ProfileHandle(selector);
        if geometry.profiles().entry(profile).class.0 != 0 { return DisplayLayout::default(); }
        let expanded = geometry.ports().expanded_rectangle(profile);
        let extent = geometry.ports().relative_rectangle(profile, false);
        DisplayLayout { areas: [DisplayArea { origin: 0, window: rectangle_words(expanded), extent: rectangle_words(extent) }, DisplayArea::default()] }
    }

    fn paired(&mut self, _: u32) -> bool { false } // fead0c compares the constant-zero ef67f0 with one.

    fn sensor_window(&mut self, crop: &CropGeometry) -> [u32; 4] {
        let geometry = self.geometry.borrow();
        let scale = state::WINDOW_SCALE.map(|at| geometry.memory().read_u32(at));
        rectangle_words(xm1_saclac_profile_geometry::controller::queries::centred_rectangle(Rectangle { top: crop.origin[0], left: crop.origin[1], width: crop.frame[0], height: crop.frame[1] }, scale))
    }

    fn generate_areas(&mut self, mode: AreaMode, selector: u32, frame: &AreaFrame, buffer: ClientRef) {
        if buffer.0 == 0 { return; }
        assert_eq!(buffer.0 & 3, 0, "area output alignment");
        let geometry = WindowGeometry { window: [frame.crop[0], frame.crop[1]], inner: rectangle(frame.crop[2..].try_into().expect("crop rectangle")), outer: rectangle(frame.sensor) };
        let kind = AreaKind(xm1_shot_sequence::capture_stock::area_mode(mode));
        if let Ok(list) = self.geometry.borrow_mut().adjusted_window_areas(kind, ProfileHandle(selector), &geometry) {
            let offset = xm1_shot_sequence::capture_stock::AREA_BUFFER_LISTS[match mode { AreaMode::Second => 1, AreaMode::Third => 2 }];
            publish_areas(&mut self.services.borrow().machine.clone(), ClientRef(buffer.0.wrapping_add(offset)), &list);
        }
    }

    fn frame_areas(&mut self, words: &GeometryWords, size: [u32; 2], buffer: ClientRef) {
        if buffer.0 == 0 { return; }
        assert_eq!(buffer.0 & 3, 0, "area output alignment");
        let source = AreaSource { area: [words.offset, words.frame_words[0], words.frame_words[1]], ratio: words.layout };
        if let Ok(list) = self.geometry.borrow_mut().paired_areas(&source, size) {
            publish_areas(&mut self.services.borrow().machine.clone(), buffer, &list);
        }
    }

    fn display_extent(&mut self, selector: u32, _: [u32; 2], words: &GeometryWords) -> [u32; 2] {
        self.geometry.borrow_mut().window_scale(ProfileHandle(selector), [words.offset, words.frame_words[0], words.frame_words[1]])
    }

    fn measurement_table(&mut self) -> [u32; 18] { self.geometry.borrow().geometry_snapshot() }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> ProfileCalibration for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn calibrate(&mut self, request: &FactorRequest) -> Calibration {
        let services = self.services.borrow();
        let native = crate::calibration_inputs::Native(&services.machine, &services.stock);
        let mut values = calibration::Triplet(request.calibration.0);
        let input = calibration::FactorRequest { mode: calibration::FactorMode::from_native(request.mode as i32), split: calibration::SplitIndex(request.slot_reference as i32), selector: calibration::ProfileSelector(request.kind as i32), deltas: request.record_words.map(|word| calibration::DeltaIndex(word as i32)) };
        native.calibration().calibrate_profile(&input, &CalibrationTables(&services.machine), &mut values).expect("profile factors configuration");
        Calibration(values.0)
    }

    fn clamp_geometry(&mut self, _: u32, values: &Calibration, rate: u8, exposure: i16) -> Calibration {
        let services = self.services.borrow();
        let native = crate::calibration_inputs::Native(&services.machine, &services.stock);
        let mut values = calibration::Triplet(values.0);
        native.calibration().clamp_geometry(&mut values, calibration::ProfileRate(u32::from(rate)), calibration::ExposureHundredths(i32::from(exposure))).expect("geometry clamp configuration");
        Calibration(values.0)
    }

    fn apply_positive(&mut self, exposure: i32, values: &mut Calibration) {
        let services = self.services.borrow();
        let mut triplet = calibration::Triplet(values.0);
        calibration::apply_positive(calibration::ExposureHundredths(exposure), &mut triplet, &CalibrationTables(&services.machine));
        values.0 = triplet.0;
    }

    fn clamp_output(&mut self, values: &Calibration) -> Calibration {
        let services = self.services.borrow();
        let native = crate::calibration_inputs::Native(&services.machine, &services.stock);
        let mut values = calibration::Triplet(values.0);
        native.calibration().clamp_profile(calibration::ClampClass(3), &mut values, calibration::ProfileRate(10)).expect("output clamp");
        Calibration(values.0)
    }
}
