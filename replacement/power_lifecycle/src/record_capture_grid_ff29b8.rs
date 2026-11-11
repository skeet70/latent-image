//! The capture grid and the 0x016f_229c measurement record: image-5 bodies
//! 0x00fe_aa64, 0x00ff_29b8 and 0x00ff_2c5c.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_hardware_requests::{helper_fe9580, CaptureCommand, CaptureQueue};

/// The measurement table 0x00ff_29b8 reads and 0x00ff_2c5c copies from.
pub const MEASUREMENT: u32 = 0x016f_229c;
pub const MEASUREMENT_BYTES: u32 = 96;
pub const MEASUREMENT_RECORD: u32 = 480;
pub const RECORD_BYTES: u32 = 96;
/// The task 43 runtime the grid bodies read.
pub const RUNTIME: u32 = 0x016f_253c;
pub const GRID: u32 = 0x5ffe_c1b4;
pub const GRID_ROWS: u32 = 0x5ffe_cdb4;
pub const BLACK: u32 = 0x5ffe_d1b4;
pub const WINDOW_SOURCE: u32 = 0x5ffe_be8c;
pub const WINDOW_COUNT: u32 = 0x5ffe_c08c;
pub const WINDOW_BLACK: u32 = 0x5ffe_e518;
/// The payload 0x00fe_aa64 copies and the length its packet declares.
pub const CAPTURE_PAYLOAD: u32 = 124;
pub const CAPTURE_LENGTH: u32 = 136;
pub const CAPTURE_COMMAND: u16 = 0x3f42;

/// The image-5 body at 0x00ff_2c5c.
pub fn helper_ff2c5c<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32, destination: GuestAddress) -> Result<(), MemoryError> {
    let source = GuestAddress::new(MEASUREMENT.wrapping_add(index.wrapping_mul(MEASUREMENT_BYTES)).wrapping_add(MEASUREMENT_RECORD));
    context.copy_bytes(destination, source, ByteCount(RECORD_BYTES))
}

/// The image-5 body at 0x00fe_aa64.
pub fn helper_feaa64<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, _first: u32, _second: u32, _third: u32, source: GuestAddress,
) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(CAPTURE_PAYLOAD), StackAlignment::Word, |context, frame| {
        context.copy_bytes(frame.base(), source, ByteCount(CAPTURE_PAYLOAD))?;
        helper_fe9580(context, CaptureCommand::new(CAPTURE_COMMAND), frame.base(), ByteCount(CAPTURE_LENGTH), &[]).map(|_| ())
    })
}

/// The image-5 body at 0x00ff_29b8.
pub fn helper_ff29b8<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, index: u32) -> Result<(), MemoryError> {
    let base = context.read_u32(GuestAddress::new(RUNTIME + 0xa0))?;
    if index == 0 {
        context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
            crate::record_hardware_deep_banks::helper_edae88(context, 3, frame.base())?;
            let source = base.wrapping_add(context.read_u32(frame.base())?);
            context.copy_bytes(GuestAddress::new(GRID), GuestAddress::new(source), ByteCount(4096))?;
            context.copy_bytes(GuestAddress::new(GRID_ROWS), GuestAddress::new(source.wrapping_add(1024)), ByteCount(1024))
        })?;
        for (selector, destination) in [(13, BLACK), (14, BLACK + 4), (15, BLACK + 8), (16, BLACK + 12)] {
            context.with_guest_frame(ByteCount(4), StackAlignment::Word, |context, frame| {
                crate::record_hardware_deep_banks::helper_edae88(context, selector, frame.base())?;
                let source = base.wrapping_add(context.read_u32(frame.base())?);
                context.copy_bytes(GuestAddress::new(destination), GuestAddress::new(source), ByteCount(4))
            })?;
        }
        let profile = context.read_u32(GuestAddress::new(RUNTIME + 0x20))?;
        let gain = i32::from(context.read_u16(GuestAddress::new(MEASUREMENT + 18))? as i16);
        crate::record_hardware_deep_correction::helper_fe9d30(
            context,
            GuestAddress::new(GRID),
            GuestAddress::new(RUNTIME + 0xac),
            GuestAddress::new(0),
            GuestAddress::new(0),
            GuestAddress::new(BLACK),
            GuestAddress::new(RUNTIME + 0xb4),
            profile,
            gain,
        )?;
    } else {
        let count = u32::from(context.read_u32(GuestAddress::new(MEASUREMENT + 0x1cc))? != 0);
        for (destination, body) in [
            (WINDOW_SOURCE, 0u32),
            (WINDOW_COUNT, 1),
            (WINDOW_BLACK, 2),
        ] {
            context.with_guest_frame(ByteCount(2), StackAlignment::Word, |context, frame| {
                context.write_u8(frame.base(), 7)?;
                context.write_u8(frame.base().checked_add(1)?, 7)?;
                match body {
                    0 => crate::record_hardware_deep_banks::helper_fe988c(context, GuestAddress::new(destination), GuestAddress::new(base), frame.base()).map(|_| ()),
                    1 => crate::record_hardware_deep_banks::helper_fe99f0(context, GuestAddress::new(destination), GuestAddress::new(base), count, 1).map(|_| ()),
                    _ => crate::record_hardware_deep_banks::helper_fe9adc(context, GuestAddress::new(destination), GuestAddress::new(base), frame.base()).map(|_| ()),
                }
            })?;
        }
    }
    let record = GuestAddress::new(MEASUREMENT.wrapping_add(index.wrapping_mul(MEASUREMENT_BYTES)));
    let source = GuestAddress::new(MEASUREMENT);
    for (at, from, length) in [(480, 248, 8), (488, 32, 8), (496, 256, 8), (524, 240, 8), (532, 24, 8), (540, 288, 8)] {
        context.copy_bytes(record.checked_add(at)?, source.checked_add(from)?, ByteCount(length))?;
    }
    let value = context.read_u32(source)?;
    context.write_u32(record.checked_add(0x1f8)?, value)?;
    let value = context.read_u16(source.checked_add(4)?)?;
    context.write_u16(record.checked_add(558)?, value)?;
    let value = context.read_u8(source.checked_add(6)?)?;
    context.write_u8(record.checked_add(0x23c)?, value)?;
    let value = context.read_u32(source.checked_add(8)?)?;
    context.write_u32(record.checked_add(0x1fc)?, value)?;
    for (at, from) in [(512, 16), (514, 18), (516, 20)] {
        let value = context.read_u16(source.checked_add(from)?)?;
        context.write_u16(record.checked_add(at)?, value)?;
    }
    let value = context.read_u32(source.checked_add(0xc)?)?;
    context.write_u32(record.checked_add(0x208)?, value)?;
    for (at, from) in [(0x224, 0x1cc), (0x228, 0x1d0), (0x230, 0x1d8), (0x234, 0x1dc), (0x238, 0x1c8)] {
        let value = context.read_u32(source.checked_add(from)?)?;
        context.write_u32(record.checked_add(at)?, value)?;
    }
    let value = context.read_u16(source.checked_add(468)?)?;
    context.write_u16(record.checked_add(556)?, value)
}

/// The image-5 body at 0x00fe_aacc.
pub fn helper_feaacc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32) -> Result<(), MemoryError> {
    helper_ff29b8(context, channel)
}

/// The image-5 body at 0x00fe_aadc.
pub fn helper_feaadc<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, channel: u32, output: GuestAddress) -> Result<(), MemoryError> {
    helper_ff2c5c(context, channel, output)
}

mod exports {
    use super::*;
    use crate::record_native_exports::stock_export;

    stock_export!(
        xm1_power_record_helper_feaacc, helper_feaacc_abi, (),
        (channel: u32) -> (),
        (channel: u32),
        (channel),
        crate::record_capture_grid_ff29b8::helper_feaacc,
        |_| ()
    );
    stock_export!(
        xm1_power_record_helper_feaadc, helper_feaadc_abi, (),
        (channel: u32, output: u32) -> (),
        (channel: u32, output: GuestAddress),
        (channel, GuestAddress::new(output)),
        crate::record_capture_grid_ff29b8::helper_feaadc,
        |_| ()
    );
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00fe_aa64-0x00ff_2c5c.
    use super::*;
    use crate::record_address_space::{GuestDomain, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission};
    use crate::record_hardware_requests::{CaptureMessage, CaptureTask};

    struct Memory {
        measurement: [u8; 0x400],
        capture: [u8; 0x10000],
        source: [u8; 0x100],
        destination: [u8; 0x80],
        queue_task: [u8; 4],
        stack: [u8; 0x100],
    }

    #[derive(Default)]
    struct Queue {
        sent: u32,
        command: u16,
        payload: u32,
        length: u32,
    }

    impl CaptureQueue<HostAddressSpace<'_, '_>> for Queue {
        fn current_task(&mut self, _memory: &mut HostAddressSpace<'_, '_>) -> Result<CaptureTask, MemoryError> { Ok(CaptureTask::new(1)) }
        fn submit(&mut self, _memory: &mut HostAddressSpace<'_, '_>, message: CaptureMessage<'_>) -> crate::shutdown::KernelStatus {
            self.sent += 1;
            self.command = message.command.get();
            self.payload = message.payload.get();
            self.length = message.length.0;
            crate::shutdown::KernelStatus(0)
        }
    }

    fn with_context<R>(memory: &mut Memory, queue: &mut Queue, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Queue>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(MEASUREMENT, 0x400), &mut memory.measurement).unwrap(),
            MemoryRegion::new(descriptor(0x5ffe_0000, 0x10000), &mut memory.capture).unwrap(),
            MemoryRegion::new(descriptor(0x0010_0000, 0x100), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(0x0010_1000, 0x80), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(0x016f_0aa4, 4), &mut memory.queue_task).unwrap(),
            MemoryRegion::new(descriptor(0x0010_2000, 0x100), &mut memory.stack).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = crate::record_address_space::GuestStack::new(GuestAddress::new(0x0010_2000), ByteCount(0x100)).unwrap();
        let mut context = RecordContext::with_stack(space, core::mem::take(queue), stack);
        let result = body(&mut context);
        *queue = core::mem::take(&mut context.effects);
        result
    }

    fn empty() -> Memory {
        Memory { measurement: [0; 0x400], capture: [0; 0x10000], source: [0; 0x100], destination: [0; 0x80], queue_task: [0; 4], stack: [0; 0x100] }
    }

    #[test]
    fn the_record_copy_takes_the_indexed_ninety_six_bytes() {
        let mut memory = empty();
        for index in 0..0x60 { memory.measurement[96 + 480 + index] = index as u8 + 1; }
        with_context(&mut memory, &mut Queue::default(), |context| {
            assert_eq!(helper_ff2c5c(context, 1, GuestAddress::new(0x0010_1000)), Ok(()));
        });
        assert_eq!(&memory.destination[..0x60], &memory.measurement[576..672]);
    }

    #[test]
    fn the_capture_packet_carries_the_copied_payload_and_the_declared_length() {
        let mut memory = empty();
        for index in 0..0x7c { memory.source[index] = index as u8 ^ 0x5a; }
        let mut queue = Queue::default();
        with_context(&mut memory, &mut queue, |context| {
            assert_eq!(helper_feaa64(context, 1, 2, 3, GuestAddress::new(0x0010_0000)), Ok(()));
        });
        assert_eq!(queue.sent, 1);
        assert_eq!(queue.command, CAPTURE_COMMAND);
        assert_eq!(queue.length, CAPTURE_LENGTH);
        assert_eq!(queue.payload, 0x0010_2084);
    }

    #[test]
    fn the_second_branch_fills_the_three_windows_and_copies_the_record() {
        let mut memory = empty();
        memory.measurement[0x2a0 + 0xa0..0x2a0 + 0xa4].copy_from_slice(&0x5ffe_8000u32.to_le_bytes());
        memory.measurement[0x2a0 + 0x20..0x2a0 + 0x24].copy_from_slice(&9u32.to_le_bytes());
        memory.measurement[0x1cc..0x1d0].copy_from_slice(&1u32.to_le_bytes());
        memory.measurement[4..6].copy_from_slice(&0x1234u16.to_le_bytes());
        memory.measurement[6] = 0x5a;
        memory.measurement[8..12].copy_from_slice(&0x0a0b_0c0du32.to_le_bytes());
        memory.measurement[16..18].copy_from_slice(&0x1111u16.to_le_bytes());
        memory.measurement[18..20].copy_from_slice(&0x2222u16.to_le_bytes());
        memory.measurement[20..22].copy_from_slice(&0x3333u16.to_le_bytes());
        memory.measurement[468..470].copy_from_slice(&0x4444u16.to_le_bytes());
        memory.measurement[0x1c8..0x1cc].copy_from_slice(&1u32.to_le_bytes());
        memory.measurement[0x1d0..0x1d4].copy_from_slice(&2u32.to_le_bytes());
        memory.measurement[0x1d8..0x1dc].copy_from_slice(&3u32.to_le_bytes());
        memory.measurement[0x1dc..0x1e0].copy_from_slice(&4u32.to_le_bytes());
        with_context(&mut memory, &mut Queue::default(), |context| assert_eq!(helper_ff29b8(context, 1), Ok(())));
        let record = 96;
        assert_eq!(&memory.measurement[record + 480..record + 488], &memory.measurement[248..256]);
        assert_eq!(&memory.measurement[record + 488..record + 496], &memory.measurement[32..40]);
        assert_eq!(&memory.measurement[record + 496..record + 504], &memory.measurement[256..264]);
        assert_eq!(&memory.measurement[record + 524..record + 532], &memory.measurement[240..248]);
        assert_eq!(&memory.measurement[record + 532..record + 540], &memory.measurement[24..32]);
        assert_eq!(&memory.measurement[record + 540..record + 548], &memory.measurement[288..296]);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x1f8..record + 0x1fc].try_into().unwrap()), 0);
        assert_eq!(u16::from_le_bytes(memory.measurement[record + 558..record + 560].try_into().unwrap()), 0x1234);
        assert_eq!(memory.measurement[record + 0x23c], 0x5a);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x1fc..record + 0x200].try_into().unwrap()), 0x0a0b_0c0d);
        assert_eq!(u16::from_le_bytes(memory.measurement[record + 512..record + 514].try_into().unwrap()), 0x1111);
        assert_eq!(u16::from_le_bytes(memory.measurement[record + 514..record + 516].try_into().unwrap()), 0x2222);
        assert_eq!(u16::from_le_bytes(memory.measurement[record + 516..record + 518].try_into().unwrap()), 0x3333);
        assert_eq!(u16::from_le_bytes(memory.measurement[record + 556..record + 558].try_into().unwrap()), 0x4444);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x224..record + 0x228].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x228..record + 0x22c].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x230..record + 0x234].try_into().unwrap()), 3);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x234..record + 0x238].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(memory.measurement[record + 0x238..record + 0x23c].try_into().unwrap()), 1);
    }
}
