use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ElapsedMicros(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Acceleration(pub u64);

pub fn integrated_acceleration(acceleration: Acceleration, elapsed: i64) -> u64 {
    let squared = (elapsed as u64).wrapping_mul(elapsed as u64) / 10_000;
    if acceleration.0 > 0x00ff_ffff {
        squared.wrapping_mul(acceleration.0 / 1_000) / 200_000
    } else {
        squared.wrapping_mul(acceleration.0) / 200_000_000
    }
}

fn word<M: GuestAddressSpace>(memory: &mut M, record: GuestAddress, offset: u32) -> Result<u32, MemoryError> {
    memory.read_u32(record.checked_add(offset)?)
}

fn half<M: GuestAddressSpace>(memory: &mut M, record: GuestAddress, offset: u32) -> Result<u16, MemoryError> {
    memory.read_u16(record.checked_add(offset)?)
}

fn linear_distance(speed: u32, duration: i32) -> u32 {
    (u64::from(speed).wrapping_mul(duration as i64 as u64) / 1_000_000) as u32
}

pub fn helper_f96004<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, elapsed: ElapsedMicros, record: GuestAddress) -> Result<i16, MemoryError> {
    let acceleration_duration = word(context, record, 0x74)? as i32;
    let transition_duration = word(context, record, 0x78)? as i32;
    let cruise_duration = word(context, record, 0x7c)? as i32;
    let elapsed = elapsed.0;
    let distance = if elapsed <= 0 {
        0
    } else if acceleration_duration >= elapsed {
        integrated_acceleration(Acceleration(u64::from(word(context, record, 0x84)?)), i64::from(elapsed)) as u16
    } else {
        let transition_end = transition_duration.wrapping_add(acceleration_duration);
        if transition_end >= elapsed {
            let duration = elapsed.wrapping_sub(acceleration_duration);
            let linear = linear_distance(word(context, record, 0x68)?, duration);
            let acceleration_distance = half(context, record, 0x6c)?;
            let distance = linear.wrapping_add(u32::from(acceleration_distance)) as u16;
            let quadratic = integrated_acceleration(Acceleration(u64::from(word(context, record, 0x88)?)), i64::from(duration));
            distance.wrapping_add(quadratic as u16)
        } else {
            let cruise_end = transition_end.wrapping_add(cruise_duration);
            if cruise_end >= elapsed {
                let transition_distance = half(context, record, 0x6e)?;
                let acceleration_distance = half(context, record, 0x6c)?;
                let linear = linear_distance(word(context, record, 0x64)?, elapsed.wrapping_sub(transition_end));
                transition_distance.wrapping_add(acceleration_distance).wrapping_add(linear as u16)
            } else {
                let deceleration_start = cruise_end.wrapping_add(transition_duration);
                if deceleration_start >= elapsed {
                    let duration = elapsed.wrapping_sub(cruise_end);
                    let transition_distance = half(context, record, 0x6e)?;
                    let acceleration_distance = half(context, record, 0x6c)?;
                    let cruise_distance = half(context, record, 0x70)?;
                    let linear = linear_distance(word(context, record, 0x64)?, duration);
                    let distance = transition_distance.wrapping_add(acceleration_distance).wrapping_add(cruise_distance).wrapping_add(linear as u16);
                    let quadratic = integrated_acceleration(Acceleration(u64::from(word(context, record, 0x88)?)), i64::from(duration));
                    distance.wrapping_sub(quadratic as u16)
                } else if deceleration_start.wrapping_add(acceleration_duration) >= elapsed {
                    let duration = elapsed.wrapping_sub(deceleration_start);
                    let cruise_distance = half(context, record, 0x70)?;
                    let acceleration_distance = half(context, record, 0x6c)?;
                    let transition_distance = half(context, record, 0x6e)?;
                    let linear = linear_distance(word(context, record, 0x68)?, duration);
                    let distance = cruise_distance.wrapping_add(acceleration_distance).wrapping_add(transition_distance.wrapping_mul(2)).wrapping_add(linear as u16);
                    let quadratic = integrated_acceleration(Acceleration(u64::from(word(context, record, 0x84)?)), i64::from(duration));
                    distance.wrapping_sub(quadratic as u16)
                } else {
                    let end = i32::from(half(context, record, 14)? as i16);
                    let start = i32::from(half(context, record, 12)? as i16);
                    (end - start).unsigned_abs() as u16
                }
            }
        }
    };
    let distance = if word(context, record, 8)? == 1 { distance.wrapping_neg() } else { distance };
    Ok(distance.wrapping_add(half(context, record, 12)?) as i16)
}

pub fn helper_fa30c0<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, record: GuestAddress, frames: u32) -> Result<i16, MemoryError> {
    let period = context.read_u32(GuestAddress::new(0x016d_74f4))?;
    let elapsed = period.wrapping_mul(frames).wrapping_sub(word(context, record, 0x80)?);
    helper_f96004(context, ElapsedMicros(elapsed as i32), record)
}
