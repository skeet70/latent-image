//! Stock-shaped capture submissions over the image-5 bodies at 0x00fea438-0x00feadfc.
//!
//! The payload lengths below are the immediates in those instructions.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_hardware_requests::{helper_fe9580, CaptureCommand, CaptureQueue};

const QUEUE_DESTINATION: u32 = 0x016f_0aa4;
const REFRESH_BLOCK: u32 = 0x016f_4488;
const SNAPSHOT_BLOCK: u32 = 0x016f_224c;

fn submit<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, command: u16, length: u32,
    write: impl FnOnce(&mut RecordContext<M, E>, GuestAddress) -> Result<(), MemoryError>,
) -> Result<(), MemoryError> {
    context.with_guest_frame(ByteCount(length), StackAlignment::Word, |context, frame| {
        write(context, frame.base())?;
        let _ = helper_fe9580(context, CaptureCommand::new(command), frame.base(), frame.length(), &[])?;
        Ok(())
    })
}

fn submit_words<M: GuestAddressSpace, E: CaptureQueue<M>, const N: usize>(
    context: &mut RecordContext<M, E>, command: u16, words: [u32; N],
) -> Result<(), MemoryError> {
    submit(context, command, N as u32 * 4, |context, base| {
        for (index, word) in words.into_iter().enumerate() {
            context.write_u32(base.checked_add(index as u32 * 4)?, word)?;
        }
        Ok(())
    })
}

fn submit_empty<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, command: u16) -> Result<(), MemoryError> {
    let _ = helper_fe9580(context, CaptureCommand::new(command), GuestAddress::new(0), ByteCount(0), &[])?;
    Ok(())
}

/// 0x00fea438
pub fn helper_fea438<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, first: u32, second: u32, third: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f13, [first, second, third])
}

/// 0x00fea4d0
pub fn helper_fea4d0<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f27, [first, second])
}

/// 0x00fea4fc
pub fn helper_fea4fc<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f28, [value])
}

/// 0x00fea524
pub fn helper_fea524<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f29, [first, second])
}

/// 0x00fea550
pub fn helper_fea550<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f2a, [first, second])
}

/// 0x00fea57c
pub fn helper_fea57c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    submit(context, 0x3f2b, 40, |context, base| context.copy_bytes(base, source, ByteCount(40)))
}

/// 0x00fea5c0
pub fn helper_fea5c0<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(REFRESH_BLOCK), source, ByteCount(296))?;
    submit_empty(context, 0x3f2c)
}

/// 0x00fea5f0
pub fn helper_fea5f0<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    submit(context, 0x3f2d, 44, |context, base| context.copy_bytes(base, source, ByteCount(44)))
}

/// 0x00feaaec
pub fn helper_feaaec<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, value: u32, source: GuestAddress) -> Result<(), MemoryError> {
    submit(context, 0x3f44, 24, |context, base| {
        context.write_u32(base, value)?;
        let halfword = context.read_u16(source)?;
        context.write_u16(base.checked_add(4)?, halfword)?;
        context.write_u16(base.checked_add(6)?, 0)?;
        for offset in [4, 8, 0xc, 0x10] {
            let word = context.read_u32(source.checked_add(offset)?)?;
            context.write_u32(base.checked_add(4 + offset)?, word)?;
        }
        Ok(())
    })
}

/// 0x00feab3c
pub fn helper_feab3c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, source: GuestAddress) -> Result<(), MemoryError> {
    context.copy_bytes(GuestAddress::new(SNAPSHOT_BLOCK), source, ByteCount(80))?;
    submit_empty(context, 0x3f45)
}

/// 0x00fead6c
pub fn helper_fead6c<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>, first: u32, second: u32) -> Result<(), MemoryError> {
    submit_words(context, 0x3f18, [first, second])
}

/// 0x00fead98
pub fn helper_fead98<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>, first: u32, second: u32, third: u8, fourth: u32,
    fifth: u32, sixth: u32, seventh: u32, eighth: u32, ninth: u32, tenth: u32,
) -> Result<(), MemoryError> {
    submit(context, 0x3f3a, 40, |context, base| {
        context.write_u32(base, first)?;
        context.write_u32(base.checked_add(4)?, second)?;
        context.write_u8(base.checked_add(8)?, third)?;
        context.write_u8(base.checked_add(9)?, 0)?;
        context.write_u8(base.checked_add(10)?, 0)?;
        context.write_u8(base.checked_add(11)?, 0)?;
        for (index, word) in [fourth, fifth, sixth, seventh, eighth, ninth, tenth].into_iter().enumerate() {
            context.write_u32(base.checked_add(12 + index as u32 * 4)?, word)?;
        }
        Ok(())
    })
}

/// 0x00feadfc
pub fn helper_feadfc<M: GuestAddressSpace, E: CaptureQueue<M>>(context: &mut RecordContext<M, E>) -> Result<(), MemoryError> {
    submit_empty(context, 0x3f3b)
}

#[cfg(test)]
mod tests {
    //! Expected commands, lengths and payloads come from the image-5 bodies at
    //! 0x00fea438-0x00feadfc.
    use super::*;
    use crate::record_address_space::{
        GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };
    use crate::record_hardware_requests::{CaptureMessage, CaptureTask};
    use crate::shutdown::KernelStatus;

    const PAYLOAD: usize = 48;

    struct Transport {
        command: u16,
        payload: [u8; PAYLOAD],
        length: u32,
        submitted: u32,
    }

    impl Default for Transport {
        fn default() -> Self { Self { command: 0, payload: [0; PAYLOAD], length: 0, submitted: 0 } }
    }

    impl<M: GuestAddressSpace> CaptureQueue<M> for Transport {
        fn current_task(&mut self, _memory: &mut M) -> Result<CaptureTask, MemoryError> { Ok(CaptureTask::new(1)) }
        fn submit(&mut self, memory: &mut M, message: CaptureMessage<'_>) -> KernelStatus {
            self.command = message.command.get();
            self.length = message.length.0;
            for offset in 0..message.length.0 as usize {
                self.payload[offset] = memory.read_u8(message.payload.checked_add(offset as u32).unwrap()).unwrap();
            }
            self.submitted += 1;
            KernelStatus(0)
        }
    }

    impl Transport {
        fn word(&self, offset: usize) -> u32 { u32::from_le_bytes(self.payload[offset..offset + 4].try_into().unwrap()) }
        fn half(&self, offset: usize) -> u16 { u16::from_le_bytes(self.payload[offset..offset + 2].try_into().unwrap()) }
    }

    struct Memory {
        stack: [u8; 512],
        destination: [u8; 4],
        source: [u8; 320],
        refresh: [u8; 296],
        snapshot: [u8; 80],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Transport>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0010_0000, 512), &mut memory.stack).unwrap(),
            MemoryRegion::new(descriptor(QUEUE_DESTINATION, 4), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(0x0020_0000, 320), &mut memory.source).unwrap(),
            MemoryRegion::new(descriptor(REFRESH_BLOCK, 296), &mut memory.refresh).unwrap(),
            MemoryRegion::new(descriptor(SNAPSHOT_BLOCK, 80), &mut memory.snapshot).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(0x0010_0000), ByteCount(512)).unwrap();
        body(&mut RecordContext::with_stack(space, Transport::default(), stack))
    }

    fn memory() -> Memory {
        Memory { stack: [0; 512], destination: [0; 4], source: [0x5a; 320], refresh: [0; 296], snapshot: [0; 80] }
    }

    #[test]
    fn word_payloads_keep_their_stock_commands() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_fea438(context, 1, 2, 3), Ok(()));
            assert_eq!(context.effects.command, 0x3f13);
            assert_eq!(context.effects.length, 12);
            assert_eq!(context.effects.word(0), 1);
            assert_eq!(context.effects.word(4), 2);
            assert_eq!(context.effects.word(8), 3);

            assert_eq!(helper_fea4d0(context, 4, 5), Ok(()));
            assert_eq!(context.effects.command, 0x3f27);
            assert_eq!(context.effects.length, 8);
            assert_eq!(context.effects.word(4), 5);

            assert_eq!(helper_fea4fc(context, 6), Ok(()));
            assert_eq!(context.effects.command, 0x3f28);
            assert_eq!(context.effects.length, 4);
            assert_eq!(context.effects.word(0), 6);

            assert_eq!(helper_fea524(context, 7, 8), Ok(()));
            assert_eq!(context.effects.command, 0x3f29);
            assert_eq!(context.effects.word(0), 7);

            assert_eq!(helper_fea550(context, 9, 10), Ok(()));
            assert_eq!(context.effects.command, 0x3f2a);
            assert_eq!(context.effects.word(4), 10);

            assert_eq!(helper_fead6c(context, 11, 12), Ok(()));
            assert_eq!(context.effects.command, 0x3f18);
            assert_eq!(context.effects.word(4), 12);
        });
    }

    #[test]
    fn record_payloads_copy_the_source_bytes() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            let source = GuestAddress::new(0x0020_0000);
            assert_eq!(helper_fea57c(context, source), Ok(()));
            assert_eq!(context.effects.command, 0x3f2b);
            assert_eq!(context.effects.length, 40);
            assert_eq!(&context.effects.payload[..40], &[0x5a; 40]);

            assert_eq!(helper_fea5f0(context, source), Ok(()));
            assert_eq!(context.effects.command, 0x3f2d);
            assert_eq!(context.effects.length, 44);
        });
    }

    #[test]
    fn refresh_and_snapshot_copy_their_blocks_before_submitting() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            let source = GuestAddress::new(0x0020_0000);
            assert_eq!(helper_fea5c0(context, source), Ok(()));
            assert_eq!(context.effects.command, 0x3f2c);
            assert_eq!(context.effects.length, 0);

            assert_eq!(helper_feab3c(context, source), Ok(()));
            assert_eq!(context.effects.command, 0x3f45);
            assert_eq!(context.effects.length, 0);
        });
        assert_eq!(&memory.refresh[..], &[0x5a; 296]);
        assert_eq!(&memory.snapshot[..], &[0x5a; 80]);
    }

    #[test]
    fn mixed_payloads_place_each_field_at_its_stock_offset() {
        let mut memory = memory();
        for index in 0..0x14 {
            memory.source[index] = index as u8;
        }
        with_memory(&mut memory, |context| {
            let source = GuestAddress::new(0x0020_0000);
            assert_eq!(helper_feaaec(context, 0x1234_5678, source), Ok(()));
            assert_eq!(context.effects.command, 0x3f44);
            assert_eq!(context.effects.length, 24);
            assert_eq!(context.effects.word(0), 0x1234_5678);
            assert_eq!(context.effects.half(4), 0x0100);
            assert_eq!(context.effects.half(6), 0);
            assert_eq!(context.effects.word(8), 0x0706_0504);
            assert_eq!(context.effects.word(12), 0x0b0a_0908);
            assert_eq!(context.effects.word(16), 0x0f0e_0d0c);
            assert_eq!(context.effects.word(20), 0x1312_1110);
        });
    }

    #[test]
    fn ten_argument_payload_keeps_the_byte_and_the_trailing_words() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_fead98(context, 1, 2, 0xab, 4, 5, 6, 7, 8, 9, 10), Ok(()));
            assert_eq!(context.effects.command, 0x3f3a);
            assert_eq!(context.effects.length, 40);
            assert_eq!(context.effects.word(0), 1);
            assert_eq!(context.effects.word(4), 2);
            assert_eq!(context.effects.payload[8], 0xab);
            assert_eq!(context.effects.payload[9], 0);
            assert_eq!(context.effects.word(12), 4);
            assert_eq!(context.effects.word(16), 5);
            assert_eq!(context.effects.word(36), 10);

            assert_eq!(helper_feadfc(context), Ok(()));
            assert_eq!(context.effects.command, 0x3f3b);
            assert_eq!(context.effects.length, 0);
        });
    }
}
