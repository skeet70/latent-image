//! Stock-shaped accessors over the power mode table at 0x00ef_6de4.
//!
//! Each body is the image-5 instruction sequence at the address in its name.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

/// 0x00ef_6de4: one record per mode.
pub const MODE_TABLE: u32 = 0x00ef_6de4;
const MODE_STRIDE: u32 = 20;
/// 0x00ef_71bc: four configuration keys per mode record.
pub const MODE_KEYS: u32 = 0x00ef_71bc;
/// 0x00ef_72cc: four halfwords per mode record.
pub const MODE_LIMITS: u32 = 0x00ef_72cc;

fn record(index: u32) -> GuestAddress {
    GuestAddress::new(MODE_TABLE.wrapping_add(index.wrapping_mul(MODE_STRIDE)))
}

fn field(index: u32, at: u32) -> Result<GuestAddress, MemoryError> {
    record(index).checked_add(at)
}

/// What 0x00ef54fc writes to `out`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModeCounters {
    pub first: u32,
    pub second: u32,
    pub third: u32,
    pub fourth: u32,
}

/// 0x00ef4fd8
pub fn helper_ef4fd8<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>, level: u32) -> Result<u32, MemoryError> {
    Ok(match level {
        5..=10 => level,
        _ => 0,
    })
}

/// 0x00ef5790
pub fn helper_ef5790<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    Ok(u32::from(context.read_u8(field(index, 0x0a)?)? & 0x0f))
}

/// 0x00ef5898
pub fn helper_ef5898<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    context.read_u32(field(index, 4)?)
}

/// 0x00ef640c
pub fn helper_ef640c<M: GuestAddressSpace, E>(_context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(4)
}

/// 0x00ef663c
pub fn helper_ef663c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<u32, MemoryError> {
    Ok(u32::from((context.read_u16(field(index, 0x0a)?)? >> 14) & 1))
}

fn keyed_halfword<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, at: GuestAddress) -> Result<u32, MemoryError> {
    let key = context.read_u32(at)?;
    if key == 0 { Ok(1) } else { crate::record_configuration::helper_69fec8(context, key) }
}

/// 0x00ef54fc
pub fn helper_ef54fc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, out: GuestAddress) -> Result<ModeCounters, MemoryError> {
    let keys = GuestAddress::new(MODE_KEYS.wrapping_add(index.wrapping_mul(16)));
    let third = keyed_halfword(context, keys.checked_add(8)?)?;
    let fourth = keyed_halfword(context, keys.checked_add(0x0c)?)?;
    let first = keyed_halfword(context, keys)?;
    let second = keyed_halfword(context, keys.checked_add(4)?)?;
    let mut counters = ModeCounters { first, second, third, fourth };
    context.write_u32(out.checked_add(8)?, counters.third)?;
    context.write_u32(out.checked_add(0x0c)?, counters.fourth)?;
    context.write_u32(out, counters.first)?;
    context.write_u32(out.checked_add(4)?, counters.second)?;
    if crate::record_hardware_deep_calibration::helper_ef5448(context, index)? != 0 {
        return Ok(counters);
    }
    let limits = GuestAddress::new(MODE_LIMITS.wrapping_add(index.wrapping_mul(8)));
    let low = u32::from(context.read_u16(limits)?);
    let high = u32::from(context.read_u16(limits.checked_add(2)?)?);
    let low_extra = u32::from(context.read_u16(limits.checked_add(4)?)?);
    let high_extra = u32::from(context.read_u16(limits.checked_add(6)?)?);
    counters.third = counters.third.wrapping_add(low_extra).wrapping_add(high_extra);
    counters.fourth = counters.fourth.wrapping_add(low).wrapping_add(high);
    counters.first = counters.first.saturating_sub(low);
    counters.second = counters.second.saturating_sub(low_extra);
    context.write_u32(out.checked_add(8)?, counters.third)?;
    context.write_u32(out.checked_add(0x0c)?, counters.fourth)?;
    context.write_u32(out, counters.first)?;
    context.write_u32(out.checked_add(4)?, counters.second)?;
    Ok(counters)
}

/// 0x00ef6700
pub fn helper_ef6700<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, mode: u32) -> Result<u32, MemoryError> {
    let unset = crate::record_hardware_deep_calibration::helper_ef5448(context, index)? == 0;
    if unset && mode != 1 {
        let _ = crate::record_statistics_getters::helper_ef53d0(context, index)?;
    }
    Ok(1)
}

/// 0x00ef6820
pub fn helper_ef6820<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, _first: u32, _second: u32, out: GuestAddress) -> Result<(), MemoryError> {
    context.write_u8(out, 0)?;
    context.write_u8(out.checked_add(1)?, 1)
}

/// 0x00ef7844: the answer is whether profile header byte 0x62 is nonzero.
pub fn helper_ef7844<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, handle: u32) -> Result<u32, MemoryError> {
    if crate::record_configuration::helper_69f8fc(context, 0x5256)? & 3 == 0
        && crate::record_configuration::helper_69f8fc(context, 0x5257)? & 3 == 0
    {
        return Ok(0);
    }
    Ok(u32::from(crate::record_profile_accessors::helper_750474(context, handle)? != 0))
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00ef4fd8-0x00ef7844.
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const HANDLE: u32 = 0x00fe_0000;
    const CONFIG: u32 = 0x0030_0000;
    const OUT: u32 = 0x0020_0000;

    struct Memory {
        config_pointer: [u8; 4],
        config: [u8; 0x6000],
        modes: [u8; 0x60],
        keys: [u8; 0x40],
        limits: [u8; 0x20],
        registry: [u8; 0x80],
        resource: [u8; 0x42b0],
        out: [u8; 16],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.config_pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG, 0x6000), &mut memory.config).unwrap(),
            MemoryRegion::new(descriptor(MODE_TABLE, 0x60), &mut memory.modes).unwrap(),
            MemoryRegion::new(descriptor(MODE_KEYS, 0x40), &mut memory.keys).unwrap(),
            MemoryRegion::new(descriptor(MODE_LIMITS, 0x20), &mut memory.limits).unwrap(),
            MemoryRegion::new(descriptor(0x0091_5d38, 0x80), &mut memory.registry).unwrap(),
            MemoryRegion::new(descriptor(0x0096_ebd0, 0x42b0), &mut memory.resource).unwrap(),
            MemoryRegion::new(descriptor(OUT, 16), &mut memory.out).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        body(&mut RecordContext::new(space, ()))
    }

    fn sample() -> Memory {
        let mut memory = Memory {
            config_pointer: CONFIG.to_le_bytes(),
            config: [0; 0x6000],
            modes: [0; 0x60],
            keys: [0; 0x40],
            limits: [0; 0x20],
            registry: [0; 0x80],
            resource: [0; 0x42b0],
            out: [0; 16],
        };
        memory.modes[2 * 20 + 4..2 * 20 + 8].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        memory.modes[2 * 20 + 0x0a..2 * 20 + 0x0c].copy_from_slice(&0x40a3u16.to_le_bytes());
        memory.modes[3 * 20 + 0x0a..3 * 20 + 0x0c].copy_from_slice(&0x00a3u16.to_le_bytes());
        memory
    }

    #[test]
    fn levels_and_mode_record_fields() {
        let mut memory = sample();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ef4fd8(context, 4), Ok(0));
            assert_eq!(helper_ef4fd8(context, 5), Ok(5));
            assert_eq!(helper_ef4fd8(context, 10), Ok(10));
            assert_eq!(helper_ef4fd8(context, 11), Ok(0));
            assert_eq!(helper_ef640c(context), Ok(4));
            assert_eq!(helper_ef5790(context, 2), Ok(3));
            assert_eq!(helper_ef5898(context, 2), Ok(0x1122_3344));
            assert_eq!(helper_ef663c(context, 2), Ok(1));
            assert_eq!(helper_ef663c(context, 3), Ok(0));
        });
    }

    #[test]
    fn counters_take_keyed_halfwords_then_the_limit_offsets() {
        let mut memory = sample();
        for (index, key) in [0x100u32, 0, 0x110, 0x112].into_iter().enumerate() {
            memory.keys[16 + index * 4..20 + index * 4].copy_from_slice(&key.to_le_bytes());
        }
        memory.config[0x100..0x102].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.config[0x110..0x112].copy_from_slice(&0x5678u16.to_le_bytes());
        memory.config[0x112..0x114].copy_from_slice(&0x9abcu16.to_le_bytes());
        for (index, limit) in [10u16, 20, 30, 40].into_iter().enumerate() {
            memory.limits[8 + index * 2..10 + index * 2].copy_from_slice(&limit.to_le_bytes());
        }
        with_memory(&mut memory, |context| {
            let expected = ModeCounters { first: 0x122a, second: 0, third: 0x56be, fourth: 0x9ada };
            assert_eq!(helper_ef54fc(context, 1, GuestAddress::new(OUT)), Ok(expected));
        });
        assert_eq!(&memory.out[0..4], &0x122au32.to_le_bytes());
        assert_eq!(&memory.out[4..8], &0u32.to_le_bytes());
        assert_eq!(&memory.out[8..12], &0x56beu32.to_le_bytes());
        assert_eq!(&memory.out[12..16], &0x9adau32.to_le_bytes());

        let mut memory = sample();
        for (index, key) in [0x100u32, 0, 0x110, 0x112].into_iter().enumerate() {
            memory.keys[16 + index * 4..20 + index * 4].copy_from_slice(&key.to_le_bytes());
        }
        memory.config[0x100..0x102].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.config[0x110..0x112].copy_from_slice(&0x5678u16.to_le_bytes());
        memory.config[0x112..0x114].copy_from_slice(&0x9abcu16.to_le_bytes());
        memory.modes[20 + 8] = 1;
        with_memory(&mut memory, |context| {
            let expected = ModeCounters { first: 0x1234, second: 1, third: 0x5678, fourth: 0x9abc };
            assert_eq!(helper_ef54fc(context, 1, GuestAddress::new(OUT)), Ok(expected));
        });
    }

    #[test]
    fn the_mode_gate_reports_one_in_every_branch() {
        let mut memory = sample();
        memory.modes[8] = 0;
        with_memory(&mut memory, |context| {
            assert_eq!(helper_ef6700(context, 0, 0), Ok(1));
            assert_eq!(helper_ef6700(context, 0, 1), Ok(1));
        });
        memory.modes[8] = 1;
        with_memory(&mut memory, |context| assert_eq!(helper_ef6700(context, 0, 0), Ok(1)));
    }

    #[test]
    fn the_two_byte_default_is_zero_then_one() {
        let mut memory = sample();
        with_memory(&mut memory, |context| assert_eq!(helper_ef6820(context, 7, 8, GuestAddress::new(OUT)), Ok(())));
        assert_eq!(&memory.out[0..2], &[0, 1]);
    }

    #[test]
    fn the_profile_byte_is_read_only_when_a_configuration_bit_is_set() {
        let mut memory = sample();
        memory.registry[0x14..0x18].copy_from_slice(&1u32.to_le_bytes());
        memory.registry[0x18] = 1;
        let bank_one = 0x2158;
        memory.resource[bank_one + 0x62] = 0x5a;
        memory.config[0x5256..0x5258].copy_from_slice(&[0x04, 0x00]);
        memory.config[0x5257..0x5259].copy_from_slice(&[0x00, 0x00]);
        with_memory(&mut memory, |context| assert_eq!(helper_ef7844(context, HANDLE), Ok(0)));
        memory.config[0x5256] = 0x02;
        with_memory(&mut memory, |context| assert_eq!(helper_ef7844(context, HANDLE), Ok(1)));
        memory.resource[bank_one + 0x62] = 0;
        with_memory(&mut memory, |context| assert_eq!(helper_ef7844(context, HANDLE), Ok(0)));
        memory.resource[bank_one + 0x62] = 0x5a;
        memory.config[0x5256] = 0;
        memory.config[0x5257] = 0x01;
        with_memory(&mut memory, |context| assert_eq!(helper_ef7844(context, HANDLE), Ok(1)));
    }
}
