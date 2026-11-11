//! X-M1 image geometry snapshots and window projections.
//! Authority: image 5 instructions10451ac..10452e8 and1050ec0..10517f8, x70view55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
#[path = "record_image_geometry_query_profiles.rs"]
pub mod profiles;
use crate::record_configuration::helper_69f8fc;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GeometryQuery { pub kind: u32, pub first: u32, pub second: u32, pub packed_coordinates: u32 }
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CaptureWindow { pub height: u32, pub width: u32 }
#[derive(Clone, Copy)]
struct Rectangle { y: u32, x: u32, height: u32, width: u32 }
#[derive(Clone, Copy)]
struct GeometryRecord { count: u32, y: u32, x: u32, width: u32, height: u32, reference_y: u32, reference_x: u32 }
struct Snapshot { dimensions: CaptureWindow, available: u32, state: u32, reserved: u32, geometry: GeometryRecord }

pub trait SnapshotLock { fn acquire(&mut self) -> i32; fn release(&mut self) -> i32; }
fn snapshot<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>) -> Result<Snapshot, MemoryError> {
    let mut words = [0; 12];
    if context.effects.acquire() == 0 {
        let result: Result<(), MemoryError> = (|| {
            for index in [2usize, 3, 4, 5, 6, 7, 9, 8, 10, 11, 1, 0] { words[index] = context.read_u32(GuestAddress::new(0x4171b800 + index as u32 * 4))?; }
            Ok(())
        })();
        context.effects.release();
        result?;
    }
    Ok(Snapshot { dimensions: CaptureWindow { height: words[0], width: words[1] }, available: words[2], state: words[3], reserved: words[4], geometry: GeometryRecord { count: words[5], y: words[6], x: words[7], width: words[8], height: words[9], reference_y: words[10], reference_x: words[11] } })
}
fn percentage(value: u32, scale: u32) -> u32 { ((u64::from(value.wrapping_mul(scale)) * 0xd1b7_1759) >> 45) as u32 }
fn project_window(outer: Rectangle, dimensions: CaptureWindow) -> Rectangle {
    let width = percentage(outer.width, dimensions.width);
    let height = percentage(outer.height, dimensions.height);
    Rectangle { y: outer.y.wrapping_add(outer.width.wrapping_sub(width) >> 1), x: outer.x.wrapping_add(outer.height.wrapping_sub(height) >> 1), height, width }
}
fn rectangle(words: &[u32; 8], offset: usize) -> Rectangle { Rectangle { y: words[offset], x: words[offset + 1], height: words[offset + 2], width: words[offset + 3] } }
fn window<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, query: GeometryQuery, ratio: u32, flag: u32) -> Result<[u32; 8], MemoryError> {
    crate::record_deep_geometry::transform_values(context, [kind, query.kind, query.first, query.second, ratio, flag])
}
fn divide(numerator: u32, denominator: u32) -> u32 { if denominator == 0 { 0 } else { numerator / denominator } }
fn scale(record: GeometryRecord, source: CaptureWindow, destination: CaptureWindow) -> GeometryRecord {
    let y = divide(record.y.wrapping_mul(destination.width), source.width);
    let x = divide(record.x.wrapping_mul(destination.height), source.height);
    let reference_y = divide(record.reference_y.wrapping_mul(destination.width), source.width);
    let reference_x = divide(record.reference_x.wrapping_mul(destination.height), source.height);
    GeometryRecord { count: record.count, y, x, width: reference_x.wrapping_sub(x).wrapping_mul(2), height: reference_y.wrapping_sub(y).wrapping_mul(2), reference_y, reference_x }
}
fn write_record<M: GuestAddressSpace>(memory: &mut M, output: GuestAddress, record: GeometryRecord) -> Result<(), MemoryError> {
    for (index, value) in [record.count, record.y, record.x, record.width, record.height, record.reference_y, record.reference_x].into_iter().enumerate() { memory.write_u32(output.checked_add(index as u32 * 4)?, value)?; }
    Ok(())
}
fn translate(mut record: GeometryRecord, y: u32, x: u32) -> GeometryRecord {
    record.y = record.y.wrapping_add(y); record.x = record.x.wrapping_add(x);
    record.reference_y = record.reference_y.wrapping_add(y); record.reference_x = record.reference_x.wrapping_add(x);
    record
}
fn status<M: GuestAddressSpace>(memory: &mut M, output: GuestAddress, snapshot: &Snapshot) -> Result<bool, MemoryError> {
    memory.write_u32(output, snapshot.available)?; memory.write_u32(output.checked_add(4)?, snapshot.state)?;
    if snapshot.available == 1 && snapshot.state != 1 { Ok(true) } else { memory.write_u32(output.checked_add(12)?, 0)?; Ok(false) }
}
fn clip<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, snapshot: &mut Snapshot, query: GeometryQuery) -> Result<(), MemoryError> {
    let geometry = &mut snapshot.geometry;
    let half_height = geometry.height >> 1;
    let half_width = geometry.width >> 1;
    let low_y = geometry.reference_y.wrapping_sub(half_height.wrapping_mul(helper_69f8fc(context, 0xa4e2)?) / 100);
    let low_x = geometry.reference_x.wrapping_sub(half_width.wrapping_mul(helper_69f8fc(context, 0xa4e0)?) / 100);
    let high_y = geometry.reference_y.wrapping_add(half_height.wrapping_mul(helper_69f8fc(context, 0xa4e2)?) / 100);
    let high_x = geometry.reference_x.wrapping_add(half_width.wrapping_mul(helper_69f8fc(context, 0xa4e1)?) / 100);
    let (height, width, origin_y, origin_x): (u32, u32, u32, u32) = match query.packed_coordinates >> 16 { 3 => (426, 640, 0, 27), 16 => (360, 640, 0, 60), 1 => (426, 426, 107, 27), _ => (480, 640, 0, 0) };
    geometry.y = (low_y as i32).max(origin_y as i32) as u32;
    geometry.x = (low_x as i32).max(origin_x as i32) as u32;
    geometry.height = (high_y as i32).min(width as i32).wrapping_sub(geometry.y as i32) as u32;
    geometry.width = (high_x as i32).min(height as i32).wrapping_sub(geometry.x as i32) as u32;
    Ok(())
}
fn read_query<M: GuestAddressSpace>(memory: &mut M, input: GuestAddress) -> Result<GeometryQuery, MemoryError> {
    Ok(GeometryQuery { kind: memory.read_u32(input)?, first: memory.read_u32(input.checked_add(4)?)?, second: memory.read_u32(input.checked_add(8)?)?, packed_coordinates: memory.read_u32(input.checked_add(12)?)? })
}
fn query<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>, kind: u32, input: GuestAddress, output: GuestAddress, details: GuestAddress, clipped: bool) -> Result<i32, MemoryError> {
    if kind > 35 || input.get() <= 1 || output.get() == 0 { return Ok(0x84); }
    let mut snapshot = snapshot(context)?;
    if !status(context, output, &snapshot)? { return Ok(0); }
    let input = if clipped { read_query(context, input)? } else {
        GeometryQuery { kind: context.read_u32(input)?, first: context.read_u32(input.checked_add(4)?)?, second: context.read_u32(input.checked_add(8)?)?, packed_coordinates: 0 }
    };
    let translated = window(context, kind, input, 0x0004_0003, 1)?;
    let outer = rectangle(&translated, 0);
    let inner = project_window(outer, snapshot.dimensions);
    let relative_y = inner.y.wrapping_sub(outer.y);
    let relative_x = inner.x.wrapping_sub(outer.x);
    if clipped { clip(context, &mut snapshot, input)?; }
    snapshot.geometry.x = snapshot.geometry.x.wrapping_sub(27);
    snapshot.geometry.reference_x = snapshot.geometry.reference_x.wrapping_sub(27);
    let record = translate(scale(snapshot.geometry, CaptureWindow { height: 426, width: 640 }, CaptureWindow { height: inner.height, width: inner.width }), relative_y, relative_x);
    write_record(context, output.checked_add(12)?, record)?;
    if details.get() != 0 {
        for (index, value) in [outer.height, outer.width, relative_y, relative_x, inner.height, inner.width, relative_y, relative_x, inner.height, inner.width].into_iter().enumerate() { context.write_u32(details.checked_add(index as u32 * 4)?, value)?; }
    }
    Ok(0)
}
pub fn helper_10510e8<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>, kind: u32, input: GuestAddress, output: GuestAddress, details: GuestAddress) -> Result<i32, MemoryError> { query(context, kind, input, output, details, false) }
pub fn helper_10512cc<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>, kind: u32, input: GuestAddress, output: GuestAddress, details: GuestAddress) -> Result<i32, MemoryError> { query(context, kind, input, output, details, true) }
pub fn helper_10517a0<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>, output: GuestAddress) -> Result<i32, MemoryError> {
    if output.get() == 0 { return Ok(0x84); }
    let snapshot = snapshot(context)?;
    let values = [snapshot.available, snapshot.state, snapshot.reserved, snapshot.geometry.count, snapshot.geometry.y, snapshot.geometry.x, snapshot.geometry.width, snapshot.geometry.height, snapshot.geometry.reference_y, snapshot.geometry.reference_x];
    for offset in (0..0x420).step_by(4) { context.write_u32(output.checked_add(offset)?, if offset < 40 { values[offset as usize / 4] } else { 0 })?; }
    Ok(0)
}
pub fn helper_10515f4<M: GuestAddressSpace, E: SnapshotLock>(context: &mut RecordContext<M, E>, input: GuestAddress, destination: GuestAddress, output: GuestAddress) -> Result<i32, MemoryError> {
    if input.get() == 0 || destination.get() == 0 || output.get() == 0 { return Ok(0x84); }
    for offset in (0..0x414).step_by(4) { context.write_u32(GuestAddress::new(0x1466e34 + offset), 0)?; }
    let mut snapshot = snapshot(context)?;
    if !status(context, output, &snapshot)? { return Ok(0); }
    let input = read_query(context, input)?;
    let first = window(context, 1, input, 0x0004_0003, 0)?;
    let second = window(context, 1, input, input.packed_coordinates, 0)?;
    let outer = rectangle(&first, 0);
    let projected = project_window(outer, snapshot.dimensions);
    snapshot.geometry.x = snapshot.geometry.x.wrapping_sub(27);
    snapshot.geometry.reference_x = snapshot.geometry.reference_x.wrapping_sub(27);
    let first_inner = rectangle(&first, 4);
    let second_inner = rectangle(&second, 4);
    let intermediate = translate(scale(snapshot.geometry, CaptureWindow { height: 426, width: 640 }, CaptureWindow { height: first_inner.height, width: first_inner.width }), projected.y.wrapping_sub(outer.y), projected.x.wrapping_sub(outer.x));
    write_record(context, GuestAddress::new(0x1466e34), intermediate)?;
    let destination = CaptureWindow { height: context.read_u32(destination)?, width: context.read_u32(destination.checked_add(4)?)? };
    let result = scale(intermediate, CaptureWindow { height: second_inner.height, width: second_inner.width }, destination);
    write_record(context, output.checked_add(12)?, result)?;
    Ok(0)
}

#[cfg(test)]
#[path = "record_image_geometry_query_tests.rs"]
mod tests;
