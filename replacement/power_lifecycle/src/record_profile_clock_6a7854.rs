//! Power profile clock bytes at 0x409c281c, image-4 bodies 0x006a7854 and
//! 0x006a7878.

use crate::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError, RecordContext};

pub const PROFILE_BLOCK: u32 = 0x409c281c;
pub const CLOCK_ENABLED: u32 = 0x5c;
pub const CLOCK_DIRECTION: u32 = 0x5b;
pub const CLOCK_HOURS: u32 = 0x28;
pub const CLOCK_QUARTERS: u32 = 0x20;

fn byte<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, offset: u32) -> Result<u8, MemoryError> {
    context.read_u8(GuestAddress::new(PROFILE_BLOCK.wrapping_add(offset)))
}

/// The image-4 body at 0x006a7854.
pub fn helper_6a7854<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>) -> Result<u32, MemoryError> {
    Ok(u32::from(byte(context, CLOCK_ENABLED)? >> 7))
}

/// The image-4 body at 0x006a7878.
pub fn helper_6a7878<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    direction: GuestAddress,
    hours: GuestAddress,
    minutes: GuestAddress,
) -> Result<(), MemoryError> {
    let direction_value = byte(context, CLOCK_DIRECTION)? >> 7;
    context.write_u32(direction, u32::from(direction_value))?;
    let hour_value = byte(context, CLOCK_HOURS)? & 0x1f;
    let value = u32::from(hour_value);
    context.write_u32(hours, if value > 23 { 0 } else { value })?;
    let quarter_value = byte(context, CLOCK_QUARTERS)? >> 6;
    let value = u32::from(quarter_value) * 15;
    context.write_u32(minutes, if value > 59 { 0 } else { value })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record_address_space::{
        ByteCount, GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };

    const OUTPUT: u32 = 0x0010_0000;

    struct Memory {
        profile: [u8; 0x60],
        output: [u8; 12],
    }

    impl Memory {
        fn new() -> Self { Self { profile: [0; 0x60], output: [0; 12] } }

        fn with_context<R>(&mut self, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, ()>) -> R) -> R {
            let descriptor = |base, length| {
                RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap()
            };
            let mut regions = [
                MemoryRegion::new(descriptor(PROFILE_BLOCK, 0x60), &mut self.profile).unwrap(),
                MemoryRegion::new(descriptor(OUTPUT, 12), &mut self.output).unwrap(),
            ];
            body(&mut RecordContext::new(HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap(), ()))
        }

        fn output(&self, index: usize) -> u32 { u32::from_le_bytes(self.output[index * 4..index * 4 + 4].try_into().unwrap()) }
    }

    #[test]
    fn clock_enabled_reports_the_top_bit() {
        let mut memory = Memory::new();
        for value in [0u8, 1, 127, 128, 255] {
            memory.profile[CLOCK_ENABLED as usize] = value;
            let expected = u32::from(value >> 7);
            memory.with_context(|context| assert_eq!(helper_6a7854(context), Ok(expected)));
        }
    }

    #[test]
    fn clock_fields_report_direction_hours_and_quarter_hours() {
        let mut memory = Memory::new();
        memory.profile[CLOCK_DIRECTION as usize] = 0x80;
        memory.profile[CLOCK_HOURS as usize] = 0x97;
        memory.profile[CLOCK_QUARTERS as usize] = 0x80;
        memory.with_context(|context| {
            assert_eq!(helper_6a7878(context, GuestAddress::new(OUTPUT), GuestAddress::new(OUTPUT + 4), GuestAddress::new(OUTPUT + 8)), Ok(()));
        });
        assert_eq!([memory.output(0), memory.output(1), memory.output(2)], [1, 23, 30]);
    }

    #[test]
    fn clock_fields_zero_an_out_of_range_hour() {
        let mut memory = Memory::new();
        memory.profile[CLOCK_DIRECTION as usize] = 0x7f;
        memory.profile[CLOCK_HOURS as usize] = 0x98;
        memory.profile[CLOCK_QUARTERS as usize] = 0xc0;
        memory.with_context(|context| {
            assert_eq!(helper_6a7878(context, GuestAddress::new(OUTPUT), GuestAddress::new(OUTPUT + 4), GuestAddress::new(OUTPUT + 8)), Ok(()));
        });
        assert_eq!([memory.output(0), memory.output(1), memory.output(2)], [0, 0, 45]);
    }
}
