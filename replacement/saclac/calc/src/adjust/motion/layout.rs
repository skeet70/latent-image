//! What a prepared run lays out before its frames (0xf99ad0): the second exposure's crop and
//! sample grids, when each sampled area is exposed, the positions a drive starts from and is
//! held within, the record's default curve and the exposure ratio the pipeline measures with.

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::adjust::lens::LensConfiguration;
use crate::adjust::metering::MeteringTables;
use crate::adjust::motion::exposure::FrameExposure;
use crate::adjust::motion::ports::{
    AreaClass, AreaCurves, AreaGrid, CaptureFraming, CropGeometry, FrameGeometry, GridRequest, LensBounds, LensCalibration, LensTables, LimitQuery,
    Rectangle, SampleFrameRequest, Tracking,
};
use crate::adjust::motion::prepare::{channel_bounds, search_bounds, stored_second_level};
use crate::adjust::motion::readings::{ListedArea, RecordReadings};
use crate::adjust::motion::sampling::{PRIMARY_AREAS, SECONDARY_AREAS};
use crate::adjust::motion::search::{AreaCell, ChannelClass, SearchMode};
use crate::adjust::motion::state::{MotionState, SampleSchedule};
use crate::adjust::motion::statistics::profile_weights;
use crate::adjust::rate_policy;
use crate::adjust::timing::{CaptureTiming, LevelScale};
use crate::exposure::ports::{Lens, ProfileAccess, Properties, StockMemory};
use crate::exposure::settings;
use crate::gains::{self, CaptureGains, GainRequest, SelectedGains};
use crate::stock::motion::{self as stock, LayoutTrace, TreatedWindowTrace};

/// Everything the layout reaches.
pub trait LayoutPorts:
    Properties
    + ProfileAccess
    + Lens
    + LensTables
    + LensCalibration
    + LensBounds
    + LensConfiguration
    + FrameGeometry
    + MeteringTables
    + CaptureTiming
    + LevelScale
    + AreaCurves
    + CaptureGains
    + StockMemory
{
}

impl<
        T: Properties
            + ProfileAccess
            + Lens
            + LensTables
            + LensCalibration
            + LensBounds
            + LensConfiguration
            + FrameGeometry
            + MeteringTables
            + CaptureTiming
            + LevelScale
            + AreaCurves
            + CaptureGains
            + StockMemory,
    > LayoutPorts for T
{
}

/// What the layout leaves for the pipeline and the run's later steps.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleLayout {
    pub grid: GridRequest,
    /// The listed area a treated run's secondary samples follow.
    pub tracking: Option<Tracking>,
    pub crop: CropGeometry,
    /// The configured setting of the second exposure's program step, at most 5.
    pub step_setting: u8,
    /// The primary and secondary sample grids.
    pub grids: [AreaGrid; 2],
    /// The end of a treated run's drive window; 0 for the others.
    pub treated_position: i16,
    /// Where the run's drive starts, within its bounds.
    pub start_position: i16,
    /// Channel 0's two table positions at the second exposure's lens step.
    pub channel_positions: [i16; 2],
    /// The lens's low limit with the calibration margins.
    pub low_limit: i16,
    pub exposure_at_06: i16,
    pub weights: [u16; 3],
    /// The default curve with the record's profile factors.
    pub curve: [u16; 3],
    pub curve_limit: u32,
    /// The second exposure's linear ratio to the stored second-channel exposure.
    pub exposure_ratio: u16,
    /// The configured rate of one primary cell.
    pub cell_rate: u32,
    /// The program's lowest level with its second range.
    pub lowest_level: i16,
}

/// The centre of `area` in the crop's eighths.
fn crop_cell_centre(crop: &Rectangle, area: AreaCell) -> [u32; 2] {
    let eighths = |origin: u32, size: u32, index: u8| origin.wrapping_add(size.wrapping_mul(u32::from(index.wrapping_add(1))) >> 3);
    [eighths(crop.left, crop.width, area.column), eighths(crop.top, crop.height, area.row)]
}

/// The centre of the grid's cell at `area`.
fn grid_cell_centre(grid: &AreaGrid, area: AreaCell) -> [u32; 2] {
    let centre = |origin: u32, size: u32, gap: u16, index: u8| {
        origin.wrapping_add(size >> 1).wrapping_add(u32::from(gap).wrapping_add(size).wrapping_mul(u32::from(index)))
    };
    [centre(grid.left, grid.cell_width, grid.gap_horizontal, area.column), centre(grid.top, grid.cell_height, grid.gap_vertical, area.row)]
}

/// One cell of `scale` percent of the grid's, floored to the profile's cell steps and held
/// within the crop and above one step, centred on `centre` as far as the frame's edge allows.
fn single_cell(steps: [u32; 2], grid: &AreaGrid, centre: [u32; 2], scale: u16, crop: &Rectangle) -> AreaGrid {
    let [height_step, width_step] = steps;
    let sized = |size: u32, step: u32, limit: u32| {
        let scaled = size.wrapping_mul(u32::from(scale)) / 100;
        (scaled - scaled.checked_rem(step).unwrap_or(0)).min(limit).max(step)
    };
    let width = sized(grid.cell_width, width_step, crop.width);
    let height = sized(grid.cell_height, height_step, crop.height);
    let [x, y] = centre;
    AreaGrid {
        cell_height: height,
        cell_width: width,
        left: x.checked_sub(width >> 1).unwrap_or(0),
        top: y.checked_sub(height >> 1).unwrap_or(0),
        gap_horizontal: 0,
        gap_vertical: 0,
        columns: 1,
        rows: 1,
        at_16: grid.at_16,
    }
}

/// The readout line of each row's centre, from the grid's first row.
fn row_lines(grid: &AreaGrid, line_offset: u32) -> impl Iterator<Item = u32> + '_ {
    let first = grid.top.wrapping_add(grid.cell_height >> 1).wrapping_add(line_offset);
    let spacing = u32::from(grid.gap_vertical).wrapping_add(grid.cell_height);
    (0..).map(move |row: u32| first.wrapping_add(spacing.wrapping_mul(row)))
}

/// The secondary areas' readout lines along a treated run's class: down the rows from the
/// bottom or the top, or every column on the first row. Areas the class leaves are line 0.
fn secondary_lines(grid: &AreaGrid, class: AreaClass, line_offset: u32) -> [u32; SECONDARY_AREAS] {
    let mut lines = [0; SECONDARY_AREAS];
    let rows = usize::from(grid.rows);
    match class {
        AreaClass::RowsUpward => {
            for (step, line) in row_lines(grid, line_offset).take(rows).enumerate() {
                if let Some(slot) = lines.get_mut(rows - 1 - step) {
                    *slot = line;
                }
            }
        }
        AreaClass::RowsDownward => lines.iter_mut().zip(row_lines(grid, line_offset).take(rows)).for_each(|(slot, line)| *slot = line),
        AreaClass::Columns | AreaClass::ColumnsAlternate => {
            let first = row_lines(grid, line_offset).next().unwrap_or(0);
            lines.iter_mut().take(usize::from(grid.columns)).for_each(|slot| *slot = first);
        }
    }
    lines
}

/// The bit of the treated window start configuration for a run and its first listed area.
pub(crate) fn window_start_case(readings: &RecordReadings, area: &ListedArea) -> u32 {
    match readings.scene {
        _ if readings.auxiliary != 0 => 0x10,
        stock::FIRST_HELD_SCENE => 4,
        stock::SECOND_HELD_SCENE => 8,
        _ if area.at_2c() != 0 => 2,
        _ if area.at_26() == 1 => 0x20,
        _ => 1,
    }
}

/// The end of a treated run's drive window: the position whose drive period the listed area's
/// rate gives, within the run's bounds, traced into the dump frame (0xf996f0).
fn treated_window(ports: &mut impl LayoutPorts, state: &MotionState, step: u32) -> i16 {
    let readings = &state.readings;
    let amount = state.areas.areas[0].at_22;
    let rate = rate_policy::drive_rate(ports, readings.auxiliary != 0, readings.lens as i32, u32::from(amount));
    let period = 100_000u32.checked_div(rate.scaled).map_or(u16::MAX, |period| period as u16);
    let position = ports.period_position(period, readings.lens, step, readings.reference);
    let position = (position.max(i32::from(state.bounds.low)) as i16).min(state.bounds.high);
    let trace = TreatedWindowTrace {
        amount: u32::from(amount),
        share: rate.share,
        rate: rate.rate,
        base_value: ports.channel_value(0),
        lens: readings.lens,
        lens_value: ports.channel_value(readings.lens as i32),
        scaled_rate: rate.scaled,
        period,
        position,
    };
    stock::trace_treated_window(state.section, &trace, ports);
    position
}

/// The second exposure's linear ratio to the record's stored second-channel exposure, at most
/// the configured ceiling and the configured level's ratio, at least the configured floor.
fn exposure_ratio(ports: &mut impl LayoutPorts, readings: &RecordReadings, second: &FrameExposure) -> u16 {
    let change = if ports.property(stock::RATIO_WITHOUT_CHANGE) == 0 { second.change.max(0) } else { 0 };
    let own = second.exposure_level.wrapping_sub(second.profile_offset.wrapping_add(second.at_0c)).wrapping_add(ports.level(second.lens_step, readings.lens));
    let level = own.wrapping_sub(stored_second_level(ports, readings)).wrapping_sub(change);
    let ceiling = u32::from(settings::tenths(ports, stock::RATIO_CEILING));
    let ratio = ports.linear(level).min(ceiling);
    let ratio_level = settings::tenths(ports, stock::RATIO_LEVEL) as i16;
    let level_ratio = ports.linear(ratio_level.wrapping_sub(second.at_0c));
    let floor = u32::from(settings::tenths(ports, stock::RATIO_FLOOR));
    ratio.min(level_ratio).max(floor) as u16
}

/// The record's stored weights when it keeps them, else the default ones.
fn curve_weights(ports: &mut impl LayoutPorts, readings: &RecordReadings) -> [u16; 3] {
    let stored: [u8; 20] = ports.profile().read_shared(readings.record, &group_22::AT_0180);
    if stored[12..16].iter().any(|&byte| byte != 0) {
        core::array::from_fn(|index| u16::from_le_bytes([stored[2 * index], stored[2 * index + 1]]))
    } else {
        ports.channel_weights()
    }
}

impl MotionState {
    /// Lays out the run's samples in its second exposure's frame and traces the layout into the
    /// dump frame.
    pub fn lay_out_samples(&mut self, ports: &mut impl LayoutPorts) {
        let second = self.exposures.second;
        let readings = self.readings;
        let [height_percent, width_percent] = stock::SAMPLE_GRID_PERCENT.map(|id| ports.property(id) as u8);
        let grid = GridRequest {
            height_percent,
            width_percent,
            bands: stock::SAMPLE_GRID_BANDS,
            row_percent: ports.property(stock::sample_row_percent(second.step)) as u8,
            column_percent: ports.property(stock::sample_column_percent(second.step)) as u8,
        };
        let treated = self.search.mode == SearchMode::Treated;
        let tracking = treated.then(|| {
            let area = self.areas.areas[0];
            Tracking { area, class: stock::area_class(area.at_1c) }
        });
        let framing = CaptureFraming { profile: second.profile, area: [readings.selection_value, readings.at_0164, readings.at_0168], row_scale: readings.row_scale };
        let crop = ports.crop_geometry(&framing);
        let step_setting = ports.property(stock::program_setting(second.program.index(), second.step)).min(stock::PROGRAM_SETTING_LIMIT) as u8;
        let grids = self.sample_grids(ports, framing, grid, tracking, &crop);
        self.bounds = search_bounds(ports, &readings, self.search.channel_class == ChannelClass::Wide, second.lens_step);
        self.sampling = self.time_areas(ports, &grids, tracking);
        let treated_position = if treated { treated_window(ports, self, second.lens_step) } else { 0 };
        let start_position = self.start_position(ports, treated);
        let channel_positions = ports.channel_positions(readings.lens, second.lens_step, readings.reference, 0);
        let limits = ports.lens_limits(&LimitQuery { channel: 0, lens: readings.lens, step: second.lens_step, reference: readings.reference, with_margins: true, lens_state: 0 });
        let weights = profile_weights(ports, second.profile);
        let curve = self.default_curve(ports);
        let deltas = [ports.profile().read(readings.record, &group_00::AT_0049), ports.profile().read(readings.record, &group_00::AT_004A)];
        let selector = ports.profile().read(readings.record, &group_00::AT_003E);
        let curve_points = ports.with_profile_factors(curve.gains, deltas, selector);
        let exposure_ratio = exposure_ratio(ports, &readings, &second);
        let expanded = ports.expanded_ratio(grids[0].cell_height.wrapping_mul(grids[0].cell_width));
        let cell_rate = rate_policy::scaled_by_configured_factor(ports, expanded);
        let lowest = &self.program[0];
        self.layout = SampleLayout {
            grid,
            tracking,
            crop,
            step_setting,
            grids,
            treated_position,
            start_position,
            channel_positions,
            low_limit: limits.low,
            exposure_at_06: second.at_06,
            weights,
            curve: curve_points,
            curve_limit: curve.class.0,
            exposure_ratio,
            cell_rate,
            lowest_level: lowest.bottom.wrapping_add(lowest.second_top),
        };
        let trace = LayoutTrace {
            grid,
            sample_grids: grids,
            search_grids: self.search.frames,
            treated: treated.then_some(crop),
            bounds: self.bounds,
            channel_bounds: channel_bounds(ports, &readings, second.lens_step),
        };
        stock::trace_layout(self.section, &trace, ports);
    }

    /// The second exposure's sample grids; a class 0 run, or an area run, samples one cell of
    /// its grid around its area, unless its scene masks the second channel (0xf9939c).
    fn sample_grids(&self, ports: &mut impl LayoutPorts, framing: CaptureFraming, grid: GridRequest, tracking: Option<Tracking>, crop: &CropGeometry) -> [AreaGrid; 2] {
        let second = &self.exposures.second;
        let request = SampleFrameRequest {
            framing,
            lens: self.readings.lens,
            exposure_level: second.exposure_level,
            at_0c: second.at_0c,
            lens_step: second.lens_step,
            program_step: second.step,
            grid,
            tracking,
            whole_frame: !matches!(self.search.mode, SearchMode::Channel | SearchMode::Area),
        };
        let [first, secondary] = ports.sample_frames(&request);
        if self.readings.scene == stock::MASKED_SCENE {
            return [first, secondary];
        }
        let area = self.search.area;
        let centre = match (self.readings.class, self.search.mode) {
            (0, _) => crop_cell_centre(&crop.crop, area),
            (_, SearchMode::Area) => grid_cell_centre(&first, area),
            _ => return [first, secondary],
        };
        let steps = ports.cell_steps(second.profile);
        [single_cell(steps, &first, centre, self.search.area_scale, &crop.crop), secondary]
    }

    /// Each sampled area's exposure start relative to its frame: its readout line's time less
    /// the exposure. A treated run also schedules its secondary areas along its class.
    fn time_areas(&self, ports: &mut impl LayoutPorts, grids: &[AreaGrid; 2], tracking: Option<Tracking>) -> SampleSchedule {
        let second = &self.exposures.second;
        let exposure = 100_000_000u32.checked_div(ports.linear(second.exposure_level)).unwrap_or(0);
        let readout_start = ports.readout_start(second.profile);
        let line_offset = u32::from(ports.reference_line(second.profile)).wrapping_sub(readout_start);
        let period = ports.line_period(second.profile).unwrap_or(0);
        let timed = |line: u32| {
            let readout = line.wrapping_mul(period);
            (readout.wrapping_sub(exposure), readout)
        };
        let [primary_grid, secondary_grid] = grids;
        let mut primary = [(0, 0); PRIMARY_AREAS];
        let primary_count = usize::from(primary_grid.rows).min(PRIMARY_AREAS);
        primary.iter_mut().zip(row_lines(primary_grid, line_offset)).take(primary_count).for_each(|(slot, line)| *slot = timed(line));
        let mut secondary = [(0, 0); SECONDARY_AREAS];
        let secondary_count = match tracking {
            Some(tracking) => {
                let lines = secondary_lines(secondary_grid, tracking.class, line_offset);
                let count = usize::from(secondary_grid.columns.wrapping_mul(secondary_grid.rows)).min(SECONDARY_AREAS);
                secondary.iter_mut().zip(lines).take(count).for_each(|(slot, line)| *slot = timed(line));
                count
            }
            None => 0,
        };
        stock::trace_schedule(self.section, &primary[..primary_count], &secondary[..secondary_count], line_offset, ports);
        let primary = primary.map(|(start, _)| start as i32);
        let secondary = secondary.map(|(start, _)| start as i32);
        SampleSchedule::new(exposure as i32, &primary[..primary_count], &secondary[..secondary_count], secondary_grid.rows, secondary_grid.columns)
    }

    /// A treated run whose case the configuration names starts at its window's end; any other
    /// run at its channel's configured start, or channel 0's table position for a scene that
    /// takes it, moved to the second exposure's lens step and held within its bounds.
    fn start_position(&self, ports: &mut impl LayoutPorts, treated: bool) -> i16 {
        let readings = &self.readings;
        let step = self.exposures.second.lens_step;
        if treated && window_start_case(readings, &self.areas.areas[0]) & ports.property(stock::TREATED_WINDOW_START) != 0 {
            return treated_window(ports, self, step);
        }
        let configured = ports.channel_start(readings.lens, readings.channel, readings.reference);
        let position = if readings.channel == 0 && stock::scene_channels(readings.scene).starts_at_channel_position {
            ports.channel_positions(readings.lens, 0, readings.reference, 0)[0]
        } else {
            configured
        };
        let moved = ports.remap(i32::from(position) as u32, readings.lens as i32, step);
        (moved.max(i32::from(self.bounds.low)) as i16).min(self.bounds.high)
    }

    /// The record's default curve and its limit (0xf4d5bc).
    fn default_curve(&self, ports: &mut impl LayoutPorts) -> SelectedGains {
        let readings = &self.readings;
        let record = readings.record;
        let request = GainRequest {
            mode: u32::from(readings.scene),
            variant: u32::from(ports.profile().read(record, &group_00::AT_0048)),
            custom_source: u32::from(ports.profile().read(record, &group_00::AT_002A)),
            bank: u32::from(readings.channel),
            fixed: gains::fixed_gains(ports, record),
            temperature: readings.levels[0] as u16,
            custom: curve_weights(ports, readings),
            forced: true,
            ..GainRequest::default()
        };
        gains::reference_gains(ports, &request)
    }
}
