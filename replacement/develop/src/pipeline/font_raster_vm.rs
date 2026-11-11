use super::font_fdl::signed_quotient;
use crate::develop::font_core::{raster as core_raster, scanlines};
use crate::develop::font_core::raster_vm as vmcore;
use core::convert::Infallible;
use core::ptr::{read_unaligned, read_volatile, write_unaligned, write_volatile};

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterAddress(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterContext(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterVm(pub u32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterCoordinate(pub i32);
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct RasterStatus(pub i32);

unsafe fn w(p: u32) -> i32 { unsafe { read_unaligned(p as *const i32) } }
unsafe fn sw(p: u32, v: i32) { unsafe { write_unaligned(p as *mut i32, v) } }
unsafe fn b(p: u32) -> u8 { unsafe { read_volatile(p as *const u8) } }
unsafe fn sb(p: u32, v: u8) { unsafe { write_volatile(p as *mut u8, v) } }
unsafe fn h(p: u32) -> i16 { unsafe { read_unaligned(p as *const i16) } }
unsafe fn sh(p: u32, v: i16) { unsafe { write_unaligned(p as *mut i16, v) } }
fn add(a: u32, v: i32) -> u32 { a.wrapping_add(v as u32) }

fn infallible<T>(value: Result<T, Infallible>) -> T { match value { Ok(value) => value, Err(error) => match error {} } }

struct AbiGeometry(RasterVm);
impl core_raster::GeometryStorage for AbiGeometry {
    type Error = Infallible;
    fn edge(&self, side: core_raster::EdgeSide, index: scanlines::Row) -> Result<core_raster::EdgeCoordinate, Infallible> {
        unsafe { Ok(core_raster::EdgeCoordinate(h(add(w(self.0.0 + match side { core_raster::EdgeSide::Left => 0x805c, core_raster::EdgeSide::Right => 0x8060 }) as u32, index.0.wrapping_mul(2))))) }
    }
    fn set_edge(&mut self, side: core_raster::EdgeSide, index: scanlines::Row, value: core_raster::EdgeCoordinate) -> Result<(), Infallible> {
        unsafe { sh(add(w(self.0.0 + match side { core_raster::EdgeSide::Left => 0x805c, core_raster::EdgeSide::Right => 0x8060 }) as u32, index.0.wrapping_mul(2)), value.0); }
        Ok(())
    }
    fn curve_point(&self, index: core_raster::CurveStackIndex) -> Result<core_raster::RasterPoint, Infallible> {
        let offset = (index.0 as u32).wrapping_mul(8);
        unsafe { Ok(core_raster::RasterPoint::new(w(self.0.0.wrapping_add(0x8078).wrapping_add(offset)), w(self.0.0.wrapping_add(0x807c).wrapping_add(offset)))) }
    }
    fn set_curve_point(&mut self, index: core_raster::CurveStackIndex, point: core_raster::RasterPoint) -> Result<(), Infallible> {
        let offset = (index.0 as u32).wrapping_mul(8);
        unsafe { sw(self.0.0.wrapping_add(0x8078).wrapping_add(offset), point.x.0); sw(self.0.0.wrapping_add(0x807c).wrapping_add(offset), point.y.0); }
        Ok(())
    }
}
unsafe fn raster_grid(vm: RasterVm) -> core_raster::RasterGrid {
    unsafe { core_raster::RasterGrid { fraction_bits: w(vm.0 + 0x7eac) as u32, mask: w(vm.0 + 0x7ea8) as u32,
        step: core_raster::FixedCoordinate(w(vm.0 + 0x7ea0)), sample: core_raster::FixedCoordinate(w(vm.0 + 0x8070)),
        last_row: scanlines::Row(w(vm.0 + 0x58)), tolerance: core_raster::FixedCoordinate(w(vm.0 + 0x8074)) } }
}
unsafe fn edge_state(vm: RasterVm) -> core_raster::EdgeState {
    unsafe { core_raster::EdgeState { first_row: scanlines::Row(w(vm.0 + 0x8064)), last_left: scanlines::Row(w(vm.0 + 0x8068)), last_right: scanlines::Row(w(vm.0 + 0x806c)) } }
}
unsafe fn store_edge_state(vm: RasterVm, state: core_raster::EdgeState) {
    unsafe { sw(vm.0 + 0x8064, state.first_row.0); sw(vm.0 + 0x8068, state.last_left.0); sw(vm.0 + 0x806c, state.last_right.0); }
}
unsafe fn edge_side(vm: RasterVm) -> core_raster::EdgeSide { if unsafe { w(vm.0 + 0x8054) } == 0 { core_raster::EdgeSide::Right } else { core_raster::EdgeSide::Left } }

struct AbiCachedCurve { vm: RasterVm, side: core_raster::EdgeSide }
impl core_raster::CachedCurveStorage for AbiCachedCurve {
    type Error = Infallible;
    fn emit(&mut self, value: core_raster::EdgeCoordinate) -> Result<(), Infallible> {
        let slot = match self.side { core_raster::EdgeSide::Left => 0x805c, core_raster::EdgeSide::Right => 0x8060 };
        unsafe { let output = w(self.vm.0 + slot) as u32; sh(output, value.0); sw(self.vm.0 + slot, output.wrapping_add(2) as i32); }
        Ok(())
    }
    fn node(&self, index: core_raster::CurveStackIndex) -> Result<core_raster::CoarseCurveNode, Infallible> {
        let pointer = self.vm.0.wrapping_add(0x7f0c).wrapping_add((index.0 as u32).wrapping_mul(16));
        unsafe { Ok(core_raster::CoarseCurveNode { end: core_raster::RasterPoint::new(w(pointer + 8), w(pointer)), control: core_raster::RasterPoint::new(w(pointer + 12), w(pointer + 4)) }) }
    }
    fn set_node(&mut self, index: core_raster::CurveStackIndex, node: core_raster::CoarseCurveNode) -> Result<(), Infallible> {
        let pointer = self.vm.0.wrapping_add(0x7f0c).wrapping_add((index.0 as u32).wrapping_mul(16));
        unsafe { sw(pointer, node.end.y.0); sw(pointer + 4, node.control.y.0); sw(pointer + 8, node.end.x.0); sw(pointer + 12, node.control.x.0); }
        Ok(())
    }
}
struct AbiBitmap(RasterVm);
impl scanlines::BitmapStorage for AbiBitmap {
    type Error = Infallible;
    fn bitmap_byte(&self, offset: scanlines::BitmapOffset) -> Result<u8, Infallible> { unsafe { Ok(b(add(w(self.0.0 + 0x68) as u32, offset.0))) } }
    fn set_bitmap_byte(&mut self, offset: scanlines::BitmapOffset, value: u8) -> Result<(), Infallible> { unsafe { sb(add(w(self.0.0 + 0x68) as u32, offset.0), value); } Ok(()) }
    fn bounds(&self) -> scanlines::Bounds {
        unsafe { scanlines::Bounds { minimum: scanlines::Point { x: scanlines::Coordinate(w(self.0.0 + 0x74)), y: scanlines::Coordinate(w(self.0.0 + 0x78)) },
            maximum: scanlines::Point { x: scanlines::Coordinate(w(self.0.0 + 0x7c)), y: scanlines::Coordinate(w(self.0.0 + 0x80)) } } }
    }
    fn set_bounds(&mut self, bounds: scanlines::Bounds) {
        unsafe { sw(self.0.0 + 0x74, bounds.minimum.x.0); sw(self.0.0 + 0x78, bounds.minimum.y.0); sw(self.0.0 + 0x7c, bounds.maximum.x.0); sw(self.0.0 + 0x80, bounds.maximum.y.0); }
    }
}
impl core_raster::CoverageStorage for AbiBitmap {
    fn head(&self, row: scanlines::Row) -> Result<core_raster::RunIndex, Infallible> {
        unsafe {
            let table = self.0.0.wrapping_sub(0xb8).wrapping_add(0x83b0);
            let index = row.0.wrapping_sub(i32::from(h(table + 12)));
            if index < 0 || index >= i32::from(h(table + 16) as u16) { return Ok(core_raster::RunIndex::NONE); }
            Ok(core_raster::RunIndex(h(add(w(table + 20) as u32, index.wrapping_mul(2))) as u16))
        }
    }
    fn run(&self, index: core_raster::RunIndex) -> Result<core_raster::RunNode, Infallible> {
        unsafe { let table = self.0.0.wrapping_sub(0xb8).wrapping_add(0x83b0); let node = (w(table + 4) as u32).wrapping_add(u32::from(index.0).wrapping_mul(6));
            Ok(core_raster::RunNode { first: core_raster::EdgeCoordinate(h(node)), last: core_raster::EdgeCoordinate(h(node + 2)), next: core_raster::RunIndex(h(node + 4) as u16) }) }
    }
    fn alignment(&self, offset: scanlines::BitmapOffset) -> u8 { unsafe { add(w(self.0.0 + 0x68) as u32, offset.0) as u8 & 3 } }
    fn lookup(&self, value: u8) -> Result<u8, Infallible> { unsafe { Ok(b(self.0.0.wrapping_sub(0xb8).wrapping_add(0x141).wrapping_add(u32::from(value)))) } }
    fn bitmap_word(&self, offset: scanlines::BitmapOffset) -> Result<u32, Infallible> { unsafe { Ok(w(add(w(self.0.0 + 0x68) as u32, offset.0)) as u32) } }
    fn set_bitmap_word(&mut self, offset: scanlines::BitmapOffset, value: u32) -> Result<(), Infallible> { unsafe { sw(add(w(self.0.0 + 0x68) as u32, offset.0), value as i32); } Ok(()) }
}
unsafe fn paint_config(vm: RasterVm) -> scanlines::Config {
    unsafe { scanlines::Config {
        clip: scanlines::Point { x: scanlines::Coordinate(0), y: scanlines::Coordinate(0) },
        origin: scanlines::Point { x: scanlines::Coordinate(w(vm.0 + 0x4c)), y: scanlines::Coordinate(w(vm.0 + 0x50)) },
        width: scanlines::Dimension(w(vm.0 + 0x54)), height: scanlines::Dimension(w(vm.0 + 0x58)),
        style: scanlines::Style::Paint, expansion: scanlines::Expansion(0), slant: scanlines::Slant::None,
        orientation: if b(vm.0 + 0x71) == 0 { scanlines::Orientation::Normal } else { scanlines::Orientation::Rotate },
        output: scanlines::Output::Bitmap, stride: scanlines::Stride(w(vm.0 + 0x6c)),
        count: scanlines::RowCount(0), first_row: scanlines::Row(0), run_first_row: scanlines::Row(0),
    } }
}

struct AbiProgram(RasterContext);
impl core_raster::AssetBytes for AbiProgram {
    type Error = Infallible;
    fn byte_at(&self, position: vmcore::ProgramOffset) -> Result<u8, Infallible> { unsafe { Ok(b(add(w(self.0.0 + 0x48) as u32, position.0))) } }
}
impl AbiProgram {
    unsafe fn record(&self, index: u32, compound: bool) -> Option<core_raster::ByteCursor> {
        unsafe {
            let (count, entries, data, width) = if compound { (0x50, 0x60, 0x64, 3) } else { (0x4c, 0x58, 0x5c, 2) };
            if index as i32 >= w(self.0.0 + count) { return None; }
            let mut cursor = core_raster::ByteCursor { position: vmcore::ProgramOffset(w(self.0.0 + entries).wrapping_add((index as i32).wrapping_mul(width))), remaining: vmcore::Remaining(width) };
            let first = infallible(core_raster::read_count(self, &mut cursor, core_raster::ProgramByteCount(width))) as i32;
            let last = infallible(core_raster::read_count(self, &mut cursor, core_raster::ProgramByteCount(width))) as i32;
            let remaining = last.wrapping_sub(first);
            if remaining <= 0 { None } else { Some(core_raster::ByteCursor { position: vmcore::ProgramOffset(first.wrapping_add(w(self.0.0 + data))), remaining: vmcore::Remaining(remaining) }) }
        }
    }
}
impl core_raster::ProgramStorage for AbiProgram {
    fn active(&self) -> core_raster::StreamSelection { unsafe { core_raster::StreamSelection(w(self.0.0 + 0xac) as u32) } }
    fn set_active(&mut self, stream: core_raster::StreamSelection) { unsafe { sw(self.0.0 + 0xac, stream.0 as i32); } }
    fn stream(&self, stream: core_raster::ProgramStream) -> Result<core_raster::ByteCursor, Infallible> {
        let slot = match stream { core_raster::ProgramStream::Primary => 0xa0, core_raster::ProgramStream::Secondary => 0xb0 };
        unsafe { Ok(core_raster::ByteCursor { position: vmcore::ProgramOffset(w(self.0.0 + slot + 4)), remaining: vmcore::Remaining(w(self.0.0 + slot)) }) }
    }
    fn set_stream(&mut self, stream: core_raster::ProgramStream, cursor: core_raster::ByteCursor) {
        let slot = match stream { core_raster::ProgramStream::Primary => 0xa0, core_raster::ProgramStream::Secondary => 0xb0 };
        unsafe { sw(self.0.0 + slot + 4, cursor.position.0); sw(self.0.0 + slot, cursor.remaining.0); }
    }
    fn subroutine(&self, id: core_raster::SubroutineId) -> Result<Option<core_raster::ByteCursor>, Infallible> { unsafe { Ok(self.record(id.0, false)) } }
    fn compound(&self, id: core_raster::CompoundId) -> Result<Option<core_raster::ByteCursor>, Infallible> { unsafe { Ok(self.record(id.0, true)) } }
    fn point_count(&self) -> Result<vmcore::PointCount, Infallible> { unsafe { Ok(vmcore::PointCount(w(self.0.0 + 0xd4))) } }
    fn set_point_count(&mut self, count: vmcore::PointCount) { unsafe { sw(self.0.0 + 0xd4, count.0); } }
    fn value_count(&self) -> Result<vmcore::ValueCount, Infallible> { unsafe { Ok(vmcore::ValueCount(w(self.0.0 + 0xe8))) } }
    fn set_value_count(&mut self, count: vmcore::ValueCount) { unsafe { sw(self.0.0 + 0xe8, count.0); } }
    fn initial_point(&self, index: vmcore::PointIndex) -> Result<vmcore::BytePoint, Infallible> {
        let pointer = self.0.0.wrapping_add(0xd8).wrapping_add(index.0.wrapping_mul(2));
        unsafe { Ok(vmcore::BytePoint { x: b(pointer), y: b(pointer + 1) }) }
    }
    fn set_initial_point(&mut self, index: vmcore::PointIndex, point: vmcore::BytePoint) -> Result<(), Infallible> {
        let pointer = self.0.0.wrapping_add(0xd8).wrapping_add(index.0.wrapping_mul(2));
        unsafe { sb(pointer, point.x); sb(pointer + 1, point.y); } Ok(())
    }
    fn initial_value(&self, index: vmcore::ScaleIndex) -> Result<u8, Infallible> { unsafe { Ok(b(self.0.0.wrapping_add(0xec).wrapping_add(index.0))) } }
    fn set_initial_value(&mut self, index: vmcore::ScaleIndex, value: u8) -> Result<(), Infallible> { unsafe { sb(self.0.0.wrapping_add(0xec).wrapping_add(index.0), value); } Ok(()) }
    fn body(&self) -> Result<core_raster::ByteCursor, Infallible> { unsafe { Ok(core_raster::ByteCursor { position: vmcore::ProgramOffset(w(self.0.0 + 0xf8)), remaining: vmcore::Remaining(w(self.0.0 + 0xf4)) }) } }
    fn set_body(&mut self, body: core_raster::ByteCursor) { unsafe { sw(self.0.0 + 0xf8, body.position.0); sw(self.0.0 + 0xf4, body.remaining.0); } }
    fn dimensions(&self) -> (scanlines::Dimension, scanlines::Dimension) { unsafe { (scanlines::Dimension(w(self.0.0 + 0x10c)), scanlines::Dimension(w(self.0.0 + 0x110))) } }
    fn set_inline_origin(&mut self, origin: scanlines::Point) { unsafe { sw(self.0.0 + 0x154, origin.x.0); sw(self.0.0 + 0x158, origin.y.0); } }
    fn divide(&mut self, numerator: i32, denominator: i32) -> Result<i32, Infallible> { unsafe { Ok(signed_quotient(numerator, denominator)) } }
    fn execute_body(&mut self) -> Result<core_raster::ProgramStatus, Infallible> {
        let result = unsafe { xm1_pipeline_image_font_raster_execute(RasterVm(self.0.0 + 0xb8), 2) };
        Ok(match result.0 { 0 => core_raster::ProgramStatus::Complete, -7 => core_raster::ProgramStatus::InvalidOpcode, -4 => core_raster::ProgramStatus::RunCapacity, _ => unreachable!("owned raster VM status") })
    }
    fn missing_producer(&self) -> Infallible { panic!("undefined raster program state") }
}

extern "C" {
    fn xm1_pipeline_image_font_raster_configure(config: u32) -> i32;
    fn xm1_pipeline_image_font_raster_load_glyph(context: u32, code: u32, hx: *mut i16, hy: *mut i16, vx: *mut i16, vy: *mut i16) -> i32;
    fn xm1_pipeline_image_font_raster_adjust_size(config: u32, glyph: u32, adjusted: *mut i32, context: u32) -> i32;
    fn xm1_pipeline_image_font_raster_execute(vm: RasterVm, mode: u32) -> RasterStatus;
    fn xm1_pipeline_image_font_fill_memory(address: u32, value: u32, size: u32) -> u32;
}

unsafe fn read_body(context: RasterContext, count: i32) -> u32 {
    let mut storage = AbiProgram(context);
    let mut cursor = infallible(core_raster::ProgramStorage::body(&storage));
    let value = infallible(core_raster::read_count(&storage, &mut cursor, core_raster::ProgramByteCount(count)));
    core_raster::ProgramStorage::set_body(&mut storage, cursor);
    value
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_read_byte(context: RasterContext) -> u8 { unsafe { read_body(context, 1) as u8 } }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_read_short(context: RasterContext) -> i16 { unsafe { read_body(context, 2) as i16 } }

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_read_stream(context: RasterContext, count: i32) -> u32 {
    let mut storage = AbiProgram(context);
    let selected = core_raster::ProgramStorage::active(&storage).stream();
    let mut cursor = infallible(core_raster::ProgramStorage::stream(&storage, selected));
    let value = infallible(core_raster::read_count(&storage, &mut cursor, core_raster::ProgramByteCount(count)));
    core_raster::ProgramStorage::set_stream(&mut storage, selected, cursor);
    value
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_run_first(table: RasterAddress, row: RasterCoordinate) -> RasterAddress {
    unsafe {
        let index = row.0.wrapping_sub(i32::from(h(table.0 + 12)));
        if index < 0 || index >= i32::from(h(table.0 + 16) as u16) { return RasterAddress(0); }
        let head = h(add(w(table.0 + 20) as u32, index.wrapping_mul(2))) as u16;
        RasterAddress(if head == u16::MAX { 0 } else { (w(table.0 + 4) as u32).wrapping_add(u32::from(head) * 6) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_run_next(table: RasterAddress, run: RasterAddress) -> RasterAddress {
    unsafe {
        let next = h(run.0 + 4) as u16;
        RasterAddress(if next == u16::MAX { 0 } else { (w(table.0 + 4) as u32).wrapping_add(u32::from(next) * 6) })
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_run_storage_used(table: RasterAddress) -> u32 {
    unsafe { core_raster::storage_used(w(table.0 + 8) as u32, h(table.0 + 28) as u16, h(table.0 + 30) as u16) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_run_table_init(table: RasterAddress, first: RasterCoordinate, last: RasterCoordinate, storage: RasterAddress, size: i32, context: RasterContext) {
    unsafe {
        let rows = last.0.wrapping_sub(first.0).wrapping_add(1);
        let head_bytes = rows.wrapping_mul(4) >> 1;
        sh(table.0 + 32, -1);
        sw(table.0, context.0 as i32);
        sh(table.0 + 12, first.0 as i16);
        sh(table.0 + 30, 0);
        sh(table.0 + 14, last.0 as i16);
        sh(table.0 + 16, rows as i16);
        sw(table.0 + 24, add(storage.0, head_bytes) as i32);
        sw(table.0 + 4, add(storage.0, rows.wrapping_mul(4)) as i32);
        sh(table.0 + 28, ((size as u32).wrapping_sub(rows.wrapping_mul(4) as u32) / 6) as i16);
        sw(table.0 + 8, size);
        sw(table.0 + 20, storage.0 as i32);
        xm1_pipeline_image_font_fill_memory(storage.0, 255, head_bytes as u32);
    }
}

struct AbiRuns(RasterAddress);
impl AbiRuns {
    unsafe fn row_pointer(&self, tail: bool, row: scanlines::Row) -> u32 {
        unsafe { add(w(self.0.0 + if tail { 24 } else { 20 }) as u32, row.0.wrapping_sub(i32::from(h(self.0.0 + 12))).wrapping_mul(2)) }
    }
    unsafe fn node_pointer(&self, index: core_raster::RunIndex) -> u32 { unsafe { (w(self.0.0 + 4) as u32).wrapping_add(u32::from(index.0).wrapping_mul(6)) } }
}
impl core_raster::RunStorage for AbiRuns {
    type Error = Infallible;
    fn first_row(&self) -> scanlines::Row { unsafe { scanlines::Row(i32::from(h(self.0.0 + 12))) } }
    fn row_count(&self) -> scanlines::RowCount { unsafe { scanlines::RowCount(i32::from(h(self.0.0 + 16) as u16)) } }
    fn head(&self, row: scanlines::Row) -> Result<core_raster::RunIndex, Infallible> { unsafe { Ok(core_raster::RunIndex(h(self.row_pointer(false, row)) as u16)) } }
    fn set_head(&mut self, row: scanlines::Row, index: core_raster::RunIndex) -> Result<(), Infallible> { unsafe { sh(self.row_pointer(false, row), index.0 as i16); } Ok(()) }
    fn tail(&self, row: scanlines::Row) -> Result<core_raster::RunIndex, Infallible> { unsafe { Ok(core_raster::RunIndex(h(self.row_pointer(true, row)) as u16)) } }
    fn set_tail(&mut self, row: scanlines::Row, index: core_raster::RunIndex) -> Result<(), Infallible> { unsafe { sh(self.row_pointer(true, row), index.0 as i16); } Ok(()) }
    fn node(&self, index: core_raster::RunIndex) -> Result<core_raster::RunNode, Infallible> {
        unsafe { let pointer = self.node_pointer(index); Ok(core_raster::RunNode { first: core_raster::EdgeCoordinate(h(pointer)), last: core_raster::EdgeCoordinate(h(pointer + 2)), next: core_raster::RunIndex(h(pointer + 4) as u16) }) }
    }
    fn initialize_node(&mut self, index: core_raster::RunIndex, node: core_raster::RunNode) -> Result<(), Infallible> {
        unsafe { let pointer = self.node_pointer(index); sh(pointer, node.first.0); sh(pointer + 2, node.last.0); sh(pointer + 4, node.next.0 as i16); } Ok(())
    }
    fn set_first(&mut self, index: core_raster::RunIndex, value: core_raster::EdgeCoordinate) -> Result<(), Infallible> { unsafe { sh(self.node_pointer(index), value.0); } Ok(()) }
    fn set_last(&mut self, index: core_raster::RunIndex, value: core_raster::EdgeCoordinate) -> Result<(), Infallible> { unsafe { sh(self.node_pointer(index) + 2, value.0); } Ok(()) }
    fn set_next(&mut self, index: core_raster::RunIndex, next: core_raster::RunIndex) -> Result<(), Infallible> { unsafe { sh(self.node_pointer(index) + 4, next.0 as i16); } Ok(()) }
    fn capacity(&self) -> u16 { unsafe { h(self.0.0 + 28) as u16 } }
    fn allocated(&self) -> u16 { unsafe { h(self.0.0 + 30) as u16 } }
    fn set_allocated(&mut self, count: u16) { unsafe { sh(self.0.0 + 30, count as i16); } }
    fn free(&self) -> core_raster::RunIndex { unsafe { core_raster::RunIndex(h(self.0.0 + 32) as u16) } }
    fn set_free(&mut self, index: core_raster::RunIndex) { unsafe { sh(self.0.0 + 32, index.0 as i16); } }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_merge_span(table: RasterAddress, row: RasterCoordinate, first: RasterCoordinate, last: RasterCoordinate) -> i32 {
    i32::from(infallible(core_raster::merge_run(&mut AbiRuns(table), scanlines::Row(row.0), scanlines::Span::new(first.0, last.0))))
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_subtract_run(table: RasterAddress, row: RasterCoordinate, first: RasterCoordinate, last: RasterCoordinate) -> i32 {
    i32::from(infallible(core_raster::subtract_run(&mut AbiRuns(table), scanlines::Row(row.0), scanlines::Span::new(first.0, last.0))))
}
#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_run_row_bounds(table: RasterAddress, first: *mut i32, last: *mut i32) {
    unsafe {
        let (start, end) = infallible(core_raster::run_row_bounds(&AbiRuns(table), scanlines::Row(i32::from(h(table.0 + 14)))));
        write_unaligned(first, start.0); write_unaligned(last, end.0);
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_fill_span(vm: RasterVm, row: RasterCoordinate, first: RasterCoordinate, last: RasterCoordinate) {
    infallible(scanlines::paint_span(unsafe { paint_config(vm) }, &mut AbiBitmap(vm), scanlines::Row(row.0), scanlines::Span::new(first.0, last.0)));
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_render_raster_program(context: RasterContext, transform: RasterAddress, selector: u32) -> RasterStatus {
    unsafe {
        let fields = if transform.0 == 0 { None } else {
            Some(core_raster::ComponentTransform {
                x: core_raster::TransformCoordinate(h(transform.0 + 4)), y: core_raster::TransformCoordinate(h(transform.0 + 6)),
                scale_x: core_raster::TransformRatio(h(transform.0 + 8)), scale_y: core_raster::TransformRatio(h(transform.0 + 10)),
                origin_x: core_raster::TransformCoordinate(h(transform.0 + 12)), origin_y: core_raster::TransformCoordinate(h(transform.0 + 14)),
                denominator_x: core_raster::TransformRatio(h(transform.0 + 16)), denominator_y: core_raster::TransformRatio(h(transform.0 + 18)),
                coverage: core_raster::CoverageMultiplier(h(transform.0 + 20)),
            })
        };
        RasterStatus(infallible(core_raster::render_program(&mut AbiProgram(context), fields.as_ref(), core_raster::StreamSelection(selector))).native_bits())
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_edge_min(vm: RasterVm, coordinate: RasterCoordinate, index: i32) {
    infallible(core_raster::update_edge(core_raster::EdgeSide::Left, scanlines::Row(index), scanlines::Coordinate(coordinate.0), &mut AbiGeometry(vm)));
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_edge_max(vm: RasterVm, coordinate: RasterCoordinate, index: i32) {
    infallible(core_raster::update_edge(core_raster::EdgeSide::Right, scanlines::Row(index), scanlines::Coordinate(coordinate.0), &mut AbiGeometry(vm)));
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_accumulate_line_edge(vm: RasterVm, x0: RasterCoordinate, y0: u32, x1: RasterCoordinate, y1: u32) {
    unsafe {
        let mut state = edge_state(vm);
        infallible(core_raster::line_edge(raster_grid(vm), &mut state, edge_side(vm), core_raster::RasterPoint::new(x0.0, y0 as i32), core_raster::RasterPoint::new(x1.0, y1 as i32), &mut AbiGeometry(vm)));
        store_edge_state(vm, state);
    }
}

unsafe fn curve_point(vm: RasterVm, offset: u32) -> core_raster::RasterPoint {
    unsafe { core_raster::RasterPoint::new(w(vm.0 + offset + 4), w(vm.0 + offset)) }
}
unsafe fn store_curve_point(vm: RasterVm, offset: u32, point: core_raster::RasterPoint) {
    unsafe { sw(vm.0 + offset, point.y.0); sw(vm.0 + offset + 4, point.x.0); }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_draw_curve(vm: RasterVm) {
    unsafe {
        let mut points = [curve_point(vm, 0x7edc), curve_point(vm, 0x7ee4), curve_point(vm, 0x7eec)];
        let shift = w(vm.0 + 0x7eac) as u32;
        let scale = if w(vm.0 + 0x8058) == 0 { core_raster::CurveScale::Dimensions { width: scanlines::Dimension(w(vm.0 + 0x54)), height: scanlines::Dimension(w(vm.0 + 0x58)) } } else { core_raster::CurveScale::Unscaled };
        core_raster::transform_curve(&mut points, scale, scanlines::Point { x: scanlines::Coordinate(w(vm.0 + 0x9c)), y: scanlines::Coordinate(w(vm.0 + 0xa0)) }, shift);
        for (offset, point) in [0x7edc, 0x7ee4, 0x7eec].into_iter().zip(points) { store_curve_point(vm, offset, point); }
        let mode = w(vm.0 + 0xb4);
        if mode != 0 && mode != 2 {
            let slot = if w(vm.0 + 0x8054) == 0 { 0x8060 } else { 0x805c };
            let output = w(vm.0 + slot) as u32;
            sw(vm.0 + slot, output.wrapping_add(12) as i32);
            for (index, point) in points.into_iter().enumerate() {
                sh(output + index as u32 * 4, core_raster::arm_asr(point.x.0, shift) as i16);
                sh(output + index as u32 * 4 + 2, core_raster::arm_asr(point.y.0, shift) as i16);
            }
        } else if mode == 2 {
            sw(vm.0 + 4, if w(vm.0 + 0x8054) == 0 { xm1_pipeline_image_font_raster_edge_max as usize as i32 } else { xm1_pipeline_image_font_raster_edge_min as usize as i32 });
            let mut state = edge_state(vm);
            let kind = if w(vm.0 + 0x8050) == 0 { core_raster::CurveKind::Line } else { core_raster::CurveKind::Quadratic };
            infallible(core_raster::draw_edge_curve(raster_grid(vm), &mut state, edge_side(vm), kind, points, &mut AbiGeometry(vm)));
            store_edge_state(vm, state);
        } else {
            let side = edge_side(vm);
            let (flag, y_slot, x_slot) = match side { core_raster::EdgeSide::Left => (0x7f04, 0x7ef4, 0x7ef8), core_raster::EdgeSide::Right => (0x7f08, 0x7efc, 0x7f00) };
            let point = if w(vm.0 + flag) == 0 { None } else { Some(core_raster::RasterPoint::new(w(vm.0 + x_slot), w(vm.0 + y_slot))) };
            let mut state = core_raster::CachedCurve { point, first_row: scanlines::Row(w(vm.0 + 0x804c)) };
            infallible(core_raster::draw_cached_curve(&mut points, side, shift, w(vm.0 + 0x7eb0) as u32, core_raster::FixedCoordinate(w(vm.0 + 0x7ea4)), &mut state, &mut AbiCachedCurve { vm, side }));
            let point = state.point.expect("draw_cached_curve records its first point");
            if w(vm.0 + flag) == 0 { sw(vm.0 + flag, 1); }
            sw(vm.0 + x_slot, point.x.0); sw(vm.0 + y_slot, point.y.0); sw(vm.0 + 0x804c, state.first_row.0);
            for (offset, point) in [0x7edc, 0x7ee4, 0x7eec].into_iter().zip(points) { store_curve_point(vm, offset, point); }
        }
    }
}

#[derive(Clone, Copy)]
enum CoverageFormat { Mask, Gray4, Gray8 }

impl CoverageFormat {
    fn encoding(self) -> crate::develop::font_core::CoverageEncoding { match self { Self::Mask => crate::develop::font_core::CoverageEncoding::PackedMsb1, Self::Gray4 => crate::develop::font_core::CoverageEncoding::Gray4, Self::Gray8 => crate::develop::font_core::CoverageEncoding::Gray8 } }
    fn factor(self) -> i32 { core_raster::coverage_factor(self.encoding()) }
}

struct AbiUnderline(RasterContext);
impl core_raster::SpanSink for AbiUnderline {
    type Error = Infallible;
    fn paint(&mut self, row: scanlines::Row, span: scanlines::Span) -> Result<(), Infallible> {
        unsafe { xm1_pipeline_image_font_raster_fill_span(RasterVm(self.0.0 + 0xb8), RasterCoordinate(row.0), RasterCoordinate(span.first.0), RasterCoordinate(span.last.0)); } Ok(())
    }
    fn merge(&mut self, row: scanlines::Row, span: scanlines::Span) -> Result<bool, Infallible> {
        unsafe { Ok(xm1_pipeline_image_font_raster_merge_span(RasterAddress(self.0.0 + 0x83b0), RasterCoordinate(row.0), RasterCoordinate(span.first.0), RasterCoordinate(span.last.0)) != 0) }
    }
}
unsafe fn underline(context: RasterContext, horizontal_advance: i16, vertical_advance: i16, format: CoverageFormat, rotated: bool) {
    unsafe {
        let config = core_raster::UnderlineConfig {
            width: scanlines::Dimension(w(context.0 + 4)), height: scanlines::Dimension(w(context.0 + 8)),
            thickness: scanlines::Dimension(w(context.0 + 28)), position: scanlines::Coordinate(w(context.0 + 24)),
            extra: scanlines::Dimension(w(context.0 + 32)), weight: scanlines::Dimension(w(context.0 + 16)),
            horizontal_advance: crate::develop::font_core::AdvancePerMille::new(horizontal_advance), vertical_advance: crate::develop::font_core::AdvancePerMille::new(vertical_advance),
            writing: if w(context.0) == 0 { crate::develop::font_core::WritingMode::Horizontal } else { crate::develop::font_core::WritingMode::Vertical },
            orientation: if rotated { scanlines::Orientation::Rotate } else { scanlines::Orientation::Normal }, encoding: format.encoding(),
            expansion: match w(context.0 + 12) { 1 | 2 => core_raster::UnderlineExpansion::Symmetric, 3 => core_raster::UnderlineExpansion::Horizontal, 4 => core_raster::UnderlineExpansion::Vertical, 5 => core_raster::UnderlineExpansion::Both, _ => core_raster::UnderlineExpansion::None },
        };
        infallible(core_raster::underline(config, &mut AbiUnderline(context)));
    }
}

unsafe fn coverage(context: RasterContext, format: CoverageFormat, rotated: bool) {
    unsafe {
        let mut first = 0; let mut last = 0;
        xm1_pipeline_image_font_raster_run_row_bounds(RasterAddress(context.0 + 0x83b0), &mut first, &mut last);
        let config = core_raster::CoverageConfig {
            format: match format { CoverageFormat::Gray4 => core_raster::GrayCoverage::Gray4, CoverageFormat::Gray8 => core_raster::GrayCoverage::Gray8, CoverageFormat::Mask => unreachable!() },
            orientation: if rotated { scanlines::Orientation::Rotate } else { scanlines::Orientation::Normal },
            origin: scanlines::Point { x: scanlines::Coordinate(w(context.0 + 0x104)), y: scanlines::Coordinate(w(context.0 + 0x108)) },
            height: scanlines::Dimension(w(context.0 + 0x110)), stride: scanlines::Stride(w(context.0 + 0x124)),
            first_row: scanlines::Row(first), last_row: scanlines::Row(last),
            palette: if b(context.0 + 0x39) == 0 { core_raster::CoveragePalette::Linear } else { core_raster::CoveragePalette::Lookup },
        };
        infallible(core_raster::coverage(config, &mut AbiBitmap(RasterVm(context.0 + 0xb8))));
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_pipeline_image_font_raster_render_bitmap(config: RasterAddress, request: RasterAddress) -> RasterStatus {
    unsafe {
        let context = RasterContext(w(config.0 + 0x40) as u32);
        let saved = [w(config.0 + 4), w(config.0 + 8), w(config.0 + 12), w(config.0 + 20)];
        let mut adjusted = 0;
        let status = xm1_pipeline_image_font_raster_adjust_size(config.0, w(request.0) as u32, &mut adjusted, context.0);
        if status != 0 { return RasterStatus(status); }
        sw(context.0 + 0xc0, context.0 as i32);
        sw(context.0 + 0x120, w(request.0 + 24));
        sw(context.0 + 0x15c, if w(config.0 + 36) == 0 { 0 } else { w(request.0 + 32) });
        sw(context.0 + 0x124, w(request.0 + 20));
        let mut hx = 0; let mut hy = 0; let mut vx = 0; let mut vy = 0;
        let present = xm1_pipeline_image_font_raster_load_glyph(context.0, w(request.0) as u32, &mut hx, &mut hy, &mut vx, &mut vy);
        if present == 0 {
            if adjusted != 0 {
                for (offset, value) in [4,8,12,20].into_iter().zip(saved) { sw(config.0 + offset, value); }
                xm1_pipeline_image_font_raster_configure(config.0);
            }
            return RasterStatus(1);
        }
        if hx != 0 { hx = signed_quotient(w(config.0 + 4).wrapping_mul(i32::from(hx)), 1000) as i16; }
        if vx != 0 { vx = signed_quotient(w(config.0 + 8).wrapping_mul(i32::from(vx)), 1000) as i16; }
        let rotated = b(context.0 + 0x129) != 0;
        if rotated { let width = w(context.0 + 0x10c); sw(context.0 + 0x10c, w(context.0 + 0x110)); sw(context.0 + 0x110, width); }
        let format = match w(context.0 + 36) { 0 => CoverageFormat::Mask, 1 => CoverageFormat::Gray4, 2 => CoverageFormat::Gray8, _ => panic!("undefined raster coverage format") };
        let factor = format.factor();
        sw(context.0 + 0xfc, factor.wrapping_mul(i32::from(hx)));
        sw(context.0 + 0x100, factor.wrapping_mul(i32::from(vx)));
        sw(context.0 + 0x104, factor.wrapping_mul(w(request.0 + 4)));
        sw(context.0 + 0x108, factor.wrapping_mul(w(request.0 + 8)));
        if matches!(format, CoverageFormat::Mask) {
            sw(context.0 + 0x12c, 32767); sw(context.0 + 0x130, 32767);
            sw(context.0 + 0x134, 0); sw(context.0 + 0x138, 0);
        } else {
            let mut last = w(context.0 + 0x118).wrapping_add(w(context.0 + 0x110)).wrapping_sub(1);
            if b(context.0 + 0x128) != 0 {
                let weight = w(config.0 + 20);
                if weight > 0 || w(config.0 + 28) > 0 {
                    let extra = if w(config.0 + 28) > 0 { w(config.0 + 32) } else { 0 };
                    last = last.wrapping_add(factor.wrapping_mul(extra.max(weight)));
                }
            }
            xm1_pipeline_image_font_raster_run_table_init(RasterAddress(context.0 + 0x83b0), RasterCoordinate(w(context.0 + 0x118).wrapping_neg()), RasterCoordinate(last), RasterAddress(context.0 + 0x83ec), w(context.0 + 0x44).wrapping_sub(0x83ec), context);
        }
        underline(context, hy, vy, format, rotated);
        let remaining = w(context.0 + 0xa0);
        let mut status = RasterStatus(0);
        if remaining > 0 {
            let style = w(context.0 + 12);
            if style == 1 {
                sw(context.0 + 0x114, 2);
                let cursor = w(context.0 + 0xa4);
                status = xm1_pipeline_image_font_render_raster_program(context, RasterAddress(0), 0);
                if status.0 == 0 {
                    sw(context.0 + 0xa0, remaining); sw(context.0 + 0xa4, cursor); sw(context.0 + 0x114, 1);
                    status = xm1_pipeline_image_font_render_raster_program(context, RasterAddress(0), 0);
                }
            } else {
                let pass = match style { 0 => 0, 2 => 2, 3 => if rotated { 5 } else { 3 }, 4 => if rotated { 3 } else { 4 }, 5 => if rotated { 7 } else { 6 }, _ => panic!("undefined raster style") };
                sw(context.0 + 0x114, pass);
                status = xm1_pipeline_image_font_render_raster_program(context, RasterAddress(0), 0);
            }
        }
        if !matches!(format, CoverageFormat::Mask) { coverage(context, format, rotated); }
        if rotated { let width = w(context.0 + 0x10c); sw(context.0 + 0x10c, w(context.0 + 0x110)); sw(context.0 + 0x110, width); }
        let left = w(context.0 + 0x12c);
        let right = w(context.0 + 0x134);
        if right < left { for offset in [4,8,12,16] { sw(request.0 + offset, 0); } }
        else {
            sw(request.0 + 12, right.wrapping_add(1).wrapping_sub(left));
            sw(request.0 + 16, w(context.0 + 0x138).wrapping_add(1).wrapping_sub(w(context.0 + 0x130)));
            sw(request.0 + 4, left); sw(request.0 + 8, w(context.0 + 0x130));
        }
        if matches!(format, CoverageFormat::Mask) { sw(request.0 + 28, 0x83ec); }
        let used = xm1_pipeline_image_font_raster_run_storage_used(RasterAddress(context.0 + 0x83b0));
        sw(request.0 + 28, used.wrapping_add(0x83ec) as i32);
        if adjusted != 0 {
            for (offset, value) in [4,8,12,20].into_iter().zip(saved) { sw(config.0 + offset, value); }
            xm1_pipeline_image_font_raster_configure(config.0);
        }
        status
    }
}
