//! Image-5 helper.
use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};
use crate::record_native_exports::stock_export;
use crate::record_scan_publication_history::fill;

pub const STATISTICS: u32 = 0x4171_7764;
pub const CONFIGURATION: u32 = 0x0169_b8f0;

const RECORD_COUNT: u32 = 5;
const RECORD_BYTES: u32 = 0x18;
const RECORDS: u32 = 0x48;
const MARKERS: u32 = 0xc0;
const MARKER: u16 = 0xffff;
const CLEARED_BYTES: u32 = 0x44;
const CONFIGURATION_BYTES: u32 = 0x48;
const DEFAULT_LIMIT: u32 = 10000;

pub fn helper_eec89c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    fill(context, STATISTICS + 4, 0, CLEARED_BYTES)?;
    for index in 0..RECORD_COUNT {
        fill(context, STATISTICS + RECORDS + index * RECORD_BYTES, 0, RECORD_BYTES)?;
        context.write_u16(GuestAddress::new(STATISTICS + MARKERS + index * 2), MARKER)?;
    }
    context.write_u32(GuestAddress::new(STATISTICS + 0xcc), 0)?;
    context.write_u16(GuestAddress::new(STATISTICS + 0x08), 0x7fff)?;
    context.write_u16(GuestAddress::new(STATISTICS + 0xd0), 0)?;
    context.write_u16(GuestAddress::new(STATISTICS + 0xd2), 0)?;
    context.write_u32(GuestAddress::new(STATISTICS + 0xd4), 0)?;
    for offset in [0x2c, 0x30, 0x38, 0x3c] {
        context.write_u32(GuestAddress::new(STATISTICS + offset), DEFAULT_LIMIT)?;
    }
    fill(context, CONFIGURATION, 0, CONFIGURATION_BYTES)?;
    let first = crate::record_configuration::helper_69fec8(context, 0xa02a)?;
    context.write_u32(GuestAddress::new(CONFIGURATION + 0x10), first)?;
    let second = crate::record_configuration::helper_69fec8(context, 0xa02c)?;
    context.write_u32(GuestAddress::new(CONFIGURATION + 0x30), second)
}

stock_export!(
    xm1_power_record_helper_eec89c,
    helper_eec89c_abi,
    (),
    () -> (),
    (),
    (),
    crate::record_helper_statistics_reset_eec89c::helper_eec89c,
    |_| ()
);

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 body at 0x00ee_c89c.
    use super::*;
    use crate::record_address_space::{ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};

    const POINTER: u32 = 0x0096_ca50;
    const TABLE: u32 = 0x0010_0000;

    struct Memory {
        pointer: [u8; 4],
        table: [u8; 0xa030],
        statistics: [u8; 0x100],
        configuration: [u8; 0x100],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(POINTER, 4), &mut memory.pointer).unwrap(),
            MemoryRegion::new(descriptor(TABLE, 0xa030), &mut memory.table).unwrap(),
            MemoryRegion::new(descriptor(STATISTICS, 0x100), &mut memory.statistics).unwrap(),
            MemoryRegion::new(descriptor(CONFIGURATION, 0x100), &mut memory.configuration).unwrap(),
        ];
        body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
    }

    #[test]
    fn clears_the_records_and_seeds_the_limits() {
        let mut memory = Memory { pointer: TABLE.to_le_bytes(), table: [0xa5; 0xa030], statistics: [0xa5; 0x100], configuration: [0xa5; 0x100] };
        memory.table[0xa02a..0xa02c].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.table[0xa02c..0xa02e].copy_from_slice(&0x5678u16.to_le_bytes());
        with_memory(&mut memory, |context| assert_eq!(helper_eec89c(context), Ok(())));

        assert_eq!(memory.statistics[0], 0xa5);
        for range in [4..8, 0x0a..0x2c, 0x34..0x38, 0x40..0x48, 0x48..0xc0] {
            assert!(memory.statistics[range].iter().all(|byte| *byte == 0));
        }
        assert!(memory.statistics[0xc0..0xca].chunks_exact(2).all(|marker| marker == [0xff, 0xff]));
        assert_eq!(memory.statistics[0xca..0xcc], [0xa5, 0xa5]);
        assert_eq!(&memory.statistics[0xcc..0xd0], &0u32.to_le_bytes());
        assert_eq!(&memory.statistics[0x08..0x0a], &0x7fffu16.to_le_bytes());
        assert_eq!(&memory.statistics[0xd0..0xd4], &[0, 0, 0, 0]);
        assert_eq!(&memory.statistics[0xd4..0xd8], &0u32.to_le_bytes());
        for offset in [0x2c, 0x30, 0x38, 0x3c] {
            assert_eq!(&memory.statistics[offset..offset + 4], &10000u32.to_le_bytes());
        }
        for range in [0..0x10, 0x14..0x30, 0x34..0x48] {
            assert!(memory.configuration[range].iter().all(|byte| *byte == 0));
        }
        assert_eq!(&memory.configuration[0x10..0x14], &0x1234u32.to_le_bytes());
        assert_eq!(&memory.configuration[0x30..0x34], &0x5678u32.to_le_bytes());
        assert_eq!(memory.configuration[0x48], 0xa5);
    }
}
