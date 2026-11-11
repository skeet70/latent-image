use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleIndex(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SampleCount(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Position(pub i16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FramePeriod(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MotionSpeed(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CadenceSpan {
    first: SampleIndex,
    end: SampleIndex,
    samples: u16,
    distance: u32,
}

impl CadenceSpan {
    pub fn speed(self, minimum_samples: SampleCount, period: FramePeriod) -> Option<MotionSpeed> {
        if u32::from(minimum_samples.0) > u32::from(self.samples) + 1 { return None; }
        let duration = period.0.wrapping_mul(u32::from(self.samples));
        let numerator = u64::from(self.distance) * 1_000_000;
        let speed = if duration == 0 {
            if numerator == 0 { 0 } else { u32::MAX }
        } else {
            (numerator / u64::from(duration)) as u32
        };
        (speed != 0).then_some(MotionSpeed(speed))
    }
    pub fn first(self) -> SampleIndex { self.first }
    pub fn end(self) -> SampleIndex { self.end }
}

fn position<M: GuestAddressSpace>(memory: &mut M, samples: GuestAddress, index: SampleIndex) -> Result<i32, MemoryError> {
    Ok(i32::from(memory.read_u16(samples.checked_add(u32::from(index.0) * 2)?)? as i16))
}

fn difference<M: GuestAddressSpace>(memory: &mut M, samples: GuestAddress, index: SampleIndex) -> Result<i32, MemoryError> {
    let next = position(memory, samples, SampleIndex(index.0.wrapping_add(1)))?;
    Ok(next - position(memory, samples, index)?)
}

fn curvature_product<M: GuestAddressSpace>(memory: &mut M, samples: GuestAddress, index: SampleIndex) -> Result<i32, MemoryError> {
    let here = position(memory, samples, index)?;
    let next = position(memory, samples, SampleIndex(index.0.wrapping_add(1)))?;
    let previous = position(memory, samples, SampleIndex(index.0.wrapping_sub(1)))?;
    let following = position(memory, samples, SampleIndex(index.0.wrapping_add(2)))?;
    Ok((following - next + here - next).wrapping_mul(next - here + previous - here))
}

pub fn cadence_span<M: GuestAddressSpace>(memory: &mut M, samples: GuestAddress, initial: SampleIndex, count: SampleCount) -> Result<Option<CadenceSpan>, MemoryError> {
    let mut current = initial;
    if i32::from(count.0) <= i32::from(current.0) + 1 { return Ok(None); }
    loop {
        if difference(memory, samples, current)?.abs() > 1 { break; }
        current.0 = current.0.wrapping_add(1);
        if i32::from(current.0) + 1 >= i32::from(count.0) { return Ok(None); }
    }
    let mut first = current;
    if i32::from(count.0) <= i32::from(current.0) + 2 { return Ok(None); }
    loop {
        if curvature_product(memory, samples, current)? <= 0 { break; }
        current.0 = current.0.wrapping_add(1);
        first = current;
        if i32::from(current.0) + 2 >= i32::from(count.0) { return Ok(None); }
    }
    if i32::from(count.0) <= i32::from(current.0) + 2 { return Ok(None); }
    let mut forced_end = false;
    loop {
        if i32::from(count.0) <= i32::from(current.0) + 1 {
            current = SampleIndex(count.0.wrapping_sub(2));
            forced_end = true;
            break;
        }
        if difference(memory, samples, current)?.abs() <= 1 { break; }
        current.0 = current.0.wrapping_add(1);
        if i32::from(current.0) + 1 >= i32::from(count.0) {
            current = SampleIndex(count.0.wrapping_sub(2));
            forced_end = true;
            break;
        }
    }
    let threshold = difference(memory, samples, first)?.abs() / 2;
    let mut end = SampleIndex(current.0.wrapping_sub(1));
    let mut found = false;
    while first.0 < end.0 {
        if curvature_product(memory, samples, end)? <= 0 && difference(memory, samples, end)?.abs() > threshold {
            found = true;
            break;
        }
        end.0 = end.0.wrapping_sub(1);
    }
    if !found {
        if !forced_end { return Ok(None); }
        end = current;
    }
    end.0 = end.0.wrapping_add(1);
    let distance = (position(memory, samples, end)? - position(memory, samples, first)?).unsigned_abs();
    Ok(Some(CadenceSpan { first, end, samples: end.0.wrapping_sub(first.0), distance }))
}
