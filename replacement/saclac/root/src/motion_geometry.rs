#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::motion::ports::{self as motion, FrameGeometry}, exposure::ports::Placement, stock::motion::SEARCH_PROFILE};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::ImageProcessor;
use xm1_saclac_profile_geometry::{controller::{queries::{ExtendedRequest, GridSummary, Requests, Selection, TrackDirection, Tracking}, windows::{CropInput, GridRequest}, Tables}, profile::ProfileHandle, stock::Parameters};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use crate::calculation_services::Calculation;

fn parameters(framing: &motion::CaptureFraming) -> Parameters {
    let mut parameters = Parameters::from_bytes([0; Parameters::BYTES]);
    parameters.set_profile(ProfileHandle(framing.profile));
    parameters.set_area(framing.area);
    parameters.set_format_ratio(framing.row_scale);
    parameters
}

fn grid(summary: GridSummary) -> motion::AreaGrid {
    motion::AreaGrid {
        cell_height: summary.row_height, cell_width: summary.column_width,
        left: summary.left, top: summary.top,
        gap_horizontal: summary.column_gap, gap_vertical: summary.row_gap,
        columns: summary.columns, rows: summary.rows, at_16: [0; 2],
    }
}

fn tracking(tracking: motion::Tracking) -> Tracking {
    let area = tracking.area;
    //edfd14..edfea8: codes0/1 traverse columns;2/3 traverse rows.
    let direction = match tracking.class {
        motion::AreaClass::RowsUpward => TrackDirection::Left,
        motion::AreaClass::RowsDownward => TrackDirection::Right,
        motion::AreaClass::Columns => TrackDirection::Up,
        motion::AreaClass::ColumnsAlternate => TrackDirection::Down,
    };
    Tracking { centre: [area.centre_y, area.centre_x], top: area.y, left: area.x, height: area.height, width: area.width, direction }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
    FrameGeometry for Calculation<'_, M, R, E, P, I, T, A> {
    fn search_frames(&mut self, request: &motion::SearchFrameRequest) -> [motion::AreaGrid; 2] {
        //ef6518..ef6534 writes class+1e first, class+1c second.
        let limits = self.geometry.profiles().class_of(ProfileHandle(SEARCH_PROFILE)).limits;
        let parameters = parameters(&motion::CaptureFraming { profile: SEARCH_PROFILE, area: [0; 3], row_scale: [limits[1], limits[0]] });
        let extended = ExtendedRequest {
            grid: GridRequest {
                height_percent: request.grid[0], width_percent: request.grid[1], bands: 7,
                row_percent: request.grid[2], column_percent: request.grid[3], first_gain: 0, second_gain: 0,
            },
            tracking: request.area.map(tracking),
        };
        let report = self.geometry.profile_geometry(&parameters,
            Selection { extended_grid: true, tracked_windows: true, ..Selection::default() },
            &Requests { extended: Some(&extended), ..Requests::default() });
        [grid(report.extended_grid.expect("requested search grid")), grid(report.tracked_windows.expect("requested tracked grid"))]
    }
    fn statistics_frame(&mut self, request: &motion::StatisticsFrameRequest) -> [u32; 2] {
        let parameters = parameters(&motion::CaptureFraming {
            profile: request.profile, area: [request.selection_value, request.at_0164, request.at_0168], row_scale: request.row_scale,
        });
        //f99874's compact request is100/100/16/0/0.
        let compact = GridRequest { height_percent: 100, width_percent: 100, bands: 16, ..GridRequest::default() };
        let report = self.geometry.profile_geometry(&parameters, Selection { compact_grid: true, ..Selection::default() },
            &Requests { compact: Some(&compact), ..Requests::default() });
        let grid = report.compact_grid.expect("requested statistics grid");
        [grid.row_height, grid.column_width]
    }
    fn sample_frames(&mut self, request: &motion::SampleFrameRequest) -> [motion::AreaGrid; 2] {
        let parameters = parameters(&request.framing);
        let source = request.grid;
        let extended = ExtendedRequest {
            grid: GridRequest {
                height_percent: u16::from(source.height_percent), width_percent: u16::from(source.width_percent), bands: u16::from(source.bands),
                row_percent: u16::from(source.row_percent), column_percent: u16::from(source.column_percent),
                first_gain: u32::from(request.whole_frame), second_gain: 0,
            },
            tracking: request.tracking.map(tracking),
        };
        let report = self.geometry.profile_geometry(&parameters,
            Selection { extended_grid: true, tracked_windows: true, ..Selection::default() },
            &Requests { extended: Some(&extended), ..Requests::default() });
        [grid(report.extended_grid.expect("requested sample grid")), grid(report.tracked_windows.expect("requested tracked sample grid"))]
    }
    fn crop_geometry(&mut self, framing: &motion::CaptureFraming) -> motion::CropGeometry {
        let geometry = self.geometry.crop_geometry(&CropInput { profile: ProfileHandle(framing.profile), area: framing.area, divisors: framing.row_scale, relative: true });
        let rectangle = |rectangle: xm1_saclac_profile_geometry::profile::Rectangle| motion::Rectangle {
            left: rectangle.left, top: rectangle.top, height: rectangle.height, width: rectangle.width,
        };
        motion::CropGeometry { window: rectangle(geometry.window), crop: rectangle(geometry.crop) }
    }
    fn cell_steps(&mut self, profile: u32) -> [u32; 2] {
        let pair = self.geometry.profiles().flagged_scale_pair(ProfileHandle(profile));
        [pair.second, pair.first]
    }
    fn expanded_ratio(&mut self, value: u32) -> u32 { self.geometry.profiles().expanded_ratio(value) as u32 }
    fn feature_class(&mut self, profile: u32) -> u8 { self.geometry.profiles().entry(ProfileHandle(profile)).feature }
}
