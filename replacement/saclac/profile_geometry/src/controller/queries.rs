#![forbid(unsafe_code)]

//! The controller's queries. A geometry query computes the records its selection names for the
//! caller's parameter record, stores each in the controller's work record of that kind, and
//! reports them.

use super::rectangles::Band;
use super::windows::{Framing, Grid, GridRequest};
use super::{ProfileGeometry, Tables};
use crate::memory::{Location, Memory};
use crate::ports::{ConfigOffset, Ports};
use crate::profile::{ProfileHandle, QuadGroup, Rectangle, RectangleKind};
use crate::stock::layout::{policy, work::records};
use crate::stock::Parameters;

pub const WINDOWS: usize = 10;
/// The tracked window's smallest height before it is widened in proportion.
const TRACKING_MINIMUM: ConfigOffset = ConfigOffset(0xa485);
const TRACKING_MAXIMUM: u32 = 512;
const QUAD_LIMIT_OFFSET: ConfigOffset = ConfigOffset(0x1a7b);
/// The bands of a banded rectangle.
const BANDED: u8 = 16;

/// The records a geometry query computes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub extended_grid: bool,
    pub compact_grid: bool,
    pub plain: bool,
    pub rows: bool,
    pub columns: bool,
    pub record_grid: bool,
    pub fixed_windows: bool,
    pub tracked_windows: bool,
}

/// Where a tracked window grid extends from the tracked window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrackDirection {
    Left,
    Right,
    Up,
    Down,
}

/// The window a tracking grid follows, in the profile's crop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tracking {
    /// Row, then column.
    pub centre: [u32; 2],
    pub top: u32,
    pub left: u32,
    pub height: u32,
    pub width: u32,
    pub direction: TrackDirection,
}

/// The extended grid's request and the tracking it may carry; its gains bound both.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExtendedRequest {
    pub grid: GridRequest,
    pub tracking: Option<Tracking>,
}

/// One fixed window, relative to the profile's window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedWindow {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// Up to ten fixed windows; entries past `count` are not read.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FixedWindows {
    pub count: u8,
    pub windows: [FixedWindow; WINDOWS],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Requests<'a> {
    pub compact: Option<&'a GridRequest>,
    pub extended: Option<&'a ExtendedRequest>,
    pub record: Option<&'a GridRequest>,
    pub fixed: Option<&'a FixedWindows>,
}

/// A start and a length on one axis.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Span {
    pub start: u16,
    pub length: u16,
}

/// Up to ten windows, each a row span and a column span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Windows {
    pub rows: [Span; WINDOWS],
    pub columns: [Span; WINDOWS],
}

impl Windows {
    const fn filled(start: u16) -> Self {
        let span = Span { start, length: 1 };
        Self { rows: [span; WINDOWS], columns: [span; WINDOWS] }
    }
}

/// A computed record as a grid: band size, origin, gaps and band counts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridSummary {
    pub column_width: u32,
    pub row_height: u32,
    pub top: u32,
    pub left: u32,
    pub row_gap: u16,
    pub column_gap: u16,
    pub rows: u8,
    pub columns: u8,
}

impl From<Grid> for GridSummary {
    fn from(grid: Grid) -> Self {
        Self {
            column_width: grid.column_width as u32,
            row_height: grid.row_height as u32,
            top: grid.top as u32,
            left: grid.left as u32,
            row_gap: grid.row_gap,
            column_gap: grid.column_gap,
            rows: grid.rows,
            columns: grid.columns,
        }
    }
}

/// What 0xeda668 reports: the profile window and crop sizes, and each selected record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryReport {
    /// Width, then height.
    pub window: [u32; 2],
    pub crop: [u32; 2],
    pub extended_grid: Option<GridSummary>,
    pub tracked_windows: Option<GridSummary>,
    pub compact_grid: Option<GridSummary>,
    pub record_grid: Option<GridSummary>,
    pub plain: Option<GridSummary>,
    pub rows: Option<GridSummary>,
    pub columns: Option<GridSummary>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// What 0xeda4c4 reports: the band size of every work record, selected or not.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeReport {
    pub extended_grid: Size,
    pub compact_grid: Size,
    pub plain: Size,
    pub rows: Size,
    pub columns: Size,
    pub record_grid: Size,
    pub tracked_windows: [Size; WINDOWS],
    pub fixed_windows: [Size; WINDOWS],
}

/// The quad selection a profile's pipeline group has configured (0xedb9fc).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuadSelection {
    Off,
    Gains,
    Area,
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xeda668; it never computes the fixed windows.
    pub fn profile_geometry(&mut self, parameters: &Parameters, selection: Selection, requests: &Requests) -> GeometryReport {
        let framing = Framing::from(parameters);
        let mut report = GeometryReport::default();
        if selection.extended_grid {
            report.extended_grid = Some(self.extended_grid(&framing, requests).into());
        }
        if selection.compact_grid {
            report.compact_grid = Some(self.compact_grid(&framing, requests).into());
        }
        if selection.record_grid {
            report.record_grid = Some(self.record_grid(&framing, requests).into());
        }
        if selection.plain {
            let band = self.store_band(framing.profile, RectangleKind::Plain);
            report.plain = Some(banded(band, 1, 1));
        }
        if selection.rows {
            let band = self.store_band(framing.profile, RectangleKind::Rows);
            report.rows = Some(banded(band, BANDED, 1));
        }
        if selection.columns {
            let band = self.store_band(framing.profile, RectangleKind::Columns);
            report.columns = Some(banded(band, 1, BANDED));
        }
        if selection.tracked_windows {
            let (windows, [rows, columns]) = self.tracked_windows(&framing, requests.extended);
            report.tracked_windows = Some(GridSummary {
                column_width: windows.columns[0].length as u32,
                row_height: windows.rows[0].length as u32,
                top: windows.rows[0].start as u32,
                left: windows.columns[0].start as u32,
                row_gap: 0,
                column_gap: 0,
                rows,
                columns,
            });
        }
        let geometry = self.crop_geometry(&super::windows::CropInput {
            profile: framing.profile,
            area: framing.area,
            divisors: framing.format_ratio,
            relative: true,
        });
        report.window = [geometry.window.width, geometry.window.height];
        report.crop = [geometry.crop.width, geometry.crop.height];
        report
    }

    /// 0xeda4c4.
    pub fn record_sizes(&mut self, parameters: &Parameters, selection: Selection, requests: &Requests) -> SizeReport {
        let framing = Framing::from(parameters);
        if selection.extended_grid {
            self.extended_grid(&framing, requests);
        }
        if selection.compact_grid {
            self.compact_grid(&framing, requests);
        }
        if selection.record_grid {
            self.record_grid(&framing, requests);
        }
        for (selected, kind) in
            [(selection.plain, RectangleKind::Plain), (selection.rows, RectangleKind::Rows), (selection.columns, RectangleKind::Columns)]
        {
            if selected {
                self.store_band(framing.profile, kind);
            }
        }
        if selection.fixed_windows {
            self.fixed_windows(&framing, requests.fixed);
        }
        if selection.tracked_windows {
            self.tracked_windows(&framing, requests.extended);
        }
        let grid_size = |at: Location| Size { width: self.memory.read_u16(at.plus(0xa)) as u32, height: self.memory.read_u16(at.plus(4)) as u32 };
        let window_sizes = |at: Location| -> [Size; WINDOWS] {
            core::array::from_fn(|index| Size {
                width: self.memory.read_u16(at.plus(0x2a + 4 * index as u16)) as u32,
                height: self.memory.read_u16(at.plus(2 + 4 * index as u16)) as u32,
            })
        };
        SizeReport {
            extended_grid: grid_size(records::EXTENDED_GRID),
            compact_grid: grid_size(records::COMPACT_GRID),
            plain: Size {
                width: self.memory.read_u16(records::RECTANGLE_SET.plus(6)) as u32,
                height: self.memory.read_u16(records::RECTANGLE_SET.plus(2)) as u32,
            },
            rows: Size {
                width: self.memory.read_u16(records::ROW_BANDS.plus(8)) as u32,
                height: self.memory.read_u16(records::ROW_BANDS.plus(4)) as u32,
            },
            columns: Size {
                width: self.memory.read_u16(records::COLUMN_BANDS.plus(8)) as u32,
                height: self.memory.read_u16(records::COLUMN_BANDS.plus(2)) as u32,
            },
            record_grid: grid_size(records::RECORD_GRID),
            tracked_windows: window_sizes(records::TRACKED_WINDOWS),
            fixed_windows: window_sizes(records::FIXED_WINDOWS),
        }
    }

    /// 0xedfa28.
    fn extended_grid(&mut self, framing: &Framing, requests: &Requests) -> Grid {
        let grid = self.grid(framing, requests.extended.map(|request| request.grid));
        self.write_grid(records::EXTENDED_GRID, grid);
        grid
    }

    /// 0xedfeb4.
    fn compact_grid(&mut self, framing: &Framing, requests: &Requests) -> Grid {
        let request = requests.compact.map(|request| GridRequest { first_gain: 0, second_gain: 0, ..*request });
        let grid = self.grid(framing, request);
        self.write_grid(records::COMPACT_GRID, grid);
        grid
    }

    /// 0xee0030.
    fn record_grid(&mut self, framing: &Framing, requests: &Requests) -> Grid {
        let grid = self.grid(framing, requests.record.copied());
        self.write_grid(records::RECORD_GRID, grid);
        grid
    }

    fn store_band(&mut self, profile: ProfileHandle, kind: RectangleKind) -> Band {
        let band = self.band_of(profile, kind);
        let at = match kind {
            RectangleKind::Plain => records::RECTANGLE_SET,
            RectangleKind::Rows => records::ROW_BANDS,
            RectangleKind::Columns => records::COLUMN_BANDS,
        };
        self.write_band(at, band, kind);
        band
    }

    fn write_windows(&mut self, at: Location, windows: &Windows) {
        for (index, (row, column)) in windows.rows.iter().zip(&windows.columns).enumerate() {
            let offset = 4 * index as u16;
            self.memory.write_u16(at.plus(offset), row.start);
            self.memory.write_u16(at.plus(offset + 2), row.length);
            self.memory.write_u16(at.plus(0x28 + offset), column.start);
            self.memory.write_u16(at.plus(0x2a + offset), column.length);
        }
    }

    /// 0xedff0c.
    fn fixed_windows(&mut self, framing: &Framing, request: Option<&FixedWindows>) -> Windows {
        let mut windows = Windows::filled(1);
        if let Some(request) = request {
            let count = request.count.min(WINDOWS as u8) as usize;
            if count != 0 {
                let pair = self.profiles.flagged_scale_pair(framing.profile);
                let window = self.ports.relative_rectangle(framing.profile, true);
                for (index, entry) in request.windows[..count].iter().enumerate() {
                    windows.rows[index] = Span {
                        start: window.top.wrapping_add(entry.top) as u16,
                        length: floor_to(entry.height, pair.second) as u16,
                    };
                    windows.columns[index] = Span {
                        start: window.left.wrapping_add(entry.left) as u16,
                        length: floor_to(entry.width, pair.first) as u16,
                    };
                }
            }
        }
        self.write_windows(records::FIXED_WINDOWS, &windows);
        windows
    }

    /// The tracked windows and their row and column counts (0xedfa7c).
    fn tracked_windows(&mut self, framing: &Framing, request: Option<&ExtendedRequest>) -> (Windows, [u8; 2]) {
        let mut windows = Windows::filled(0);
        let mut counts = [1, 1];
        if let Some((tracking, gains)) = request.and_then(|request| Some((request.tracking?, [request.grid.first_gain, request.grid.second_gain]))) {
            let (layout, laid) = self.track(framing, &tracking, gains);
            counts = laid;
            windows = layout;
        }
        self.write_windows(records::TRACKED_WINDOWS, &windows);
        (windows, counts)
    }

    fn track(&mut self, framing: &Framing, tracking: &Tracking, gains: [u32; 2]) -> (Windows, [u8; 2]) {
        let geometry = self.gain_limited_crop(framing, gains);
        let origin = self.ports.relative_rectangle(framing.profile, true);
        let pair = self.profiles.flagged_scale_pair(framing.profile);
        let (row_step, column_step) = (pair.second, pair.first);
        let crop = geometry.crop;
        let margin_top = geometry.window.height.wrapping_sub(crop.height) >> 1;
        let margin_left = geometry.window.width.wrapping_sub(crop.width) >> 1;

        let mut top = tracking.top;
        let mut left = tracking.left;
        let mut height = tracking.height - tracking.height % row_step;
        let mut width = tracking.width - tracking.width % column_step;
        let minimum = self.ports.byte(TRACKING_MINIMUM) as u32;
        if minimum > height {
            width = width.wrapping_mul(minimum) / height;
            height = minimum;
            top = tracking.centre[0].saturating_sub(minimum >> 1);
            left = tracking.centre[1].saturating_sub(width >> 1);
        } else if height > TRACKING_MAXIMUM {
            top = tracking.centre[0].saturating_sub(TRACKING_MAXIMUM >> 1);
            height = TRACKING_MAXIMUM;
        }
        height = height.max(row_step);
        // Suspected stock defect, kept (0xedfc80..0xedfc88): a width below the column step
        // becomes the row step.
        if width < column_step {
            width = row_step;
        }
        top = top.max(margin_top);
        left = left.max(margin_left);
        let bottom = margin_top.wrapping_add(crop.height);
        (top, height) = fit(top, height, bottom, row_step);
        let right = margin_left.wrapping_add(crop.width);
        (left, width) = fit(left, width, right, column_step);

        let count = |space: u32, size: u32| -> u8 {
            match (space / size).wrapping_add(1) as u8 {
                0 => 1,
                count => count.min(WINDOWS as u8),
            }
        };
        let (rows, columns) = match tracking.direction {
            TrackDirection::Up => (count(top.wrapping_sub(margin_top), height), 1),
            TrackDirection::Down => (count(bottom.wrapping_sub(top.wrapping_add(height)), height), 1),
            TrackDirection::Left => (1, count(left.wrapping_sub(margin_left), width)),
            TrackDirection::Right => (1, count(right.wrapping_sub(width.wrapping_add(left)), width)),
        };
        if matches!(tracking.direction, TrackDirection::Left | TrackDirection::Up) {
            top = top.wrapping_sub((rows as u32 - 1).wrapping_mul(height));
            left = left.wrapping_sub((columns as u32 - 1).wrapping_mul(width));
        }
        let first_top = (top as u16).wrapping_add(origin.top as u16);
        let first_left = (left as u16).wrapping_add(origin.left as u16);
        let (height, width) = (height as u16, width as u16);
        let mut windows = Windows::filled(0);
        for row in 0..rows as usize {
            for column in 0..columns as usize {
                let index = row * columns as usize + column;
                windows.rows[index] = Span { start: first_top.wrapping_add(height.wrapping_mul(row as u16)), length: height };
                windows.columns[index] = Span { start: first_left.wrapping_add(width.wrapping_mul(column as u16)), length: width };
            }
        }
        (windows, [rows, columns])
    }

    /// 0xedb9fc.
    pub fn quad_selection_of(&mut self, profile: ProfileHandle) -> QuadSelection {
        let Some(offset) = self.ports.quad_selection_offset(self.profiles.pipeline_group(profile)) else {
            return QuadSelection::Off;
        };
        match self.ports.byte(offset) {
            1 => QuadSelection::Gains,
            2 => QuadSelection::Area,
            _ => QuadSelection::Off,
        }
    }

    /// The quad gains of `profile`'s quad group at `limit`, raised by the configured offset while
    /// the current profile's kind is above 1; zero for a profile without a quad group (0xedb08c).
    pub fn adjusted_quad(&mut self, limit: i16, profile: ProfileHandle, base_only: bool) -> [u16; 4] {
        let group = self.profiles.quad_group(profile);
        if group == QuadGroup::NONE {
            return [0; 4];
        }
        let limit = self.adjusted_quad_limit(limit);
        self.ports.quad_gains(group, limit, base_only)
    }

    pub(crate) fn adjusted_quad_limit(&mut self, limit: i16) -> i16 {
        if self.profiles.entry(self.current_profile()).kind.0 > 1 {
            limit.wrapping_add(self.ports.signed_halfword(QUAD_LIMIT_OFFSET))
        } else {
            limit
        }
    }

    /// The adjusted quad of a profile whose quad selection takes gains, else zero (0xedba78).
    pub fn enabled_quad(&mut self, limit: i16, profile: ProfileHandle) -> [u16; 4] {
        match self.quad_selection_of(profile) {
            QuadSelection::Gains => self.adjusted_quad(limit, profile, false),
            _ => [0; 4],
        }
    }

    /// 0xedb0ac: the pipeline layer's signed quad (0xdab8d8) is zero for every group and mode.
    pub fn signed_quad(&self) -> [i32; 4] { [0; 4] }

    /// The scan slot's window as a share of the profile's window, width then height, in
    /// ten-thousandths (0xedb0bc). Stores the slot as the policy's current slot.
    pub fn window_scale(&mut self, profile: ProfileHandle, area: [u32; 3]) -> [u32; 2] {
        let window = self.ports.relative_rectangle(profile, true);
        let slot = self.scan_slot_index(profile);
        self.memory.write_u32(policy::SLOT, slot);
        let table = self.profiles.tables().scan_slot(slot);
        let (mut width, mut height) = (table.width as u32, table.height as u32);
        if height > window.height {
            (width, height) = (window.width, window.height);
        }
        let [vertical, horizontal] = self.profiles.class_of(profile).limits.map(|limit| if limit == 0 { 1 } else { limit as u32 });
        let divisor = table.divisor as u32;
        let (first_scale, second_scale) = self.scaled_pair(profile);
        let samples = area[0].wrapping_mul(area[2]);
        if vertical == 1 && horizontal == 1 {
            let trim = ((samples / second_scale as u32) >> 2) * 3;
            width = width.saturating_sub(trim);
            let lines = width.wrapping_mul(vertical) / horizontal;
            height = (divisor.wrapping_mul(lines) / 4000) << 2;
        } else {
            let trim = samples / first_scale as u32;
            height = height.saturating_sub(trim) & !3;
            let lines = height.wrapping_mul(horizontal) / vertical;
            width = 1000u32.wrapping_mul(lines) / divisor;
        }
        [10000u32.wrapping_mul(width) / window.width, 10000u32.wrapping_mul(height) / window.height]
    }
}

/// `rectangle` scaled by `scale`, width then height in ten-thousandths, about its centre
/// (0xedb0dc).
pub fn centred_rectangle(rectangle: Rectangle, scale: [u32; 2]) -> Rectangle {
    let height = rectangle.height.wrapping_mul(scale[1]) / 10000;
    let width = rectangle.width.wrapping_mul(scale[0]) / 10000;
    Rectangle {
        top: (rectangle.height.wrapping_sub(height) >> 1).wrapping_add(rectangle.top),
        left: (rectangle.width.wrapping_sub(width) >> 1).wrapping_add(rectangle.left),
        width,
        height,
    }
}

const fn banded(band: Band, rows: u8, columns: u8) -> GridSummary {
    GridSummary {
        column_width: band.width as u32,
        row_height: band.height as u32,
        top: band.top as u32,
        left: band.left as u32,
        row_gap: 0,
        column_gap: 0,
        rows,
        columns,
    }
}

/// `value` rounded down to a multiple of `step`, at least `step`.
const fn floor_to(value: u32, step: u32) -> u32 {
    let floored = value - value % step;
    if floored < step { step } else { floored }
}

/// Moves or shrinks a span that passes `end`: shrunk to end there when at least `step` remains,
/// else a `step` span ending there.
fn fit(start: u32, length: u32, end: u32, step: u32) -> (u32, u32) {
    if length.wrapping_add(start) <= end {
        (start, length)
    } else if end >= start.wrapping_add(step) {
        (start, end - start)
    } else {
        (end.saturating_sub(step), step)
    }
}
