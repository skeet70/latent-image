//! Stock-shaped bodies over the image-5 bodies at 0x00fe0b08-0x00fead5c.

use crate::record_address_space::{ByteCount, GuestAddress, GuestAddressSpace, MemoryError, RecordContext, StackAlignment};
use crate::record_capture_machine::{CaptureKind, CaptureMode, CaptureRequestFlags};
use crate::record_hardware_requests::{helper_fe9580, CaptureCommand, CaptureQueue};
use crate::shutdown::KernelStatus;

const FAST_KEYS: u32 = 0x7943;
const SLOW_KEYS: u32 = 0x793b;
const NO_KEY: u32 = 5;

/// 0x00fe0b08
pub fn helper_fe0b08<M: GuestAddressSpace, E>(context: &mut RecordContext<M, E>, kind: u32, row: u32, column: u32, mode: u32) -> Result<u32, MemoryError> {
    let base = if mode == 7 || kind == 0x1c { FAST_KEYS } else { SLOW_KEYS };
    let index = match column {
        1..=4 => column - 1,
        _ => return Ok(NO_KEY),
    };
    let key = base + if row == 0 { 0 } else { 4 } + index;
    Ok(u32::from(crate::record_configuration::helper_69ff68(context, key)? as u8))
}

/// 0x00feac00
pub fn helper_feac00<M: GuestAddressSpace, E: CaptureQueue<M>>(
    context: &mut RecordContext<M, E>,
    first: u32,
    second: u32,
    third: u32,
    fourth: u32,
    fifth: u32,
    sixth: u32,
    source: GuestAddress,
) -> Result<KernelStatus, MemoryError> {
    context.with_guest_frame(ByteCount(176), StackAlignment::Word, |context, frame| {
        for (index, word) in [first, third, second, fourth, fifth, sixth].into_iter().enumerate() {
            context.write_u32(frame.at(ByteCount(index as u32 * 4), ByteCount(4))?, word)?;
        }
        context.copy_bytes(frame.at(ByteCount(24), ByteCount(152))?, source, ByteCount(152))?;
        helper_fe9580(context, CaptureCommand::new(0x3f49), frame.base(), frame.length(), &[])
    })
}

/// 0x00fead5c
pub fn helper_fead5c<M: GuestAddressSpace, E>(
    context: &mut RecordContext<M, E>,
    kind: CaptureKind,
    request: CaptureRequestFlags,
    mode: CaptureMode,
    fallback: CaptureRequestFlags,
) -> Result<u32, MemoryError> {
    crate::record_capture_machine::helper_fea3a0(context, kind, request, mode, fallback)
}

#[cfg(test)]
mod tests {
    //! Expected values come from the image-5 bodies at 0x00fe0b08-0x00fead5c.
    use super::*;
    use crate::record_address_space::{
        GuestDomain, GuestStack, HostAddressSpace, MemoryRegion, RegionDescriptor, RegionDomain, RegionPermission,
    };
    use crate::record_hardware_requests::{CaptureMessage, CaptureTask};

    const CONFIG: u32 = 0x0030_0000;
    const STACK: u32 = 0x0010_0000;
    const QUEUE_DESTINATION: u32 = 0x016f_0aa4;
    const SOURCE: u32 = 0x0020_0000;
    const PAYLOAD: usize = 176;

    struct Transport {
        command: u16,
        payload: [u8; PAYLOAD],
        length: u32,
    }

    impl Default for Transport {
        fn default() -> Self { Self { command: 0, payload: [0; PAYLOAD], length: 0 } }
    }

    impl<M: GuestAddressSpace> CaptureQueue<M> for Transport {
        fn current_task(&mut self, _memory: &mut M) -> Result<CaptureTask, MemoryError> { Ok(CaptureTask::new(9)) }
        fn submit(&mut self, memory: &mut M, message: CaptureMessage<'_>) -> KernelStatus {
            self.command = message.command.get();
            self.length = message.length.0;
            for offset in 0..message.length.0 as usize {
                self.payload[offset] = memory.read_u8(message.payload.checked_add(offset as u32).unwrap()).unwrap();
            }
            KernelStatus(0)
        }
    }

    struct Memory {
        config_pointer: [u8; 4],
        config: [u8; 0x9000],
        stack: [u8; 512],
        destination: [u8; 4],
        source: [u8; 160],
    }

    fn with_memory<R>(memory: &mut Memory, body: impl FnOnce(&mut RecordContext<HostAddressSpace<'_, '_>, Transport>) -> R) -> R {
        let descriptor = |base, length| RegionDescriptor::new(GuestAddress::new(base), ByteCount(length), RegionDomain::Shared, RegionPermission::ReadWrite).unwrap();
        let mut regions = [
            MemoryRegion::new(descriptor(0x0096_ca50, 4), &mut memory.config_pointer).unwrap(),
            MemoryRegion::new(descriptor(CONFIG, 0x9000), &mut memory.config).unwrap(),
            MemoryRegion::new(descriptor(STACK, 512), &mut memory.stack).unwrap(),
            MemoryRegion::new(descriptor(QUEUE_DESTINATION, 4), &mut memory.destination).unwrap(),
            MemoryRegion::new(descriptor(SOURCE, 160), &mut memory.source).unwrap(),
        ];
        let space = HostAddressSpace::new(GuestDomain::A, &mut regions).unwrap();
        let stack = GuestStack::new(GuestAddress::new(STACK), ByteCount(512)).unwrap();
        body(&mut RecordContext::with_stack(space, Transport::default(), stack))
    }

    fn memory() -> Memory {
        let mut memory = Memory {
            config_pointer: CONFIG.to_le_bytes(),
            config: [0; 0x9000],
            stack: [0; 512],
            destination: 0x016f_1234u32.to_le_bytes(),
            source: [0; 160],
        };
        for offset in 0x793bu32..0x794b {
            memory.config[offset as usize] = offset as u8;
        }
        for (index, byte) in memory.source.iter_mut().enumerate() { *byte = (index as u8).wrapping_add(0x30); }
        memory
    }

    #[test]
    fn the_key_group_follows_the_mode_kind_row_and_column() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            assert_eq!(helper_fe0b08(context, 0x1c, 0, 1, 0), Ok(0x43));
            assert_eq!(helper_fe0b08(context, 0, 0, 4, 7), Ok(0x46));
            assert_eq!(helper_fe0b08(context, 0, 1, 1, 7), Ok(0x47));
            assert_eq!(helper_fe0b08(context, 0, 0, 1, 0), Ok(0x3b));
            assert_eq!(helper_fe0b08(context, 0, 1, 4, 0), Ok(0x42));
            assert_eq!(helper_fe0b08(context, 0, 0, 0, 0), Ok(NO_KEY));
            assert_eq!(helper_fe0b08(context, 0x1c, 0, 5, 7), Ok(NO_KEY));
        });
    }

    #[test]
    fn the_payload_words_are_stored_in_their_stock_order() {
        let mut memory = memory();
        let transport = with_memory(&mut memory, |context| {
            assert_eq!(helper_feac00(context, 1, 2, 3, 4, 5, 6, GuestAddress::new(SOURCE)), Ok(KernelStatus(0)));
            core::mem::take(&mut context.effects)
        });
        assert_eq!(transport.command, 0x3f49);
        assert_eq!(transport.length, 176);
        let words: Vec<u32> = transport.payload[..24].chunks(4).map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap())).collect();
        assert_eq!(words, vec![1, 3, 2, 4, 5, 6]);
        assert_eq!(&transport.payload[24..176], &memory.source[..152]);
    }

    #[test]
    fn the_forwarder_repeats_the_capture_machine_decision() {
        let mut memory = memory();
        with_memory(&mut memory, |context| {
            for kind in [0x31u32, 0x32, 0x30] {
                for request in [0u32, 2] {
                    for mode in [0u32, 3] {
                        for fallback in [0u32, 2] {
                            let expected = crate::record_capture_machine::helper_fea3a0(
                                context, CaptureKind(kind), CaptureRequestFlags(request), CaptureMode(mode), CaptureRequestFlags(fallback),
                            );
                            let forwarded = helper_fead5c(
                                context, CaptureKind(kind), CaptureRequestFlags(request), CaptureMode(mode), CaptureRequestFlags(fallback),
                            );
                            assert_eq!(forwarded, expected);
                        }
                    }
                }
            }
        });
    }
}
