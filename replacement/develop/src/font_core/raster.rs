use super::scanlines::{Coordinate, Dimension, Row, Span};
use super::{ByteStride, CoverageEncoding, FontConfig, FontError, FontSize, GlyphAdvance, GlyphId, GlyphMask, GlyphMetrics, PixelExtent, PixelOffset, WritingMode};
use super::raster_vm as vmcore;
use core::mem::MaybeUninit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct FixedCoordinate(pub i32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RasterPoint { pub x: FixedCoordinate, pub y: FixedCoordinate }
impl RasterPoint {
    pub const fn new(x: i32, y: i32) -> Self { Self { x: FixedCoordinate(x), y: FixedCoordinate(y) } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeSide { Left, Right }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind { Line, Quadratic }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct EdgeCoordinate(pub i16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurveStackIndex(pub usize);
#[derive(Clone, Copy)]
pub struct RasterGrid {
    pub fraction_bits: u32, pub mask: u32, pub step: FixedCoordinate,
    pub sample: FixedCoordinate, pub last_row: Row, pub tolerance: FixedCoordinate,
}
#[derive(Clone, Copy)]
pub struct EdgeState { pub first_row: Row, pub last_left: Row, pub last_right: Row }
pub trait GeometryStorage {
    type Error;
    fn edge(&self, side: EdgeSide, index: Row) -> Result<EdgeCoordinate, Self::Error>;
    fn set_edge(&mut self, side: EdgeSide, index: Row, value: EdgeCoordinate) -> Result<(), Self::Error>;
    fn curve_point(&self, index: CurveStackIndex) -> Result<RasterPoint, Self::Error>;
    fn set_curve_point(&mut self, index: CurveStackIndex, point: RasterPoint) -> Result<(), Self::Error>;
}
pub fn arm_asr(value: i32, shift: u32) -> i32 { let shift = shift & 255; if shift < 32 { value >> shift } else { value >> 31 } }
pub fn arm_lsl(value: i32, shift: u32) -> i32 { let shift = shift & 255; if shift < 32 { value.wrapping_shl(shift) } else { 0 } }
pub fn scale8(value: FixedCoordinate, scale: Dimension) -> FixedCoordinate {
    let product = value.0.wrapping_mul(scale.0).wrapping_add(128);
    FixedCoordinate(if product < 0 { product.wrapping_add(255) >> 8 } else { product >> 8 })
}
#[derive(Clone, Copy)]
pub enum CurveScale { Unscaled, Dimensions { width: Dimension, height: Dimension } }
pub fn transform_curve(points: &mut [RasterPoint; 3], scale: CurveScale, translation: super::scanlines::Point, shift: u32) {
    let x = arm_lsl(translation.x.0, shift); let y = arm_lsl(translation.y.0, shift);
    for point in points {
        if let CurveScale::Dimensions { width, height } = scale { point.x = scale8(point.x, width); point.y = scale8(point.y, height); }
        point.x.0 = point.x.0.wrapping_add(x); point.y.0 = point.y.0.wrapping_add(y);
    }
}
fn middle(first: RasterPoint, last: RasterPoint) -> RasterPoint {
    RasterPoint::new(first.x.0.wrapping_add(last.x.0) / 2, first.y.0.wrapping_add(last.y.0) / 2)
}
pub fn edge_value<T: GeometryStorage>(state: &mut EdgeState, side: EdgeSide, row: Row, value: Coordinate, lower_base: bool, storage: &mut T) -> Result<(), T::Error> {
    if lower_base && row.0 < state.first_row.0 { state.first_row = row; }
    let maximum = match side { EdgeSide::Left => &mut state.last_left, EdgeSide::Right => &mut state.last_right };
    if maximum.0 < row.0 { *maximum = row; }
    let index = Row(row.0.wrapping_sub(state.first_row.0));
    update_edge(side, index, value, storage)
}
pub fn update_edge<T: GeometryStorage>(side: EdgeSide, index: Row, value: Coordinate, storage: &mut T) -> Result<(), T::Error> {
    let previous = i32::from(storage.edge(side, index)?.0);
    if match side { EdgeSide::Left => value.0 <= previous, EdgeSide::Right => value.0 >= previous } {
        storage.set_edge(side, index, EdgeCoordinate(value.0 as i16))?;
    }
    Ok(())
}
pub fn line_edge<T: GeometryStorage>(grid: RasterGrid, state: &mut EdgeState, side: EdgeSide, first: RasterPoint, last: RasterPoint, storage: &mut T) -> Result<(), T::Error> {
    let dy = last.y.0.wrapping_sub(first.y.0);
    let dx = last.x.0.wrapping_sub(first.x.0);
    if dy == 0 {
        let row = Row(arm_asr(first.y.0, grid.fraction_bits));
        edge_value(state, side, row, Coordinate(arm_asr(first.x.0, grid.fraction_bits)), true, storage)?;
        if dx != 0 { edge_value(state, side, row, Coordinate(arm_asr(last.x.0, grid.fraction_bits)), true, storage)?; }
        return Ok(());
    }
    let fraction = (first.y.0 as u32 & grid.mask) as i32;
    let start = first.y.0.wrapping_add(grid.sample.0).wrapping_sub(fraction).wrapping_add(if fraction > grid.sample.0 { grid.step.0 } else { 0 });
    let fraction = (last.y.0 as u32 & grid.mask) as i32;
    let mut end = if fraction < grid.sample.0 { last.y.0.wrapping_sub(fraction).wrapping_sub(grid.sample.0) } else { last.y.0.wrapping_add(grid.sample.0).wrapping_sub(fraction) };
    if end == last.y.0 { end = end.wrapping_sub(grid.step.0); }
    let (mut offset, step, mut error, remainder) = if dx == 0 { (0i32, 0i32, 0i32, 0i32) } else {
        let product = dx.wrapping_mul(start.wrapping_sub(first.y.0));
        let quotient = product.wrapping_div(dy);
        let step_product = arm_lsl(dx, grid.fraction_bits);
        let step = step_product.wrapping_div(dy);
        (quotient, step, product.wrapping_sub(dy.wrapping_mul(quotient)), step_product.wrapping_sub(dy.wrapping_mul(step)))
    };
    let last_row = arm_asr(end, grid.fraction_bits).min(grid.last_row.0);
    let first_row = arm_asr(start, grid.fraction_bits).max(0);
    let maximum = match side { EdgeSide::Left => &mut state.last_left, EdgeSide::Right => &mut state.last_right };
    if maximum.0 < last_row { *maximum = Row(last_row); }
    let fractional = remainder.wrapping_mul(last_row.wrapping_sub(first_row)).wrapping_add(error) > 0;
    let mut x = first.x.0;
    for row in first_row..=last_row {
        let value;
        if fractional {
            error = error.wrapping_add(remainder);
            value = arm_asr(first.x.0.wrapping_add(offset), grid.fraction_bits);
            offset = offset.wrapping_add(step);
            if dy <= error { offset = offset.wrapping_add(1); error = error.wrapping_sub(dy); }
        } else { value = arm_asr(x, grid.fraction_bits); x = x.wrapping_add(step); }
        edge_value(state, side, Row(row), Coordinate(value), false, storage)?;
    }
    Ok(())
}
fn ordered_line<T: GeometryStorage>(grid: RasterGrid, state: &mut EdgeState, side: EdgeSide, first: RasterPoint, last: RasterPoint, storage: &mut T) -> Result<(), T::Error> {
    if first.y.0 <= last.y.0 { line_edge(grid, state, side, first, last, storage) } else { line_edge(grid, state, side, last, first, storage) }
}
pub fn draw_edge_curve<T: GeometryStorage>(grid: RasterGrid, state: &mut EdgeState, side: EdgeSide, kind: CurveKind, points: [RasterPoint; 3], storage: &mut T) -> Result<(), T::Error> {
    let [mut first, mut control, mut last] = points;
    edge_value(state, side, Row(arm_asr(first.y.0, grid.fraction_bits)), Coordinate(arm_asr(first.x.0, grid.fraction_bits)), true, storage)?;
    edge_value(state, side, Row(arm_asr(last.y.0, grid.fraction_bits)), Coordinate(arm_asr(last.x.0, grid.fraction_bits)), true, storage)?;
    if kind == CurveKind::Line { return ordered_line(grid, state, side, first, last, storage); }
    let mut depth = 0usize;
    loop {
        let left = middle(first, control);
        let right = middle(control, last);
        let center = middle(left, right);
        if control.x.0.wrapping_sub(center.x.0).wrapping_abs() < grid.tolerance.0 && control.y.0.wrapping_sub(center.y.0).wrapping_abs() < grid.tolerance.0 {
            ordered_line(grid, state, side, first, last, storage)?;
            if depth < 1 { return Ok(()); }
            depth -= 3;
            first = storage.curve_point(CurveStackIndex(depth + 2))?;
            control = storage.curve_point(CurveStackIndex(depth + 1))?;
            last = storage.curve_point(CurveStackIndex(depth))?;
        } else {
            for (index, point) in [last, right, center].into_iter().enumerate() { storage.set_curve_point(CurveStackIndex(depth + index), point)?; }
            depth += 3;
            last = center; control = left;
            if depth >= 80 { return Ok(()); }
        }
    }
}

#[derive(Clone, Copy)]
pub struct CachedCurve { pub point: Option<RasterPoint>, pub first_row: Row }
#[derive(Clone, Copy)]
pub struct CoarseCurveNode { pub end: RasterPoint, pub control: RasterPoint }
pub trait CachedCurveStorage {
    type Error;
    fn emit(&mut self, coordinate: EdgeCoordinate) -> Result<(), Self::Error>;
    fn node(&self, index: CurveStackIndex) -> Result<CoarseCurveNode, Self::Error>;
    fn set_node(&mut self, index: CurveStackIndex, node: CoarseCurveNode) -> Result<(), Self::Error>;
}
fn cache_point<T: CachedCurveStorage>(point: RasterPoint, side: EdgeSide, shift: u32, state: &mut CachedCurve, storage: &mut T) -> Result<(), T::Error> {
    let row = arm_asr(point.y.0, shift);
    match state.point {
        None => { state.point = Some(point); if state.first_row.0 < 0 { state.first_row = Row(row); } }
        Some(previous) if arm_lsl(arm_asr(previous.y.0, shift), shift) < arm_lsl(row, shift) => {
            state.point = Some(point);
            storage.emit(EdgeCoordinate(arm_asr(previous.x.0, shift) as i16))?;
        }
        Some(previous) => {
            if match side { EdgeSide::Left => point.x.0 < previous.x.0, EdgeSide::Right => point.x.0 > previous.x.0 } { state.point = Some(point); }
        }
    }
    Ok(())
}
pub fn draw_cached_curve<T: CachedCurveStorage>(points: &mut [RasterPoint; 3], side: EdgeSide, shift: u32, coarse_shift: u32, coarse_step: FixedCoordinate, state: &mut CachedCurve, storage: &mut T) -> Result<(), T::Error> {
    cache_point(points[0], side, shift, state, storage)?;
    let mut threshold = coarse_step.0.wrapping_add(arm_lsl(arm_asr(points[0].y.0, coarse_shift), coarse_shift));
    let mut depth = 0usize;
    loop {
        let last = points[2];
        let level = arm_lsl(arm_asr(last.y.0, coarse_shift), coarse_shift);
        if level > threshold {
            let left = middle(points[0], points[1]);
            let right = middle(points[1], last);
            let center = middle(left, right);
            storage.set_node(CurveStackIndex(depth), CoarseCurveNode { end: last, control: right })?;
            depth = depth.wrapping_add(1); points[1] = left; points[2] = center;
        } else {
            if level == threshold {
                cache_point(last, side, shift, state, storage)?;
                threshold = threshold.wrapping_add(coarse_step.0); points[0] = last;
            }
            if depth == 0 { return Ok(()); }
            depth -= 1;
            let node = storage.node(CurveStackIndex(depth))?;
            points[1] = node.control; points[2] = node.end;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct RunIndex(pub u16);
impl RunIndex { pub const NONE: Self = Self(u16::MAX); }
#[derive(Clone, Copy)]
#[repr(C)]
pub struct RunNode { pub first: EdgeCoordinate, pub last: EdgeCoordinate, pub next: RunIndex }
pub trait RunStorage {
    type Error;
    fn first_row(&self) -> Row;
    fn row_count(&self) -> super::scanlines::RowCount;
    fn head(&self, row: Row) -> Result<RunIndex, Self::Error>;
    fn set_head(&mut self, row: Row, index: RunIndex) -> Result<(), Self::Error>;
    fn tail(&self, row: Row) -> Result<RunIndex, Self::Error>;
    fn set_tail(&mut self, row: Row, index: RunIndex) -> Result<(), Self::Error>;
    fn node(&self, index: RunIndex) -> Result<RunNode, Self::Error>;
    fn initialize_node(&mut self, index: RunIndex, node: RunNode) -> Result<(), Self::Error>;
    fn set_first(&mut self, index: RunIndex, value: EdgeCoordinate) -> Result<(), Self::Error>;
    fn set_last(&mut self, index: RunIndex, value: EdgeCoordinate) -> Result<(), Self::Error>;
    fn set_next(&mut self, index: RunIndex, next: RunIndex) -> Result<(), Self::Error>;
    fn capacity(&self) -> u16;
    fn allocated(&self) -> u16;
    fn set_allocated(&mut self, count: u16);
    fn free(&self) -> RunIndex;
    fn set_free(&mut self, index: RunIndex);
}
fn allocate_run<T: RunStorage>(storage: &mut T) -> Result<Option<RunIndex>, T::Error> {
    let allocated = storage.allocated();
    if allocated < storage.capacity() { storage.set_allocated(allocated.wrapping_add(1)); return Ok(Some(RunIndex(allocated))); }
    let index = storage.free();
    if index == RunIndex::NONE { return Ok(None); }
    storage.set_free(storage.node(index)?.next); Ok(Some(index))
}
fn recycle_run<T: RunStorage>(storage: &mut T, index: RunIndex) -> Result<(), T::Error> {
    storage.set_next(index, storage.free())?; storage.set_free(index); Ok(())
}
pub fn merge_run<T: RunStorage>(storage: &mut T, row: Row, span: Span) -> Result<bool, T::Error> {
    let mut index = storage.head(row)?; let mut previous = RunIndex::NONE;
    if index != RunIndex::NONE {
        let tail = storage.tail(row)?;
        if i32::from(storage.node(tail)?.last.0) < span.first.0 {
            let Some(fresh) = allocate_run(storage)? else { return Ok(false); };
            storage.initialize_node(fresh, RunNode { first: EdgeCoordinate(span.first.0 as i16), last: EdgeCoordinate(span.last.0 as i16), next: RunIndex::NONE })?;
            let tail = storage.tail(row)?;
            storage.set_next(tail, fresh)?; storage.set_tail(row, fresh)?; return Ok(true);
        }
    }
    while index != RunIndex::NONE {
        let current = storage.node(index)?;
        if span.last.0 < i32::from(current.first.0).wrapping_sub(1) { break; }
        if span.first.0 <= i32::from(current.last.0).wrapping_add(1) {
            if span.first.0 < i32::from(current.first.0) { storage.set_first(index, EdgeCoordinate(span.first.0 as i16))?; }
            if span.last.0 <= i32::from(current.last.0) { return Ok(true); }
            storage.set_last(index, EdgeCoordinate(span.last.0 as i16))?;
            let mut next = current.next;
            while next != RunIndex::NONE {
                let following = storage.node(next)?;
                if i32::from(storage.node(index)?.last.0).wrapping_add(1) < i32::from(following.first.0) { break; }
                if following.last.0 > storage.node(index)?.last.0 { storage.set_last(index, following.last)?; }
                storage.set_next(index, following.next)?; recycle_run(storage, next)?; next = following.next;
            }
            if next == RunIndex::NONE { storage.set_tail(row, index)?; }
            return Ok(true);
        }
        previous = index; index = current.next;
    }
    let Some(fresh) = allocate_run(storage)? else { return Ok(false); };
    storage.initialize_node(fresh, RunNode { first: EdgeCoordinate(span.first.0 as i16), last: EdgeCoordinate(span.last.0 as i16), next: index })?;
    if previous == RunIndex::NONE { storage.set_head(row, fresh)?; } else { storage.set_next(previous, fresh)?; }
    if index == RunIndex::NONE { storage.set_tail(row, fresh)?; }
    Ok(true)
}
pub fn subtract_run<T: RunStorage>(storage: &mut T, row: Row, span: Span) -> Result<bool, T::Error> {
    let index = row.0.wrapping_sub(storage.first_row().0);
    if index < 0 || index >= storage.row_count().0 { return Ok(true); }
    let mut index = storage.head(row)?; let mut previous = RunIndex::NONE;
    while index != RunIndex::NONE {
        let current = storage.node(index)?;
        let left = i32::from(current.first.0); let right = i32::from(current.last.0);
        if left > span.last.0 { break; }
        if left < span.first.0 {
            if right > span.last.0 {
                let Some(fresh) = allocate_run(storage)? else { return Ok(false); };
                storage.initialize_node(fresh, RunNode { first: EdgeCoordinate(span.last.0.wrapping_add(1) as i16), last: current.last, next: current.next })?;
                storage.set_last(index, EdgeCoordinate(span.first.0.wrapping_sub(1) as i16))?; storage.set_next(index, fresh)?; break;
            }
            if right >= span.first.0 { storage.set_last(index, EdgeCoordinate(span.first.0.wrapping_sub(1) as i16))?; }
            previous = index;
        } else if right > span.last.0 { storage.set_first(index, EdgeCoordinate(span.last.0.wrapping_add(1) as i16))?; break; }
        else {
            if previous == RunIndex::NONE { storage.set_head(row, current.next)?; } else { storage.set_next(previous, current.next)?; }
            recycle_run(storage, index)?;
        }
        index = current.next;
    }
    Ok(true)
}
pub fn run_row_bounds<T: RunStorage>(storage: &T, last: Row) -> Result<(Row, Row), T::Error> {
    let first = storage.first_row(); let mut end = last.0.wrapping_sub(first.0); let mut start = 0;
    if end > 0 {
        while start < end && storage.head(Row(first.0.wrapping_add(start)))? == RunIndex::NONE { start += 1; }
        while end > 0 && storage.head(Row(first.0.wrapping_add(end)))? == RunIndex::NONE { end -= 1; }
    }
    Ok((Row(first.0.wrapping_add(start)), Row(first.0.wrapping_add(end))))
}
pub fn storage_used(bytes: u32, capacity: u16, allocated: u16) -> u32 { bytes.wrapping_sub(u32::from(capacity).wrapping_sub(u32::from(allocated)).wrapping_mul(6)) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrayCoverage { Gray4, Gray8 }
impl GrayCoverage {
    pub const fn factor(self) -> i32 { match self { Self::Gray4 => 4, Self::Gray8 => 16 } }
    pub const fn shift(self) -> u32 { match self { Self::Gray4 => 2, Self::Gray8 => 4 } }
    pub const fn maximum(self) -> i32 { match self { Self::Gray4 => 15, Self::Gray8 => 255 } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoveragePalette { Linear, Lookup }
#[derive(Clone, Copy)]
pub struct CoverageConfig {
    pub format: GrayCoverage, pub orientation: super::scanlines::Orientation,
    pub origin: super::scanlines::Point, pub height: Dimension, pub stride: super::scanlines::Stride,
    pub first_row: Row, pub last_row: Row, pub palette: CoveragePalette,
}
pub trait CoverageStorage: super::scanlines::BitmapStorage {
    fn head(&self, row: Row) -> Result<RunIndex, Self::Error>;
    fn run(&self, index: RunIndex) -> Result<RunNode, Self::Error>;
    fn alignment(&self, offset: super::scanlines::BitmapOffset) -> u8;
    fn lookup(&self, value: u8) -> Result<u8, Self::Error>;
    fn bitmap_word(&self, offset: super::scanlines::BitmapOffset) -> Result<u32, Self::Error> {
        let mut bytes = [0; 4];
        for (index, value) in bytes.iter_mut().enumerate() { *value = self.bitmap_byte(super::scanlines::BitmapOffset(offset.0.wrapping_add(index as i32)))?; }
        Ok(u32::from_le_bytes(bytes))
    }
    fn set_bitmap_word(&mut self, offset: super::scanlines::BitmapOffset, value: u32) -> Result<(), Self::Error> {
        for (index, value) in value.to_le_bytes().into_iter().enumerate() { self.set_bitmap_byte(super::scanlines::BitmapOffset(offset.0.wrapping_add(index as i32)), value)?; }
        Ok(())
    }
}
fn add_coverage<T: super::scanlines::BitmapStorage>(storage: &mut T, offset: super::scanlines::BitmapOffset, amount: i32, maximum: Option<i32>) -> Result<(), T::Error> {
    let value = i32::from(storage.bitmap_byte(offset)?).wrapping_add(amount);
    storage.set_bitmap_byte(offset, maximum.map_or(value, |maximum| value.min(maximum)) as u8)
}
fn interior_scalar<T: CoverageStorage>(storage: &mut T, offset: super::scanlines::BitmapOffset, format: GrayCoverage, first: bool, last: bool) -> Result<(), T::Error> {
    if first { storage.set_bitmap_byte(offset, format.factor() as u8) }
    else { add_coverage(storage, offset, format.factor(), if last { Some(format.maximum()) } else { None }) }
}
pub fn coverage_interior<T: CoverageStorage>(storage: &mut T, mut offset: super::scanlines::BitmapOffset, count: i32, format: GrayCoverage, first: bool, last: bool) -> Result<(), T::Error> {
    let prefix = count.min(4 - i32::from(storage.alignment(offset)));
    for _ in 0..prefix { interior_scalar(storage, offset, format, first, last)?; offset.0 = offset.0.wrapping_add(1); }
    let mut remaining = count.wrapping_sub(prefix);
    let packed = match format { GrayCoverage::Gray4 => 0x04040404u32, GrayCoverage::Gray8 => 0x0f0f0f0f };
    while remaining >= 4 {
        let value = if first { packed } else { storage.bitmap_word(offset)?.wrapping_add(packed) };
        storage.set_bitmap_word(offset, value)?;
        if last {
            for index in 0..4 {
                let byte = super::scanlines::BitmapOffset(offset.0.wrapping_add(index));
                if i32::from(storage.bitmap_byte(byte)?) > format.maximum() { storage.set_bitmap_byte(byte, format.maximum() as u8)?; }
            }
        }
        offset.0 = offset.0.wrapping_add(4); remaining -= 4;
    }
    for _ in 0..remaining { interior_scalar(storage, offset, format, first, last)?; offset.0 = offset.0.wrapping_add(1); }
    Ok(())
}
pub fn coverage<T: CoverageStorage>(config: CoverageConfig, storage: &mut T) -> Result<(), T::Error> {
    use super::scanlines::{BitmapOffset, Bounds, Orientation, Point};
    let empty = Bounds { minimum: Point { x: Coordinate(32767), y: Coordinate(32767) }, maximum: Point { x: Coordinate(0), y: Coordinate(0) } };
    if config.last_row.0 < config.first_row.0 { storage.set_bounds(empty); return Ok(()); }
    let shift = config.format.shift(); let factor = config.format.factor(); let maximum = config.format.maximum();
    let rotated = config.orientation == Orientation::Rotate;
    let x_origin = config.origin.x.0; let y_origin = config.origin.y.0;
    let mut bounds = empty;
    if rotated {
        bounds.minimum.x = Coordinate(arm_asr(x_origin.wrapping_add(config.height.0).wrapping_sub(config.last_row.0), shift));
        bounds.maximum.x = Coordinate(arm_asr(x_origin.wrapping_add(config.height.0).wrapping_sub(config.first_row.0), shift));
    } else {
        bounds.minimum.y = Coordinate(arm_asr(y_origin.wrapping_add(config.first_row.0), shift));
        bounds.maximum.y = Coordinate(arm_asr(y_origin.wrapping_add(config.last_row.0), shift));
    }
    storage.set_bounds(bounds);
    for row in config.first_row.0..=config.last_row.0 {
        let mut index = storage.head(Row(row))?;
        while index != RunIndex::NONE {
            let run = storage.run(index)?;
            let origin = if rotated { y_origin } else { x_origin };
            let left = i32::from(run.first.0).wrapping_add(origin); let right = i32::from(run.last.0).wrapping_add(origin);
            let low_pixel = arm_asr(left, shift); let high_pixel = arm_asr(right, shift);
            if rotated {
                let column = arm_asr(x_origin.wrapping_add(config.height.0).wrapping_sub(1).wrapping_sub(row), shift);
                for pixel in low_pixel..=high_pixel {
                    let amount = right.wrapping_add(1).min(arm_lsl(pixel.wrapping_add(1), shift)).wrapping_sub(left.max(arm_lsl(pixel, shift)));
                    add_coverage(storage, BitmapOffset(pixel.wrapping_mul(config.stride.0).wrapping_add(column)), amount, Some(maximum))?;
                }
                bounds.minimum.y.0 = bounds.minimum.y.0.min(left); bounds.maximum.y.0 = bounds.maximum.y.0.max(right);
            } else {
                let y = y_origin.wrapping_add(row);
                let base = arm_asr(y, shift).wrapping_mul(config.stride.0);
                let last = y & (factor - 1) == factor - 1;
                if low_pixel == high_pixel {
                    add_coverage(storage, BitmapOffset(base.wrapping_add(low_pixel)), right.wrapping_add(1).wrapping_sub(left), if last { Some(maximum) } else { None })?;
                } else {
                    add_coverage(storage, BitmapOffset(base.wrapping_add(low_pixel)), factor.wrapping_sub(left & (factor - 1)), if last { Some(maximum) } else { None })?;
                    let interior = high_pixel.wrapping_sub(low_pixel).wrapping_sub(1);
                    if interior > 0 { coverage_interior(storage, BitmapOffset(base.wrapping_add(low_pixel).wrapping_add(1)), interior, config.format, y & (factor - 1) == 0, last)?; }
                    add_coverage(storage, BitmapOffset(base.wrapping_add(high_pixel)), (right & (factor - 1)).wrapping_add(1), if last { Some(maximum) } else { None })?;
                }
                bounds.minimum.x.0 = bounds.minimum.x.0.min(left); bounds.maximum.x.0 = bounds.maximum.x.0.max(right);
            }
            storage.set_bounds(bounds); index = run.next;
        }
    }
    if rotated { bounds.minimum.y.0 = arm_asr(bounds.minimum.y.0, shift); bounds.maximum.y.0 = arm_asr(bounds.maximum.y.0, shift); }
    else { bounds.minimum.x.0 = arm_asr(bounds.minimum.x.0, shift); bounds.maximum.x.0 = arm_asr(bounds.maximum.x.0, shift); }
    storage.set_bounds(bounds);
    if config.format == GrayCoverage::Gray4 && config.palette == CoveragePalette::Lookup {
        for row in bounds.minimum.y.0..=bounds.maximum.y.0 {
            for column in 0..config.stride.0 {
                let offset = BitmapOffset(row.wrapping_mul(config.stride.0).wrapping_add(column));
                storage.set_bitmap_byte(offset, storage.lookup(storage.bitmap_byte(offset)?)?)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
pub struct ByteCursor { pub position: super::raster_vm::ProgramOffset, pub remaining: super::raster_vm::Remaining }
#[derive(Clone, Copy)]
pub struct ProgramByteCount(pub i32);
pub trait AssetBytes {
    type Error;
    fn byte_at(&self, position: super::raster_vm::ProgramOffset) -> Result<u8, Self::Error>;
}
pub fn read_count<T: AssetBytes>(asset: &T, cursor: &mut ByteCursor, count: ProgramByteCount) -> Result<u32, T::Error> {
    let mut offset = cursor.position.0.wrapping_add(count.0).wrapping_sub(1);
    let mut value = u32::from(asset.byte_at(super::raster_vm::ProgramOffset(offset))?);
    let mut remaining = count.0.wrapping_sub(1);
    while count.0 > 1 && remaining > 0 {
        offset = offset.wrapping_sub(1);
        value = (value << 8) | u32::from(asset.byte_at(super::raster_vm::ProgramOffset(offset))?);
        remaining -= 1;
    }
    cursor.position.0 = cursor.position.0.wrapping_add(count.0);
    cursor.remaining.0 = cursor.remaining.0.wrapping_sub(count.0);
    Ok(value)
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgramStream { Primary, Secondary }
#[derive(Clone, Copy)]
pub struct StreamSelection(pub u32);
impl StreamSelection {
    pub const PRIMARY: Self = Self(0);
    pub const SECONDARY: Self = Self(1);
    pub const fn stream(self) -> ProgramStream { if self.0 == 0 { ProgramStream::Primary } else { ProgramStream::Secondary } }
}
#[derive(Clone, Copy)]
pub struct SubroutineId(pub u32);
#[derive(Clone, Copy)]
pub struct CompoundId(pub u32);
#[derive(Clone, Copy)]
pub struct TransformCoordinate(pub i16);
#[derive(Clone, Copy)]
pub struct TransformRatio(pub i16);
#[derive(Clone, Copy)]
pub struct CoverageMultiplier(pub i16);
#[derive(Clone, Copy)]
pub struct ComponentTransform {
    pub x: TransformCoordinate, pub y: TransformCoordinate,
    pub scale_x: TransformRatio, pub scale_y: TransformRatio,
    pub origin_x: TransformCoordinate, pub origin_y: TransformCoordinate,
    pub denominator_x: TransformRatio, pub denominator_y: TransformRatio, pub coverage: CoverageMultiplier,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProgramStatus { Complete, InvalidOpcode, RunCapacity }
impl ProgramStatus { pub const fn native_bits(self) -> i32 { match self { Self::Complete => 0, Self::InvalidOpcode => -7, Self::RunCapacity => -4 } } }
pub trait ProgramStorage: AssetBytes {
    fn active(&self) -> StreamSelection;
    fn set_active(&mut self, stream: StreamSelection);
    fn stream(&self, stream: ProgramStream) -> Result<ByteCursor, Self::Error>;
    fn set_stream(&mut self, stream: ProgramStream, cursor: ByteCursor);
    fn read_selected(&mut self, count: ProgramByteCount) -> Result<u32, Self::Error> where Self: Sized { read_selected_cursor(self, count) }
    fn subroutine(&self, id: SubroutineId) -> Result<Option<ByteCursor>, Self::Error>;
    fn compound(&self, id: CompoundId) -> Result<Option<ByteCursor>, Self::Error>;
    fn point_count(&self) -> Result<super::raster_vm::PointCount, Self::Error>;
    fn set_point_count(&mut self, count: super::raster_vm::PointCount);
    fn value_count(&self) -> Result<super::raster_vm::ValueCount, Self::Error>;
    fn set_value_count(&mut self, count: super::raster_vm::ValueCount);
    fn initial_point(&self, index: super::raster_vm::PointIndex) -> Result<super::raster_vm::BytePoint, Self::Error>;
    fn set_initial_point(&mut self, index: super::raster_vm::PointIndex, point: super::raster_vm::BytePoint) -> Result<(), Self::Error>;
    fn initial_value(&self, index: super::raster_vm::ScaleIndex) -> Result<u8, Self::Error>;
    fn set_initial_value(&mut self, index: super::raster_vm::ScaleIndex, value: u8) -> Result<(), Self::Error>;
    fn body(&self) -> Result<ByteCursor, Self::Error>;
    fn set_body(&mut self, body: ByteCursor);
    fn dimensions(&self) -> (Dimension, Dimension);
    fn set_inline_origin(&mut self, origin: super::scanlines::Point);
    fn divide(&mut self, numerator: i32, denominator: i32) -> Result<i32, Self::Error>;
    fn execute_body(&mut self) -> Result<ProgramStatus, Self::Error>;
    fn missing_producer(&self) -> Self::Error;
}
pub fn read_selected_cursor<T: ProgramStorage>(storage: &mut T, count: ProgramByteCount) -> Result<u32, T::Error> {
    let selection = storage.active().stream();
    let mut cursor = storage.stream(selection)?;
    let value = read_count(storage, &mut cursor, count)?;
    storage.set_stream(selection, cursor);
    Ok(value)
}
fn stream_value<T: ProgramStorage>(storage: &mut T, count: ProgramByteCount) -> Result<u32, T::Error> { storage.read_selected(count) }
fn stream_byte<T: ProgramStorage>(storage: &mut T) -> Result<u8, T::Error> { Ok(stream_value(storage, ProgramByteCount(1))? as u8) }
pub fn render_program<T: ProgramStorage>(storage: &mut T, transform: Option<&ComponentTransform>, selection: StreamSelection) -> Result<ProgramStatus, T::Error> {
    use super::raster_vm::{BytePoint, PointCount, PointIndex, ProgramOffset, ScaleIndex, ValueCount};
    storage.set_active(selection);
    let mut reuse_mask: Option<u8> = None;
    let mut component: Option<ComponentTransform> = None;
    loop {
        if storage.stream(selection.stream())?.remaining.0 < 1 { storage.set_active(StreamSelection::PRIMARY); return Ok(ProgramStatus::Complete); }
        let opcode = stream_byte(storage)?;
        let mut adjust_inline = false;
        if opcode == 0xfc {
            let code = stream_value(storage, ProgramByteCount(2))?;
            if let Some(mut record) = storage.compound(CompoundId(code))? {
                let first = i16::from(storage.byte_at(record.position)?);
                let second = i16::from(storage.byte_at(ProgramOffset(record.position.0.wrapping_add(1)))?);
                let scale_x = i16::from(storage.byte_at(ProgramOffset(record.position.0.wrapping_add(2)))?);
                let scale_y = i16::from(storage.byte_at(ProgramOffset(record.position.0.wrapping_add(3)))?);
                component = Some(ComponentTransform { x: TransformCoordinate(first), y: TransformCoordinate(second),
                    scale_x: TransformRatio(scale_x), scale_y: TransformRatio(scale_y), origin_x: TransformCoordinate(first), origin_y: TransformCoordinate(second),
                    denominator_x: TransformRatio(scale_x), denominator_y: TransformRatio(scale_y), coverage: CoverageMultiplier(128) });
                record.position.0 = record.position.0.wrapping_add(4); record.remaining.0 = record.remaining.0.wrapping_sub(4);
                storage.set_stream(ProgramStream::Secondary, record);
            }
            let modifiers = stream_byte(storage)?;
            let status = if modifiers == 0 { render_program(storage, None, StreamSelection::SECONDARY)? } else {
                let Some(fields) = component.as_mut() else { return Err(storage.missing_producer()); };
                if modifiers & 1 != 0 { fields.x = TransformCoordinate(i16::from(stream_byte(storage)?)); fields.scale_x = TransformRatio(i16::from(stream_byte(storage)?)); }
                if modifiers & 2 != 0 { fields.y = TransformCoordinate(i16::from(stream_byte(storage)?)); fields.scale_y = TransformRatio(i16::from(stream_byte(storage)?)); }
                if modifiers & 4 != 0 { fields.coverage = CoverageMultiplier(i16::from(stream_byte(storage)?)); }
                render_program(storage, Some(fields), StreamSelection::SECONDARY)?
            };
            if status != ProgramStatus::Complete { storage.set_active(StreamSelection::PRIMARY); return Ok(status); }
            continue;
        }
        if opcode == 0xfd {
            let skip = stream_value(storage, ProgramByteCount(2))? as i32;
            let mut primary = storage.stream(ProgramStream::Primary)?;
            primary.position.0 = primary.position.0.wrapping_add(skip); primary.remaining.0 = primary.remaining.0.wrapping_sub(skip);
            storage.set_stream(ProgramStream::Primary, primary); continue;
        }
        if opcode == 0xfa {
            storage.set_point_count(PointCount(0));
            let count = i32::from(stream_byte(storage)?);
            let padded = count.wrapping_add(count & 1);
            storage.set_value_count(ValueCount(padded));
            for index in 0..padded { let value = stream_byte(storage)?; storage.set_initial_value(ScaleIndex(index as u32), value)?; }
            let length = stream_value(storage, ProgramByteCount(2))? as i32;
            let mut source = storage.stream(selection.stream())?;
            storage.set_body(ByteCursor { position: source.position, remaining: super::raster_vm::Remaining(length) });
            source.position.0 = source.position.0.wrapping_add(length); source.remaining.0 = source.remaining.0.wrapping_sub(length);
            storage.set_stream(selection.stream(), source);
            adjust_inline = selection.0 != 0 && transform.is_some();
        } else {
            let code = if opcode == 0xff { stream_value(storage, ProgramByteCount(2))? } else { u32::from(opcode) };
            if let Some(mut record) = storage.subroutine(SubroutineId(code))? {
                let sizes = storage.byte_at(record.position)?;
                storage.set_value_count(ValueCount(i32::from(sizes & 15))); storage.set_point_count(PointCount(i32::from(sizes >> 4)));
                reuse_mask = Some(storage.byte_at(ProgramOffset(record.position.0.wrapping_add(1)))?);
                record.position.0 = record.position.0.wrapping_add(2); record.remaining.0 = record.remaining.0.wrapping_sub(2);
                storage.set_body(record);
            }
            let points = storage.point_count()?.0;
            let mut bit = 1u16;
            for index in 0..points {
                let reuse = if (1..=4).contains(&index) { reuse_mask.ok_or_else(|| storage.missing_producer())? } else { 0 };
                let point = if reuse == 0 { BytePoint { x: stream_byte(storage)?, y: stream_byte(storage)? } } else {
                    let previous = storage.initial_point(PointIndex(index as u32 - 1))?;
                    let x = if bit & u16::from(reuse) == 0 { stream_byte(storage)? } else { previous.x }; bit = bit.wrapping_shl(1);
                    let y = if bit & u16::from(reuse) == 0 { stream_byte(storage)? } else { previous.y }; bit = bit.wrapping_shl(1);
                    BytePoint { x, y }
                };
                storage.set_initial_point(PointIndex(index as u32), point)?;
            }
            let values = storage.value_count()?.0;
            for index in 0..values { let value = stream_byte(storage)?; storage.set_initial_value(ScaleIndex(index as u32), value)?; }
            if let Some(fields) = transform {
                if fields.coverage.0 != 128 {
                    for index in 0..values {
                        let index = ScaleIndex(index as u32);
                        let product = i32::from(storage.initial_value(index)?).wrapping_mul(i32::from(fields.coverage.0));
                        let biased = product.wrapping_add(64);
                        storage.set_initial_value(index, arm_asr(if biased < 0 { product.wrapping_add(191) } else { biased }, 7) as u8)?;
                    }
                }
                for index in 0..points {
                    let index = PointIndex(index as u32); let point = storage.initial_point(index)?;
                    let x = storage.divide(i32::from(fields.scale_x.0).wrapping_mul(i32::from(point.y).wrapping_sub(i32::from(fields.origin_x.0))).wrapping_add(i32::from(fields.denominator_x.0)), i32::from(fields.denominator_x.0))?.wrapping_add(i32::from(fields.x.0));
                    let y = storage.divide(i32::from(fields.scale_y.0).wrapping_mul(i32::from(point.x).wrapping_sub(i32::from(fields.origin_y.0))).wrapping_add(i32::from(fields.denominator_y.0)), i32::from(fields.denominator_y.0))?.wrapping_add(i32::from(fields.y.0));
                    storage.set_initial_point(index, BytePoint { x: y as u8, y: x as u8 })?;
                }
            }
        }
        if adjust_inline {
            if let Some(fields) = transform {
                let (width, height) = storage.dimensions();
                storage.set_inline_origin(super::scanlines::Point {
                    x: Coordinate(width.0.wrapping_mul(i32::from(fields.x.0).wrapping_sub(i32::from(fields.origin_x.0))) / 256),
                    y: Coordinate(height.0.wrapping_mul(i32::from(fields.y.0).wrapping_sub(i32::from(fields.origin_y.0))) / 256) });
            }
        }
        let status = storage.execute_body()?;
        if adjust_inline { storage.set_inline_origin(super::scanlines::Point { x: Coordinate(0), y: Coordinate(0) }); }
        if status != ProgramStatus::Complete { storage.set_active(StreamSelection::PRIMARY); return Ok(status); }
    }
}

const ROW_CAPACITY: usize = 6400;
const RUN_CAPACITY: usize = u16::MAX as usize;
const CURVE_CAPACITY: usize = 80;
const COARSE_CAPACITY: usize = 20;

struct Registers<T: Copy, const N: usize> { values: [MaybeUninit<T>; N], produced: u16 }
impl<T: Copy, const N: usize> Registers<T, N> {
    fn new() -> Self { Self { values: [MaybeUninit::uninit(); N], produced: 0 } }
    fn get(&self, index: usize) -> Result<T, FontError> {
        if index >= N { return Err(FontError::workspace((index + 1) * core::mem::size_of::<T>(), N * core::mem::size_of::<T>())); }
        if self.produced & (1 << index) == 0 { return Err(FontError::UninitializedRasterState); }
        Ok(unsafe { self.values[index].assume_init_read() })
    }
    fn set(&mut self, index: usize, value: T) -> Result<(), FontError> {
        if index >= N { return Err(FontError::workspace((index + 1) * core::mem::size_of::<T>(), N * core::mem::size_of::<T>())); }
        self.values[index].write(value); self.produced |= 1 << index; Ok(())
    }
}
struct EdgeRows { values: [MaybeUninit<EdgeCoordinate>; ROW_CAPACITY], produced: [u64; ROW_CAPACITY / 64] }
impl EdgeRows {
    fn new() -> Self { Self { values: [MaybeUninit::uninit(); ROW_CAPACITY], produced: [0; ROW_CAPACITY / 64] } }
    fn get(&self, index: usize) -> Result<EdgeCoordinate, FontError> {
        if index >= ROW_CAPACITY { return Err(FontError::workspace((index + 1) * 2, ROW_CAPACITY * 2)); }
        if self.produced[index / 64] & (1 << (index % 64)) == 0 { return Err(FontError::UninitializedRasterState); }
        Ok(unsafe { self.values[index].assume_init_read() })
    }
    fn set(&mut self, index: usize, value: EdgeCoordinate) -> Result<(), FontError> {
        if index >= ROW_CAPACITY { return Err(FontError::workspace((index + 1) * 2, ROW_CAPACITY * 2)); }
        self.values[index].write(value); self.produced[index / 64] |= 1 << (index % 64); Ok(())
    }
}
struct CurveStack<T: Copy, const N: usize> { values: [MaybeUninit<T>; N], produced: usize }
impl<T: Copy, const N: usize> CurveStack<T, N> {
    fn new() -> Self { Self { values: [MaybeUninit::uninit(); N], produced: 0 } }
    fn get(&self, index: CurveStackIndex) -> Result<T, FontError> {
        if index.0 >= N { return Err(FontError::workspace((index.0 + 1) * core::mem::size_of::<T>(), N * core::mem::size_of::<T>())); }
        if index.0 >= self.produced { return Err(FontError::UninitializedRasterState); }
        Ok(unsafe { self.values[index.0].assume_init_read() })
    }
    fn set(&mut self, index: CurveStackIndex, value: T) -> Result<(), FontError> {
        if index.0 >= N { return Err(FontError::workspace((index.0 + 1) * core::mem::size_of::<T>(), N * core::mem::size_of::<T>())); }
        if index.0 > self.produced { return Err(FontError::UninitializedRasterState); }
        self.values[index.0].write(value); self.produced = self.produced.max(index.0 + 1); Ok(())
    }
}
pub struct RasterWorkspace {
    vm: vmcore::VmState,
    points: [Registers<vmcore::GridCoordinate, 16>; 2],
    coefficients: [Registers<vmcore::ScaleValue, 8>; 3],
    initial_points: Registers<vmcore::BytePoint, 8>, initial_values: Registers<u8, 8>,
    point_header: bool, value_header: bool, body_produced: bool,
    primary: Option<ByteCursor>, secondary: Option<ByteCursor>, active: StreamSelection,
    scale: Option<vmcore::Scale>, inline_origin: super::scanlines::Point,
    left: EdgeRows, right: EdgeRows, scratch_left: EdgeRows, scratch_right: EdgeRows,
    curves: CurveStack<RasterPoint, CURVE_CAPACITY>, coarse: CurveStack<CoarseCurveNode, COARSE_CAPACITY>,
    heads: [MaybeUninit<RunIndex>; ROW_CAPACITY], tails: [MaybeUninit<RunIndex>; ROW_CAPACITY],
    nodes: [MaybeUninit<RunNode>; RUN_CAPACITY], run_rows: usize, runs_produced: bool,
    allocated: u16, nodes_produced: u16, free: RunIndex,
}
impl RasterWorkspace {
    pub fn new() -> Self {
        Self {
            vm: vmcore::VmState::default(),
            points: [Registers::new(), Registers::new()], coefficients: [Registers::new(), Registers::new(), Registers::new()],
            initial_points: Registers::new(), initial_values: Registers::new(), point_header: false, value_header: false, body_produced: false,
            primary: None, secondary: None, active: StreamSelection::PRIMARY, scale: None,
            inline_origin: super::scanlines::Point { x: Coordinate(0), y: Coordinate(0) },
            left: EdgeRows::new(), right: EdgeRows::new(), scratch_left: EdgeRows::new(), scratch_right: EdgeRows::new(),
            curves: CurveStack::new(), coarse: CurveStack::new(),
            heads: [MaybeUninit::uninit(); ROW_CAPACITY], tails: [MaybeUninit::uninit(); ROW_CAPACITY],
            nodes: [MaybeUninit::uninit(); RUN_CAPACITY], run_rows: 0, runs_produced: false, allocated: 0, nodes_produced: 0, free: RunIndex::NONE,
        }
    }
    fn reset_runs(&mut self, rows: usize) -> Result<(), FontError> {
        if rows > ROW_CAPACITY { return Err(FontError::workspace(rows * 4, ROW_CAPACITY * 4)); }
        for head in &mut self.heads[..rows] { head.write(RunIndex::NONE); }
        self.run_rows = rows; self.allocated = 0; self.nodes_produced = 0; self.free = RunIndex::NONE; self.runs_produced = true; Ok(())
    }
    fn heads(&self) -> Result<&[RunIndex], FontError> {
        if !self.runs_produced { return Err(FontError::UninitializedRasterState); }
        Ok(unsafe { core::slice::from_raw_parts(self.heads.as_ptr() as *const RunIndex, self.run_rows) })
    }
    fn apply_run(&mut self, row: Row, span: Span, erase: bool) -> Result<bool, FontError> {
        if !self.runs_produced { return Err(FontError::UninitializedRasterState); }
        if erase { subtract_run(&mut WorkspaceRuns(self), row, span) } else { merge_run(&mut WorkspaceRuns(self), row, span) }
    }
}
impl Default for RasterWorkspace { fn default() -> Self { Self::new() } }

#[derive(Clone, Copy)]
struct RenderProfile {
    config: FontConfig, rotated: bool, factor: i32, clip: super::scanlines::Point, origin: super::scanlines::Point,
    width: Dimension, height: Dimension, canvas_height: PixelExtent,
    stride: ByteStride, advance: PixelOffset, baseline: PixelOffset,
}
fn checked_divide(numerator: i32, denominator: i32) -> Result<i32, FontError> {
    if denominator == 0 { Err(FontError::DivisionByZero { numerator }) } else { Ok(numerator.wrapping_div(denominator)) }
}
impl RenderProfile {
    fn new(config: FontConfig, effective: FontSize, glyph: GlyphId, metrics: super::outline::GlyphProgramMetrics) -> Result<Self, FontError> {
        if effective.width.get() == 0 || effective.height.get() == 0 || effective.width.get() > 400 || effective.height.get() > 400 { return Err(FontError::InvalidSize); }
        let factor = coverage_factor(config.coverage);
        let hx = ((effective.width.get() as i32).wrapping_mul(i32::from(metrics.horizontal_x.get())) / 1000) as i16;
        let vx = ((effective.height.get() as i32).wrapping_mul(i32::from(metrics.vertical_x.get())) / 1000) as i16;
        let rotated = config.writing == WritingMode::Vertical && glyph.get() < 256;
        let (origin_x, origin_y, extra_x, extra_y) = if rotated {
            (i32::from(vx).wrapping_neg().max(0), i32::from(hx).max(0), i32::from(vx).max(0), i32::from(hx).wrapping_neg().max(0))
        } else { (i32::from(hx).max(0), i32::from(vx).max(0), i32::from(hx).wrapping_neg().max(0), i32::from(vx).wrapping_neg().max(0)) };
        let canvas_width = PixelExtent::new(effective.width.get().checked_add(extra_x as u32).and_then(|value| value.checked_add(u32::from(rotated && config.coverage != CoverageEncoding::PackedMsb1))).ok_or(FontError::InvalidSize)?);
        let canvas_height = PixelExtent::new(effective.height.get().checked_add(extra_y as u32).ok_or(FontError::InvalidSize)?);
        let advance = match config.writing { WritingMode::Horizontal => metrics.horizontal_y, WritingMode::Vertical if rotated => metrics.horizontal_y, WritingMode::Vertical => metrics.vertical_y };
        let extent = match config.writing { WritingMode::Horizontal => effective.width, WritingMode::Vertical => effective.height };
        let advance = if advance.get() == 0 { GlyphAdvance::FullEm } else { GlyphAdvance::PerMille(advance) }.pixels(extent);
        let width = (effective.width.get() as i32).wrapping_mul(factor); let height = (effective.height.get() as i32).wrapping_mul(factor);
        Ok(Self { config, rotated, factor,
            clip: super::scanlines::Point { x: Coordinate(i32::from(hx).wrapping_mul(factor)), y: Coordinate(i32::from(vx).wrapping_mul(factor)) },
            origin: super::scanlines::Point { x: Coordinate(origin_x), y: Coordinate(origin_y) },
            width: Dimension(if rotated { height } else { width }), height: Dimension(if rotated { width } else { height }),
            canvas_height, stride: ByteStride::new(config.coverage.row_bytes(canvas_width)?), advance,
            baseline: PixelOffset::new((config.size.height.get() as i32).wrapping_mul(860).wrapping_add(999) / 1000) })
    }
    fn scanlines(self, count: super::scanlines::RowCount, first: Row) -> super::scanlines::Config {
        super::scanlines::Config {
            clip: self.clip, origin: self.origin, width: self.width, height: self.height,
            style: super::scanlines::Style::Paint, expansion: super::scanlines::Expansion(0), slant: super::scanlines::Slant::None,
            orientation: if self.rotated { super::scanlines::Orientation::Rotate } else { super::scanlines::Orientation::Normal },
            output: if self.config.coverage == CoverageEncoding::PackedMsb1 { super::scanlines::Output::Bitmap } else { super::scanlines::Output::Runs },
            stride: super::scanlines::Stride(self.stride.get() as i32), count, first_row: first, run_first_row: Row(0),
        }
    }
}

struct RenderStorage<'resource, 'asset, 'work, 'pixels> {
    resource: &'resource super::outline::FdlResource<'asset>, workspace: &'work mut RasterWorkspace,
    pixels: &'pixels mut [u8], profile: RenderProfile, bounds: super::scanlines::Bounds,
}
impl AssetBytes for RenderStorage<'_, '_, '_, '_> {
    type Error = FontError;
    fn byte_at(&self, position: vmcore::ProgramOffset) -> Result<u8, FontError> {
        let index = usize::try_from(position.0).map_err(|_| FontError::Truncated)?;
        self.resource.bytes().get(index).copied().ok_or(FontError::Truncated)
    }
}
fn program_cursor(span: super::outline::ProgramSpan<'_>) -> ByteCursor {
    ByteCursor { position: vmcore::ProgramOffset(span.offset().get() as i32), remaining: vmcore::Remaining(span.bytes().len() as i32) }
}
impl ProgramStorage for RenderStorage<'_, '_, '_, '_> {
    fn active(&self) -> StreamSelection { self.workspace.active }
    fn set_active(&mut self, stream: StreamSelection) { self.workspace.active = stream; }
    fn stream(&self, stream: ProgramStream) -> Result<ByteCursor, FontError> {
        match stream { ProgramStream::Primary => self.workspace.primary, ProgramStream::Secondary => self.workspace.secondary }.ok_or(FontError::UninitializedRasterState)
    }
    fn set_stream(&mut self, stream: ProgramStream, cursor: ByteCursor) {
        match stream { ProgramStream::Primary => self.workspace.primary = Some(cursor), ProgramStream::Secondary => self.workspace.secondary = Some(cursor) }
    }
    fn read_selected(&mut self, count: ProgramByteCount) -> Result<u32, FontError> {
        if count.0 > self.stream(self.active().stream())?.remaining.0 { return Err(FontError::Truncated); }
        read_selected_cursor(self, count)
    }
    fn subroutine(&self, id: SubroutineId) -> Result<Option<ByteCursor>, FontError> {
        let span = self.resource.subroutine_directory(GlyphId::new(id.0))?;
        if span.bytes().is_empty() { Ok(None) } else if span.bytes().len() < 2 { Err(FontError::Truncated) } else { Ok(Some(program_cursor(span))) }
    }
    fn compound(&self, id: CompoundId) -> Result<Option<ByteCursor>, FontError> {
        let span = self.resource.compound_directory(GlyphId::new(id.0))?;
        if span.bytes().is_empty() { Ok(None) } else if span.bytes().len() < 4 { Err(FontError::Truncated) } else { Ok(Some(program_cursor(span))) }
    }
    fn point_count(&self) -> Result<vmcore::PointCount, FontError> { if self.workspace.point_header { Ok(self.workspace.vm.point_count) } else { Err(FontError::UninitializedRasterState) } }
    fn set_point_count(&mut self, count: vmcore::PointCount) { self.workspace.vm.point_count = count; self.workspace.point_header = true; }
    fn value_count(&self) -> Result<vmcore::ValueCount, FontError> { if self.workspace.value_header { Ok(self.workspace.vm.value_count) } else { Err(FontError::UninitializedRasterState) } }
    fn set_value_count(&mut self, count: vmcore::ValueCount) { self.workspace.vm.value_count = count; self.workspace.value_header = true; }
    fn initial_point(&self, index: vmcore::PointIndex) -> Result<vmcore::BytePoint, FontError> { self.workspace.initial_points.get(index.0 as usize) }
    fn set_initial_point(&mut self, index: vmcore::PointIndex, point: vmcore::BytePoint) -> Result<(), FontError> { self.workspace.initial_points.set(index.0 as usize, point) }
    fn initial_value(&self, index: vmcore::ScaleIndex) -> Result<u8, FontError> { self.workspace.initial_values.get(index.0 as usize) }
    fn set_initial_value(&mut self, index: vmcore::ScaleIndex, value: u8) -> Result<(), FontError> { self.workspace.initial_values.set(index.0 as usize, value) }
    fn body(&self) -> Result<ByteCursor, FontError> {
        if !self.workspace.body_produced { return Err(FontError::UninitializedRasterState); }
        Ok(ByteCursor { position: self.workspace.vm.cursor, remaining: self.workspace.vm.remaining })
    }
    fn set_body(&mut self, body: ByteCursor) { self.workspace.vm.cursor = body.position; self.workspace.vm.remaining = body.remaining; self.workspace.body_produced = true; }
    fn dimensions(&self) -> (Dimension, Dimension) { (self.profile.width, self.profile.height) }
    fn set_inline_origin(&mut self, origin: super::scanlines::Point) { self.workspace.inline_origin = origin; }
    fn divide(&mut self, numerator: i32, denominator: i32) -> Result<i32, FontError> { checked_divide(numerator, denominator) }
    fn execute_body(&mut self) -> Result<ProgramStatus, FontError> {
        if !self.workspace.body_produced || !self.workspace.point_header || !self.workspace.value_header { return Err(FontError::UninitializedRasterState); }
        Ok(match vmcore::execute(self, vmcore::RasterMode::SCANLINES)? { vmcore::ExecuteStatus::Complete => ProgramStatus::Complete, vmcore::ExecuteStatus::InvalidOpcode => ProgramStatus::InvalidOpcode, vmcore::ExecuteStatus::RunCapacity => ProgramStatus::RunCapacity })
    }
    fn missing_producer(&self) -> FontError { FontError::UninitializedRasterState }
}

impl RenderStorage<'_, '_, '_, '_> {
    fn output_index(offset: vmcore::OutputOffset) -> Result<usize, FontError> {
        if offset.0 < 0 || offset.0 & 1 != 0 { Err(FontError::InvalidBitmap) } else { Ok(offset.0 as usize / 2) }
    }
    fn read_body(&mut self, count: i32) -> Result<u32, FontError> {
        let mut cursor = ProgramStorage::body(self)?;
        if count > cursor.remaining.0 { return Err(FontError::Truncated); }
        let value = read_count(self, &mut cursor, ProgramByteCount(count))?;
        ProgramStorage::set_body(self, cursor); Ok(value)
    }
    fn geometry_grid(&self) -> RasterGrid {
        let state = &self.workspace.vm;
        RasterGrid { fraction_bits: state.shift.0 as u32, mask: state.mask.0 as u32,
            step: FixedCoordinate(state.unit.0), sample: FixedCoordinate(state.scan_half.0),
            last_row: Row(state.height.0), tolerance: FixedCoordinate(state.scan_step.0) }
    }
    fn draw_curve(&mut self) -> Result<(), FontError> {
        if self.workspace.scale.is_none() { return Err(FontError::UninitializedRasterState); }
        let state = &self.workspace.vm;
        let mut points = [RasterPoint::new(state.start_y.0, state.start_x.0), RasterPoint::new(state.control_y.0, state.control_x.0), RasterPoint::new(state.end_y.0, state.end_x.0)];
        let shift = state.shift.0 as u32;
        let scale = if state.small_scale { CurveScale::Unscaled } else { CurveScale::Dimensions { width: Dimension(state.width.0), height: Dimension(state.height.0) } };
        transform_curve(&mut points, scale, self.workspace.inline_origin, shift);
        self.workspace.vm.start_x = vmcore::GridCoordinate(points[0].y.0); self.workspace.vm.start_y = vmcore::GridCoordinate(points[0].x.0);
        self.workspace.vm.control_x = vmcore::GridCoordinate(points[1].y.0); self.workspace.vm.control_y = vmcore::GridCoordinate(points[1].x.0);
        self.workspace.vm.end_x = vmcore::GridCoordinate(points[2].y.0); self.workspace.vm.end_y = vmcore::GridCoordinate(points[2].x.0);
        let side = if self.workspace.vm.edge_side { EdgeSide::Left } else { EdgeSide::Right };
        let mode = self.workspace.vm.mode.0;
        if mode == 2 {
            let mut state = EdgeState { first_row: Row(self.workspace.vm.minimum_row.0), last_left: Row(self.workspace.vm.left_maximum_row.0), last_right: Row(self.workspace.vm.right_maximum_row.0) };
            let kind = if self.workspace.vm.has_control { CurveKind::Quadratic } else { CurveKind::Line };
            draw_edge_curve(self.geometry_grid(), &mut state, side, kind, points, self)?;
            self.workspace.vm.minimum_row = vmcore::Row(state.first_row.0);
            self.workspace.vm.left_maximum_row = vmcore::Row(state.last_left.0); self.workspace.vm.right_maximum_row = vmcore::Row(state.last_right.0);
        } else if mode == 0 {
            let present = match side { EdgeSide::Left => self.workspace.vm.left_state, EdgeSide::Right => self.workspace.vm.right_state };
            let point = if present { Some(match side {
                EdgeSide::Left => RasterPoint::new(self.workspace.vm.finish_x.0, self.workspace.vm.left_y.0),
                EdgeSide::Right => RasterPoint::new(self.workspace.vm.finish_y.0, self.workspace.vm.right_y.0),
            }) } else { None };
            let mut cached = CachedCurve { point, first_row: Row(self.workspace.vm.first_row.0) };
            draw_cached_curve(&mut points, side, shift, self.workspace.vm.half_shift.0 as u32, FixedCoordinate(self.workspace.vm.half_unit.0), &mut cached, self)?;
            let point = cached.point.ok_or(FontError::UninitializedRasterState)?;
            match side {
                EdgeSide::Left => { self.workspace.vm.left_state = true; self.workspace.vm.finish_x = vmcore::GridCoordinate(point.x.0); self.workspace.vm.left_y = vmcore::GridCoordinate(point.y.0); }
                EdgeSide::Right => { self.workspace.vm.right_state = true; self.workspace.vm.finish_y = vmcore::GridCoordinate(point.x.0); self.workspace.vm.right_y = vmcore::GridCoordinate(point.y.0); }
            }
            self.workspace.vm.first_row = vmcore::Row(cached.first_row.0);
            self.workspace.vm.start_x = vmcore::GridCoordinate(points[0].y.0); self.workspace.vm.start_y = vmcore::GridCoordinate(points[0].x.0);
            self.workspace.vm.control_x = vmcore::GridCoordinate(points[1].y.0); self.workspace.vm.control_y = vmcore::GridCoordinate(points[1].x.0);
            self.workspace.vm.end_x = vmcore::GridCoordinate(points[2].y.0); self.workspace.vm.end_y = vmcore::GridCoordinate(points[2].x.0);
        } else {
            let (output, cursor) = match side { EdgeSide::Left => (vmcore::Side::Minimum, self.workspace.vm.left_cursor), EdgeSide::Right => (vmcore::Side::Maximum, self.workspace.vm.right_cursor) };
            match side { EdgeSide::Left => self.workspace.vm.left_cursor.0 = cursor.0.wrapping_add(12), EdgeSide::Right => self.workspace.vm.right_cursor.0 = cursor.0.wrapping_add(12) }
            for (index, point) in points.into_iter().enumerate() {
                vmcore::Storage::set_output_half(self, output, vmcore::OutputOffset(cursor.0.wrapping_add(index as i32 * 4)), arm_asr(point.x.0, shift) as i16)?;
                vmcore::Storage::set_output_half(self, output, vmcore::OutputOffset(cursor.0.wrapping_add(index as i32 * 4 + 2)), arm_asr(point.y.0, shift) as i16)?;
            }
        }
        Ok(())
    }
}
impl vmcore::Storage for RenderStorage<'_, '_, '_, '_> {
    type Error = FontError;
    type State = vmcore::VmState;
    fn state(&self) -> &Self::State { &self.workspace.vm }
    fn state_mut(&mut self) -> &mut Self::State { &mut self.workspace.vm }
    fn read_byte(&mut self) -> Result<u8, FontError> { Ok(self.read_body(1)? as u8) }
    fn read_short(&mut self) -> Result<i16, FontError> { Ok(self.read_body(2)? as i16) }
    fn point(&self, axis: vmcore::Axis, index: vmcore::PointIndex) -> Result<vmcore::GridCoordinate, FontError> {
        self.workspace.points[match axis { vmcore::Axis::X => 0, vmcore::Axis::Y => 1 }].get(index.0 as usize)
    }
    fn set_point(&mut self, axis: vmcore::Axis, index: vmcore::PointIndex, value: vmcore::GridCoordinate) -> Result<(), FontError> {
        self.workspace.points[match axis { vmcore::Axis::X => 0, vmcore::Axis::Y => 1 }].set(index.0 as usize, value)
    }
    fn coefficient(&self, bank: vmcore::CoefficientBank, index: vmcore::ScaleIndex) -> Result<vmcore::ScaleValue, FontError> {
        self.workspace.coefficients[match bank { vmcore::CoefficientBank::Values => 0, vmcore::CoefficientBank::X => 1, vmcore::CoefficientBank::Y => 2 }].get(index.0 as usize)
    }
    fn set_coefficient(&mut self, bank: vmcore::CoefficientBank, index: vmcore::ScaleIndex, value: vmcore::ScaleValue) -> Result<(), FontError> {
        self.workspace.coefficients[match bank { vmcore::CoefficientBank::Values => 0, vmcore::CoefficientBank::X => 1, vmcore::CoefficientBank::Y => 2 }].set(index.0 as usize, value)
    }
    fn initial_half(&self, index: vmcore::ScaleIndex) -> Result<u16, FontError> {
        self.resource.default_values().get(index.0 as usize).copied().ok_or(FontError::workspace((index.0 as usize + 1) * 2, 16))
    }
    fn initial_value(&self, index: vmcore::ScaleIndex) -> Result<u8, FontError> { self.workspace.initial_values.get(index.0 as usize) }
    fn initial_point(&self, index: vmcore::PointIndex) -> Result<vmcore::BytePoint, FontError> { self.workspace.initial_points.get(index.0 as usize) }
    fn output_half(&self, side: vmcore::Side, offset: vmcore::OutputOffset) -> Result<i16, FontError> {
        let index = Self::output_index(offset)?;
        Ok(match side { vmcore::Side::Minimum => self.workspace.left.get(index)?, vmcore::Side::Maximum => self.workspace.right.get(index)? }.0)
    }
    fn set_output_half(&mut self, side: vmcore::Side, offset: vmcore::OutputOffset, value: i16) -> Result<(), FontError> {
        let index = Self::output_index(offset)?;
        match side { vmcore::Side::Minimum => self.workspace.left.set(index, EdgeCoordinate(value)), vmcore::Side::Maximum => self.workspace.right.set(index, EdgeCoordinate(value)) }
    }
    fn set_output_pair(&mut self, side: vmcore::Side, offset: vmcore::OutputOffset, value: vmcore::RowPair) -> Result<(), FontError> {
        self.set_output_half(side, offset, value.first)?; self.set_output_half(side, vmcore::OutputOffset(offset.0.wrapping_add(2)), value.second)
    }
    fn select_scale(&mut self, scale: vmcore::Scale) { self.workspace.scale = Some(scale); }
    fn draw_curve(&mut self) -> Result<(), FontError> { RenderStorage::draw_curve(self) }
    fn flush_scanlines(&mut self) -> Result<vmcore::FlushStatus, FontError> {
        let config = self.profile.scanlines(self.workspace.vm.row_count, self.workspace.vm.first_row);
        Ok(match super::scanlines::render(config, self)? { super::scanlines::RenderStatus::Complete => vmcore::FlushStatus::Complete, super::scanlines::RenderStatus::RunCapacity => vmcore::FlushStatus::RunCapacity })
    }
    fn contour_rows(&mut self) -> Result<super::scanlines::RowCount, FontError> {
        Self::output_index(self.workspace.vm.left_cursor)?; Self::output_index(self.workspace.vm.right_cursor)?;
        if self.workspace.vm.left_cursor.0 as usize > ROW_CAPACITY * 2 || self.workspace.vm.right_cursor.0 as usize > ROW_CAPACITY * 2 { return Err(FontError::workspace((self.workspace.vm.left_cursor.0.max(self.workspace.vm.right_cursor.0) as usize).saturating_add(2), ROW_CAPACITY * 2)); }
        Ok(super::scanlines::RowCount(self.workspace.vm.left_cursor.0 / 2))
    }
}
impl GeometryStorage for RenderStorage<'_, '_, '_, '_> {
    type Error = FontError;
    fn edge(&self, side: EdgeSide, index: Row) -> Result<EdgeCoordinate, FontError> {
        let (side, cursor) = match side { EdgeSide::Left => (vmcore::Side::Minimum, self.workspace.vm.left_cursor), EdgeSide::Right => (vmcore::Side::Maximum, self.workspace.vm.right_cursor) };
        Ok(EdgeCoordinate(vmcore::Storage::output_half(self, side, vmcore::OutputOffset(cursor.0.wrapping_add(index.0.wrapping_mul(2))))?))
    }
    fn set_edge(&mut self, side: EdgeSide, index: Row, value: EdgeCoordinate) -> Result<(), FontError> {
        let (side, cursor) = match side { EdgeSide::Left => (vmcore::Side::Minimum, self.workspace.vm.left_cursor), EdgeSide::Right => (vmcore::Side::Maximum, self.workspace.vm.right_cursor) };
        vmcore::Storage::set_output_half(self, side, vmcore::OutputOffset(cursor.0.wrapping_add(index.0.wrapping_mul(2))), value.0)
    }
    fn curve_point(&self, index: CurveStackIndex) -> Result<RasterPoint, FontError> { self.workspace.curves.get(index) }
    fn set_curve_point(&mut self, index: CurveStackIndex, point: RasterPoint) -> Result<(), FontError> { self.workspace.curves.set(index, point) }
}
impl CachedCurveStorage for RenderStorage<'_, '_, '_, '_> {
    type Error = FontError;
    fn emit(&mut self, value: EdgeCoordinate) -> Result<(), FontError> {
        let (side, offset) = if self.workspace.vm.edge_side { (vmcore::Side::Minimum, self.workspace.vm.left_cursor) } else { (vmcore::Side::Maximum, self.workspace.vm.right_cursor) };
        vmcore::Storage::set_output_half(self, side, offset, value.0)?;
        if self.workspace.vm.edge_side { self.workspace.vm.left_cursor.0 = offset.0.wrapping_add(2); } else { self.workspace.vm.right_cursor.0 = offset.0.wrapping_add(2); }
        Ok(())
    }
    fn node(&self, index: CurveStackIndex) -> Result<CoarseCurveNode, FontError> { self.workspace.coarse.get(index) }
    fn set_node(&mut self, index: CurveStackIndex, node: CoarseCurveNode) -> Result<(), FontError> { self.workspace.coarse.set(index, node) }
}

impl super::scanlines::BitmapStorage for RenderStorage<'_, '_, '_, '_> {
    type Error = FontError;
    fn bitmap_byte(&self, offset: super::scanlines::BitmapOffset) -> Result<u8, FontError> {
        let index = usize::try_from(offset.0).map_err(|_| FontError::InvalidBitmap)?;
        self.pixels.get(index).copied().ok_or(FontError::workspace(index.saturating_add(1), self.pixels.len()))
    }
    fn set_bitmap_byte(&mut self, offset: super::scanlines::BitmapOffset, value: u8) -> Result<(), FontError> {
        let index = usize::try_from(offset.0).map_err(|_| FontError::InvalidBitmap)?;
        let length = self.pixels.len();
        *self.pixels.get_mut(index).ok_or(FontError::workspace(index.saturating_add(1), length))? = value; Ok(())
    }
    fn bounds(&self) -> super::scanlines::Bounds { self.bounds }
    fn set_bounds(&mut self, bounds: super::scanlines::Bounds) { self.bounds = bounds; }
}
fn row_index(row: Row) -> Result<usize, FontError> { usize::try_from(row.0).map_err(|_| FontError::InvalidBitmap) }
impl super::scanlines::Storage for RenderStorage<'_, '_, '_, '_> {
    fn input(&self, row: Row) -> Result<Span, FontError> {
        let row = row_index(row)?;
        Ok(Span::new(i32::from(self.workspace.left.get(row)?.0), i32::from(self.workspace.right.get(row)?.0)))
    }
    fn set_input(&mut self, row: Row, span: Span) -> Result<(), FontError> {
        let row = row_index(row)?;
        self.workspace.left.set(row, EdgeCoordinate(span.first.0 as i16))?; self.workspace.right.set(row, EdgeCoordinate(span.last.0 as i16))
    }
    fn scratch(&self, row: Row) -> Result<Span, FontError> {
        let row = row_index(row)?;
        Ok(Span::new(i32::from(self.workspace.scratch_left.get(row)?.0), i32::from(self.workspace.scratch_right.get(row)?.0)))
    }
    fn set_scratch(&mut self, row: Row, span: Span) -> Result<(), FontError> {
        let row = row_index(row)?;
        self.workspace.scratch_left.set(row, EdgeCoordinate(span.first.0 as i16))?; self.workspace.scratch_right.set(row, EdgeCoordinate(span.last.0 as i16))
    }
    fn merge(&mut self, row: Row, span: Span) -> Result<bool, FontError> { self.workspace.apply_run(row, span, false) }
    fn subtract(&mut self, row: Row, span: Span) -> Result<bool, FontError> { self.workspace.apply_run(row, span, true) }
    fn fill_span(&mut self, row: Row, span: Span) -> Result<(), FontError> {
        super::scanlines::paint_span(self.profile.scanlines(self.workspace.vm.row_count, self.workspace.vm.first_row), self, row, span)
    }
    fn divide(&mut self, numerator: i32, denominator: i32) -> Result<i32, FontError> { checked_divide(numerator, denominator) }
}
impl CoverageStorage for RenderStorage<'_, '_, '_, '_> {
    fn head(&self, row: Row) -> Result<RunIndex, FontError> {
        let heads = self.workspace.heads()?;
        if row.0 < 0 || row.0 as usize >= heads.len() { Ok(RunIndex::NONE) } else { Ok(heads[row.0 as usize]) }
    }
    fn run(&self, index: RunIndex) -> Result<RunNode, FontError> {
        if !self.workspace.runs_produced { return Err(FontError::UninitializedRasterState); }
        if index.0 >= self.workspace.nodes_produced { return Err(FontError::UninitializedRasterState); }
        Ok(unsafe { self.workspace.nodes[usize::from(index.0)].assume_init_read() })
    }
    fn alignment(&self, offset: super::scanlines::BitmapOffset) -> u8 { ((self.pixels.as_ptr() as usize).wrapping_add(offset.0 as usize) & 3) as u8 }
    fn lookup(&self, value: u8) -> Result<u8, FontError> { Ok(value) }
}

pub fn render_glyph<'pixels>(resource: &super::outline::FdlResource<'_>, glyph: GlyphId, config: FontConfig, workspace: &mut RasterWorkspace, pixels: &'pixels mut [u8]) -> Result<GlyphMask<'pixels>, FontError> {
    let mut effective = config.size;
    if let Some(scale) = resource.glyph_scale(glyph)? {
        let denominator = i32::from(scale.get());
        let numerator = (config.size.height.get() as i32).wrapping_mul(1000).wrapping_sub(1).wrapping_add(denominator);
        let extent = checked_divide(numerator, denominator)?;
        if extent < 0 { return Err(FontError::InvalidSize); }
        effective = FontSize { width: PixelExtent::new(extent as u32), height: PixelExtent::new(extent as u32) };
    }
    let program = resource.glyph_program(glyph, config.writing)?;
    let profile = RenderProfile::new(config, effective, glyph, program.metrics)?;
    let required = profile.stride.get().checked_mul(profile.canvas_height.get() as usize).ok_or(FontError::InvalidSize)?;
    if pixels.len() < required { return Err(FontError::workspace(required, pixels.len())); }
    workspace.vm.width = vmcore::Dimension(profile.width.0); workspace.vm.height = vmcore::Dimension(profile.height.0);
    workspace.vm.oversample = false;
    workspace.primary = Some(program_cursor(program.program));
    if config.coverage != CoverageEncoding::PackedMsb1 { workspace.reset_runs(profile.height.0 as usize)?; }
    pixels[..required].fill(0);
    let bounds = super::scanlines::Bounds {
        minimum: super::scanlines::Point { x: Coordinate(32767), y: Coordinate(32767) },
        maximum: super::scanlines::Point { x: Coordinate(0), y: Coordinate(0) },
    };
    let bounds = {
        let mut storage = RenderStorage { resource, workspace, pixels: &mut pixels[..required], profile, bounds };
        match render_program(&mut storage, None, StreamSelection::PRIMARY)? {
            ProgramStatus::Complete => {}
            ProgramStatus::InvalidOpcode => return Err(FontError::InvalidInstruction(storage.workspace.vm.operand)),
            ProgramStatus::RunCapacity => return Err(FontError::workspace((RUN_CAPACITY + 1) * 6, RUN_CAPACITY * 6)),
        }
        if config.coverage != CoverageEncoding::PackedMsb1 {
            let last = Row(storage.workspace.run_rows as i32 - 1);
            let (first, last) = run_row_bounds(&WorkspaceRuns(&mut *storage.workspace), last)?;
            coverage(CoverageConfig {
                format: if config.coverage == CoverageEncoding::Gray4 { GrayCoverage::Gray4 } else { GrayCoverage::Gray8 },
                orientation: if profile.rotated { super::scanlines::Orientation::Rotate } else { super::scanlines::Orientation::Normal },
                origin: super::scanlines::Point { x: Coordinate(profile.origin.x.0.wrapping_mul(profile.factor)), y: Coordinate(profile.origin.y.0.wrapping_mul(profile.factor)) },
                height: profile.height, stride: super::scanlines::Stride(profile.stride.get() as i32), first_row: first, last_row: last, palette: CoveragePalette::Linear,
            }, &mut storage)?;
        }
        storage.bounds
    };
    crop_mask(pixels, profile, bounds)
}
fn crop_mask<'pixels>(pixels: &'pixels mut [u8], profile: RenderProfile, bounds: super::scanlines::Bounds) -> Result<GlyphMask<'pixels>, FontError> {
    let empty = bounds.maximum.x.0 < bounds.minimum.x.0;
    let (left, top, width, height) = if empty { (0, 0, 0, 0) } else {
        if bounds.minimum.x.0 < 0 || bounds.minimum.y.0 < 0 || bounds.maximum.y.0 < bounds.minimum.y.0 { return Err(FontError::InvalidBitmap); }
        (bounds.minimum.x.0 as usize, bounds.minimum.y.0 as usize,
            bounds.maximum.x.0.checked_sub(bounds.minimum.x.0).and_then(|value| value.checked_add(1)).ok_or(FontError::InvalidSize)? as usize,
            bounds.maximum.y.0.checked_sub(bounds.minimum.y.0).and_then(|value| value.checked_add(1)).ok_or(FontError::InvalidSize)? as usize)
    };
    let width = PixelExtent::new(u32::try_from(width).map_err(|_| FontError::InvalidSize)?);
    let height = PixelExtent::new(u32::try_from(height).map_err(|_| FontError::InvalidSize)?);
    let row_bytes = profile.config.coverage.row_bytes(width)?;
    if row_bytes > profile.stride.get() { return Err(FontError::InvalidBitmap); }
    let column = if profile.config.coverage == CoverageEncoding::PackedMsb1 { left / 8 } else { left };
    let start = top.checked_mul(profile.stride.get()).and_then(|offset| offset.checked_add(column)).ok_or(FontError::InvalidSize)?;
    let needed = if width.get() == 0 || height.get() == 0 { 0 } else { (height.get() as usize - 1).checked_mul(profile.stride.get()).and_then(|prefix| prefix.checked_add(row_bytes)).ok_or(FontError::InvalidSize)? };
    let end = start.checked_add(needed).ok_or(FontError::InvalidSize)?;
    if end > pixels.len() { return Err(FontError::workspace(end, pixels.len())); }
    if profile.config.coverage == CoverageEncoding::PackedMsb1 && left & 7 != 0 {
        let shift = left & 7;
        for row in 0..height.get() as usize {
            let source = start + row * profile.stride.get();
            for index in 0..row_bytes {
                let next = if column + index + 1 < profile.stride.get() { pixels[source + index + 1] } else { 0 };
                pixels[source + index] = (pixels[source + index] << shift) | (next >> (8 - shift));
            }
            if width.get() & 7 != 0 { pixels[source + row_bytes - 1] &= 255u8 << (8 - (width.get() & 7)); }
        }
    }
    let (advance_x, advance_y) = match profile.config.writing { WritingMode::Horizontal => (profile.advance, PixelOffset::new(0)), WritingMode::Vertical => (PixelOffset::new(0), profile.advance) };
    let metrics = GlyphMetrics { width, height, advance_x, advance_y,
        bearing_x: PixelOffset::new((left as i32).wrapping_sub(profile.origin.x.0)),
        bearing_y: PixelOffset::new((top as i32).wrapping_sub(profile.origin.y.0).wrapping_sub(profile.baseline.get())) };
    GlyphMask::from_renderer(&pixels[start..end], width, height, profile.stride, profile.config.coverage, metrics)
}

struct WorkspaceRuns<'a>(&'a mut RasterWorkspace);
impl WorkspaceRuns<'_> {
    fn row(&self, row: Row) -> Result<usize, FontError> {
        if !self.0.runs_produced { return Err(FontError::UninitializedRasterState); }
        let index = row_index(row)?;
        if index >= self.0.run_rows { Err(FontError::InvalidBitmap) } else { Ok(index) }
    }
    fn node_index(&self, index: RunIndex) -> Result<usize, FontError> {
        if !self.0.runs_produced || index.0 >= self.0.nodes_produced { return Err(FontError::UninitializedRasterState); }
        Ok(usize::from(index.0))
    }
}
impl RunStorage for WorkspaceRuns<'_> {
    type Error = FontError;
    fn first_row(&self) -> Row { Row(0) }
    fn row_count(&self) -> super::scanlines::RowCount { super::scanlines::RowCount(self.0.run_rows as i32) }
    fn head(&self, row: Row) -> Result<RunIndex, FontError> { Ok(unsafe { self.0.heads[self.row(row)?].assume_init_read() }) }
    fn set_head(&mut self, row: Row, index: RunIndex) -> Result<(), FontError> { let row = self.row(row)?; self.0.heads[row].write(index); Ok(()) }
    fn tail(&self, row: Row) -> Result<RunIndex, FontError> { Ok(unsafe { self.0.tails[self.row(row)?].assume_init_read() }) }
    fn set_tail(&mut self, row: Row, index: RunIndex) -> Result<(), FontError> { let row = self.row(row)?; self.0.tails[row].write(index); Ok(()) }
    fn node(&self, index: RunIndex) -> Result<RunNode, FontError> { Ok(unsafe { self.0.nodes[self.node_index(index)?].assume_init_read() }) }
    fn initialize_node(&mut self, index: RunIndex, node: RunNode) -> Result<(), FontError> {
        if usize::from(index.0) >= RUN_CAPACITY { return Err(FontError::workspace((usize::from(index.0) + 1) * 6, RUN_CAPACITY * 6)); }
        if index.0 > self.0.nodes_produced { return Err(FontError::UninitializedRasterState); }
        self.0.nodes[usize::from(index.0)].write(node); self.0.nodes_produced = self.0.nodes_produced.max(index.0.wrapping_add(1)); Ok(())
    }
    fn set_first(&mut self, index: RunIndex, value: EdgeCoordinate) -> Result<(), FontError> { let index = self.node_index(index)?; unsafe { self.0.nodes[index].assume_init_mut() }.first = value; Ok(()) }
    fn set_last(&mut self, index: RunIndex, value: EdgeCoordinate) -> Result<(), FontError> { let index = self.node_index(index)?; unsafe { self.0.nodes[index].assume_init_mut() }.last = value; Ok(()) }
    fn set_next(&mut self, index: RunIndex, next: RunIndex) -> Result<(), FontError> { let index = self.node_index(index)?; unsafe { self.0.nodes[index].assume_init_mut() }.next = next; Ok(()) }
    fn capacity(&self) -> u16 { RUN_CAPACITY as u16 }
    fn allocated(&self) -> u16 { self.0.allocated }
    fn set_allocated(&mut self, count: u16) { self.0.allocated = count; }
    fn free(&self) -> RunIndex { self.0.free }
    fn set_free(&mut self, index: RunIndex) { self.0.free = index; }
}

pub const fn coverage_factor(encoding: CoverageEncoding) -> i32 { match encoding { CoverageEncoding::PackedMsb1 => 1, CoverageEncoding::Gray4 => 4, CoverageEncoding::Gray8 => 16 } }
#[derive(Clone, Copy)]
pub enum UnderlineExpansion { None, Symmetric, Horizontal, Vertical, Both }
#[derive(Clone, Copy)]
pub struct UnderlineConfig {
    pub width: Dimension, pub height: Dimension, pub thickness: Dimension, pub position: Coordinate, pub extra: Dimension,
    pub horizontal_advance: super::AdvancePerMille, pub vertical_advance: super::AdvancePerMille,
    pub writing: WritingMode, pub orientation: super::scanlines::Orientation, pub encoding: CoverageEncoding,
    pub expansion: UnderlineExpansion, pub weight: Dimension,
}
pub trait SpanSink {
    type Error;
    fn paint(&mut self, row: Row, span: Span) -> Result<(), Self::Error>;
    fn merge(&mut self, row: Row, span: Span) -> Result<bool, Self::Error>;
}
pub fn underline<T: SpanSink>(config: UnderlineConfig, sink: &mut T) -> Result<(), T::Error> {
    if config.thickness.0 <= 0 { return Ok(()); }
    let vertical = config.writing == WritingMode::Vertical;
    let rotated = config.orientation == super::scanlines::Orientation::Rotate;
    let metric = if !vertical || rotated { config.horizontal_advance } else { config.vertical_advance };
    let nominal = if vertical { config.height.0 } else { config.width.0 };
    let advance = if metric.get() == 0 { nominal as i16 } else { (nominal.wrapping_mul(i32::from(metric.get())).wrapping_add(999) / 1000) as i16 };
    let mut extent = i32::from(advance).wrapping_add(config.extra.0); let mut begin = 0;
    match config.expansion {
        UnderlineExpansion::Symmetric => { extent = extent.wrapping_add(config.weight.0); begin = config.weight.0.wrapping_neg(); }
        UnderlineExpansion::Horizontal if !vertical => extent = extent.wrapping_add(config.weight.0),
        UnderlineExpansion::Vertical if vertical => extent = extent.wrapping_add(config.weight.0),
        UnderlineExpansion::Both => extent = extent.wrapping_add(config.weight.0),
        _ => {}
    }
    let factor = coverage_factor(config.encoding);
    let (first_row, last_row, first, last) = if !vertical {
        (factor.wrapping_mul(config.height.0.wrapping_sub(config.position.0).wrapping_sub(config.thickness.0)),
            factor.wrapping_mul(config.height.0.wrapping_sub(config.position.0)).wrapping_sub(1), factor.wrapping_mul(begin), factor.wrapping_mul(extent).wrapping_sub(1))
    } else if rotated {
        (factor.wrapping_mul(config.position.0), factor.wrapping_mul(config.position.0.wrapping_add(config.thickness.0)).wrapping_sub(1),
            factor.wrapping_mul(begin), factor.wrapping_mul(extent).wrapping_sub(1))
    } else {
        (factor.wrapping_mul(begin), factor.wrapping_mul(extent).wrapping_sub(1),
            factor.wrapping_mul(config.width.0.wrapping_sub(config.position.0).wrapping_sub(config.thickness.0)),
            factor.wrapping_mul(config.width.0.wrapping_sub(config.position.0)).wrapping_sub(1))
    };
    for row in first_row..=last_row {
        let span = Span::new(first, last);
        if config.encoding == CoverageEncoding::PackedMsb1 { sink.paint(Row(row), span)?; } else { let _ = sink.merge(Row(row), span)?; }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{FontAsset, FontEngine, FontWorkspace};

    const GLYPH: GlyphId = GlyphId::new(65);
    const RECTANGLE: &[u8] = &[0xfa, 0, 14, 0, 0x90, 1, 0, 0, 0xff, 0, 0x94, 0x23, 0, 0xff, 0xff, 0xff, 0x71, 0];

    fn font(program: &[u8], advance: i16) -> [u8; 0x300] {
        let mut bytes = [0; 0x300];
        bytes[..8].copy_from_slice(b"FDL Font");
        bytes[0x1c..0x1e].copy_from_slice(&66u16.to_le_bytes());
        bytes[0x2e..0x32].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x32..0x36].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[0x36..0x38].copy_from_slice(&4u16.to_le_bytes());
        bytes[0x38..0x3a].copy_from_slice(&0x100u16.to_le_bytes());
        bytes[0x80 + 65 * 4..0x80 + 66 * 4].copy_from_slice(&0x01000000u32.to_le_bytes());
        bytes[0x80 + 66 * 4..0x80 + 67 * 4].copy_from_slice(&(6u32 + program.len() as u32).to_le_bytes());
        bytes[0x202..0x204].copy_from_slice(&advance.to_le_bytes());
        bytes[0x204..0x206].copy_from_slice(&(program.len() as u16).to_le_bytes());
        bytes[0x206..0x206 + program.len()].copy_from_slice(program);
        bytes
    }
    fn config(size: u32, coverage: CoverageEncoding) -> FontConfig {
        FontConfig { size: FontSize { width: PixelExtent::new(size), height: PixelExtent::new(size) }, coverage, writing: super::super::WritingMode::Horizontal }
    }

    #[test]
    fn constructed_font_produces_msb_first_mask_and_signed_metrics() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 8];
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(8, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
        let mask = engine.glyph_mask(GLYPH).unwrap();
        assert_eq!(mask.pixels(), &[0xff; 8]);
        assert_eq!((mask.width().get(), mask.height().get(), mask.stride().get()), (8, 8, 1));
        assert_eq!((mask.metrics().advance_x.get(), mask.metrics().advance_y.get()), (4, 0));
        assert_eq!((mask.metrics().bearing_x.get(), mask.metrics().bearing_y.get()), (0, -7));
    }

    #[test]
    fn constructed_font_produces_gray4_samples() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 64];
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(8, CoverageEncoding::Gray4), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
        let mask = engine.glyph_mask(GLYPH).unwrap();
        assert_eq!(mask.pixels(), &[15; 64]);
        assert_eq!((mask.width().get(), mask.height().get(), mask.stride().get()), (8, 8, 8));
    }

    #[test]
    fn constructed_small_gray8_glyph_uses_scalar_coverage() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 16];
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(4, CoverageEncoding::Gray8), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
        let mask = engine.glyph_mask(GLYPH).unwrap();
        assert_eq!(mask.pixels(), &[255; 16]);
        assert_eq!((mask.width().get(), mask.height().get()), (4, 4));
    }

    #[test]
    fn fresh_one_pixel_glyph_reports_missing_row_producer_without_touching_guards() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 3];
        {
            let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(1, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels[1..2])).unwrap();
            assert!(matches!(engine.glyph_mask(GLYPH), Err(FontError::UninitializedRasterState)));
        }
        assert_eq!(pixels, [0xa5, 0, 0xa5]);
    }

    #[test]
    fn aligned_gray8_interior_preserves_original_packed_accumulation() {
        #[repr(align(4))]
        struct Pixels([u8; 1024]);
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = Pixels([0xa5; 1024]);
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(32, CoverageEncoding::Gray8), FontWorkspace::fdl(&mut raster, &mut pixels.0)).unwrap();
        let mask = engine.glyph_mask(GLYPH).unwrap();
        assert_eq!((mask.width().get(), mask.height().get(), mask.stride().get()), (32, 32, 32));
        for row in mask.pixels().chunks_exact(32) {
            assert_eq!(&row[..4], &[255; 4]);
            assert_eq!(&row[4..28], &[240; 24]);
            assert_eq!(&row[28..], &[255; 4]);
        }
    }

    #[test]
    fn produced_rows_persist_across_real_glyph_requests() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 8];
        {
            let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(8, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
            assert_eq!(engine.glyph_mask(GLYPH).unwrap().pixels(), &[0xff; 8]);
        }
        {
            let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(1, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels[..1])).unwrap();
            let mask = engine.glyph_mask(GLYPH).unwrap();
            assert_eq!(mask.pixels(), &[0xff]);
            assert_eq!((mask.width().get(), mask.height().get()), (8, 1));
            assert_eq!((mask.metrics().advance_x.get(), mask.metrics().bearing_y.get()), (1, -1));
        }
    }

    #[test]
    fn insufficient_pixel_storage_is_unchanged() {
        let bytes = font(RECTANGLE, 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 7];
        {
            let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(8, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
            assert!(matches!(engine.glyph_mask(GLYPH), Err(FontError::InsufficientWorkspace { .. })));
        }
        assert_eq!(pixels, [0xa5; 7]);
    }

    #[test]
    fn invalid_body_opcode_is_returned_from_actual_vm() {
        let bytes = font(&[0xfa, 0, 2, 0, 1, 0], 500);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 8];
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), config(8, CoverageEncoding::PackedMsb1), FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
        assert!(matches!(engine.glyph_mask(GLYPH), Err(FontError::InvalidInstruction(opcode)) if opcode.get() == 1));
    }

    #[test]
    fn raw_per_mille_advance_is_not_quantized_through_outline_units() {
        let bytes = font(&[0xfd, 0, 0], 5);
        let mut raster = RasterWorkspace::new();
        let mut pixels = [0xa5; 832];
        let mut requested = config(201, CoverageEncoding::PackedMsb1);
        requested.size.height = PixelExtent::new(32);
        let mut engine = FontEngine::new(FontAsset::parse(&bytes).unwrap(), requested, FontWorkspace::fdl(&mut raster, &mut pixels)).unwrap();
        let mask = engine.glyph_mask(GLYPH).unwrap();
        assert_eq!((mask.width().get(), mask.height().get(), mask.metrics().advance_x.get()), (0, 0, 2));
        assert!(mask.pixels().is_empty());
    }
}
