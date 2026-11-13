#![forbid(unsafe_code)]

//! The geometry operation: starting and resetting the transition layer's geometry service, and
//! the area lists it answers for a profile window, laid out in the caller's frame.

use crate::controller::queries::Selection;
use crate::controller::windows::{CropInput, GridRequest};
use crate::controller::{queries, ProfileGeometry, Refusal, Status, Tables};
use crate::memory::Memory;
use crate::ports::{AreaAnswer, ConfigOffset, Ports, StockCode};
use crate::profile::{ProfileHandle, Rectangle};
use crate::stock::areas::{area_list, AreaList, TransitionAreas};
use crate::stock::layout::{callbacks, geometry as state};
use crate::stock::Parameters;

/// The status the start entry gives a starter it refuses while the service runs.
const BUSY: i32 = -14;
/// The selection words 0xeeccc0 reads from 0xeee4a4 on; past them it reads code.
const SELECTIONS: [u32; 7] = [0, 1, 0, 1, 0, 1, 2];
/// The profile whose window the standard and paired area queries use.
const STANDARD_PROFILE: ProfileHandle = ProfileHandle(1);
/// The class limits of a 4:3 class.
const FOUR_THREE: [u16; 2] = [4, 3];
const UNIT_SCALE: u32 = 10000;
/// The standard frame the standard query lays its window into, width then height.
const STANDARD_FRAME: [u32; 2] = [480, 640];
const STANDARD_CODE: u32 = 3;
const PAIRED_CODE: u32 = 4;

/// What 0xeeccc0 asks the geometry service to start.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryStart {
    pub mode: u32,
    pub selection: u32,
    /// 1 for record kind 51, 2 for 52, else 0.
    pub kind: u32,
}

/// The kinds an area query takes: 0 to 2 and 5 to 8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaKind(pub u32);

impl AreaKind {
    const fn code(self) -> Option<u32> {
        match self.0 {
            0..=2 | 5..=8 => Some(self.0),
            _ => None,
        }
    }
}

/// An area query for the geometry service; each pair and rectangle is height before width.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaQuery {
    /// Top, left, height, width.
    pub window: [u16; 4],
    pub extent: [u16; 2],
    pub reference: [u16; 4],
    pub pair: [u16; 2],
}

/// The area and format ratio a window is cropped for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaSource {
    pub area: [u32; 3],
    pub ratio: [u16; 2],
}

/// A profile's window size, width then height, and the inner and outer rectangles laid in it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowGeometry {
    pub window: [u32; 2],
    pub inner: Rectangle,
    pub outer: Rectangle,
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    fn service_running(&self) -> bool { self.memory.read_u32(state::RUNNING) != 0 }

    /// 0xeeccc0.
    pub fn start_geometry(&mut self, mode: u32, selection: u32, record_kind: u32, callback: StockCode) -> Status {
        if self.service_running() {
            self.ports.notify_geometry(callback, BUSY);
            return Err(Refusal::WrongState);
        }
        let start = GeometryStart {
            mode: if mode <= 1 { mode } else { 0 },
            selection: SELECTIONS.get(selection as usize).copied().unwrap_or(0),
            kind: match record_kind {
                51 => 1,
                52 => 2,
                _ => 0,
            },
        };
        self.memory.write_u32(state::SELECTION, selection);
        if !self.ports.start_geometry(&start, callback) {
            return Err(Refusal::PipelineRefused);
        }
        self.memory.write_u32(state::RUNNING, 1);
        Ok(())
    }

    /// 0xeecddc.
    pub fn reset_geometry(&mut self) -> Status {
        if !matches!(self.memory.read_u32(state::RUNNING), 1 | 2) {
            return Err(Refusal::WrongState);
        }
        self.clear_geometry_state();
        if self.ports.stop_geometry() { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }

    /// 0xeec89c.
    fn clear_geometry_state(&mut self) {
        let (at, bytes) = state::RESET;
        self.memory.fill(at, bytes, 0);
        for entry in 0..5 {
            self.memory.fill(state::ENTRIES.plus(24 * entry), 24, 0);
            self.memory.write_u16(state::ENTRY_MARKS.plus(2 * entry), 0xffff);
        }
        self.memory.write_u32(state::RESET_ZERO_WORDS[0], 0);
        self.memory.write_u16(state::LIMIT, 0x7fff);
        for at in state::RESET_ZERO_HALVES {
            self.memory.write_u16(at, 0);
        }
        self.memory.write_u32(state::RESET_ZERO_WORDS[1], 0);
        for at in state::WINDOW_SCALE.into_iter().chain(state::UNIT_SCALE) {
            self.memory.write_u32(at, UNIT_SCALE);
        }
        self.memory.fill(state::SNAPSHOT, 4 * state::SNAPSHOT_WORDS as u16, 0);
        for (at, offset) in state::SNAPSHOT_CONFIGURED {
            let value = self.ports.halfword(ConfigOffset(offset));
            self.memory.write_u32(at, value as u32);
        }
    }

    /// 0xeed630.
    pub fn geometry_snapshot(&self) -> [u32; state::SNAPSHOT_WORDS] {
        core::array::from_fn(|index| self.memory.read_u32(state::SNAPSHOT.plus(4 * index as u16)))
    }

    /// 0xeef1c0.
    pub fn set_operation_callbacks(&mut self, enabled: u32) { self.memory.write_u32(callbacks::ENABLED, enabled) }

    /// Whether the profile's class is other than 4:3 (0xeec168).
    fn off_ratio(&self, profile: ProfileHandle) -> bool { self.profiles.class_of(profile).limits != FOUR_THREE }

    /// The profile's 3:4 crop window, with inner and outer rectangles scaled by the service's
    /// window scale, the inner one fitted to `source`'s ratio, both relative to the window
    /// (0xeeda24).
    pub fn window_geometry(&mut self, profile: ProfileHandle, source: &AreaSource) -> WindowGeometry {
        let input = CropInput { profile, area: source.area, divisors: [3, 4], relative: true };
        let window = self.crop_geometry(&input).window;
        let scale = state::WINDOW_SCALE.map(|at| self.memory.read_u32(at));
        let scaled = queries::centred_rectangle(window, scale);
        let (mut inner, mut outer) = (scaled, scaled);
        let first = self.profiles.scaled_first(profile, true) as u32;
        let second = self.profiles.scaled_second(profile, true) as u32;
        let [across, down] = source.ratio.map(u32::from);
        if source.ratio == [1, 1] {
            let height = outer.width.wrapping_mul(down).wrapping_mul(second) / across.wrapping_mul(first);
            inner.height = height.min(outer.height);
            inner.top = inner.top.wrapping_add(outer.height.wrapping_sub(inner.height) >> 1);
        } else {
            let width = outer.height.wrapping_mul(across).wrapping_mul(first) / down.wrapping_mul(second);
            inner.width = width.min(outer.width);
            inner.left = inner.left.wrapping_add(outer.width.wrapping_sub(inner.width) >> 1);
        }
        for rectangle in [&mut inner, &mut outer] {
            rectangle.top = rectangle.top.wrapping_sub(window.top);
            rectangle.left = rectangle.left.wrapping_sub(window.left);
        }
        WindowGeometry { window: [window.width, window.height], inner, outer }
    }

    /// `window_geometry` with the inner rectangle bounded by the compact grid of the window and
    /// centred in it (0xeedb98). Replaces the controller's compact grid record.
    fn gridded_window_geometry(&mut self, profile: ProfileHandle, source: &AreaSource) -> WindowGeometry {
        let mut geometry = self.window_geometry(profile, source);
        let mut parameters = Parameters::from_bytes([0; Parameters::BYTES]);
        parameters.set_profile(profile);
        parameters.set_area(source.area);
        parameters.set_format_ratio(source.ratio);
        let request = GridRequest { height_percent: 100, width_percent: 100, bands: 16, ..GridRequest::default() };
        let requests = queries::Requests { compact: Some(&request), ..queries::Requests::default() };
        let selection = Selection { compact_grid: true, ..Selection::default() };
        let grid = self.profile_geometry(&parameters, selection, &requests).compact_grid.unwrap_or_default();
        let width = (grid.columns as u32).wrapping_mul(grid.column_width).min(geometry.inner.width);
        let height = (grid.rows as u32).wrapping_mul(grid.row_height).min(geometry.inner.height);
        let [window_width, window_height] = geometry.window;
        geometry.inner = Rectangle {
            top: window_height.wrapping_sub(height) >> 1,
            left: window_width.wrapping_sub(width) >> 1,
            width,
            height,
        };
        geometry
    }

    /// Narrows `outer` to 3:4 of its height in the profile's scale and fits `inner` into it,
    /// both centred (0xeec300).
    fn fit_four_three(&self, profile: ProfileHandle, outer: &mut Rectangle, inner: &mut Rectangle) {
        let first = self.profiles.scaled_first(profile, true) as u32;
        let second = self.profiles.scaled_second(profile, true) as u32;
        let width = first.wrapping_mul(outer.height.wrapping_mul(3)) / (second << 2);
        inner.width = inner.width.min(width);
        inner.height = inner.height.min(outer.height);
        *outer = Rectangle { top: 0, left: 0, width, height: outer.height };
        inner.left = width.wrapping_sub(inner.width) >> 1;
        inner.top = outer.height.wrapping_sub(inner.height) >> 1;
    }

    fn query(&mut self, code: u32, query: &AreaQuery) -> Result<AreaList, Refusal> {
        match self.ports.query_areas(code, query) {
            AreaAnswer::Areas(answer) => Ok(area_list(&answer)),
            AreaAnswer::Empty => Ok(area_list(&TransitionAreas::EMPTY)),
            AreaAnswer::Refused => Err(Refusal::WrongState),
        }
    }

    fn checked(&self, kind: AreaKind) -> Result<u32, Refusal> {
        if !self.service_running() {
            return Err(Refusal::WrongState);
        }
        kind.code().ok_or(Refusal::BadArgument)
    }

    /// The areas of the inner rectangle within the outer one (0xeed2e4).
    pub fn window_areas(&mut self, kind: AreaKind, geometry: &WindowGeometry) -> Result<AreaList, Refusal> {
        let code = self.checked(kind)?;
        let window = within(geometry.outer, geometry.inner);
        let query = AreaQuery { window: halves(window), extent: extent(geometry.outer), ..AreaQuery::default() };
        let mut list = self.query(code, &query)?;
        rescale(&mut list, geometry.outer.height, geometry.window[1]);
        Ok(list)
    }

    /// `window_areas` with the outer rectangle cut to 3:4 for a class other than 4:3, laid
    /// into the window (0xeed3f0).
    pub fn adjusted_window_areas(&mut self, kind: AreaKind, profile: ProfileHandle, geometry: &WindowGeometry) -> Result<AreaList, Refusal> {
        let mut geometry = *geometry;
        let off_ratio = self.off_ratio(profile);
        if off_ratio {
            let height = geometry.outer.height;
            let width = height.wrapping_mul(3) >> 2;
            let inner = &mut geometry.inner;
            inner.width = inner.width.min(width);
            inner.height = inner.height.min(height);
            inner.left = width.wrapping_sub(inner.width) >> 1;
            inner.top = height.wrapping_sub(inner.height) >> 1;
            geometry.outer = Rectangle { top: 0, left: 0, width, height };
        }
        let mut list = self.window_areas(kind, &geometry).map_err(|_| Refusal::WrongState)?;
        self.lay_into_window(&mut list, &geometry, off_ratio);
        Ok(list)
    }

    fn lay_into_window(&self, list: &mut AreaList, geometry: &WindowGeometry, centred: bool) {
        if centred {
            let [width, height] = geometry.window;
            let half = |whole: u32, part: u32| (whole.wrapping_sub(part) as i32 / 2) as u32;
            shift(list, [half(height, geometry.outer.height), half(width, geometry.outer.width)]);
        } else {
            shift(list, [geometry.outer.top, geometry.outer.left]);
        }
    }

    /// The areas of standard profile's gridded window laid into a 480 by 640 frame (0xeedc84).
    pub fn standard_areas(&mut self, source: &AreaSource) -> Result<AreaList, Refusal> {
        if !self.service_running() {
            return Err(Refusal::WrongState);
        }
        let mut geometry = self.gridded_window_geometry(STANDARD_PROFILE, source);
        let [frame_width, frame_height] = STANDARD_FRAME;
        let (mut window, frame) = (Rectangle::default(), Rectangle { top: 0, left: 0, width: frame_width, height: frame_height });
        let (reference_frame, scale) = if geometry.outer.height != 0 && geometry.outer.width != 0 {
            if self.off_ratio(STANDARD_PROFILE) {
                self.fit_four_three(STANDARD_PROFILE, &mut geometry.outer, &mut geometry.inner);
            }
            let height = (geometry.inner.height.wrapping_mul(frame_height) / geometry.outer.height).min(frame_height);
            let width = (geometry.inner.width.wrapping_mul(frame_width) / geometry.outer.width).min(frame_width);
            window = Rectangle { top: (frame_height - height) >> 1, left: (frame_width - width) >> 1, width, height };
            (frame, frame_height)
        } else {
            (Rectangle::default(), 0)
        };
        let window = within(reference_frame, window);
        let reference = Rectangle { top: 0, left: 0, ..reference_frame };
        let query = AreaQuery {
            window: halves(window),
            extent: extent(reference_frame),
            reference: halves(reference),
            ..AreaQuery::default()
        };
        let mut list = self.query(STANDARD_CODE, &query)?;
        rescale(&mut list, reference_frame.height, scale);
        Ok(list)
    }

    fn laid_areas(&mut self, kind: AreaKind, profile: ProfileHandle, source: &AreaSource, gridded: bool) -> Result<(AreaList, WindowGeometry), Refusal> {
        let code = self.checked(kind)?;
        let mut geometry =
            if gridded { self.gridded_window_geometry(profile, source) } else { self.window_geometry(profile, source) };
        let mut window = within(geometry.outer, geometry.inner);
        let off_ratio = self.off_ratio(profile);
        if off_ratio {
            self.fit_four_three(profile, &mut geometry.outer, &mut window);
        }
        let query = AreaQuery { window: halves(window), extent: extent(geometry.outer), ..AreaQuery::default() };
        let mut list = self.query(code, &query)?;
        self.lay_into_window(&mut list, &geometry, off_ratio);
        rescale(&mut list, geometry.outer.height, geometry.window[1]);
        Ok((list, geometry))
    }

    /// The areas of the profile's gridded window, laid into it, with the geometry used
    /// (0xeedf48).
    pub fn extended_areas(&mut self, kind: AreaKind, profile: ProfileHandle, source: &AreaSource) -> Result<(AreaList, WindowGeometry), Refusal> {
        self.laid_areas(kind, profile, source, true)
    }

    /// As `extended_areas` without the grid bound (0xeee214).
    pub fn geometry_areas(&mut self, kind: AreaKind, profile: ProfileHandle, source: &AreaSource) -> Result<(AreaList, WindowGeometry), Refusal> {
        self.laid_areas(kind, profile, source, false)
    }

    /// The standard profile's areas for `pair`, width then height (0xeee0f4).
    pub fn paired_areas(&mut self, source: &AreaSource, pair: [u32; 2]) -> Result<AreaList, Refusal> {
        if !self.service_running() {
            return Err(Refusal::WrongState);
        }
        let mut geometry = self.window_geometry(STANDARD_PROFILE, source);
        let mut window = within(geometry.outer, geometry.inner);
        if self.off_ratio(STANDARD_PROFILE) {
            self.fit_four_three(STANDARD_PROFILE, &mut geometry.outer, &mut window);
        }
        let query = AreaQuery {
            window: halves(window),
            extent: extent(geometry.outer),
            pair: [pair[1] as u16, pair[0] as u16],
            ..AreaQuery::default()
        };
        let mut list = self.query(PAIRED_CODE, &query)?;
        rescale(&mut list, geometry.outer.height, geometry.window[1]);
        Ok(list)
    }
}

/// `inner` clipped to `outer` and made relative to it; an axis that does not fit takes
/// `outer`'s (0xeebd8c).
pub fn within(outer: Rectangle, inner: Rectangle) -> Rectangle {
    let outer_bottom = outer.top.wrapping_add(outer.height);
    let outer_right = outer.left.wrapping_add(outer.width);
    let top = inner.top.max(outer.top);
    let left = inner.left.max(outer.left);
    let height = inner.top.wrapping_add(inner.height).min(outer_bottom).wrapping_sub(top);
    let width = inner.left.wrapping_add(inner.width).min(outer_right).wrapping_sub(left);
    let (top, height) = if inner.height >= height { (top, height) } else { (outer.top, outer.height) };
    let (left, width) = if width <= inner.width { (left, width) } else { (outer.left, outer.width) };
    Rectangle { top: top.wrapping_sub(outer.top), left: left.wrapping_sub(outer.left), width, height }
}

fn halves(rectangle: Rectangle) -> [u16; 4] {
    [rectangle.top as u16, rectangle.left as u16, rectangle.height as u16, rectangle.width as u16]
}

fn extent(rectangle: Rectangle) -> [u16; 2] { [rectangle.height as u16, rectangle.width as u16] }

/// 0xeebc30, 0xeebca4.
fn shift(list: &mut AreaList, by: [u32; 2]) {
    for area in list.present() {
        let origins = area.origins().map(|[top, left]| [top.wrapping_add(by[0]), left.wrapping_add(by[1])]);
        area.set_origins(origins);
    }
}

/// Each area's extent times `numerator` over `denominator`; nothing for a zero denominator
/// (0xeebd30).
fn rescale(list: &mut AreaList, numerator: u32, denominator: u32) {
    if denominator == 0 {
        return;
    }
    for area in list.present() {
        area.set_extent((numerator.wrapping_mul(area.extent() as u32) / denominator) as u16);
    }
}
