#![forbid(unsafe_code)]

//! The geometry records the running operations send: the two scan windows, the record grids,
//! the rectangle set and the indexed pair.

use super::changes::{Mask, Subscription, Work};
use super::rectangles::Band;
use super::{ProfileGeometry, Tables};
use crate::memory::{Location, Memory};
use crate::numeric::runtime_quotient;
use crate::ports::{ConfigOffset, Ports};
use crate::profile::{ProfileHandle, Rectangle, RectangleKind};
use crate::stock::layout::{controller as field, parameters, policy, status, work::records};
use crate::stock::Parameters;

/// The record kind whose class-one profiles take the fifth scan slot.
const EXTENDED_RECORD_KIND: u32 = 7;
const SECONDARY_SLOT_TOP: u16 = 0x195;
const SECONDARY_SLOT_TOP_ADJUSTED: u16 = 0x198;
/// The policy layout that adds 4 to the base of every scan slot but slot 1.
const DELAYED_LAYOUT: u8 = 2;
const COLUMN_GAP: ConfigOffset = ConfigOffset(0x243);
const SECOND_GAIN_LEVEL: ConfigOffset = ConfigOffset(0x879b);
const FIRST_GAIN_LEVEL: ConfigOffset = ConfigOffset(0x926c);
/// The profile whose window bounds every gain-limited area.
const GAIN_REFERENCE: ProfileHandle = ProfileHandle(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScanSide {
    Primary,
    Secondary,
}

/// The parameter fields a profile's crop is computed from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Framing {
    pub profile: ProfileHandle,
    pub area: [u32; 3],
    pub format_ratio: [u16; 2],
}

impl From<&Parameters> for Framing {
    fn from(parameters: &Parameters) -> Self {
        Self { profile: parameters.profile(), area: parameters.area(), format_ratio: parameters.format_ratio() }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropInput {
    pub profile: ProfileHandle,
    pub area: [u32; 3],
    pub divisors: [u16; 2],
    pub relative: bool,
}

/// The profile's window and the crop centred in it (0xef6030).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CropGeometry {
    pub window: Rectangle,
    pub crop: Rectangle,
}

/// A grid of bands a record step asks for, from its starter's fields.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridRequest {
    pub height_percent: u16,
    pub width_percent: u16,
    pub bands: u16,
    pub row_percent: u16,
    pub column_percent: u16,
    pub first_gain: u32,
    pub second_gain: u32,
}

/// A grid of equal row and column bands with gaps between them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grid {
    pub top: u16,
    pub rows: u8,
    pub row_height: u16,
    pub left: u16,
    pub columns: u8,
    pub column_width: u16,
    pub row_gap: u16,
    pub column_gap: u16,
}

impl Grid {
    const EMPTY: Self = Self { top: 0, rows: 1, row_height: 1, left: 0, columns: 1, column_width: 1, row_gap: 0, column_gap: 0 };
}

const fn percent(value: u32, percent: u32) -> u32 { value.wrapping_mul(percent) / 100 }

/// `value` rounded down to a multiple of `step`, at least `step`.
const fn floor_to(value: u32, step: u32) -> u32 {
    let floored = value - value % step;
    if floored < step { step } else { floored }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xede884.
    pub(crate) fn scan_slot_index(&self, profile: ProfileHandle) -> u32 {
        let entry = self.profiles.entry(profile);
        let class = entry.class.0;
        match entry.kind.0 {
            0 => 9,
            2 => match class {
                4 => 1,
                9..=15 => 2,
                _ => 0,
            },
            3 if class == 1 && self.memory.read_u32(status::RECORD_KIND) == EXTENDED_RECORD_KIND => 4,
            3 => 3,
            5 => match class {
                4 => 6,
                6 => 7,
                7 => 8,
                _ => 5,
            },
            _ => 0,
        }
    }

    /// The lines the scan slot's margin leaves above the configured top (0xedc1cc).
    fn scan_margin(&mut self, profile: ProfileHandle, slot: u32) -> u16 {
        let table = self.profiles.tables().scan_slot(slot);
        if slot == 2 {
            return table.margin;
        }
        let class = self.profiles.class_of(profile);
        let mut top = self.ports.configured_rectangle(profile).top;
        if class.first_flag != 0 {
            top >>= 1;
        }
        let top = top / self.profiles.selection_multiplier(profile);
        let margin = table.margin as u32;
        if margin > top { (margin - top) as u16 } else { 0 }
    }

    /// The columns the scan slot's base leaves beside the configured width (0xedc270).
    fn scan_column_margin(&mut self, profile: ProfileHandle, slot: u32) -> u16 {
        let base = self.profiles.tables().scan_slot(slot).base as u32;
        let width = self.ports.configured_rectangle(profile).width;
        if base > width.wrapping_shl(1) { (base + 2).wrapping_sub(width.wrapping_mul(2)) as u16 } else { 0 }
    }

    /// The scaled profile pair (0xedbed8).
    pub(crate) fn scaled_pair(&self, profile: ProfileHandle) -> (u16, u16) {
        (self.profiles.scaled_first(profile, true), self.profiles.scaled_second(profile, true))
    }

    fn parameter_area(&self) -> [u32; 3] { core::array::from_fn(|index| self.memory.read_u32(parameters::AREA.plus(index as u16 * 4))) }

    /// The framing of the controller's current parameter record.
    pub(crate) fn current_framing(&self) -> Framing {
        Framing {
            profile: self.current_profile(),
            area: self.parameter_area(),
            format_ratio: [parameters::FIRST_DIVISOR, parameters::SECOND_DIVISOR].map(|at| self.memory.read_u16(at)),
        }
    }

    fn status_area(&self) -> [u32; 3] { core::array::from_fn(|index| self.memory.read_u32(status::AREA.plus(index as u16 * 4))) }

    /// 0xee75a8, 0xee7658.
    pub(crate) fn prepare_scan_rectangles(&mut self, side: ScanSide) {
        let (work, stage) = match side {
            ScanSide::Primary => (Work::PrimaryRectangles, 0x800),
            ScanSide::Secondary => (Work::SecondaryRectangles, 0x1000),
        };
        if !self.requested(work) {
            return;
        }
        let profile = self.current_profile();
        let slot = self.scan_slot_index(profile);
        self.memory.write_u32(policy::SLOT, slot);
        self.write_scan_record(profile, slot, side);
        self.add_to_mask(Mask::Geometry, stage);
        self.clear_request(work);
    }

    /// 0xede4e0.
    fn write_scan_record(&mut self, profile: ProfileHandle, slot: u32, side: ScanSide) {
        let table = self.profiles.tables().scan_slot(slot);
        let margin = self.scan_margin(profile, slot) as u32;
        let height = table.height as u32;
        self.memory.write_u32(policy::SCAN_CLEARED, 0);
        self.memory.write_u16(policy::SCAN_CLEARED_HALF, 0);
        let (first_scale, _) = self.scaled_pair(profile);
        let [rows, _, columns] = self.status_area();
        let samples = rows.wrapping_mul(columns) / first_scale as u32;
        let rate = (0x1320 / first_scale as i32).wrapping_mul(100);
        let headroom = (rate as u32).wrapping_sub(samples.wrapping_mul(100));
        let lines = (height as u64 * 100).wrapping_mul(headroom as u64);
        let lines = runtime_quotient(runtime_quotient(lines, rate as i64 as u64), 100);
        let time = (lines << 12).wrapping_mul(1000);
        let scaled = if (3..=4).contains(&slot) {
            time.wrapping_add(time << 3) >> 4
        } else {
            runtime_quotient(time.wrapping_mul(2), 3)
        };
        let scaled = runtime_quotient(scaled, table.divisor as u64);

        let (pair_at, index) = match side {
            ScanSide::Primary => (status::PRIMARY_PAIR, 0),
            ScanSide::Secondary => (status::SECONDARY_PAIR, 1),
        };
        let pair = [self.memory.read_u32(pair_at), self.memory.read_u32(pair_at.plus(4))];
        self.memory.write_u32(policy::SCAN_PAIRS[index], pair[0]);
        self.memory.write_u32(policy::SCAN_PAIRS[index].plus(4), pair[1]);

        let mut base = table.base as u32;
        if self.memory.read_u8(policy::LAYOUT) == DELAYED_LAYOUT && slot != 1 {
            base += 4;
        }
        let per_first = runtime_quotient(scaled, pair[0] as u64) as u16;
        let count = height.wrapping_sub(1).wrapping_add(margin * 2);
        let per_second = (runtime_quotient(time, pair[1] as u64) as u32 / 1000) as u16;

        let origin =
            if self.remainder_geometry() { self.ports.expanded_rectangle(profile) } else { self.ports.configured_rectangle(profile) };
        let class = self.profiles.class_of(profile);
        let origin_height = if class.first_flag != 0 { origin.height >> 1 } else { origin.height };
        let origin_width = if class.second_flag != 0 { origin.width >> 1 } else { origin.width };
        let origin_height = origin_height / self.profiles.selection_multiplier(profile);
        let origin_width = origin_width.wrapping_add(self.profiles.class_limit(profile) as u32);

        let out = match side {
            ScanSide::Primary => records::PRIMARY_SCAN,
            ScanSide::Secondary => records::SECONDARY_SCAN,
        };
        let mut second = pair[0] as u16;
        // Suspected stock defect, kept (0xede4b8): the check compares the word at +0x16, so it
        // also needs the previous record's +0x18 to be zero.
        if (3..=4).contains(&slot)
            && side == ScanSide::Secondary
            && second == SECONDARY_SLOT_TOP
            && self.memory.read_u16(out.plus(0x18)) == 0
        {
            second = SECONDARY_SLOT_TOP_ADJUSTED;
        }
        self.memory.write_u16(out.plus(0x30), pair[1] as u16);
        self.memory.write_u16(out, origin_height as u16);
        self.memory.write_u16(out.plus(0x2), origin_width as u16);
        self.memory.write_u32(out.plus(0x4), 0);
        self.memory.write_u16(out.plus(0x8), per_second);
        self.memory.write_u16(out.plus(0xa), per_first);
        self.memory.write_u32(out.plus(0xc), count & 0x7fff_ffff);
        self.memory.write_u32(out.plus(0x10), base);
        self.memory.write_u16(out.plus(0x14), pair[1] as u16);
        self.memory.write_u16(out.plus(0x16), second);
        self.memory.write_u16(out.plus(0x18), 0);
        self.memory.write_u16(out.plus(0x1a), pair[1] as u16);
    }

    /// Bounds the area's first value by the reference profile's window at `level` percent
    /// below full (0xef65cc).
    fn limit_area(&mut self, level: u8, area: &mut [u32; 3]) {
        if area[2] == 0 {
            return;
        }
        let window = self.ports.relative_rectangle(GAIN_REFERENCE, true);
        let scale = self.profiles.scaled_first(GAIN_REFERENCE, true) as u32;
        let bound = scale.wrapping_mul(100u32.wrapping_sub(level as u32).wrapping_mul(window.height)) / 100u32.wrapping_mul(area[2]);
        if bound < area[0] {
            area[0] = bound;
        }
    }

    /// 0xef6030.
    pub fn crop_geometry(&mut self, input: &CropInput) -> CropGeometry {
        let window = self.ports.relative_rectangle(input.profile, input.relative);
        let second_divisor = if input.divisors[1] == 0 { 1 } else { input.divisors[1] as u32 };
        let first_divisor = if input.divisors[0] == 0 { 1 } else { input.divisors[0] as u32 };
        let first_scale = self.profiles.scaled_first(input.profile, input.relative) as u32;
        let second_scale = self.profiles.scaled_second(input.profile, input.relative) as u32;
        let (height, width) = (window.height, window.width);
        let second = second_divisor.wrapping_mul(second_scale);
        let fitted = height.wrapping_mul(first_divisor).wrapping_mul(first_scale) / second;
        let fitted = if second_divisor == 4 {
            fitted.min(height.wrapping_mul(3).wrapping_mul(first_scale) / second_scale.wrapping_shl(2))
        } else {
            fitted.min(width)
        };
        let first = first_divisor.wrapping_mul(first_scale);
        let crop_height = second.wrapping_mul(fitted) / first;
        let trimmed =
            crop_height.wrapping_mul(input.area[0].wrapping_mul(input.area[2])) / height.wrapping_mul(first_scale);
        let crop_height = crop_height.saturating_sub(trimmed);
        let crop_width = (first.wrapping_mul(crop_height) / second).min(width);
        CropGeometry {
            window,
            crop: Rectangle {
                top: window.top.wrapping_add(height.wrapping_sub(crop_height) >> 1),
                left: window.left.wrapping_add(width.wrapping_sub(crop_width) >> 1),
                width: crop_width,
                height: crop_height,
            },
        }
    }

    /// The crop of `framing`, its area first bounded at the gain level of the second gain when it
    /// is set, else of the first (0xedeba0, 0xedfa7c).
    pub(crate) fn gain_limited_crop(&mut self, framing: &Framing, gains: [u32; 2]) -> CropGeometry {
        let mut area = framing.area;
        let level = match gains {
            [_, second] if second != 0 => Some(SECOND_GAIN_LEVEL),
            [first, _] if first != 0 => Some(FIRST_GAIN_LEVEL),
            _ => None,
        };
        if let Some(level) = level {
            let level = self.ports.byte(level);
            self.limit_area(level, &mut area);
        }
        self.crop_geometry(&CropInput { profile: framing.profile, area, divisors: framing.format_ratio, relative: true })
    }

    /// 0xedeba0.
    pub(crate) fn grid(&mut self, framing: &Framing, request: Option<GridRequest>) -> Grid {
        let Some(request) = request else {
            return Grid::EMPTY;
        };
        let profile = framing.profile;
        let geometry = self.gain_limited_crop(framing, [request.first_gain, request.second_gain]);
        let window = self.ports.relative_rectangle(profile, true);
        let pair = self.profiles.flagged_scale_pair(profile);

        let clamp = |value: u16, limit: u16| if value == 0 { 1 } else { value.min(limit) as u32 };
        let height_percent = clamp(request.height_percent, 100);
        let width_percent = clamp(request.width_percent, 100);
        let bands = clamp(request.bands, 16);
        let (centre_rows, centre_columns) =
            if bands <= 1 { (false, false) } else { (request.row_percent != 0, request.column_percent != 0) };
        let height = percent(geometry.crop.height, height_percent);
        let width = percent(geometry.crop.width, width_percent);
        let gap = self.ports.byte(COLUMN_GAP) as u32;
        let gaps = bands - 1;
        let row_height = (height / bands) as u16 as u32;
        let column_width = (width.wrapping_sub(gaps.wrapping_mul(gap)) / bands) as u16 as u32;
        let row_height = match request.row_percent {
            0 => row_height,
            limit => row_height.min(percent(geometry.crop.height, limit as u32)),
        };
        let column_width = match request.column_percent {
            0 => column_width,
            limit => column_width.min(percent(geometry.crop.width, limit as u32)),
        };
        let row_height = floor_to(row_height, pair.second);
        let column_width = floor_to(column_width, pair.first);
        let spread = |total: u32, band: u32| if total > bands * band { (total - bands * band) / gaps } else { 0 };
        let row_gap = if centre_rows { spread(height, row_height) } else { 0 };
        let column_gap = match centre_columns {
            true if width > bands * column_width => (width - bands * column_width) / gaps,
            _ => gap,
        };
        let left_over = geometry.window.width.wrapping_sub(bands * column_width).wrapping_sub(gaps.wrapping_mul(column_gap));
        let top_over = geometry.window.height.wrapping_sub(bands * row_height).wrapping_sub(gaps.wrapping_mul(row_gap));
        Grid {
            top: window.top.wrapping_add(top_over >> 1) as u16,
            rows: bands as u8,
            row_height: row_height as u16,
            left: window.left.wrapping_add(left_over >> 1) as u16,
            columns: bands as u8,
            column_width: column_width as u16,
            row_gap: row_gap as u16,
            column_gap: column_gap as u16,
        }
    }

    pub(crate) fn write_grid(&mut self, at: Location, grid: Grid) {
        self.memory.write_u16(at, grid.top);
        self.memory.write_u8(at.plus(0x2), grid.rows);
        self.memory.write_u16(at.plus(0x4), grid.row_height);
        self.memory.write_u16(at.plus(0x6), grid.left);
        self.memory.write_u8(at.plus(0x8), grid.columns);
        self.memory.write_u16(at.plus(0xa), grid.column_width);
        self.memory.write_u16(at.plus(0xc), grid.row_gap);
        self.memory.write_u16(at.plus(0xe), grid.column_gap);
    }

    fn grid_request(&self, kind: GridKind) -> GridRequest {
        let at = |offset: u16| kind.request().plus(offset);
        let half = |offset: u16| self.memory.read_u16(at(offset));
        let word = |offset: u16| self.memory.read_u32(at(offset));
        match kind {
            GridKind::Compact => GridRequest {
                height_percent: half(0),
                width_percent: half(2),
                bands: half(4),
                row_percent: half(6),
                column_percent: half(8),
                first_gain: 0,
                second_gain: 0,
            },
            GridKind::Extended => GridRequest {
                height_percent: half(4),
                width_percent: half(6),
                bands: half(8),
                row_percent: half(0xc),
                column_percent: half(0xe),
                first_gain: word(0x34),
                second_gain: word(0x38),
            },
            GridKind::Record => GridRequest {
                height_percent: half(4),
                width_percent: half(6),
                bands: half(8),
                row_percent: half(0xa),
                column_percent: half(0xc),
                first_gain: word(0x10),
                second_gain: word(0x14),
            },
        }
    }

    fn prepare_grid(&mut self, kind: GridKind) {
        if !self.requested(kind.work()) {
            return;
        }
        let request = self.subscribed(kind.subscription()).then(|| self.grid_request(kind));
        let framing = self.current_framing();
        let grid = self.grid(&framing, request);
        self.write_grid(kind.record(), grid);
        self.add_to_mask(Mask::Geometry, kind.stage());
        self.clear_request(kind.work());
    }

    /// 0xee7708.
    pub(crate) fn prepare_compact_rectangles(&mut self) { self.prepare_grid(GridKind::Compact) }

    /// 0xee7810.
    pub(crate) fn prepare_extended_rectangles(&mut self) { self.prepare_grid(GridKind::Extended) }

    /// 0xee79e8.
    pub(crate) fn prepare_record_rectangles(&mut self) { self.prepare_grid(GridKind::Record) }

    pub(crate) fn write_band(&mut self, at: Location, band: Band, kind: RectangleKind) {
        match kind {
            RectangleKind::Plain => {
                for (offset, value) in [(0, band.top), (2, band.height), (4, band.left), (6, band.width)] {
                    self.memory.write_u16(at.plus(offset), value);
                }
            }
            RectangleKind::Rows => {
                self.memory.write_u16(at, band.top);
                self.memory.write_u8(at.plus(2), band.step);
                for (offset, value) in [(4, band.height), (6, band.left), (8, band.width)] {
                    self.memory.write_u16(at.plus(offset), value);
                }
            }
            RectangleKind::Columns => {
                for (offset, value) in [(0, band.top), (2, band.height), (4, band.left), (8, band.width)] {
                    self.memory.write_u16(at.plus(offset), value);
                }
                self.memory.write_u8(at.plus(6), band.step);
            }
        }
    }

    /// 0xee7924.
    pub(crate) fn prepare_rectangle_set(&mut self) {
        if !self.requested(Work::RectangleSet) {
            return;
        }
        let wanted = [9, 10, 11].into_iter().any(|index| self.subscribed(Subscription::Grouped(index)));
        let profile = self.current_profile();
        for (kind, offset) in [(RectangleKind::Plain, 0), (RectangleKind::Rows, 8), (RectangleKind::Columns, 0x12)] {
            let band = if wanted { self.band_of(profile, kind) } else { Band::ONES };
            self.write_band(records::RECTANGLE_SET.plus(offset), band, kind);
        }
        if wanted {
            self.memory.write_u32(field::CHANNEL_TOGGLE, 1);
        }
        self.add_to_mask(Mask::Geometry, 0x200);
        self.clear_request(Work::RectangleSet);
    }

    /// 0xee7a64.
    pub(crate) fn prepare_indexed_pair(&mut self) {
        if !self.requested(Work::IndexedPair) {
            return;
        }
        let profile = self.current_profile();
        let pair = self.indexed_pair(profile);
        let at = field::INDEXED_PAIR;
        for (index, value) in pair.into_iter().enumerate() {
            self.memory.write_u16(at.plus(index as u16 * 2), value);
            self.memory.write_u16(records::INDEXED_PAIR.plus(index as u16 * 2), value);
        }
        self.add_to_mask(Mask::Geometry, 0x400);
        self.clear_request(Work::IndexedPair);
    }

    /// The indexed operation's window: top, height, left and width (0xee028c, 0xede154).
    fn indexed_pair(&mut self, profile: ProfileHandle) -> [u16; 4] {
        let window = self.ports.relative_rectangle(profile, true);
        let slot = self.scan_slot_index(profile);
        let table = self.profiles.tables().scan_slot(slot);
        let margin = self.scan_margin(profile, slot) as u32;
        let column_margin = self.scan_column_margin(profile, slot) as u32;
        let (first_scale, second_scale) = self.scaled_pair(profile);
        let [first_divisor, second_divisor] =
            [parameters::FIRST_DIVISOR, parameters::SECOND_DIVISOR].map(|at| self.memory.read_u16(at) as u64);
        let [rows, _, columns] = self.parameter_area();
        let (lines, span) = if first_divisor == 1 && second_divisor == 1 {
            let samples = (rows.wrapping_mul(columns) / second_scale as u32 >> 2).wrapping_mul(3);
            let rate = (3264 / second_scale as i32).wrapping_mul(1000);
            let headroom = (rate as u32).wrapping_sub(samples.wrapping_mul(1000));
            let span = (window.width as u64 * 1000).wrapping_mul(headroom as u64);
            let span = runtime_quotient(runtime_quotient(span, rate as i64 as u64), 1000) & !1;
            let lines = runtime_quotient(span.wrapping_mul(second_divisor), first_divisor);
            let lines = runtime_quotient(lines.wrapping_mul(table.divisor as u64), 1000) & !3;
            (lines, span)
        } else {
            let samples = rows.wrapping_mul(columns) / first_scale as u32;
            let rate = (0x1320 / first_scale as i32).wrapping_mul(100);
            let headroom = (rate as u32).wrapping_sub(samples.wrapping_mul(100));
            let lines = (window.height as u64 * 100).wrapping_mul(headroom as u64);
            let lines = runtime_quotient(runtime_quotient(lines, rate as i64 as u64), 100) & !3;
            let span = runtime_quotient(lines.wrapping_mul(1000).wrapping_mul(first_divisor), second_divisor);
            let span = runtime_quotient(span, table.divisor as u64) & !1;
            (runtime_quotient(lines.wrapping_mul(1000), 1000), span)
        };
        let extent = table.height as u32 + margin * 2;
        let (mut top, mut height) = if extent & 1 != 0 {
            (((extent >> 1) + 1).wrapping_sub((lines >> 1) as u32), (lines as u32).wrapping_sub(2))
        } else {
            ((extent >> 1).wrapping_sub((lines >> 1) as u32), lines as u32)
        };
        let half_margin = (column_margin >> 1) & 0xffff;
        let (mut left, mut width) = if column_margin & 1 != 0 {
            ((half_margin + 1).wrapping_sub((span >> 1) as u32), (span as u32).wrapping_sub(2))
        } else {
            (half_margin.wrapping_sub((span >> 1) as u32), span as u32)
        };
        top &= 0xffff;
        height &= 0xffff;
        left &= 0xffff;
        width &= 0xffff;
        if top < window.top {
            top = window.top & 0xffff;
        }
        if top + height > window.top.wrapping_add(window.height) {
            height = (window.top.wrapping_add(window.height).wrapping_sub(top)) & 0xffff;
        }
        if left < window.left {
            left = window.left & 0xffff;
        }
        if left + width > window.left.wrapping_add(window.width) {
            width = (window.left.wrapping_sub(left).wrapping_add(window.width)) & 0xffff;
        }
        [top as u16, height as u16, left as u16, width as u16]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GridKind {
    Compact,
    Extended,
    Record,
}

impl GridKind {
    /// The starter fields the grid is computed from.
    const fn request(self) -> Location {
        match self {
            GridKind::Compact => field::grids::COMPACT,
            GridKind::Extended => field::grids::EXTENDED,
            GridKind::Record => field::grids::RECORD,
        }
    }

    const fn work(self) -> Work {
        match self {
            GridKind::Compact => Work::CompactRectangles,
            GridKind::Extended => Work::ExtendedRectangles,
            GridKind::Record => Work::RecordRectangles,
        }
    }

    const fn subscription(self) -> Subscription {
        match self {
            GridKind::Compact => Subscription::Grouped(6),
            GridKind::Extended => Subscription::Grouped(7),
            GridKind::Record => Subscription::Grouped(8),
        }
    }

    const fn stage(self) -> u32 {
        match self {
            GridKind::Compact => 0x10,
            GridKind::Extended => 0x40,
            GridKind::Record => 0x100,
        }
    }

    const fn record(self) -> Location {
        match self {
            GridKind::Compact => records::COMPACT_GRID,
            GridKind::Extended => records::EXTENDED_GRID,
            GridKind::Record => records::RECORD_GRID,
        }
    }
}
